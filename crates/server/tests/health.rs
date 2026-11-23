//! GetHealth の振る舞い（処理より先に書く）。生成した service に本物の handler を入れ、Router を通して要求を 1 つ送る。
mod common;

use axum::http::StatusCode;
use common::{get, send};

#[tokio::test]
async fn health_says_ok_with_the_version_and_the_time_of_castlist() {
    let (status, body) = send(get("/healthz")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    let now = json["now"].as_str().expect(&body);
    assert!(
        now.ends_with('Z') && now.len() >= 20,
        "RFC 3339 の UTC: {now}"
    );
}

#[tokio::test]
async fn an_unknown_path_is_a_json_404_from_the_generated_layer() {
    let (status, body) = send(get("/v1/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, "{}");
}
