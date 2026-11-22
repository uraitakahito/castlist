//! binary を起こして、終わり方の印と記録の file を見る（決まりは docs/src/run.md の「落ちた後に調べる」）。
//! 試験ごとに空の folder を作り、その中に DB を置く。記録は DB の隣の logs/ にできるので、試験どうしで混ざらない。
#![allow(clippy::disallowed_methods)]

mod common;

use common::Server;
use common::app::{remove_db, temp_db};
use std::path::{Path, PathBuf};

fn logs(db: &Path) -> PathBuf {
    db.with_file_name("logs")
}

fn read(db: &Path, name: &str) -> String {
    std::fs::read_to_string(logs(db).join(name)).unwrap_or_default()
}

#[test]
fn 初めての起動は_前回が無いと出て_起きた行に時刻が付く() {
    let db = temp_db("first");
    let mut server = Server::start_at(&db);
    let out = server.stdout();
    assert!(out.contains("previous: none (first start)"), "{out}");
    let line = out.lines().find(|l| l.contains(" listen=")).unwrap();
    assert!(
        line.starts_with("20") && line.contains("Z  INFO "),
        "{line}"
    );
    assert!(
        line.contains(&format!(
            "castlist {} listen=127.0.0.1:{} db=",
            env!("CARGO_PKG_VERSION"),
            server.port
        )),
        "{line}"
    );
    assert_eq!(server.stop(), Some(0));
    remove_db(&db);
}

#[test]
fn 止める合図で止めると_次の起動で_きれいに止まったと出る() {
    let db = temp_db("clean");
    assert_eq!(Server::start_at(&db).stop(), Some(0));
    let runs = read(&db, "runs.log");
    let kinds: Vec<&str> = runs
        .lines()
        .map(|l| l.split_whitespace().nth(1).unwrap())
        .collect();
    assert_eq!(kinds, ["start", "signal", "stopped"], "{runs}");
    let mut server = Server::start_at(&db);
    assert!(
        server.stdout().contains("previous: stopped cleanly ("),
        "{}",
        server.stdout()
    );
    server.stop();
    remove_db(&db);
}

#[test]
fn 合図なしで切られると_次の起動で_停止の印なしと最後に生きていた時刻が出る() {
    let db = temp_db("killed");
    Server::start_at(&db).kill();
    let mut server = Server::start_at(&db);
    let out = server.stdout();
    let line = out.lines().find(|l| l.contains("previous: ")).unwrap();
    assert!(line.contains("previous: no stop mark ("), "{line}");
    assert!(line.contains("last alive 20"), "{line}");
    server.stop();
    remove_db(&db);
}

#[test]
fn db_を開けずに終わると_次の起動で_誤りで終わったと訳が出る() {
    let db = temp_db("failed");
    // DB の名の所に folder があると、開けない。
    std::fs::create_dir_all(&db).unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_castlist-server"))
        .env("CASTLIST_LISTEN", "127.0.0.1:1")
        .env("CASTLIST_DB", &db)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    std::fs::remove_dir_all(&db).unwrap();
    let mut server = Server::start_at(&db);
    assert!(
        server.stdout().contains("previous: failed ("),
        "{}",
        server.stdout()
    );
    assert!(read(&db, "runs.log").contains(" failed "));
    server.stop();
    remove_db(&db);
}

#[test]
fn 問い合わせの行は記録の_file_に時刻つきで残り_stdout_には出ない_読む道で読める() {
    let db = temp_db("requests");
    let mut server = Server::start_at(&db);
    assert_eq!(server.get("/healthz").status, 200);
    assert_eq!(server.get("/no/such").status, 404);
    let log = read(&db, "server.log");
    let line = log
        .lines()
        .find(|l| l.contains("uri=/healthz"))
        .unwrap_or_else(|| panic!("{log}"));
    assert!(
        line.starts_with("20") && line.contains("status=200"),
        "{line}"
    );
    assert!(log.contains("uri=/no/such") && log.contains("status=404"));
    assert!(log.contains(" listen=127.0.0.1:"));
    let out = server.stdout();
    assert!(out.contains(" listen=127.0.0.1:"));
    assert!(!out.contains("uri=/healthz"), "{out}");
    // 読む道は、印と記録を字で返す。
    let answer = server.get("/v1/logs");
    assert_eq!(answer.status, 200);
    assert_eq!(
        answer.header("content-type").as_deref(),
        Some("text/plain; charset=utf-8")
    );
    assert!(answer.body.starts_with("== runs.log"), "{}", answer.body);
    assert!(answer.body.contains(" start version=") && answer.body.contains("uri=/healthz"));
    // 機械の外からの名では断る。
    let outside =
        server.send("GET /v1/logs HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n");
    assert_eq!(outside.status, 403);
    server.stop();
    remove_db(&db);
}

#[test]
fn 多く問い合わせても_logs_の合わせた大きさは上限の内() {
    let db = temp_db("cap");
    let mut server = Server::start_at(&db);
    // 1 行 約 200 byte × 7,000 = 約 1.4 MiB。file 1 つの上限（1 MiB）を超えて回る。
    let long = "x".repeat(60);
    for n in 0..7000 {
        assert_eq!(server.get(&format!("/no/{n}-{long}")).status, 404);
    }
    let limit = castlist::runlog::LOG_FILE_MAX * u64::from(castlist::runlog::LOG_FILES)
        + castlist::runlog::RUNS_MAX
        + 64;
    let size: u64 = std::fs::read_dir(logs(&db))
        .unwrap()
        .flatten()
        .map(|e| e.metadata().unwrap().len())
        .sum();
    assert!(size <= limit, "{size}");
    let rotated = std::fs::metadata(logs(&db).join("server.log.1"))
        .unwrap()
        .len();
    assert!(rotated <= castlist::runlog::LOG_FILE_MAX, "{rotated}");
    assert!(rotated > castlist::runlog::LOG_FILE_MAX - 1024, "{rotated}");
    server.stop();
    remove_db(&db);
}

#[test]
fn 並べて起こした_2_つの_server_の記録は混ざらない() {
    let (a, b) = (temp_db("a"), temp_db("b"));
    let (mut sa, mut sb) = (Server::start_at(&a), Server::start_at(&b));
    assert_eq!(sa.get("/only-a").status, 404);
    assert_eq!(sb.get("/only-b").status, 404);
    assert!(read(&a, "server.log").contains("uri=/only-a"));
    assert!(!read(&a, "server.log").contains("uri=/only-b"));
    assert!(!read(&b, "server.log").contains("uri=/only-a"));
    assert_eq!(read(&a, "runs.log").lines().count(), 1);
    sa.stop();
    sb.stop();
    remove_db(&a);
    remove_db(&b);
}

#[test]
fn 記録の置き場を付けない道筋には_読む道が無い() {
    // 試験の道具（Router を直に呼ぶ）は記録の置き場を付けない。その時 /v1/logs は 404。
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let app = common::app::App::start().await;
        let (status, _) = app.get("/v1/logs").await;
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
    });
}
