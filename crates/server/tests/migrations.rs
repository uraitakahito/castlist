//! 移行（crates/server/migrations/）: 出した移行の file は替えず、後の版が書いた DB は、移行を当てずに断る。
mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use castlist::db::Db;
use common::temp_db;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// 出した版が当てた移行の file と、その SHA-256。1 byte も替えない（替えると checksum が変わり、動いている DB
/// （`castlist-data-v1`）を sqlx の移行が「中身の違う移行」として断る）。直す時は新しい番号の file を足す。
const RELEASED: [(&str, &str); 2] = [
    (
        "0001_init.sql",
        "fa5a6f7ad5dbaf3669571346cf70d3b6ca5bb8167709f56c8c6cacc9227457a8",
    ),
    (
        "0002_usage.sql",
        "8965b173735677e449c17db18bf1e17493caa236eaad18ca36e012c1f1ef7453",
    ),
];
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

/// 後の版が書いた DB の本当の形: この server で開いて移行を当て、閉じてから、移行の表に最も新しい版の次の版の行を足す。
/// 足した後の移行の表の版を返す。
async fn later_db(tag: &str) -> (PathBuf, Vec<i64>) {
    let path = temp_db(tag);
    Db::open(&path)
        .await
        .expect("新しい DB を開く")
        .close()
        .await;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .expect("DB を開く");
    sqlx::query(
        "insert into _sqlx_migrations (version, description, success, checksum, execution_time) \
         select max(version) + 1, 'later', true, zeroblob(48), 1 from _sqlx_migrations",
    )
    .execute(&pool)
    .await
    .unwrap();
    let versions = sqlx::query_scalar("select version from _sqlx_migrations order by version")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    (path, versions)
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
    for file in UNRELEASED {
        assert!(
            migrations_dir().join(file).is_file(),
            "{file}: UNRELEASED に名指した file が migrations/ に無い"
        );
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
/// （この server の版 1・2 を当てていない DB に、後の版 3 だけがある形。当てれば、0001_init.sql の表ができる）。
#[tokio::test]
async fn a_database_from_a_later_version_is_refused_without_migrating() {
    let path = ledger_db("later", &[(3, "later", "00".repeat(48))]).await;
    let e = match Db::open(&path).await {
        Ok(_) => panic!("後の版の DB を開いた"),
        Err(e) => e,
    };
    assert_eq!(
        e.to_string(),
        "この server より新しい版の DB（この server の migrations に無い移行 3 を当ててある）。\
         開かずに止める。書いた版の image で起こす"
    );
    assert_eq!(
        tables_and_versions(&path).await,
        (vec!["_sqlx_migrations".to_owned()], vec![3])
    );
    common::remove_db(&path);
}

/// 本当の形（この server の移行をみな当てた上に、後の版の移行が 1 つある）でも、後の版の訳で断る。sqlx が、版ごとの
/// 中身の写しを見る前に、無い版を見ることも見る（逆になれば「DB の移行に落ちた」になる）。移行の表は替わらない。
#[tokio::test]
async fn a_migrated_database_with_one_later_migration_is_refused_as_later() {
    let (path, versions) = later_db("later-real").await;
    let later = *versions.last().unwrap();
    let e = match Db::open(&path).await {
        Ok(_) => panic!("後の版の DB を開いた"),
        Err(e) => e,
    };
    assert_eq!(
        e.to_string(),
        format!(
            "この server より新しい版の DB（この server の migrations に無い移行 {later} を当ててある）。\
             開かずに止める。書いた版の image で起こす"
        )
    );
    assert_eq!(tables_and_versions(&path).await.1, versions);
    common::remove_db(&path);
}

/// binary も、後の版の DB では訳を出して 1 で終わる。訳は stdout と、DB の隣の記録（logs/ の server.log と runs.log）に出る。
#[tokio::test]
async fn the_binary_refuses_a_database_from_a_later_version() {
    let (path, versions) = later_db("later-binary").await;
    let mut child = Command::new(common::server::binary())
        .env("CASTLIST_DB", &path)
        .env("CASTLIST_LISTEN", "127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起こす");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("後の版の DB で起きたまま");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(status.code(), Some(1), "{stdout}{stderr}");
    let reason = "この server より新しい版の DB";
    assert!(stdout.contains(reason), "{stdout}{stderr}");
    let logs = path.with_file_name("logs");
    let server = tokio::fs::read_to_string(logs.join("server.log"))
        .await
        .unwrap();
    assert!(server.contains(reason), "{server}");
    let runs = tokio::fs::read_to_string(logs.join("runs.log"))
        .await
        .unwrap();
    assert!(runs.contains(&format!("failed {reason}")), "{runs}");
    assert_eq!(tables_and_versions(&path).await.1, versions);
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
