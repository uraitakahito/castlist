//! 手の道 `GET /api/backup`（DB の揃ったバックアップ）と `GET /api/logs`（残した記録）。今の server の `GET /v1/backup`・`GET /v1/logs` と同じ。
mod common;

use std::path::Path;

use axum::http::{StatusCode, header};
use castlist_next::runlog::RunLog;
use common::{App, ai, get, human, temp_db};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// DB の folder の中の、バックアップの一時の file の名（`castlist.db.backup-…`）。
fn leftovers(db: &Path) -> Vec<String> {
    let dir = db.parent().unwrap();
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".backup-"))
        .collect()
}

#[tokio::test]
async fn backup_is_a_whole_sqlite_file_with_the_registered_actors() {
    let app = App::start().await;
    let a = app.register(&ai("se", "s-1")).await;
    let b = app.register(&human("pm")).await;
    let (status, headers, bytes) = app.send_bytes(get("/api/backup")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "application/vnd.sqlite3"
    );
    assert_eq!(
        headers.get(header::CONTENT_DISPOSITION).unwrap(),
        "attachment; filename=\"castlist.db\""
    );
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    // 送り始めたら、一時の file は消す。
    assert_eq!(leftovers(&app.path), Vec::<String>::new());

    // 受けた file を別の所に置いて開く: 壊れておらず、外のキーの崩れも無く、登録したアクターが入っている。
    let copy = temp_db("backup-copy");
    tokio::fs::write(&copy, &bytes).await.unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&copy).read_only(true))
        .await
        .unwrap();
    let integrity: String = sqlx::query_scalar("pragma integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    let broken: Vec<(String,)> = sqlx::query_as("select \"table\" from pragma_foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(broken.is_empty(), "{broken:?}");
    let ids: Vec<String> = sqlx::query_scalar("select id from actors order by seq")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(
        ids,
        [
            a["actor"]["id"].as_str().unwrap(),
            b["actor"]["id"].as_str().unwrap()
        ]
    );
    pool.close().await;
    common::remove_db(&copy);
}

#[tokio::test]
async fn backup_of_an_empty_ledger_is_a_sqlite_file_too() {
    let app = App::start().await;
    let (status, _, bytes) = app.send_bytes(get("/api/backup")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.starts_with(b"SQLite format 3\0"));
}

#[tokio::test]
async fn logs_are_the_marks_and_the_lines_as_text() {
    let dir = temp_db("logs").with_file_name("logs");
    let log = RunLog::at(dir.clone());
    log.start("9.9.9");
    let app = App::start_with(castlist_next::Config {
        runlog: Some(log),
        ..castlist_next::Config::default()
    })
    .await;
    let (status, headers, body) = app.send(get("/api/logs")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    assert!(body.starts_with("== runs.log\n"), "{body}");
    assert!(body.contains(" start version=9.9.9\n"), "{body}");
    assert!(body.contains("== server.log (oldest first)\n"), "{body}");
    common::remove_db(&dir.with_file_name("castlist.db"));
}
