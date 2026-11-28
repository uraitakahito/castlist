//! 外に出す文の言語（AI-48）。castlist は、頼みの `Accept-Language` を見て、誤りの文を日本語か英語で返す。
//! - 並びを前から見て、初めに当たった `ja`・`en`。header が無い・`*`・どちらでもない言語は、英語。
//! - 守りの層（Host・Origin・本文の上限・時間切れ）が断る文も、同じ決まりで返す。
//! - 誤りの種類（`X-Amzn-Errortype`）と状態の数は、言語で変わらない。
//!
//! ほかの試験は、共通の送り方（tests/common の `App::send`）が日本語を頼む。ここは `App::send_raw` で、何も足さずに送る。
mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::{App, ai, error_type};
use serde_json::Value;

/// 断られる要求（Host が違う）。言語は `language` のとおりに付ける（None なら付けない）。
fn foreign_host(language: Option<&str>) -> Request<Body> {
    let mut request = Request::get("/healthz").header(header::HOST, "evil.example");
    if let Some(language) = language {
        request = request.header(header::ACCEPT_LANGUAGE, language);
    }
    request.body(Body::empty()).unwrap()
}

/// 何も足さずに送り、状態・誤りの種類・`message` を返す。
async fn refused(app: &App, request: Request<Body>) -> (StatusCode, String, String) {
    let (status, headers, bytes): (StatusCode, HeaderMap, Vec<u8>) = app.send_raw(request).await;
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let json: Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"));
    let message = json["message"].as_str().unwrap_or_else(|| panic!("{body}"));
    (status, error_type(&headers).to_owned(), message.to_owned())
}

const HOST_JA: &str = "Host は 127.0.0.1 か localhost だけを受ける";
const HOST_EN: &str = "Only Host 127.0.0.1 or localhost is accepted";

#[tokio::test]
async fn 言語を付けない頼みには_英語で返す() {
    let app = App::start().await;
    let (status, kind, message) = refused(&app, foreign_host(None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(kind, "Forbidden");
    assert_eq!(message, HOST_EN);
}

#[tokio::test]
async fn 頼みの言語の並びを前から見て_初めに当たった言語で返す() {
    let app = App::start().await;
    for (language, want) in [
        ("ja", HOST_JA),
        ("ja-JP", HOST_JA),
        ("ja,en;q=0.9", HOST_JA),
        ("en", HOST_EN),
        ("en-US,ja;q=0.8", HOST_EN),
        ("fr-CH, fr;q=0.9, ja;q=0.8", HOST_JA),
        ("fr", HOST_EN),
        ("*", HOST_EN),
    ] {
        let (status, kind, message) = refused(&app, foreign_host(Some(language))).await;
        // 状態と種類は、言語で変わらない。
        assert_eq!(status, StatusCode::FORBIDDEN, "{language}");
        assert_eq!(kind, "Forbidden", "{language}");
        assert_eq!(message, want, "{language}");
    }
}

fn write_with_origin(language: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/api/actors")
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::ORIGIN, "http://127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(language) = language {
        request = request.header(header::ACCEPT_LANGUAGE, language);
    }
    request
        .body(Body::from(ai("se", "s-1").to_string()))
        .unwrap()
}

#[tokio::test]
async fn origin_の付いた書く要求を断る文も_頼みの言語で返す() {
    let app = App::start().await;
    let (_, kind, message) = refused(&app, write_with_origin(None)).await;
    assert_eq!(kind, "Forbidden");
    assert_eq!(message, "Pages in the browser cannot write");
    let (_, _, message) = refused(&app, write_with_origin(Some("ja"))).await;
    assert_eq!(message, "browser の中のページからは書けない");
}

fn too_large(language: Option<&str>) -> Request<Body> {
    let mut body = ai("se", "s-1").to_string().into_bytes();
    body.resize(castlist::BODY_LIMIT + 1, b' ');
    let mut request = Request::post("/api/actors")
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(language) = language {
        request = request.header(header::ACCEPT_LANGUAGE, language);
    }
    request.body(Body::from(body)).unwrap()
}

#[tokio::test]
async fn 本文の上限を超えた時の文も_頼みの言語で返す() {
    let app = App::start().await;
    let (status, kind, message) = refused(&app, too_large(None)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(kind, "PayloadTooLarge");
    assert_eq!(message, "The body is limited to 2 MiB");
    let (_, _, message) = refused(&app, too_large(Some("ja"))).await;
    assert_eq!(message, "本文は 2 MiB まで");
}

fn register(language: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/api/actors")
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(language) = language {
        request = request.header(header::ACCEPT_LANGUAGE, language);
    }
    request
        .body(Body::from(ai("se", "s-1").to_string()))
        .unwrap()
}

#[tokio::test]
async fn 時間切れの文も_頼みの言語で返す() {
    // 時間切れを短くし、1 本だけの書く接続を握って書く操作を待たせる（tests/guard.rs の時間切れの試験と同じ）。
    let app = App::start_with(castlist::Config {
        timeout: Duration::from_millis(200),
        ..castlist::Config::default()
    })
    .await;
    let held = app.db.write().acquire().await.unwrap();
    let (status, kind, message) = refused(&app, register(None)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(kind, "Unavailable");
    assert_eq!(
        message,
        "Took too long (200 ms). Wait a moment and send again"
    );
    let (_, _, message) = refused(&app, register(Some("ja"))).await;
    assert_eq!(
        message,
        "時間がかかりすぎた（200 ms）。少し待ってから送り直す"
    );
    drop(held);
}

#[tokio::test]
async fn 混んでいる時の文も_頼みの言語で返す() {
    // 書く接続を握ったまま、時間切れ（既定）より先に、書く接続の待ち（2 秒）が切れる。生成した handler の返す誤りの道。
    let app = App::start().await;
    let held = app.db.write().acquire().await.unwrap();
    let (status, kind, message) = refused(&app, register(Some("en"))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(kind, "Unavailable");
    assert_eq!(message, "Busy. Wait a moment and send again");
    let (_, _, message) = refused(&app, register(Some("ja"))).await;
    assert_eq!(message, "混んでいる。少し待ってから送り直す");
    drop(held);
}

#[tokio::test]
async fn 共通の送り方は_日本語を頼む() {
    // ほかの試験が使う送り方（App::send）は、言語を付けていない要求に日本語を頼む（roster と同じ）。
    let app = App::start().await;
    let (_, _, body) = app.send(foreign_host(None)).await;
    assert!(body.contains(HOST_JA), "{body}");
    // 付けてあれば、そのまま。
    let (_, _, body) = app.send(foreign_host(Some("en"))).await;
    assert!(body.contains(HOST_EN), "{body}");
}
