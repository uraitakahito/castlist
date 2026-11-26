//! 移行（crates/server/migrations/）: 出した移行の file は替えず、後の版が書いた DB は、移行を当てずに断る。
mod common;

use std::path::{Path, PathBuf};

use castlist::db::Db;
use common::temp_db;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// 出した版が当てた移行の file と、その SHA-256。1 byte も替えない（替えると checksum が変わり、動いている DB
/// （`castlist-data-v1`）を sqlx の移行が「中身の違う移行」として断る）。直す時は新しい番号の file を足す。
const RELEASED: [(&str, &str); 1] = [(
    "0001_init.sql",
    "fa5a6f7ad5dbaf3669571346cf70d3b6ca5bb8167709f56c8c6cacc9227457a8",
)];
/// 足したが、まだ出していない移行の file。出したら [`RELEASED`] に移す。
const UNRELEASED: [&str; 0] = [];

fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

async fn sha256(path: &Path) -> String {
    hex(&Sha256::digest(tokio::fs::read(path).await.unwrap()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 移行の表（行は版・名・checksum の 16 進）だけを持つ DB を作る。
async fn ledger_db(tag: &str, rows: &[(i64, &str, String)]) -> PathBuf {
    let path = temp_db(tag);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .expect("DB を作る");
    sqlx::query(
        "create table _sqlx_migrations (version bigint primary key, description text not null, \
         installed_on timestamp not null default current_timestamp, success boolean not null, \
         checksum blob not null, execution_time bigint not null)",
    )
    .execute(&pool)
    .await
    .unwrap();
    for (version, description, checksum) in rows {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "insert into _sqlx_migrations (version, description, success, checksum, execution_time) \
             values ({version}, '{description}', true, x'{checksum}', 1)"
        )))
        .execute(&pool)
        .await
        .unwrap();
    }
    pool.close().await;
    path
}

/// DB の表の名と、移行の表の版（断った後に、何も当てていないかを見る）。
async fn tables_and_versions(path: &Path) -> (Vec<String>, Vec<i64>) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
        .expect("DB を読む");
    let tables =
        sqlx::query_scalar("select name from sqlite_master where type = 'table' order by name")
            .fetch_all(&pool)
            .await
            .unwrap();
    let versions = sqlx::query_scalar("select version from _sqlx_migrations order by version")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    (tables, versions)
}

/// 出した移行の file は 1 byte も替えず、migrations/ には、出した物と、まだ出していないと名指した物のほかを置かない
/// （次の file を足した時に、ここに足し忘れると落ちる）。
#[tokio::test]
async fn released_migrations_stay_byte_for_byte() {
    for (file, want) in RELEASED {
        assert_eq!(sha256(&migrations_dir().join(file)).await, want, "{file}");
    }
    let mut files: Vec<String> = std::fs::read_dir(migrations_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    for file in files {
        assert!(
            RELEASED.iter().any(|(f, _)| *f == file) || UNRELEASED.contains(&file.as_str()),
            "{file}: tests/migrations.rs の RELEASED か UNRELEASED に足す"
        );
    }
}

/// 後の版（移行を足した版）が書いた DB は、書いた版で起こすよう言って断り、この server の移行を 1 つも当てない
/// （版 1 を当てていない DB に、後の版 2 だけがある形。当てれば、0001_init.sql の表ができる）。
#[tokio::test]
async fn a_database_from_a_later_version_is_refused_without_migrating() {
    let path = ledger_db("later", &[(2, "later", "00".repeat(48))]).await;
    let e = match Db::open(&path).await {
        Ok(_) => panic!("後の版の DB を開いた"),
        Err(e) => e,
    };
    assert_eq!(
        e.to_string(),
        "この server より新しい版の DB（この server の migrations に無い移行 2 を当ててある）。\
         開かずに止める。書いた版の image で起こす"
    );
    assert_eq!(
        tables_and_versions(&path).await,
        (vec!["_sqlx_migrations".to_owned()], vec![2])
    );
    common::remove_db(&path);
}

/// 中身の写しの違う移行（この server の版のうち）を当てた DB は、ふつうの移行の誤りで断り、移行を当てない。
#[tokio::test]
async fn a_database_with_a_changed_migration_is_refused_as_a_migrate_failure() {
    let path = ledger_db("changed", &[(1, "init", "00".repeat(48))]).await;
    let e = match Db::open(&path).await {
        Ok(_) => panic!("中身の違う移行の DB を開いた"),
        Err(e) => e,
    };
    assert!(e.to_string().starts_with("DB の移行に落ちた: "), "{e}");
    assert_eq!(
        tables_and_versions(&path).await,
        (vec!["_sqlx_migrations".to_owned()], vec![1])
    );
    common::remove_db(&path);
}
