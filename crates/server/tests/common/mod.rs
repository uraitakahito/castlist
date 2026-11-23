//! 試験の足場: Router に要求を 1 つ通し、状態と本文の字を返す。
#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

pub async fn send(request: Request<Body>) -> (StatusCode, String) {
    let response = castlist_next::app().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

pub fn get(path: &str) -> Request<Body> {
    Request::get(path).header("host", "127.0.0.1:8802").body(Body::empty()).unwrap()
}
