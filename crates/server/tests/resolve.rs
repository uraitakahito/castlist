//! ResolveActors の振る舞い（処理より先に書く）。鍵の付随する id から、出し手の中のアクターを引く。
mod common;

use axum::http::StatusCode;
use common::{App, ai, error_type, human};
use serde_json::{Value, json};

/// 引いて 200 であることを見て、答えの results を返す。
async fn resolve(app: &App, issuer: &str, refs: &[&str]) -> Vec<Value> {
    let (status, _, answer) = app
        .post("/api/resolve", &json!({ "issuer": issuer, "refs": refs }))
        .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(
        answer["asOf"].as_str().is_some_and(|t| t.ends_with('Z')),
        "{answer}"
    );
    answer["results"].as_array().expect("results").clone()
}

/// 形の違う問いが 400 の ValidationException であることを見る。
async fn rejected(app: &App, body: &Value) {
    let (status, headers, answer) = app.post("/api/resolve", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body} {answer}");
    assert_eq!(error_type(&headers), "ValidationException", "{body}");
}

#[tokio::test]
async fn keys_resolve_within_the_issuer_in_the_order_asked() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": "cmux.workspace.ref", "value": "workspace:1" }));
    let se1 = app.register(&body).await["actor"].clone();
    let pm = app.register(&human("pm")).await["actor"].clone();
    let results = resolve(
        &app,
        "tpm",
        &[
            "tpm.person:pm",
            "claude.session:none",
            "claude.session:s-1",
            "cmux.workspace.ref:workspace:1",
            "tpm.person:pm",
        ],
    )
    .await;
    assert_eq!(results.len(), 5);
    assert_eq!(results[0], json!({ "ref": "tpm.person:pm", "actor": pm }));
    // 知らない物と、鍵でない付随する id では引かない（actor が無い）。
    assert_eq!(results[1], json!({ "ref": "claude.session:none" }));
    assert_eq!(
        results[2],
        json!({ "ref": "claude.session:s-1", "actor": se1 })
    );
    assert_eq!(
        results[3],
        json!({ "ref": "cmux.workspace.ref:workspace:1" })
    );
    assert_eq!(results[4], results[0]);
    assert_eq!(se1["name"], "se.1");
    assert_eq!(pm["kind"], "human");
}

#[tokio::test]
async fn another_issuer_does_not_resolve() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let results = resolve(&app, "other", &["claude.session:s-1"]).await;
    assert_eq!(results, vec![json!({ "ref": "claude.session:s-1" })]);
}

#[tokio::test]
async fn a_ref_splits_at_the_first_colon_so_a_value_may_have_colons() {
    let app = App::start().await;
    let mut body = ai("se", "x");
    body["bindings"] = json!([{ "type": "cmux.workspace.ref", "value": "workspace:1:pane:2" }]);
    let se1 = app.register(&body).await["actor"].clone();
    let results = resolve(&app, "tpm", &["cmux.workspace.ref:workspace:1:pane:2"]).await;
    assert_eq!(results[0]["actor"], se1);
    let results = resolve(&app, "tpm", &["cmux.workspace.ref:workspace:1"]).await;
    assert!(results[0].get("actor").is_none(), "{:?}", results);
}

#[tokio::test]
async fn up_to_100_refs_and_a_value_of_256_chars_are_taken() {
    let app = App::start().await;
    let refs: Vec<String> = (0..100).map(|n| format!("claude.session:s-{n}")).collect();
    let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
    assert_eq!(resolve(&app, "tpm", &refs).await.len(), 100);
    let long = format!("claude.session:{}", "あ".repeat(256));
    assert_eq!(resolve(&app, "tpm", &[&long]).await.len(), 1);
    let kind = format!("{}:v", "a".repeat(64));
    assert_eq!(resolve(&app, "tpm", &[&kind]).await.len(), 1);
}

#[tokio::test]
async fn too_many_or_no_refs_are_400() {
    let app = App::start().await;
    let refs: Vec<String> = (0..101).map(|n| format!("claude.session:s-{n}")).collect();
    rejected(&app, &json!({ "issuer": "tpm", "refs": refs })).await;
    rejected(&app, &json!({ "issuer": "tpm", "refs": [] })).await;
    rejected(&app, &json!({ "issuer": "tpm" })).await;
}

#[tokio::test]
async fn one_ref_of_the_wrong_form_rejects_the_whole_request() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    for bad in [
        "colon-less".to_owned(),
        ":s-1".to_owned(),
        "claude.session:".to_owned(),
        "Claude.Session:s-1".to_owned(),
        "claude session:s-1".to_owned(),
        "claude.session:s\u{7}1".to_owned(),
        "claude.session:s\n1".to_owned(),
        "claude.session:s\u{85}1".to_owned(),
        format!("claude.session:{}", "a".repeat(257)),
        format!("{}:v", "a".repeat(65)),
    ] {
        rejected(
            &app,
            &json!({ "issuer": "tpm", "refs": ["claude.session:s-1", bad] }),
        )
        .await;
    }
}

#[tokio::test]
async fn an_issuer_of_the_wrong_form_is_400() {
    let app = App::start().await;
    rejected(
        &app,
        &json!({ "issuer": "TPM", "refs": ["claude.session:s-1"] }),
    )
    .await;
}
