//! 試験の足場: 試験ごとに一時の DB を作り、Router に要求を通す。DB の決まりは、書く接続と読む接続を直に使って見る。
//! 本物の binary を起こす道具は [`server`]。
#![allow(dead_code)]

pub mod sample;
pub mod server;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use castlist::db::Db;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

/// 一時の DB の path（試験ごとに別の、空の folder の中）。
pub fn temp_db(tag: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "castlist-test-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("試験の folder");
    dir.join("castlist.db")
}

/// DB の file と WAL の file を、folder ごと消す（temp_db が作った folder だけ）。
pub fn remove_db(path: &Path) {
    let Some(dir) = path.parent() else { return };
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    assert!(
        name.starts_with("castlist-test-"),
        "temp_db の folder ではない: {}",
        dir.display()
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// 一時の DB と、それを使う Router。
pub struct App {
    pub app: Router,
    pub db: Arc<Db>,
    pub path: PathBuf,
}

impl Drop for App {
    fn drop(&mut self) {
        remove_db(&self.path);
    }
}

impl App {
    pub async fn start() -> Self {
        Self::start_with(castlist::Config::default()).await
    }

    /// 守りの層の設定（時間切れの長さ・記録の置き場）を決めて起こす。
    pub async fn start_with(config: castlist::Config) -> Self {
        let path = temp_db("app");
        let db = Arc::new(Db::open(&path).await.expect("Db::open"));
        let app = castlist::app_with(db.clone(), config);
        Self { app, db, path }
    }

    /// 要求を 1 つ通し、状態と header と本文の字を返す。
    pub async fn send(&self, request: Request<Body>) -> (StatusCode, HeaderMap, String) {
        let (status, headers, bytes) = self.send_bytes(request).await;
        (
            status,
            headers,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    /// 要求を 1 つ通し、状態と header と本文の byte を返す。
    ///
    /// 言語を付けていない要求には、日本語を頼む（`Accept-Language: ja`）。castlist は、言語を付けない頼みに英語で返す（AI-48）が、
    /// 試験の多くは日本語の字を見る（roster と担当が読む字）。言語の決まりを見る試験（tests/i18n.rs）は、[`App::send_raw`] で
    /// 何も足さずに送る。
    pub async fn send_bytes(&self, mut request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        request
            .headers_mut()
            .entry(header::ACCEPT_LANGUAGE)
            .or_insert(axum::http::HeaderValue::from_static("ja"));
        self.send_raw(request).await
    }

    /// 要求を、何も足さずに 1 つ通す（言語を付けない頼みを見る試験が使う）。
    pub async fn send_raw(&self, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        let response = self.app.clone().oneshot(request).await.expect("oneshot");
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.expect("本文").to_bytes();
        (parts.status, parts.headers, bytes.to_vec())
    }

    /// `POST`（本文は JSON）。答えの本文は JSON に読む（読めなければ Null）。
    pub async fn post(&self, uri: &str, body: &Value) -> (StatusCode, HeaderMap, Value) {
        let (status, headers, body) = self.send(post(uri, body)).await;
        (
            status,
            headers,
            serde_json::from_str(&body).unwrap_or(Value::Null),
        )
    }

    /// `GET`。答えの本文は JSON に読む（読めなければ Null）。
    pub async fn get(&self, uri: &str) -> (StatusCode, HeaderMap, Value) {
        let (status, headers, body) = self.send(get(uri)).await;
        (
            status,
            headers,
            serde_json::from_str(&body).unwrap_or(Value::Null),
        )
    }

    /// `PUT`（本文は JSON）。答えの本文は JSON に読む（読めなければ Null）。
    pub async fn put(&self, uri: &str, body: &Value) -> (StatusCode, HeaderMap, Value) {
        let (status, headers, body) = self.send(put(uri, body)).await;
        (
            status,
            headers,
            serde_json::from_str(&body).unwrap_or(Value::Null),
        )
    }

    /// アクターを登録し、答え（actor・created）を返す。
    pub async fn register(&self, body: &Value) -> Value {
        let (status, _, answer) = self.post("/api/actors", body).await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "登録: {status} {answer}"
        );
        answer
    }

    /// 表の行の数（読む接続で数える）。
    pub async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("select count(*) from {table}")))
            .fetch_one(self.db.read())
            .await
            .unwrap_or_else(|e| panic!("{table} を数える: {e}"))
    }

    /// 1 列の値を全部読む SQL（読む接続）。
    pub async fn column<T>(&self, sql: &str) -> Vec<T>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
    {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .fetch_all(self.db.read())
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
    }

    /// 出来事を 1 つ、アクターの id で書く接続に直に置く（受けた時刻を決めて置く試験のため）。受けた時刻は castlist の今。
    pub async fn event(&self, id: &str, actor: &str, kind: &str, detail: &str) {
        self.event_at(id, actor, kind, detail, &castlist::db::now())
            .await;
    }

    /// 出来事を 1 つ、受けた時刻（送り手の時刻も同じ）を決めて直に置く。
    pub async fn event_at(&self, id: &str, actor: &str, kind: &str, detail: &str, now: &str) {
        sqlx::query(
            "insert into events(id, actor_id, type, detail, sent_at, received_at, clock_skew, body_hash)
             values(?1, ?2, ?3, ?4, ?5, ?5, 0, 'x')",
        )
        .bind(id)
        .bind(actor)
        .bind(kind)
        .bind(detail)
        .bind(now)
        .execute(self.db.write())
        .await
        .unwrap_or_else(|e| panic!("出来事 {id}: {e}"));
    }

    /// 線を 1 つ、アクターの id で書く接続に直に置く。
    pub async fn link(&self, id: &str, kind: &str, from: &str, to: &str) {
        let now = castlist::db::now();
        sqlx::query(
            "insert into links(id, type, from_actor, to_actor, sent_at, received_at, clock_skew, body_hash)
             values(?1, ?2, ?3, ?4, ?5, ?5, 0, 'x')",
        )
        .bind(id)
        .bind(kind)
        .bind(from)
        .bind(to)
        .bind(&now)
        .execute(self.db.write())
        .await
        .unwrap_or_else(|e| panic!("線 {id}: {e}"));
    }

    /// 見た model と effort を 1 つ、アクターの id で書く接続に直に置く。知らない名は一覧に足す。
    pub async fn observed(&self, id: &str, actor: &str, model: &str, effort: &str) {
        let now = castlist::db::now();
        for (table, name) in [("models", model), ("efforts", effort)] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "insert into {table}(name, first_seen_at) values(?1, ?2) on conflict(name) do nothing"
            )))
            .bind(name)
            .bind(&now)
            .execute(self.db.write())
            .await
            .unwrap_or_else(|e| panic!("{table} {name}: {e}"));
        }
        sqlx::query(
            "insert into settings(id, actor_id, model_id, effort_id, source, sent_at, received_at, clock_skew, body_hash)
             values(?1, ?2, (select id from models where name = ?3), (select id from efforts where name = ?4),
                    'observed', ?5, ?5, 0, 'x')",
        )
        .bind(id)
        .bind(actor)
        .bind(model)
        .bind(effort)
        .bind(&now)
        .execute(self.db.write())
        .await
        .unwrap_or_else(|e| panic!("設定 {id}: {e}"));
    }

    /// トークンの使用量の行を 1 つ、アクターの id で書く接続に直に置く（区間の終わりの時刻と受けた時刻を決めて置く試験のため）。
    /// 区間の始めは終わりと同じ、回の数は 1、ほかの数は 0。model は claude-opus-5-5（無ければ一覧に足す）。
    pub async fn usage_at(&self, id: &str, actor: &str, to_at: &str, received: &str) {
        sqlx::query(
            "insert into models(name, first_seen_at) values('claude-opus-5-5', ?1) on conflict(name) do nothing",
        )
        .bind(received)
        .execute(self.db.write())
        .await
        .unwrap_or_else(|e| panic!("models: {e}"));
        sqlx::query(
            "insert into usage(id, actor_id, model_id, speed, service_tier, from_at, to_at, turns,
                               input_tokens, output_tokens, cache_read_tokens, cache_write_5m_tokens, cache_write_1h_tokens,
                               web_search_requests, web_fetch_requests,
                               cost_micro_usd, max_context, buckets, sent_at, received_at, clock_skew, body_hash)
             values(?1, ?2, (select id from models where name = 'claude-opus-5-5'), 'standard', 'standard', ?3, ?3, 1,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, '[{\"bucket\":0,\"costMicroUsd\":0,\"turns\":1}]', ?4, ?4, 0, 'x')",
        )
        .bind(id)
        .bind(actor)
        .bind(to_at)
        .bind(received)
        .execute(self.db.write())
        .await
        .unwrap_or_else(|e| panic!("トークンの使用量 {id}: {e}"));
    }

    /// 書く接続で SQL を 1 つ流す（試験の見本を直に置く）。
    pub async fn execute(&self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(self.db.write())
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
}

/// Router に要求を 1 つ通し、状態と本文の字を返す（DB は試験ごとの一時の物）。
pub async fn send(request: Request<Body>) -> (StatusCode, String) {
    let app = App::start().await;
    let (status, _, body) = app.send(request).await;
    (status, body)
}

pub fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header(header::HOST, "127.0.0.1:8802")
        .body(Body::empty())
        .unwrap()
}

pub fn post(path: &str, body: &Value) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub fn put(path: &str, body: &Value) -> Request<Body> {
    Request::put(path)
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// 1 つの値を全部読む SQL（読む接続）。
pub async fn scalar<T>(app: &App, sql: &str) -> T
where
    T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(app.db.read())
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// 今から `secs` 秒ずらした時刻（RFC 3339・UTC）。
pub fn at(secs: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::seconds(secs))
        .format(&Rfc3339)
        .expect("時刻")
}

/// AI のアクターの登録の本文（鍵は claude.session）。
pub fn ai(prefix: &str, session: &str) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "ai",
        "namePrefix": prefix,
        "labels": { "project": "castlist" },
        "bindings": [{ "type": "claude.session", "value": session }],
        "sentAt": at(0),
    })
}

/// 人のアクターの登録の本文（鍵は tpm.person）。
pub fn human(person: &str) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "human",
        "namePrefix": "pm",
        "labels": { "role": "pm" },
        "bindings": [{ "type": "tpm.person", "value": person }],
        "sentAt": at(0),
    })
}

/// `ai("se", "s-1")` の鍵。
pub const SE1: &str = "claude.session:s-1";
/// `ai("se", "s-2")` の鍵。
pub const SE2: &str = "claude.session:s-2";
/// `human("pm")` の鍵。
pub const PM: &str = "tpm.person:pm";

/// 出来事の本文（出し手は tpm・送り手の時刻は今）。
pub fn event(by: &str, kind: &str) -> Value {
    json!({ "issuer": "tpm", "by": by, "type": kind, "sentAt": at(0) })
}

/// 線の本文（出し手は tpm・送り手の時刻は今）。
pub fn link(kind: &str, from: &str, to: &str) -> Value {
    json!({ "issuer": "tpm", "type": kind, "from": from, "to": to, "sentAt": at(0) })
}

/// 見た model と effort の本文（出し手は tpm・送り手の時刻は今）。
pub fn setting(by: &str, model: &str, effort: &str) -> Value {
    json!({
        "issuer": "tpm",
        "by": by,
        "model": model,
        "effort": effort,
        "source": "observed",
        "sentAt": at(0),
    })
}

/// 誤りの種類（header `X-Amzn-Errortype`）。
pub fn error_type(headers: &HeaderMap) -> &str {
    headers
        .get("x-amzn-errortype")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}
