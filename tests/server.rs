//! 受け入れの試験: 本物の binary を空いた port で起こし、HTTP で叩く。
// 試験の道具は同期で書く（async の中ではないので、糸を塞いでも困らない）。
#![allow(clippy::disallowed_methods)]

mod common;

use common::Server;
use std::time::{Duration, Instant};

#[test]
fn healthz_は版と今の時刻を返す() {
    let server = Server::start();
    let answer = server.get("/healthz");
    assert_eq!(answer.status, 200, "{answer:?}");
    let body: serde_json::Value = serde_json::from_str(&answer.body).expect("JSON");
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    // 時計の差を測る（tpm が Mac の時刻と比べる）ための、castlist の今の時刻（RFC 3339・UTC）。
    let now = body["now"].as_str().expect("now");
    assert!(now.len() >= 20 && now.contains('T') && now.ends_with('Z'), "now: {now}");
}

#[test]
fn host_が_127_0_0_1_と_localhost_でなければ断る() {
    let server = Server::start();
    let answer = server.send("GET /healthz HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n");
    assert_eq!(answer.status, 403, "{answer:?}");
    let answer = server.send("GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    assert_eq!(answer.status, 200, "{answer:?}");
}

#[test]
fn origin_の付いた書く要求を断る() {
    let server = Server::start();
    let answer = server.send(
        "POST /v1/actors HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://evil.example\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
    );
    assert_eq!(answer.status, 403, "{answer:?}");
}

#[test]
fn 画面の応答に_csp_を付ける() {
    let server = Server::start();
    let answer = server.get("/");
    assert_eq!(answer.status, 200, "{answer:?}");
    assert!(answer.header("content-type").unwrap_or_default().starts_with("text/html"), "{answer:?}");
    assert_eq!(
        answer.header("content-security-policy").as_deref(),
        Some(castlist_csp()),
        "{answer:?}"
    );
    assert_eq!(answer.header("x-content-type-options").as_deref(), Some("nosniff"));
    assert!(!answer.body.contains("<script>"), "inline の script を出さない");
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
        assert!(started.elapsed() < Duration::from_secs(5), "SIGTERM の後 5 秒で終わらない");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0), "{status:?}");
}

fn castlist_csp() -> &'static str {
    "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
}
