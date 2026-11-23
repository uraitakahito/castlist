//! 守りの層の振る舞い。生成した道の前でも、手の道（画面・backup・logs）の前でも効く。
//! - 断る答えは、生成した層の誤りと同じ restJson1 の形（header `X-Amzn-Errortype` と、本文の JSON の `message`）。
//!   種類は定義（ai.common）の `Forbidden`（403）・`PayloadTooLarge`（413）・`Unavailable`（503）。
//! - Host と Origin（今の server と同じ）・HEAD（GET と同じに答え、本文を捨てる）・本文の上限・時間切れ。
mod common;

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::{App, ai, error_type, send};
use serde_json::Value;

/// 断る答えの形: 種類の header・JSON の本文・`message` の字。`message` を返す。
fn refused(headers: &HeaderMap, body: &str, kind: &str) -> String {
    assert_eq!(error_type(headers), kind, "{body}");
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json"),
        "{body}"
    );
    let json: Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"));
    let message = json["message"].as_str().unwrap_or_else(|| panic!("{body}"));
    assert!(!message.is_empty());
    message.to_owned()
}

fn with_host(method: &str, path: &str, host: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, host)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_foreign_host_is_refused_before_the_generated_service() {
    let request = with_host("GET", "/healthz", "evil.example");
    assert_eq!(send(request).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn localhost_with_any_port_is_accepted() {
    let request = with_host("GET", "/healthz", "localhost:9999");
    assert_eq!(send(request).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_write_with_an_origin_is_refused() {
    let request = Request::post("/healthz")
        .header("host", "127.0.0.1:8802")
        .header("origin", "http://127.0.0.1:8802")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(request).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_foreign_host_gets_the_forbidden_error_shape_on_every_kind_of_path() {
    let app = App::start().await;
    // 生成した道・画面・backup・知らない道のどれも、同じ形で断る。
    for path in ["/healthz", "/api/actors", "/", "/api/backup", "/no/such"] {
        let (status, headers, body) = app.send(with_host("GET", path, "evil.example")).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        let message = refused(&headers, &body, "Forbidden");
        assert!(message.contains("Host"), "{path}: {message}");
        // 断った答えにも stamp が付く（一番外）。
        assert_eq!(
            headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
            "nosniff"
        );
    }
    // Host の無い要求も断る。
    let request = Request::get("/healthz").body(Body::empty()).unwrap();
    let (status, headers, body) = app.send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    refused(&headers, &body, "Forbidden");
}

#[tokio::test]
async fn a_write_with_an_origin_gets_the_forbidden_error_shape_and_writes_nothing() {
    let app = App::start().await;
    let request = Request::post("/api/actors")
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::ORIGIN, "http://127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(ai("se", "s-1").to_string()))
        .unwrap();
    let (status, headers, body) = app.send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let message = refused(&headers, &body, "Forbidden");
    assert!(message.contains("browser"), "{message}");
    assert_eq!(app.count("actors").await, 0);
    // 読む要求は、Origin が付いていても通す。
    let request = Request::get("/healthz")
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::ORIGIN, "http://127.0.0.1:8802")
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.send(request).await.0, StatusCode::OK);
}

#[tokio::test]
async fn head_is_answered_like_get_without_the_body() {
    let app = App::start().await;
    let answer = app.register(&ai("se", "s-1")).await;
    let id = answer["actor"]["id"].as_str().unwrap().to_owned();
    // 生成した道（GET の操作）と画面の道のどちらも、GET と同じ状態と header で、本文なし。
    for (path, content_type) in [
        ("/healthz".to_owned(), "application/json"),
        (format!("/api/actors/{id}"), "application/json"),
        ("/".to_owned(), "text/html; charset=utf-8"),
    ] {
        let (status, headers, body) = app.send(with_host("HEAD", &path, "127.0.0.1:8802")).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert!(body.is_empty(), "{path}: {body}");
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            content_type,
            "{path}"
        );
    }
    // GET の無い道（書く操作）の HEAD も、GET と同じ答え（生成した層の 405）。
    for method in ["HEAD", "GET"] {
        let (status, _, _) = app
            .send(with_host(method, "/api/events/e-1", "127.0.0.1:8802"))
            .await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
    }
    // HEAD で書かれはしない。
    assert_eq!(app.count("actors").await, 1);
}

#[test]
fn the_limits_are_five_seconds_and_two_mebibytes() {
    assert_eq!(castlist_next::REQUEST_TIMEOUT, Duration::from_secs(5));
    assert_eq!(castlist_next::BODY_LIMIT, 2 * 1024 * 1024);
    assert_eq!(
        castlist_next::Config::default().timeout,
        castlist_next::REQUEST_TIMEOUT
    );
}

/// 登録の本文を、空白で詰めて `len` byte にする（JSON として同じ物）。
fn padded(len: usize) -> Vec<u8> {
    let mut body = ai("se", "s-1").to_string().into_bytes();
    assert!(body.len() < len);
    body.resize(len, b' ');
    body
}

fn post_bytes(path: &str, body: Body) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap()
}

#[tokio::test]
async fn a_body_up_to_the_limit_is_read() {
    let app = App::start().await;
    let body = padded(castlist_next::BODY_LIMIT);
    let (status, _, answer) = app.send(post_bytes("/api/actors", Body::from(body))).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_with_payload_too_large() {
    let app = App::start().await;
    let body = padded(castlist_next::BODY_LIMIT + 1);
    let (status, headers, answer) = app.send(post_bytes("/api/actors", Body::from(body))).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{answer}");
    let message = refused(&headers, &answer, "PayloadTooLarge");
    assert!(message.contains("2 MiB"), "{message}");
    assert_eq!(app.count("actors").await, 0);
}

/// 長さを前に言わない本文（chunked で送られた物と同じ）。
struct Chunks(VecDeque<Bytes>);

impl http_body::Body for Chunks {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        Poll::Ready(self.0.pop_front().map(|b| Ok(http_body::Frame::data(b))))
    }
}

#[tokio::test]
async fn a_body_of_unknown_length_over_the_limit_is_refused_too() {
    let app = App::start().await;
    let body = Bytes::from(padded(castlist_next::BODY_LIMIT + 1));
    let chunks = Chunks(
        (0..body.len())
            .step_by(64 * 1024)
            .map(|at| body.slice(at..(at + 64 * 1024).min(body.len())))
            .collect(),
    );
    let (status, headers, answer) = app.send(post_bytes("/api/actors", Body::new(chunks))).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{answer}");
    refused(&headers, &answer, "PayloadTooLarge");
    assert_eq!(app.count("actors").await, 0);
}

#[tokio::test]
async fn a_request_over_the_time_limit_is_answered_with_unavailable() {
    // 試験では時間切れを短くし、1 本だけの書く接続を握って書く操作を待たせる（書く接続を待つのは 2 秒。その前に時間切れになる）。
    let app = App::start_with(castlist_next::Config {
        timeout: Duration::from_millis(200),
        ..castlist_next::Config::default()
    })
    .await;
    let held = app.db.write().acquire().await.unwrap();
    let started = Instant::now();
    let (status, headers, body) = app
        .send(common::post("/api/actors", &ai("se", "s-1")))
        .await;
    let took = started.elapsed();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    let message = refused(&headers, &body, "Unavailable");
    assert!(message.contains("時間がかかりすぎた"), "{message}");
    assert!(
        took < Duration::from_secs(1),
        "書く接続の待ち（2 秒）より前に答える: {took:?}"
    );
    drop(held);
    // 鍵を放せば、同じ要求は通る（待っていた物は捨てた）。
    let (status, _, body) = app
        .send(common::post("/api/actors", &ai("se", "s-1")))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(app.count("actors").await, 1);
}

/// 接続を待つ長さは、問い合わせの時間切れより短い。等しいと、混んでいる時に時間切れ（503 の別の訳）が先に答え、
/// 送り手が「混んでいる」と「時間がかかりすぎた」を見分けられない（AI-36 の review-13）。
#[test]
fn waiting_for_a_connection_is_shorter_than_the_request_timeout() {
    assert!(castlist_next::db::WRITE_WAIT < castlist_next::REQUEST_TIMEOUT);
    assert!(castlist_next::db::READ_WAIT < castlist_next::REQUEST_TIMEOUT);
}
