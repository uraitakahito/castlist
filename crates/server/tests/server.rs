//! 受け入れの試験: 本物の binary（castlist-server）を空いた port で起こし、HTTP で叩く。
//! 守りの層の誤りの形・HEAD・本文の上限・backup は、hyper を通した本物の答えでも見る。
// 試験の道具は同期で書く（async の中ではないので、糸を塞いでも困らない）。
#![allow(clippy::disallowed_methods)]

mod common;

use std::time::{Duration, Instant};

use common::server::Server;
use common::{ai, temp_db};

#[test]
fn healthz_は版と今の時刻を返す() {
    let server = Server::start();
    let answer = server.get("/healthz");
    assert_eq!(answer.status, 200, "{answer:?}");
    let body = answer.json();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    // 時計の差を測る（送り手の roster が、自分の動く機械の時刻と比べる）ための、castlist の今の時刻（RFC 3339・UTC）。
    let now = body["now"].as_str().expect("now");
    assert!(
        now.len() >= 20 && now.contains('T') && now.ends_with('Z'),
        "now: {now}"
    );
}

#[test]
fn head_の_healthz_は_本文なしの_200() {
    let server = Server::start();
    let answer =
        server.send_text("HEAD /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    assert_eq!(answer.status, 200, "{answer:?}");
    assert!(answer.body.is_empty(), "{answer:?}");
    assert_eq!(
        answer.header("content-type").as_deref(),
        Some("application/json")
    );
}

#[test]
fn host_が_127_0_0_1_と_localhost_でなければ_forbidden_で断る() {
    let server = Server::start();
    let answer = server
        .send_text("GET /healthz HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n");
    assert_eq!(answer.status, 403, "{answer:?}");
    assert_eq!(
        answer.header("x-amzn-errortype").as_deref(),
        Some("Forbidden")
    );
    assert!(answer.json()["message"].is_string(), "{answer:?}");
    let answer =
        server.send_text("GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    assert_eq!(answer.status, 200, "{answer:?}");
}

#[test]
fn origin_の付いた書く要求を_forbidden_で断る() {
    let server = Server::start();
    let answer = server.send_text(
        "POST /api/actors HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://evil.example\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
    );
    assert_eq!(answer.status, 403, "{answer:?}");
    assert_eq!(
        answer.header("x-amzn-errortype").as_deref(),
        Some("Forbidden")
    );
}

#[test]
fn 上限を_1_byte_超える本文は_payload_too_large_で断る() {
    let server = Server::start();
    let body = vec![b' '; castlist::BODY_LIMIT + 1];
    let mut raw = format!(
        "POST /api/actors HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    raw.extend_from_slice(&body);
    let answer = server.send(&raw);
    assert_eq!(answer.status, 413, "{}", answer.head);
    assert_eq!(
        answer.header("x-amzn-errortype").as_deref(),
        Some("PayloadTooLarge")
    );
    assert!(answer.json()["message"].is_string(), "{answer:?}");
}

#[test]
fn 画面は無く_前の画面の道は_404() {
    // 2.0.0 で画面を外した（画面は callboard が出す）。本物の接続でも、前の道は 404 で、HTML も CSP も返さない。
    let server = Server::start();
    for path in ["/", "/graph/network", "/static/site.css"] {
        let answer = server.get(path);
        assert_eq!(answer.status, 404, "{path}: {answer:?}");
        assert!(
            !answer
                .header("content-type")
                .unwrap_or_default()
                .starts_with("text/html"),
            "{path}: {answer:?}"
        );
        assert_eq!(answer.header("content-security-policy"), None, "{path}");
        assert_eq!(
            answer.header("x-content-type-options").as_deref(),
            Some("nosniff"),
            "{path}"
        );
    }
}

#[test]
fn head_の_content_length_は_get_と同じ() {
    // HEAD で大きさを見る client のため（pagetray の review-17）。時刻の入らない答え（無い道の 404。本文は空でない JSON か、空）で、
    // 本物の接続の頭を比べる。本文は、HEAD では返さない。
    let server = Server::start();
    let get = server.get("/no/such");
    let head =
        server.send_text("HEAD /no/such HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    assert_eq!(get.status, 404, "{get:?}");
    assert_eq!(head.status, 404, "{head:?}");
    assert!(head.body.is_empty(), "{head:?}");
    assert!(get.header("content-length").is_some(), "{get:?}");
    assert_eq!(head.header("content-length"), get.header("content-length"));
    assert_eq!(
        get.header("content-length"),
        Some(get.body.len().to_string())
    );
}

#[test]
fn backup_は_sqlite_の_file_を返す() {
    let server = Server::start();
    let answer = server.json("POST", "/api/actors", &ai("se", "s-1"));
    assert_eq!(answer.status, 201, "{answer:?}");
    let answer = server.get("/api/backup");
    assert_eq!(answer.status, 200, "{}", answer.head);
    assert_eq!(
        answer.header("content-type").as_deref(),
        Some("application/vnd.sqlite3")
    );
    assert!(
        answer.body.starts_with(b"SQLite format 3\0"),
        "{}",
        answer.head
    );
}

#[test]
fn 止める合図で自分から終わる() {
    let mut server = Server::start();
    server.terminate();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = server.try_wait() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "SIGTERM の後 5 秒で終わらない"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0), "{status:?}");
}

#[test]
fn 起きた行は版と待ち受ける所と_db_を出す() {
    let db = temp_db("start-line");
    let mut server = Server::start_at(&db);
    let out = server.stdout();
    let line = out
        .lines()
        .find(|l| l.contains(" listen="))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        line.contains(&format!(
            "castlist {} listen=127.0.0.1:{} db={}",
            env!("CARGO_PKG_VERSION"),
            server.port,
            db.display()
        )),
        "{line}"
    );
    assert_eq!(server.stop(), Some(0));
    common::remove_db(&db);
}
