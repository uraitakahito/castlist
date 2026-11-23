//! PutLink の振る舞い（処理より先に書く）。試験ごとに一時の DB を作り、Router を通して要求を送り、DB の行を見る。
mod common;

use axum::http::StatusCode;
use common::{App, PM, SE1, SE2, ai, at, error_type, human, link, scalar};
use serde_json::{Value, json};

/// 線を送り、状態と本文を返す。
async fn put(app: &App, id: &str, body: &Value) -> (StatusCode, Value) {
    let (status, _, answer) = app.put(&format!("/api/links/{id}"), body).await;
    (status, answer)
}

/// 送って 400 の ValidationException であることを見て、message を返す。
async fn invalid(app: &App, id: &str, body: &Value) -> String {
    let (status, headers, answer) = app.put(&format!("/api/links/{id}"), body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body} {answer}");
    assert_eq!(error_type(&headers), "ValidationException", "{body}");
    answer["message"].as_str().unwrap_or_default().to_owned()
}

/// 送って 409 の Conflict であることを見る。
async fn conflict(app: &App, id: &str, body: &Value) {
    let (status, headers, answer) = app.put(&format!("/api/links/{id}"), body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body} {answer}");
    assert_eq!(error_type(&headers), "Conflict", "{body}");
    assert!(answer["message"].is_string(), "{answer}");
}

/// pm・se.1・se.2 を登録し、アクターの id を返す。
async fn three(app: &App) -> (String, String, String) {
    let id = |answer: Value| answer["actor"]["id"].as_str().expect("id").to_owned();
    (
        id(app.register(&human("pm")).await),
        id(app.register(&ai("se", "s-1")).await),
        id(app.register(&ai("se", "s-2")).await),
    )
}

#[tokio::test]
async fn a_new_link_is_201_and_returns_the_stored_row_in_the_same_shape_as_get_actor() {
    let app = App::start().await;
    let (pm, se1, _) = three(&app).await;
    let (status, answer) = put(&app, "l-1", &link("launched", PM, SE1)).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["id"], "l-1");
    assert_eq!(answer["type"], "launched");
    assert_eq!(answer["from"], pm.as_str());
    assert_eq!(answer["to"], se1.as_str());
    assert_eq!(answer["clockSkew"], false);
    assert!(
        answer["sentAt"].as_str().unwrap().ends_with('Z'),
        "{answer}"
    );
    assert!(
        answer["receivedAt"].as_str().unwrap().ends_with('Z'),
        "{answer}"
    );
    let (_, _, actor) = app.get(&format!("/api/actors/{se1}")).await;
    assert_eq!(actor["linksIn"], json!([answer]), "{actor}");
    let (_, _, actor) = app.get(&format!("/api/actors/{pm}")).await;
    assert_eq!(actor["linksOut"], json!([answer]), "{actor}");
}

#[tokio::test]
async fn the_same_id_and_the_same_body_again_is_200_with_the_identical_answer() {
    let app = App::start().await;
    three(&app).await;
    let mut body = link("launched", PM, SE1);
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    let (status, first) = put(&app, "l-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    // 起こされるのは 1 度だけだが、同じ線の送り直しは 409 でなく 200。
    let (status, again) = put(&app, "l-1", &body).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, first);
    let mut same = body.clone();
    same["sentAt"] = json!("2099-01-01T00:00:00.000Z");
    let (status, answer) = put(&app, "l-1", &same).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer, first);
    assert_eq!(app.count("links").await, 1);
}

#[tokio::test]
async fn the_same_id_with_a_different_body_is_409_and_the_old_row_stays() {
    let app = App::start().await;
    three(&app).await;
    let mut body = link("launched", PM, SE1);
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    let (_, first) = put(&app, "l-1", &body).await;
    for (field, value) in [
        ("type", json!("continued")),
        ("from", json!(SE2)),
        ("to", json!(SE2)),
        ("sentAt", json!("2099-01-01T00:00:01Z")),
        ("issuer", json!("other")),
    ] {
        let mut other = body.clone();
        other[field] = value;
        conflict(&app, "l-1", &other).await;
    }
    let kind: String = scalar(&app, "select type from links where id = 'l-1'").await;
    assert_eq!(kind, "launched", "前の行はそのまま");
    assert_eq!(app.count("links").await, 1);
    let (_, again) = put(&app, "l-1", &body).await;
    assert_eq!(again, first);
}

/// 同じ id かは、from と to を引くより先に見る。from と to が同じかも、その後に見る。
#[tokio::test]
async fn the_same_id_is_checked_before_resolving_and_before_from_equals_to() {
    let app = App::start().await;
    three(&app).await;
    put(&app, "l-1", &link("launched", PM, SE1)).await;
    conflict(&app, "l-1", &link("launched", "tpm.person:none", SE1)).await;
    conflict(&app, "l-1", &link("launched", PM, "claude.session:none")).await;
    conflict(&app, "l-1", &link("launched", SE1, SE1)).await;
}

#[tokio::test]
async fn from_and_to_are_resolved_by_the_issuers_key_only_and_unknown_is_400() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": "cmux.workspace.ref", "value": "workspace:1" }));
    app.register(&body).await;
    app.register(&human("pm")).await;
    let message = invalid(&app, "l-1", &link("launched", "tpm.person:none", SE1)).await;
    assert!(message.contains("tpm.person:none"), "{message}");
    let message = invalid(&app, "l-2", &link("launched", PM, "claude.session:none")).await;
    assert!(message.contains("claude.session:none"), "{message}");
    invalid(
        &app,
        "l-3",
        &link("launched", PM, "cmux.workspace.ref:workspace:1"),
    )
    .await;
    let mut other = link("launched", PM, SE1);
    other["issuer"] = json!("other");
    invalid(&app, "l-4", &other).await;
    assert_eq!(app.count("links").await, 0);
}

#[tokio::test]
async fn from_and_to_the_same_actor_is_400() {
    let app = App::start().await;
    three(&app).await;
    invalid(&app, "l-1", &link("continued", SE1, SE1)).await;
    assert_eq!(app.count("links").await, 0);
}

#[tokio::test]
async fn an_actor_is_launched_only_once() {
    let app = App::start().await;
    three(&app).await;
    let (status, _) = put(&app, "l-1", &link("launched", PM, SE1)).await;
    assert_eq!(status, StatusCode::CREATED);
    // 別の id でも、2 度目の起こされるは 409。
    conflict(&app, "l-2", &link("launched", SE2, SE1)).await;
    // 後を継ぐ線と、ほかのアクターを起こす線は足せる。
    let (status, _) = put(&app, "l-3", &link("continued", SE2, SE1)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = put(&app, "l-4", &link("launched", PM, SE2)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(app.count("links").await, 3);
}

#[tokio::test]
async fn link_types_are_launched_and_continued_only() {
    let app = App::start().await;
    three(&app).await;
    invalid(&app, "l-1", &link("adopted", PM, SE1)).await;
    assert_eq!(app.count("links").await, 0);
}

#[tokio::test]
async fn received_at_never_goes_backwards() {
    let app = App::start().await;
    let (pm, _, _) = three(&app).await;
    for i in 0..5 {
        app.register(&ai("se", &format!("k-{i}"))).await;
    }
    // 先の時刻の行が既にあっても、次の行はそれより前にならない。
    app.execute(&format!(
        "insert into labels(actor_id, key, value, received_at)
         values('{pm}', 'future', 'x', '2099-01-01T00:00:00.000Z')"
    ))
    .await;
    for (i, offset) in [3600, -3600, 0, 240, -360].into_iter().enumerate() {
        let mut body = link("launched", PM, &format!("claude.session:k-{i}"));
        body["sentAt"] = json!(at(offset));
        let (status, answer) = put(&app, &format!("c-{i}"), &body).await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    // 受けた時刻が 2099 年になるので、今の時刻の送り手はみなずれている。
    let skews: Vec<i64> = app
        .column("select clock_skew from links order by seq")
        .await;
    assert_eq!(skews, vec![1, 1, 1, 1, 1]);
    let stamps: Vec<String> = app
        .column("select received_at from links order by seq")
        .await;
    assert!(
        stamps
            .iter()
            .all(|s| s.as_str() >= "2099-01-01T00:00:00.000Z" && s.len() == 24),
        "{stamps:?}"
    );
    assert!(stamps.windows(2).all(|w| w[0] <= w[1]), "{stamps:?}");
}

#[tokio::test]
async fn clock_skew_is_set_when_sent_at_is_more_than_5_minutes_away() {
    let app = App::start().await;
    app.register(&human("pm")).await;
    for i in 0..5 {
        app.register(&ai("se", &format!("k-{i}"))).await;
    }
    for (i, offset) in [3600, -3600, 0, 240, -360].into_iter().enumerate() {
        let mut body = link("launched", PM, &format!("claude.session:k-{i}"));
        body["sentAt"] = json!(at(offset));
        put(&app, &format!("c-{i}"), &body).await;
    }
    let skews: Vec<i64> = app
        .column("select clock_skew from links order by seq")
        .await;
    assert_eq!(skews, vec![1, 1, 0, 0, 1]);
}

/// DB の誤り（混んでいる以外）は、500 の InternalError で、決まった字を返す（中の誤りの字を外に出さない）。
#[tokio::test]
async fn a_db_error_is_a_500_internal_error_with_a_fixed_message() {
    let app = App::start().await;
    three(&app).await;
    app.execute("alter table links rename to links_gone").await;
    let (status, headers, answer) = app.put("/api/links/l-1", &link("launched", PM, SE1)).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "{answer}");
    assert!(
        !message.contains("links"),
        "中の誤りの字が出ている: {message}"
    );
}
