//! 守り（Host・Origin）の振る舞い。生成した道の前で効く。
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::send;

#[tokio::test]
async fn a_foreign_host_is_refused_before_the_generated_service() {
    let request = Request::get("/healthz")
        .header("host", "evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(request).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn localhost_with_any_port_is_accepted() {
    let request = Request::get("/healthz")
        .header("host", "localhost:9999")
        .body(Body::empty())
        .unwrap();
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
