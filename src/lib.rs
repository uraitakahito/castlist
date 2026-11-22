//! アクターの台帳のサーバー。Claude Code の session 1 つ（起動 1 回）ごとに、本当の id と名を出し、
//! 登場・退場と、誰が誰を起こしたかを足すだけの記録で持つ（[`db`]・[`api`]）。監査の検索は [`search`]。
//!
//! この Mac の中からの要求だけを受ける（[`guard`]）。画面の応答には、外の script と inline の script を
//! 断る Content-Security-Policy を付ける（[`stamp`]）。

pub mod api;
pub mod db;
pub mod search;
pub mod tree;
pub mod ui;

use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::json;
use std::time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// 画面の応答に付ける Content-Security-Policy。script と style は、castlist 自身が配る file だけを許す。
/// form は castlist 自身へだけ送れる（一覧の絞りの form は GET で `/` に送る）。
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// 1 つの問い合わせにかけてよい長さ。過ぎれば 503 で返す。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// 問い合わせの道。
pub fn router(db: db::Db) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/", get(ui::list))
        .route("/actors/{id}", get(ui::profile))
        .route("/graph/tree", get(ui::tree))
        .route("/static/site.css", get(ui::site_css))
        .route("/v1/actors", post(api::register).get(search::actors))
        .route("/v1/actors/{id}", get(api::actor))
        .route("/v1/actors/{id}/lineage", get(search::lineage))
        .route("/v1/actors/{id}/settings", get(search::actor_settings))
        .route("/v1/events", get(search::events))
        .route("/v1/events/{id}", put(api::put_event))
        .route("/v1/links/{id}", put(api::put_link))
        .route("/v1/settings", get(search::settings))
        .route("/v1/settings/{id}", put(api::put_setting))
        .route("/v1/resolve", post(api::resolve))
        .route("/v1/graph", get(api::graph))
        .route("/v1/backup", get(api::backup))
        .with_state(db)
        .layer(middleware::from_fn(timeout))
        .layer(middleware::from_fn(guard))
        // 一番外に置く。guard が断った答えにも付く。
        .layer(middleware::from_fn(stamp))
}

/// 5 秒で答えられなければ、503 で返す（待っている物は捨てる）。
async fn timeout(request: Request, next: Next) -> Response {
    match tokio::time::timeout(REQUEST_TIMEOUT, next.run(request)).await {
        Ok(response) => response,
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "時間がかかりすぎた（5 秒）。少し待ってから送り直す",
        ),
    }
}

/// 生きているかと、版と、castlist の今の時刻（tpm が Mac の時計との差を測る）。
async fn healthz() -> Json<serde_json::Value> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default();
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION"), "now": now }))
}

/// この Mac の中からの要求だけを受ける。
/// - `Host` が 127.0.0.1・localhost でなければ断る（外の site が名前を付け替えて届ける要求を防ぐ）。
/// - 書く要求に `Origin` が付いていれば断る（browser の中のページからは書かせない。tpm と `curl` は付けない）。
async fn guard(request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.rsplit_once(':').map_or(v, |(name, _port)| name));
    if !matches!(host, Some("127.0.0.1" | "localhost")) {
        return error(
            StatusCode::FORBIDDEN,
            "Host は 127.0.0.1 か localhost だけを受ける",
        );
    }
    if request.method() != Method::GET
        && request.method() != Method::HEAD
        && request.headers().contains_key(header::ORIGIN)
    {
        return error(StatusCode::FORBIDDEN, "browser の中のページからは書けない");
    }
    next.run(request).await
}

/// どの答えにも、型の推し量りを止める header を付け、画面（HTML）の答えには CSP を付ける。
async fn stamp(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    let html = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"));
    if html {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        );
    }
    response
}

/// 誤りの答え（`{"error":"…"}`）。
pub(crate) fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}
