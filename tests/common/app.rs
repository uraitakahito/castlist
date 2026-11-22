//! 道筋を直に呼ぶ試験の道具: 試験ごとに一時の DB を作り、Router を tower の oneshot で叩く。

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use castlist::db::Db;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

/// 一時の DB の path（試験ごとに別）。
pub fn temp_db(tag: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!(
        "castlist-test-{tag}-{}-{}.db",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// DB の file と、WAL の file を消す。
pub fn remove_db(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(name));
    }
}

pub struct App {
    pub app: Router,
    pub db: Db,
    pub path: PathBuf,
}

impl Drop for App {
    fn drop(&mut self) {
        remove_db(&self.path);
    }
}

/// 要求を作る（Host は 127.0.0.1。本文があれば JSON）。
pub fn request(method: &str, uri: &str, body: Option<&Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1");
    match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("要求")
}

impl App {
    pub async fn start() -> Self {
        let path = temp_db("api");
        let db = Db::open(&path).await.expect("Db::open");
        let app = castlist::router(db.clone());
        Self { app, db, path }
    }

    pub async fn raw(
        &self,
        method: &str,
        uri: &str,
        body: Option<&Value>,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let response = self
            .app
            .clone()
            .oneshot(request(method, uri, body))
            .await
            .expect("oneshot");
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.expect("本文").to_bytes().to_vec();
        (parts.status, parts.headers, bytes)
    }

    /// JSON の答え（JSON でなければ Null）。
    pub async fn call(&self, method: &str, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
        let (status, _, bytes) = self.raw(method, uri, body).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    pub async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.call("GET", uri, None).await
    }

    pub async fn post(&self, uri: &str, body: &Value) -> (StatusCode, Value) {
        self.call("POST", uri, Some(body)).await
    }

    pub async fn put(&self, uri: &str, body: &Value) -> (StatusCode, Value) {
        self.call("PUT", uri, Some(body)).await
    }

    /// アクターを登録し、答え（id・name・kind・created）を返す。
    pub async fn register(&self, body: &Value) -> Value {
        let (status, answer) = self.post("/v1/actors", body).await;
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

    /// 1 つの値を読む SQL（読む接続）。
    pub async fn scalar<T>(&self, sql: &str) -> T
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
    {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .fetch_one(self.db.read())
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
    }
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
        "name_prefix": prefix,
        "labels": { "project": "castlist" },
        "bindings": [{ "type": "claude.session", "value": session }],
        "at": at(0),
    })
}

/// 人のアクターの登録の本文（鍵は tpm.person）。
pub fn human(person: &str) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "human",
        "name_prefix": "pm",
        "labels": { "role": "pm" },
        "bindings": [{ "type": "tpm.person", "value": person }],
        "at": at(0),
    })
}

/// 出来事の本文。
pub fn event(by: &str, kind: &str, at: &str) -> Value {
    json!({ "issuer": "tpm", "by": by, "type": kind, "at": at })
}

/// model と effort の本文。
pub fn setting(by: &str, model: &str, effort: &str, at: &str) -> Value {
    json!({
        "issuer": "tpm",
        "by": by,
        "model": model,
        "effort": effort,
        "source": "observed",
        "at": at,
    })
}

/// 線の本文。
pub fn link(kind: &str, from: &str, to: &str) -> Value {
    json!({ "issuer": "tpm", "type": kind, "from": from, "to": to, "at": at(0) })
}
