//! 今の server の DB を新しい DB へ移す（`castlist-next migrate`）。移す元の DB は、同じ workspace の今の server
//! （repo の頭の crate `castlist`）の Router に /v1 の道で書き、`GET /v1/backup` で取った写し。0.1.x の DB は、その写しの
//! 時刻の列の名を `at` に戻して作る（0003 の前の形）。移した後は、新しい API（/api）で読み、今の API で読んだ物と比べる。
// 試験の道具は同期で書く所がある（file の大きさ・中身の写しを見る）。
#![allow(clippy::disallowed_methods)]
mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use castlist_next::db::Db;
use castlist_next::migrate::{self, MigrateError};
use common::{App, error_type, temp_db};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};
use tower::ServiceExt;

const OPUS: &str = "claude-opus-5-5";
const SONNET: &str = "claude-sonnet-5";
/// 今の DB に、番号を飛ばして直に置く model と effort（新しい DB では番号が替わるので、名で結び直すかを見る）。
const HAIKU: &str = "claude-haiku-5";
const ULTRA: &str = "ultra";
const FIRST_SEEN: &str = "2026-01-01T00:00:00.000Z";
/// 移した表（親が先）。
const TABLES: [&str; 8] = [
    "actors", "labels", "bindings", "links", "events", "models", "efforts", "settings",
];
/// seq を持つ表。
const SEQ_TABLES: [&str; 6] = [
    "actors", "labels", "bindings", "links", "events", "settings",
];

// --- 今の server ---

/// 今の server の DB と Router（/v1 の道）。
struct Old {
    app: Router,
    db: castlist::db::Db,
    path: PathBuf,
}

impl Drop for Old {
    fn drop(&mut self) {
        common::remove_db(&self.path);
    }
}

impl Old {
    async fn start() -> Self {
        let path = temp_db("old");
        let db = castlist::db::Db::open(&path).await.expect("今の Db::open");
        let app = castlist::router(db.clone());
        Self { app, db, path }
    }

    async fn send(&self, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Vec<u8>) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "127.0.0.1:8802")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.expect("oneshot");
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, bytes.to_vec())
    }

    /// 書いて、201 か 200 であることを見て答えを返す。
    async fn write(&self, method: Method, uri: &str, body: Value) -> Value {
        let (status, bytes) = self.send(method, uri, Some(&body)).await;
        let answer: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "今の {uri}: {status} {answer}"
        );
        answer
    }

    async fn get(&self, uri: &str) -> Value {
        let (status, bytes) = self.send(Method::GET, uri, None).await;
        let answer: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(status, StatusCode::OK, "今の {uri}: {answer}");
        answer
    }

    /// `GET /v1/backup` の写しを `to` に置く。
    async fn backup(&self, to: &Path) {
        let (status, bytes) = self.send(Method::GET, "/v1/backup", None).await;
        assert_eq!(status, StatusCode::OK, "今の backup");
        tokio::fs::write(to, bytes).await.expect("写しを置く");
    }

    async fn execute(&self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(self.db.write())
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
}

/// 今の時刻を、日本時間（+09:00）の RFC 3339 で（今の server は字のまま残す）。
fn jst_now() -> String {
    OffsetDateTime::now_utc()
        .to_offset(UtcOffset::from_hms(9, 0, 0).unwrap())
        .format(&Rfc3339)
        .unwrap()
}

fn utc_now() -> String {
    // ナノ秒まで付ける（Linux の CI は 9 桁の時刻を送る。macOS は 6 桁なので、手元でも同じ形を作る）。
    let now = OffsetDateTime::now_utc();
    let now = now
        .replace_nanosecond(now.nanosecond() / 1000 * 1000 + 123)
        .unwrap_or(now);
    now.format(&Rfc3339).unwrap()
}

/// 書いた物（移す元に入れた見本）。
struct Seeded {
    pm: String,
    se1: String,
    se2: String,
    se3: String,
}

impl Seeded {
    fn actors(&self) -> [&str; 4] {
        [&self.pm, &self.se1, &self.se2, &self.se3]
    }
}

/// 今の server に、どの表にも行のある見本を書く。人 1・AI 3（se.1〜se.3）。時差の付いた送り手の時刻・時計のずれ・
/// 鍵でない付随する id の付け替え・推しの幕引き・頼んだ物（id の無い設定）と見た物・番号を飛ばした model と effort も入れる。
async fn seed(old: &Old) -> Seeded {
    old.execute(&format!(
        "insert into models(id, name, first_seen_at) values(50, '{HAIKU}', '{FIRST_SEEN}')"
    ))
    .await;
    old.execute(&format!(
        "insert into efforts(id, name, first_seen_at) values(100, '{ULTRA}', '{FIRST_SEEN}')"
    ))
    .await;
    let id = |answer: Value| answer["id"].as_str().expect("id").to_owned();
    let pm = id(old
        .write(
            Method::POST,
            "/v1/actors",
            json!({
                "issuer": "tpm", "kind": "human", "name_prefix": "pm",
                "labels": { "project": "castlist", "role": "pm" },
                "bindings": [{ "type": "tpm.person", "value": "pm" }],
                "sent_at": utc_now(),
            }),
        )
        .await);
    let se1_body = |workspace: &str| {
        json!({
            "issuer": "tpm", "kind": "ai", "name_prefix": "se",
            "labels": { "project": "castlist", "tpm.key": "t-1" },
            "bindings": [
                { "type": "claude.session", "value": "s-1" },
                { "type": "cmux.workspace.ref", "value": workspace },
            ],
            "settings": { "model": OPUS, "effort": "high" },
            "sent_at": jst_now(),
        })
    };
    let se1 = id(old
        .write(Method::POST, "/v1/actors", se1_body("workspace:1"))
        .await);
    // 同じ鍵で、鍵でない付随する id を替えて登録し直す（行が 1 つ足される）。
    assert_eq!(
        id(old
            .write(Method::POST, "/v1/actors", se1_body("workspace:2"))
            .await),
        se1
    );
    // 送り手の時計が 6 年ずれている（clock_skew = 1）。
    let se2 = id(old
        .write(
            Method::POST,
            "/v1/actors",
            json!({
                "issuer": "tpm", "kind": "ai", "name_prefix": "se",
                "labels": { "project": "castlist" },
                "bindings": [{ "type": "claude.session", "value": "s-2" }],
                "sent_at": "2020-01-01T00:00:00Z",
            }),
        )
        .await);
    let se3 = id(old
        .write(
            Method::POST,
            "/v1/actors",
            json!({
                "issuer": "tpm", "kind": "ai", "name_prefix": "se",
                "labels": { "project": "castlist" },
                "bindings": [{ "type": "claude.session", "value": "s-3" }],
                "settings": { "model": SONNET, "effort": "medium" },
                "sent_at": utc_now(),
            }),
        )
        .await);
    let events = [
        (
            "e-1",
            "claude.session:s-1",
            "entered",
            json!({ "source": "startup" }),
            jst_now(),
        ),
        ("e-2", "claude.session:s-2", "entered", json!({}), utc_now()),
        (
            "e-3",
            "claude.session:s-2",
            "exited",
            json!({ "reason": "logout" }),
            utc_now(),
        ),
        (
            "e-4",
            "claude.session:s-3",
            "entered",
            json!({ "source": "resume" }),
            utc_now(),
        ),
        (
            "e-5",
            "claude.session:s-3",
            "closed",
            json!({ "inferred": true }),
            utc_now(),
        ),
        (
            "e-6",
            "claude.session:s-1",
            "compacted",
            json!({ "z": 1, "a": { "y": [1, 2], "b": "x" } }),
            "2020-06-01T12:00:00.5+09:00".to_owned(),
        ),
        (
            "e-7",
            "tpm.person:pm",
            "reported_in",
            json!({ "note": "移す前" }),
            utc_now(),
        ),
    ];
    for (id, by, kind, detail, sent_at) in events {
        old.write(
            Method::PUT,
            &format!("/v1/events/{id}"),
            json!({ "issuer": "tpm", "by": by, "type": kind, "detail": detail, "sent_at": sent_at }),
        )
        .await;
    }
    let links = [
        ("l-1", "launched", "tpm.person:pm", "claude.session:s-1"),
        (
            "l-2",
            "launched",
            "claude.session:s-1",
            "claude.session:s-2",
        ),
        (
            "l-3",
            "continued",
            "claude.session:s-2",
            "claude.session:s-3",
        ),
        (
            "l-4",
            "launched",
            "claude.session:s-1",
            "claude.session:s-3",
        ),
    ];
    for (id, kind, from, to) in links {
        old.write(
            Method::PUT,
            &format!("/v1/links/{id}"),
            json!({ "issuer": "tpm", "type": kind, "from": from, "to": to, "sent_at": jst_now() }),
        )
        .await;
    }
    let settings = [
        ("o-1", "claude.session:s-1", SONNET, "medium", "observed"),
        ("o-2", "claude.session:s-1", HAIKU, ULTRA, "observed"),
        ("o-3", "claude.session:s-3", OPUS, "max", "requested"),
    ];
    for (id, by, model, effort, source) in settings {
        old.write(
            Method::PUT,
            &format!("/v1/settings/{id}"),
            json!({
                "issuer": "tpm", "by": by, "model": model, "effort": effort,
                "source": source, "sent_at": utc_now(),
            }),
        )
        .await;
    }
    Seeded { pm, se1, se2, se3 }
}

/// 見本を書いた今の server と、その写しの path。
async fn backup() -> (Old, Seeded, PathBuf) {
    let old = Old::start().await;
    let seeded = seed(&old).await;
    let path = temp_db("backup");
    old.backup(&path).await;
    (old, seeded, path)
}

/// 写しを、時刻の列の名が `at` の 0.1.x の形にした複写（0003 の前）。
async fn backup_0_1(from: &Path) -> PathBuf {
    let path = temp_db("backup-0-1");
    tokio::fs::copy(from, &path).await.expect("写しの複写");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .expect("複写を開く");
    for table in ["actors", "bindings", "links", "events", "settings"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "alter table {table} rename column sent_at to at"
        )))
        .execute(&pool)
        .await
        .unwrap();
    }
    pool.close().await;
    path
}

// --- 新しい server ---

/// 移した DB を、新しい server で開く。
async fn open_new(path: &Path) -> App {
    let db = Arc::new(Db::open(path).await.expect("新しい Db::open"));
    let app = castlist_next::app(db.clone());
    App {
        app,
        db,
        path: path.to_owned(),
    }
}

/// 移す（library の関数）。突き合わせが合うことも見る。
async fn migrated(from: &Path) -> (PathBuf, migrate::Summary) {
    let to = temp_db("new");
    let summary = migrate::migrate(from, &to)
        .await
        .unwrap_or_else(|e| panic!("移す: {e}"));
    assert!(summary.ok(), "{summary}");
    (to, summary)
}

async fn new_get(app: &App, uri: &str) -> Value {
    let (status, _, answer) = app.get(uri).await;
    assert_eq!(status, StatusCode::OK, "新しい {uri}: {answer}");
    answer
}

// --- 比べる形 ---

fn camel(key: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in key.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// 今の答えと新しい答えを、同じ形にして比べる: 欄の名は camelCase、null の欄は無い物、時刻の字は UTC に直した物、
/// 読んだ時刻（`as_of`）・中の番号（`model_id`・`effort_id`）・頁の印は外す。
fn norm(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let k = camel(k);
                if v.is_null()
                    || matches!(
                        k.as_str(),
                        "asOf" | "modelId" | "effortId" | "next" | "nextToken"
                    )
                {
                    continue;
                }
                out.insert(k, norm(v));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(norm).collect()),
        Value::String(s) => match OffsetDateTime::parse(s, &Rfc3339) {
            Ok(t) => Value::String(t.to_offset(UtcOffset::UTC).format(&Rfc3339).unwrap()),
            Err(_) => v.clone(),
        },
        _ => v.clone(),
    }
}

/// 新しい GetActor の答えから、今の答えに無い物（今の model と effort の `id`・`actorId`・`clockSkew`）を外す。
fn new_actor(mut v: Value) -> Value {
    if let Some(current) = v.get_mut("currentSettings").and_then(Value::as_object_mut) {
        for key in ["id", "actorId", "clockSkew"] {
            current.remove(key);
        }
    }
    norm(&v)
}

/// 新しい図の点からも同じ物を外す。
fn new_graph(mut v: Value) -> Value {
    if let Some(nodes) = v.get_mut("nodes").and_then(Value::as_array_mut) {
        for node in nodes {
            if let Some(current) = node
                .get_mut("currentSettings")
                .and_then(Value::as_object_mut)
            {
                for key in ["id", "actorId", "clockSkew"] {
                    current.remove(key);
                }
            }
        }
    }
    norm(&v)
}

/// 今の API で読んだ物と、移した後に新しい API で読んだ物が同じか。
async fn same_reads(old: &Old, seeded: &Seeded, new: &App) {
    for id in seeded.actors() {
        let want = norm(&old.get(&format!("/v1/actors/{id}")).await);
        let got = new_actor(new_get(new, &format!("/api/actors/{id}")).await);
        assert_eq!(got, want, "GetActor {id}");
        let want = norm(&old.get(&format!("/v1/actors/{id}/lineage")).await);
        let got = norm(&new_get(new, &format!("/api/actors/{id}/lineage")).await);
        assert_eq!(got, want, "GetLineage {id}");
    }
    let want = norm(&old.get("/v1/actors?label=project:castlist").await);
    let got = norm(&new_get(new, "/api/actors?label=project:castlist").await);
    assert_eq!(got, want, "ListActors");
    assert_eq!(got["actors"].as_array().map(Vec::len), Some(4), "{got}");
    let want = norm(&old.get("/v1/events").await);
    let got = norm(&new_get(new, "/api/events").await);
    assert_eq!(got, want, "ListEvents");
    assert_eq!(got["events"].as_array().map(Vec::len), Some(7), "{got}");
    let want = norm(&old.get("/v1/settings").await);
    let got = norm(&new_get(new, "/api/settings").await);
    assert_eq!(got, want, "ListSettings");
    assert_eq!(got["settings"].as_array().map(Vec::len), Some(5), "{got}");
    let want = norm(&old.get("/v1/graph?label=project:castlist").await);
    let got = new_graph(new_get(new, "/api/graph?label=project:castlist").await);
    assert_eq!(got, want, "GetGraph");
}

/// 1 つの値を読む SQL（DB の file を、読むだけで開く）。
async fn scalars<T>(path: &Path, sql: &str) -> Vec<T>
where
    T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let rows = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    pool.close().await;
    rows
}

fn sha256(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    Sha256::digest(bytes)
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

// --- 試験 ---

/// 今の DB の写しを移すと、どのアクターも出来事も線も設定も、今の API で読んだ物と同じに新しい API で読める
/// （id・名・順・居る・推しの幕引き・ラベル・付随する id と鍵・時刻（時差の付いた物は UTC に直して）・時計のずれ）。
#[tokio::test]
async fn a_backup_moves_whole_and_reads_the_same_through_the_new_api() {
    let (old, seeded, from) = backup().await;
    let (to, summary) = migrated(&from).await;
    for table in TABLES {
        let t = summary
            .table(table)
            .unwrap_or_else(|| panic!("{table} の突き合わせが無い"));
        assert!(t.ok(), "{t:?}");
    }
    assert_eq!(summary.table("actors").unwrap().new_rows, 4);
    assert_eq!(summary.table("bindings").unwrap().new_rows, 6);
    assert_eq!(summary.table("events").unwrap().new_rows, 7);
    assert_eq!(summary.table("links").unwrap().new_rows, 4);
    assert_eq!(summary.table("settings").unwrap().new_rows, 5);
    let new = open_new(&to).await;
    same_reads(&old, &seeded, &new).await;

    // 中身の幾つかを、新しい答えの字で直に見る。
    let se1 = new_get(&new, &format!("/api/actors/{}", seeded.se1)).await;
    assert_eq!(se1["name"], "se.1");
    let keys: Vec<(&str, bool)> = se1["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["value"].as_str().unwrap(), b["isKey"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        keys,
        [
            ("s-1", true),
            ("workspace:1", false),
            ("workspace:2", false)
        ]
    );
    assert!(se1["sentAt"].as_str().unwrap().ends_with('Z'), "{se1}");
    assert_eq!(se1["currentSettings"]["model"], HAIKU);
    assert_eq!(se1["currentSettings"]["effort"], ULTRA);
    assert_eq!(se1["settings"][0]["source"], "requested");
    assert!(se1["settings"][0].get("id").is_none(), "{se1}");
    let e6 = &se1["events"][1];
    assert_eq!(e6["id"], "e-6");
    assert_eq!(e6["sentAt"], "2020-06-01T03:00:00.5Z");
    assert_eq!(e6["clockSkew"], true);
    assert_eq!(
        e6["detail"],
        json!({ "a": { "b": "x", "y": [1, 2] }, "z": 1 })
    );
    let se2 = new_get(&new, &format!("/api/actors/{}", seeded.se2)).await;
    assert_eq!(se2["clockSkew"], true);
    assert_eq!(se2["state"]["present"], false);
    let se3 = new_get(&new, &format!("/api/actors/{}", seeded.se3)).await;
    assert_eq!(se3["state"]["presence"], "closed");
    assert_eq!(se3["state"]["inferred"], true);
    assert_eq!(se3["linksIn"].as_array().unwrap().len(), 2);
    drop(new);
    common::remove_db(&from);
}

/// 行の無い表（sqlite_sequence に行がまだ無い表）がある写しも、空の台帳の写しも移せる。
#[tokio::test]
async fn a_backup_with_empty_tables_moves() {
    let old = Old::start().await;
    let empty = temp_db("backup-empty");
    old.backup(&empty).await;
    let (to, summary) = migrated(&empty).await;
    assert_eq!(summary.table("actors").unwrap().new_rows, 0);
    common::remove_db(&to);
    old.write(
        Method::POST,
        "/v1/actors",
        json!({
            "issuer": "tpm", "kind": "ai", "name_prefix": "se",
            "bindings": [{ "type": "claude.session", "value": "s-1" }],
            "sent_at": jst_now(),
        }),
    )
    .await;
    let one = temp_db("backup-one");
    old.backup(&one).await;
    let (to, summary) = migrated(&one).await;
    assert_eq!(summary.table("actors").unwrap().new_rows, 1);
    assert_eq!(summary.table("events").unwrap().new_rows, 0);
    let new = open_new(&to).await;
    let answer = new.register(&common::ai("se", "s-2")).await;
    assert_eq!(answer["actor"]["name"], "se.2", "{answer}");
    let seq: Vec<i64> = new.column("select seq from labels order by seq").await;
    assert_eq!(seq, [1], "行の無かった表の seq も 1 から");
    drop(new);
    common::remove_db(&empty);
    common::remove_db(&one);
}

/// 時刻の列の名が `at` の 0.1.x の DB も、列を見て読み、同じに移す。
#[tokio::test]
async fn a_0_1_database_with_at_columns_moves_the_same() {
    let (old, seeded, current) = backup().await;
    let from = backup_0_1(&current).await;
    let columns: Vec<String> = scalars(&from, "select name from pragma_table_info('events')").await;
    assert!(columns.iter().any(|c| c == "at"), "{columns:?}");
    let (to, _) = migrated(&from).await;
    let new = open_new(&to).await;
    same_reads(&old, &seeded, &new).await;
    let sent: Vec<String> = scalars(&to, "select sent_at from events where id = 'e-6'").await;
    assert_eq!(
        sent,
        ["2020-06-01T12:00:00.5+09:00"],
        "送り手の時刻は残した字のまま"
    );
    // 0.1.x の写しと今の写しを移した物は、同じ digest になる。
    let a = migrate::verify(&from, &to)
        .await
        .expect("0.1.x と突き合わせる");
    let b = migrate::verify(&current, &to)
        .await
        .expect("今の物と突き合わせる");
    assert!(a.ok() && b.ok(), "{a}\n{b}");
    for table in TABLES {
        assert_eq!(
            a.table(table).unwrap().old_digest,
            b.table(table).unwrap().old_digest,
            "{table}"
        );
    }
    drop(new);
    common::remove_db(&from);
    common::remove_db(&current);
}

/// 行の番号（seq）も送り手の id も、写しのまま残る。sqlite_sequence も続く。model と effort は名で結び直し、
/// 前から在った model の first_seen_at も残る。
#[tokio::test]
async fn ids_and_seq_stay_and_models_are_matched_by_name() {
    let (_old, _seeded, from) = backup().await;
    let (to, summary) = migrated(&from).await;
    for table in SEQ_TABLES {
        let id = if matches!(table, "labels" | "bindings") {
            "actor_id"
        } else {
            "id"
        };
        let sql = format!("select seq || ' ' || coalesce({id}, '-') from {table} order by seq");
        let old: Vec<String> = scalars(&from, &sql).await;
        let new: Vec<String> = scalars(&to, &sql).await;
        assert!(!old.is_empty(), "{table}");
        assert_eq!(new, old, "{table}");
        let sql = format!("select seq from sqlite_sequence where name = '{table}'");
        let old: Vec<i64> = scalars(&from, &sql).await;
        let new: Vec<i64> = scalars(&to, &sql).await;
        assert_eq!(new, old, "sqlite_sequence の {table}");
        let s = summary
            .sequences
            .iter()
            .find(|s| s.table == table)
            .expect("seq の突き合わせ");
        assert!(s.ok() && Some(s.new) == old.first().copied(), "{s:?}");
    }
    let received = "select received_at from actors order by seq";
    assert_eq!(
        scalars::<String>(&to, received).await,
        scalars::<String>(&from, received).await
    );
    let hashes = "select coalesce(body_hash, '-') from settings order by seq";
    assert_eq!(
        scalars::<String>(&to, hashes).await,
        scalars::<String>(&from, hashes).await
    );
    let first_seen: Vec<String> = scalars(
        &to,
        &format!("select first_seen_at from models where name = '{HAIKU}'"),
    )
    .await;
    assert_eq!(first_seen, [FIRST_SEEN]);
    // 新しい DB の efforts は、移行が先に置いた 5 つの後に ultra が足される（番号は替わる）。
    let efforts: Vec<String> = scalars(&to, "select name from efforts order by id").await;
    assert_eq!(efforts, ["low", "medium", "high", "xhigh", "max", ULTRA]);
    let ultra: Vec<String> = scalars(
        &to,
        "select s.id from settings s join efforts f on f.id = s.effort_id where f.name = 'ultra'",
    )
    .await;
    assert_eq!(ultra, ["o-2"]);
    common::remove_db(&from);
    common::remove_db(&to);
}

/// 移した後も、外のキーは崩れず、足すだけの決まり（trigger）が効く。
#[tokio::test]
async fn after_moving_foreign_keys_hold_and_rows_stay_append_only() {
    let (_old, _seeded, from) = backup().await;
    let (to, _) = migrated(&from).await;
    let new = open_new(&to).await;
    let broken: i64 = common::scalar(&new, "select count(*) from pragma_foreign_key_check").await;
    assert_eq!(broken, 0);
    for table in TABLES {
        for sql in [
            format!("update {table} set rowid = rowid"),
            format!("delete from {table}"),
        ] {
            let e = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .execute(new.db.write())
                .await
                .expect_err(&sql);
            assert!(e.to_string().contains("append-only"), "{sql}: {e}");
        }
    }
    drop(new);
    common::remove_db(&from);
}

/// 移した後に登録すると、番号は次（se.4）で、seq も続き、受けた時刻は移した行より前にならない。
#[tokio::test]
async fn a_new_actor_after_moving_takes_the_next_number_and_seq() {
    let (_old, _seeded, from) = backup().await;
    let (to, _) = migrated(&from).await;
    let new = open_new(&to).await;
    let last: i64 = common::scalar(&new, "select max(seq) from actors").await;
    let latest: String = common::scalar(&new, "select max(received_at) from events").await;
    let answer = new.register(&common::ai("se", "s-4")).await;
    assert_eq!(answer["actor"]["name"], "se.4", "{answer}");
    assert_eq!(answer["created"], true);
    let id = answer["actor"]["id"].as_str().unwrap();
    let seq: i64 = common::scalar(&new, &format!("select seq from actors where id = '{id}'")).await;
    assert_eq!(seq, last + 1);
    let received: String = common::scalar(
        &new,
        &format!("select received_at from actors where id = '{id}'"),
    )
    .await;
    assert!(received >= latest, "{received} < {latest}");
    // 移したアクターの鍵は、新しい server でも同じアクターを引く（作り直さない）。
    let again = new.register(&common::ai("se", "s-1")).await;
    assert_eq!(again["actor"]["name"], "se.1", "{again}");
    assert_eq!(again["created"], false);
    drop(new);
    common::remove_db(&from);
}

/// 移した出来事の id を、新しい本文の形で送り直すと 409（写しの形が違う。送り手は済んだとみなす）。
#[tokio::test]
async fn resending_a_moved_event_in_the_new_shape_is_a_conflict() {
    let (_old, _seeded, from) = backup().await;
    let (to, _) = migrated(&from).await;
    let new = open_new(&to).await;
    let body = json!({
        "issuer": "tpm", "by": "claude.session:s-2", "type": "entered", "detail": {},
        "sentAt": common::at(0),
    });
    let (status, headers, answer) = new.put("/api/events/e-2", &body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(error_type(&headers), "Conflict");
    // 新しい id なら足せる。
    let (status, _, answer) = new.put("/api/events/e-8", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    drop(new);
    common::remove_db(&from);
}

/// 移す元の file には書かない（中身の写しが同じで、隣に -wal・-shm も作らない）。
#[tokio::test]
async fn the_source_file_is_only_read() {
    let (_old, _seeded, from) = backup().await;
    let before = sha256(&from);
    let (to, _) = migrated(&from).await;
    assert_eq!(sha256(&from), before);
    assert_eq!(siblings(&from), Vec::<String>::new());
    common::remove_db(&from);
    common::remove_db(&to);
}

/// 移す先にアクターが 1 人でもいれば断り、何も足さない。
#[tokio::test]
async fn a_target_with_actors_is_refused() {
    let (_old, _seeded, from) = backup().await;
    let new = App::start().await;
    new.register(&common::ai("se", "s-9")).await;
    let to = new.path.clone();
    new.db.close().await;
    let e = migrate::migrate(&from, &to)
        .await
        .expect_err("空でない移す先");
    assert!(matches!(e, MigrateError::Target(_)), "{e}");
    assert_eq!(
        scalars::<i64>(&to, "select count(*) from actors").await,
        [1]
    );
    assert_eq!(
        scalars::<i64>(&to, "select count(*) from events").await,
        [0]
    );
    // 移し終えた先に、もう 1 度移すのも断る。
    let (done, _) = migrated(&from).await;
    let e = migrate::migrate(&from, &done)
        .await
        .expect_err("移し終えた先");
    assert!(matches!(e, MigrateError::Target(_)), "{e}");
    assert_eq!(
        scalars::<i64>(&done, "select count(*) from actors").await,
        [4]
    );
    // 元と同じ file も断る。
    let e = migrate::migrate(&from, &from).await.expect_err("同じ file");
    assert!(matches!(e, MigrateError::Target(_)), "{e}");
    drop(new);
    common::remove_db(&from);
    common::remove_db(&done);
}

/// 移す元が無い・SQLite の file でない・castlist の DB でない・隣に WAL の残りがある時は断り、移す先を作らない。
#[tokio::test]
async fn a_missing_or_garbage_source_is_refused() {
    let dir = temp_db("garbage");
    let missing = dir.with_file_name("missing.db");
    let garbage = dir.with_file_name("garbage.db");
    tokio::fs::write(
        &garbage,
        b"not a sqlite database at all, just bytes".repeat(200),
    )
    .await
    .unwrap();
    let empty = dir.with_file_name("empty.db");
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&empty)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::query("create table other(x)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let (_old, _seeded, from) = backup().await;
    let with_wal = dir.with_file_name("with-wal.db");
    tokio::fs::copy(&from, &with_wal).await.unwrap();
    tokio::fs::write(with_wal.with_file_name("with-wal.db-wal"), b"x")
        .await
        .unwrap();
    for source in [&missing, &garbage, &empty, &with_wal] {
        let to = dir.with_file_name("new.db");
        let e = migrate::migrate(source, &to)
            .await
            .expect_err(&source.display().to_string());
        assert!(
            matches!(e, MigrateError::Source(_)),
            "{}: {e}",
            source.display()
        );
        assert!(!to.exists(), "{}: 移す先を作った", source.display());
    }
    common::remove_db(&dir);
    common::remove_db(&from);
}

/// 突き合わせは、表の中身の違いを見つける（別の写しと比べると、違う表だけが NG）。
#[tokio::test]
async fn verify_finds_a_table_that_differs() {
    let (old, _seeded, from) = backup().await;
    let (to, _) = migrated(&from).await;
    old.write(
        Method::PUT,
        "/v1/events/e-8",
        json!({ "issuer": "tpm", "by": "claude.session:s-1", "type": "exited", "sent_at": utc_now() }),
    )
    .await;
    let other = temp_db("backup-other");
    old.backup(&other).await;
    let summary = migrate::verify(&other, &to).await.expect("突き合わせる");
    assert!(!summary.ok(), "{summary}");
    let events = summary.table("events").unwrap();
    assert_eq!((events.old_rows, events.new_rows), (8, 7));
    assert!(!events.ok());
    let seq = summary
        .sequences
        .iter()
        .find(|s| s.table == "events")
        .unwrap();
    assert!(!seq.ok(), "{seq:?}");
    for table in [
        "actors", "labels", "bindings", "links", "models", "efforts", "settings",
    ] {
        assert!(summary.table(table).unwrap().ok(), "{table}");
    }
    assert!(migrate::verify(&from, &to).await.unwrap().ok());
    common::remove_db(&from);
    common::remove_db(&to);
    common::remove_db(&other);
}

/// binary の `castlist-next migrate --from … --to …`: 移して表ごとの突き合わせを出し、0 で終わる。
/// 移し終えた先へもう 1 度移せば、訳を出して 1 で終わる。引数の誤りは 2。
#[tokio::test]
async fn the_binary_migrates_and_prints_the_checks() {
    let (_old, _seeded, from) = backup().await;
    let to = temp_db("new-bin");
    let run = |args: &[&Path]| {
        let mut command = Command::new(common::server::binary());
        command
            .arg("migrate")
            .arg("--from")
            .arg(args[0])
            .arg("--to")
            .arg(args[1]);
        command.output().expect("binary を起こす")
    };
    let out = run(&[&from, &to]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    for table in TABLES {
        let line = stdout
            .lines()
            .find(|l| l.starts_with(&format!("{table} ")))
            .unwrap_or_else(|| panic!("{table} の行が無い: {stdout}"));
        assert!(line.ends_with(" ok"), "{line}");
    }
    assert!(stdout.contains("actors 4 → 4"), "{stdout}");
    assert!(
        !to.with_file_name("logs").exists(),
        "移すだけで記録の置き場を作った"
    );

    let out = run(&[&from, &to]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("移す先"), "{stderr}");

    let out = Command::new(common::server::binary())
        .args(["migrate", "--from"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(common::server::binary())
        .args(["migrate", "--from", "a", "--to", "b", "--force"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(!Path::new("b").exists());

    let new = open_new(&to).await;
    let names: Vec<String> = new.column("select name from actors order by seq").await;
    assert_eq!(names, ["pm.1", "se.1", "se.2", "se.3"]);
    drop(new);
    common::remove_db(&from);
}
