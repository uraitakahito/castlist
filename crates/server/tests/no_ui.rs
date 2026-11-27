//! 画面は castlist に無い（2.0.0 で外した。画面は callboard が持つ）。前の画面の道は、どれも 404。
//! API（`/api/` の下と `/healthz`）と、手の道（backup・logs）は、今までどおり。
mod common;

use axum::http::{StatusCode, header};
use common::{App, ai, get};

#[tokio::test]
async fn the_former_screen_paths_are_404_and_no_answer_is_html() {
    let app = App::start().await;
    let answer = app.register(&ai("se", "s-1")).await;
    let id = answer["actor"]["id"].as_str().unwrap().to_owned();
    for path in [
        "/".to_owned(),
        "/?label=tpm.key:t-23zs".to_owned(),
        format!("/actors/{id}"),
        "/graph/timeline".to_owned(),
        "/graph/network".to_owned(),
        "/static/site.css".to_owned(),
        "/static/network.js".to_owned(),
        "/static/vendor/cytoscape-3.35.0.min.js".to_owned(),
    ] {
        let (status, headers, body) = app.send(get(&path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            !content_type.starts_with("text/html"),
            "{path}: {content_type}"
        );
        // 画面のための頭の印（CSP）は、もう付けない。型の推し量りを止める印は、どの答えにも付く。
        assert!(
            headers.get(header::CONTENT_SECURITY_POLICY).is_none(),
            "{path}"
        );
        assert_eq!(
            headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
            "nosniff",
            "{path}"
        );
    }
}

#[tokio::test]
async fn the_api_still_answers_and_the_row_still_carries_the_profile_path() {
    let app = App::start().await;
    let answer = app.register(&ai("se", "s-1")).await;
    let id = answer["actor"]["id"].as_str().unwrap().to_owned();
    for path in [
        "/healthz".to_owned(),
        format!("/api/actors/{id}"),
        "/api/graph".to_owned(),
        "/api/timeline".to_owned(),
    ] {
        let (status, _, body) = app.send(get(&path)).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
    }
    // 行の url は、プロフィールの画面の path のまま（画面は callboard が、同じ path で出す）。
    let (_, _, body) = app.send(get("/api/actors?label=project:castlist")).await;
    let rows: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(rows["actors"][0]["url"], format!("/actors/{id}"), "{body}");
}
