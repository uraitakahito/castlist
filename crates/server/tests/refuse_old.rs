//! 0.4.x までの server の DB（切り替えの前の volume `castlist-data`）を、この server は開かず、何も書かない。
//! 古い DB は、0.4.x の移行の表の形（版 1〜3。1 は中身の違う写し）を SQL で作る（古い server の code は無い）。
mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use castlist::db::Db;
use common::temp_db;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// 0.4.x の DB の形: sqlx の移行の表に版 1・2・3（どれもこの server の migrations と合わない）と、表を 1 つ。
/// `journal` は、0.4.x の写し（`VACUUM INTO`）と同じく WAL でない file にする。
async fn old_db(tag: &str) -> PathBuf {
    let path = temp_db(tag);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .expect("古い DB を作る");
    for sql in [
        "create table _sqlx_migrations (version bigint primary key, description text not null, \
         installed_on timestamp not null default current_timestamp, success boolean not null, \
         checksum blob not null, execution_time bigint not null)",
        "insert into _sqlx_migrations (version, description, success, checksum, execution_time) values \
         (1, 'init', true, x'00', 1), (2, 'search', true, x'01', 1), (3, 'sent at', true, x'02', 1)",
        "create table actors (id text primary key, name text not null)",
        "insert into actors (id, name) values ('a-1', 'old-1')",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    pool.close().await;
    path
}

async fn sha256(path: &Path) -> String {
    Sha256::digest(tokio::fs::read(path).await.unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 同じ folder に、`name` で始まる別の file（`-wal`・`-shm`・`-journal`）があるか。
fn siblings(path: &Path) -> Vec<String> {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&name) && *n != name)
        .collect()
}

/// 出した版（1.0.0 から）が当てた移行の file は、1 byte も替えない。替えると checksum が変わり、動いている DB
/// （`castlist-data-v1`）を [`castlist::db::refuse_current`] が「別の版の DB」として断る。直す時は新しい番号の file を足す。
#[tokio::test]
async fn released_migrations_stay_byte_for_byte() {
    let released = [(
        "0001_init.sql",
        "fa5a6f7ad5dbaf3669571346cf70d3b6ca5bb8167709f56c8c6cacc9227457a8",
    )];
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for (file, want) in released {
        assert_eq!(sha256(&dir.join(file)).await, want, "{file}");
    }
}

/// `Db::open` は訳（移す道）を出して断り、file を替えず、隣に file を作らない。
#[tokio::test]
async fn opening_an_old_database_is_refused_without_writing() {
    let path = old_db("refuse-open").await;
    let before = sha256(&path).await;
    let e = match Db::open(&path).await {
        Ok(_) => panic!("0.4.x の DB を開いた"),
        Err(e) => e,
    };
    assert!(e.to_string().contains("1・2・3"), "{e}");
    assert!(e.to_string().contains("v1.0.0 の image の migrate"), "{e}");
    assert_eq!(sha256(&path).await, before);
    assert_eq!(siblings(&path), Vec::<String>::new());
    common::remove_db(&path);
}

/// binary も、訳を出して 0 でない数で終わり、DB の隣に記録の置き場（logs/）も作らない。
#[tokio::test]
async fn the_binary_refuses_an_old_database_and_leaves_no_logs() {
    let path = old_db("refuse-binary").await;
    let before = sha256(&path).await;
    let mut child = Command::new(common::server::binary())
        .env("CASTLIST_DB", &path)
        .env("CASTLIST_LISTEN", "127.0.0.1:0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("binary を起こす");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            panic!("0.4.x の DB で起きたまま");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    let out = child.wait_with_output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!status.success(), "{text}");
    assert!(text.contains("v1.0.0 の image の migrate"), "{text}");
    assert_eq!(sha256(&path).await, before);
    assert_eq!(siblings(&path), Vec::<String>::new());
    assert!(
        !path.with_file_name("logs").exists(),
        "古い volume に記録の置き場を作った"
    );
    common::remove_db(&path);
}
