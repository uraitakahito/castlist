//! 0.4.x までの server の DB（切り替えの前の volume `castlist-data`）と、後の版が書いた DB を、この server は開かず、何も書かない。
//! 古い DB は、0.4.x の移行の表の形（版 1〜3。checksum は v0.4.0 の tag の migrations/ の SHA-384）を SQL で作る
//! （0.4.x の server の code は repo に無い）。
mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use castlist::db::Db;
use common::temp_db;
use sha2::{Digest, Sha256, Sha384};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// v0.4.0 の tag の `migrations/0001_init.sql`・`0002_search.sql`・`0003_sent_at.sql` の SHA-384（sqlx の checksum）。
const V0_4_CHECKSUMS: [(i64, &str, &str); 3] = [
    (
        1,
        "init",
        "1d6ba41426b0bf0ec5ca9fe0f03639ee86e98d28255cd8548948ffc409ac7b87890152bb208165e93de20580ac856027",
    ),
    (
        2,
        "search",
        "4034f180e703cfc773f5d0c0734d548e344006d189da756a35067ffab345a6e5f9667a32a8e452dc967104d3510c40e2",
    ),
    (
        3,
        "sent at",
        "fe6305ee8b6285dbf3fdb19b176e762eee58c8de623315c2114755cc7935f63c757e6581828deca090c56890f5980389",
    ),
];

/// 出した版が当てた移行の file と、その SHA-256。1 byte も替えない（替えると checksum が変わり、動いている DB
/// （`castlist-data-v1`）を [`castlist::db::refuse_current`] が「別の版の DB」として断る）。直す時は新しい番号の file を足す。
const RELEASED: [(&str, &str); 1] = [(
    "0001_init.sql",
    "fa5a6f7ad5dbaf3669571346cf70d3b6ca5bb8167709f56c8c6cacc9227457a8",
)];
/// 足したが、まだ出していない移行の file。出したら [`RELEASED`] に移す。
const UNRELEASED: [&str; 0] = [];

fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

/// 移行の表の行（版・名・checksum の 16 進）と表を 1 つ持つ DB を作る。`wal` なら WAL の形で閉じる
/// （0.4.x の server は DB を WAL で開いていたので、古い volume の file は WAL の形。写しの `VACUUM INTO` の file は WAL でない）。
async fn ledger_db(tag: &str, rows: &[(i64, &str, String)], wal: bool) -> PathBuf {
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
    sqlx::query("create table actors (id text primary key, name text not null)")
        .execute(&pool)
        .await
        .unwrap();
    if wal {
        let mode: String = sqlx::query_scalar("pragma journal_mode = wal")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode, "wal");
    }
    pool.close().await;
    // 閉じると -wal と -shm は消える（古い volume で止めた castlist と同じ形）。
    assert_eq!(siblings(&path), Vec::<String>::new(), "{}", path.display());
    path
}

async fn old_db(tag: &str, wal: bool) -> PathBuf {
    let rows: Vec<(i64, &str, String)> = V0_4_CHECKSUMS
        .iter()
        .map(|(v, d, c)| (*v, *d, (*c).to_owned()))
        .collect();
    ledger_db(tag, &rows, wal).await
}

async fn sha256(path: &Path) -> String {
    hex(&Sha256::digest(tokio::fs::read(path).await.unwrap()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
            "{file}: tests/refuse_old.rs の RELEASED か UNRELEASED に足す"
        );
    }
}

/// `Db::open` は訳（移す道）を出して断り、file を替えず、隣に file を作らない。WAL の形の file（古い volume）も、写しの形も同じ。
#[tokio::test]
async fn opening_an_old_database_is_refused_without_writing() {
    for wal in [false, true] {
        let path = old_db(&format!("refuse-open-{wal}"), wal).await;
        let before = sha256(&path).await;
        let e = match Db::open(&path).await {
            Ok(_) => panic!("0.4.x の DB を開いた（wal={wal}）"),
            Err(e) => e,
        };
        assert!(e.to_string().contains("1・2・3"), "wal={wal}: {e}");
        assert!(
            e.to_string().contains("v1.0.0 の image の migrate"),
            "wal={wal}: {e}"
        );
        assert_eq!(sha256(&path).await, before, "wal={wal}");
        assert_eq!(siblings(&path), Vec::<String>::new(), "wal={wal}");
        common::remove_db(&path);
    }
}

/// 後の版（移行を足した版）が書いた DB は、移す道でなく、書いた版で起こすよう言って断る。
#[tokio::test]
async fn a_database_from_a_later_version_is_refused_with_its_own_reason() {
    let ours = Sha384::digest(
        tokio::fs::read(migrations_dir().join("0001_init.sql"))
            .await
            .unwrap(),
    );
    let rows = [(1, "init", hex(&ours)), (2, "later", "00".repeat(48))];
    let path = ledger_db("refuse-later", &rows, true).await;
    let e = match Db::open(&path).await {
        Ok(_) => panic!("後の版の DB を開いた"),
        Err(e) => e,
    };
    assert!(
        e.to_string().contains("この server より新しい版の DB"),
        "{e}"
    );
    assert!(!e.to_string().contains("migrate"), "{e}");
    assert_eq!(siblings(&path), Vec::<String>::new());
    common::remove_db(&path);
}

/// binary も、訳を出して 0 でない数で終わり、DB の隣に記録の置き場（logs/）も作らない。
#[tokio::test]
async fn the_binary_refuses_an_old_database_and_leaves_no_logs() {
    for wal in [false, true] {
        let path = old_db(&format!("refuse-binary-{wal}"), wal).await;
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
                panic!("0.4.x の DB で起きたまま（wal={wal}）");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        let out = child.wait_with_output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!status.success(), "wal={wal}: {text}");
        assert!(
            text.contains("v1.0.0 の image の migrate"),
            "wal={wal}: {text}"
        );
        assert_eq!(sha256(&path).await, before, "wal={wal}");
        assert_eq!(siblings(&path), Vec::<String>::new(), "wal={wal}");
        assert!(
            !path.with_file_name("logs").exists(),
            "古い volume に記録の置き場を作った（wal={wal}）"
        );
        common::remove_db(&path);
    }
}

/// `castlist-server migrate …` は、起きずに、移す命令のある所（v1.0.0 の image）を言って 2 で終わる。
#[tokio::test]
async fn the_migrate_argument_points_to_the_v1_0_0_image() {
    let out = Command::new(common::server::binary())
        .args([
            "migrate",
            "--from",
            "/in/castlist.db",
            "--to",
            "/data/castlist.db",
        ])
        .output()
        .expect("binary を起こす");
    let text = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("v1.0.0 の image"), "{text}");
}
