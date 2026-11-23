//! PutSetting の振る舞い（処理より先に書く）。試験ごとに一時の DB を作り、Router を通して要求を送り、DB の行を見る。
mod common;

use axum::http::StatusCode;
use common::{App, SE1, SE2, ai, at, error_type, scalar, setting};
use serde_json::{Value, json};

/// model と effort を送り、状態と本文を返す。
async fn put(app: &App, id: &str, body: &Value) -> (StatusCode, Value) {
    let (status, _, answer) = app.put(&format!("/api/settings/{id}"), body).await;
    (status, answer)
}

/// 送って 400 の ValidationException であることを見て、message を返す。
async fn invalid(app: &App, id: &str, body: &Value) -> String {
    let (status, headers, answer) = app.put(&format!("/api/settings/{id}"), body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body} {answer}");
    assert_eq!(error_type(&headers), "ValidationException", "{body}");
    answer["message"].as_str().unwrap_or_default().to_owned()
}

/// 送って 409 の Conflict であることを見る。
async fn conflict(app: &App, id: &str, body: &Value) {
    let (status, headers, answer) = app.put(&format!("/api/settings/{id}"), body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body} {answer}");
    assert_eq!(error_type(&headers), "Conflict", "{body}");
    assert!(answer["message"].is_string(), "{answer}");
}

fn id(answer: &Value) -> String {
    answer["actor"]["id"].as_str().expect("id").to_owned()
}

#[tokio::test]
async fn a_new_setting_is_201_and_returns_the_stored_row_in_the_same_shape_as_get_actor() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    let (status, answer) = put(&app, "o-1", &setting(SE1, "claude-opus-5-5", "high")).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["id"], "o-1");
    assert_eq!(answer["actorId"], se1.as_str());
    assert_eq!(answer["model"], "claude-opus-5-5");
    assert_eq!(answer["effort"], "high");
    assert_eq!(answer["source"], "observed");
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
    assert_eq!(actor["settings"], json!([answer]), "{actor}");
    assert_eq!(actor["currentSettings"], answer, "{actor}");
}

#[tokio::test]
async fn the_same_id_and_the_same_body_again_is_200_with_the_identical_answer() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let mut body = setting(SE1, "claude-opus-5-5", "high");
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    let (status, first) = put(&app, "o-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    // 間にほかの model と effort が来て、もう最新でなくても、同じ id の送り直しは 200 で前の行。
    put(&app, "o-2", &setting(SE1, "claude-opus-5-5", "max")).await;
    let (status, again) = put(&app, "o-1", &body).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, first);
    let mut same = body.clone();
    same["sentAt"] = json!("2099-01-01T00:00:00.000Z");
    let (status, answer) = put(&app, "o-1", &same).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer, first);
    assert_eq!(app.count("settings").await, 2);
}

#[tokio::test]
async fn the_same_id_with_a_different_body_is_409_and_the_old_row_stays() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    app.register(&ai("se", "s-2")).await;
    let mut body = setting(SE1, "claude-opus-5-5", "high");
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    let (_, first) = put(&app, "o-1", &body).await;
    for (field, value) in [
        ("model", json!("claude-sonnet-5")),
        ("effort", json!("max")),
        ("source", json!("requested")),
        ("by", json!(SE2)),
        ("sentAt", json!("2099-01-01T00:00:01Z")),
        ("issuer", json!("other")),
    ] {
        let mut other = body.clone();
        other[field] = value;
        conflict(&app, "o-1", &other).await;
    }
    let effort: String = scalar(
        &app,
        "select e.name from settings s join efforts e on e.id = s.effort_id where s.id = 'o-1'",
    )
    .await;
    assert_eq!(effort, "high", "前の行はそのまま");
    assert_eq!(app.count("settings").await, 1);
    let (_, again) = put(&app, "o-1", &body).await;
    assert_eq!(again, first);
}

/// 同じ id かは、by を引くより先に見る。
#[tokio::test]
async fn the_same_id_is_checked_before_by_is_resolved() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    put(&app, "o-1", &setting(SE1, "claude-opus-5-5", "high")).await;
    conflict(
        &app,
        "o-1",
        &setting("claude.session:none", "claude-opus-5-5", "high"),
    )
    .await;
}

/// 新しい id でも、最新と model と effort が同じなら足さず、最新の行を 200 で返す（source は比べない。新しい id は残らない）。
#[tokio::test]
async fn the_same_model_and_effort_as_the_latest_is_200_with_the_latest_row_and_adds_nothing() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["settings"] = json!({ "model": "claude-opus-5-5", "effort": "high" });
    let se1 = id(&app.register(&body).await);
    // 最新は登録の時に頼んだ物（id の無い requested）。
    let (status, answer) = put(&app, "o-1", &setting(SE1, "claude-opus-5-5", "high")).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer.get("id").is_none(), "{answer}");
    assert_eq!(answer["source"], "requested");
    assert_eq!(answer["actorId"], se1.as_str());
    assert_eq!(app.count("settings").await, 1);
    // 違えば足す。
    let (status, added) = put(&app, "o-2", &setting(SE1, "claude-opus-5-5", "max")).await;
    assert_eq!(status, StatusCode::CREATED, "{added}");
    // 頼んだ物（source が違う）でも、最新と同じなら足さず、最新（o-2）を返す。
    let mut requested = setting(SE1, "claude-opus-5-5", "max");
    requested["source"] = json!("requested");
    let (status, answer) = put(&app, "o-3", &requested).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer, added);
    assert_eq!(app.count("settings").await, 2);
    let ids: Vec<Option<String>> = app.column("select id from settings order by seq").await;
    assert_eq!(ids, vec![None, Some("o-2".to_owned())]);
    // 足さなかった id は残らないので、同じ id で違う物を送れば足す。
    let (status, _) = put(&app, "o-3", &setting(SE1, "claude-opus-5-5", "low")).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn unknown_model_and_effort_names_are_added_to_the_lists_once() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    app.register(&ai("se", "s-2")).await;
    assert_eq!(
        app.count("efforts").await,
        5,
        "low・medium・high・xhigh・max"
    );
    let models = app.count("models").await;
    let (status, answer) = put(&app, "u-1", &setting(SE1, "claude-new-9", "ultra")).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(app.count("models").await, models + 1);
    assert_eq!(app.count("efforts").await, 6);
    let (status, _) = put(&app, "u-2", &setting(SE2, "claude-new-9", "ultra")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(app.count("models").await, models + 1, "2 度目は足さない");
    assert_eq!(app.count("efforts").await, 6);
    let pointed: i64 = scalar(
        &app,
        "select count(*) from settings s
         join models m on m.id = s.model_id and m.name = 'claude-new-9'
         join efforts e on e.id = s.effort_id and e.name = 'ultra'",
    )
    .await;
    assert_eq!(pointed, 2);
}

#[tokio::test]
async fn by_is_resolved_by_the_issuers_key_only_and_unknown_is_400() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let message = invalid(
        &app,
        "o-1",
        &setting("claude.session:none", "claude-opus-5-5", "high"),
    )
    .await;
    assert!(message.contains("claude.session:none"), "{message}");
    let mut other = setting(SE1, "claude-opus-5-5", "high");
    other["issuer"] = json!("other");
    invalid(&app, "o-2", &other).await;
    assert_eq!(app.count("settings").await, 0);
    assert_eq!(
        scalar::<i64>(
            &app,
            "select count(*) from models where name = 'claude-opus-5-5'"
        )
        .await,
        0,
        "引けなければ名も足さない"
    );
}

#[tokio::test]
async fn malformed_model_effort_and_source_are_400() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let long = "a".repeat(65);
    for (i, (field, value)) in [
        ("model", "Opus"),
        ("model", ""),
        ("model", long.as_str()),
        ("model", "claude opus"),
        ("model", "claude_opus"),
        ("effort", "ULTRA"),
        ("effort", "高い"),
        ("source", "guessed"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut body = setting(SE1, "claude-opus-5-5", "high");
        body[field] = json!(value);
        invalid(&app, &format!("v-{i}"), &body).await;
    }
    assert_eq!(app.count("settings").await, 0);
    let ok = "a".repeat(64);
    let (status, _) = put(&app, "v-ok", &setting(SE1, &ok, "high")).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn the_latest_is_decided_by_the_order_received_not_by_sent_at() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    for (sid, model, offset) in [
        ("o-1", "model-ahead", 3600),
        ("o-2", "model-behind", -3600),
        ("o-3", "model-now", 0),
    ] {
        let mut body = setting(SE1, model, "high");
        body["sentAt"] = json!(at(offset));
        let (status, _) = put(&app, sid, &body).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (_, _, actor) = app.get(&format!("/api/actors/{se1}")).await;
    assert_eq!(actor["currentSettings"]["model"], "model-now", "{actor}");
    let skews: Vec<i64> = app
        .column("select clock_skew from settings order by seq")
        .await;
    assert_eq!(skews, vec![1, 1, 0]);
}

#[tokio::test]
async fn received_at_never_goes_backwards() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    app.event_at(
        "future",
        &se1,
        "reported_in",
        "{}",
        "2099-01-01T00:00:00.000Z",
    )
    .await;
    for i in 0..3 {
        put(
            &app,
            &format!("r-{i}"),
            &setting(SE1, &format!("m-{i}"), "high"),
        )
        .await;
    }
    let stamps: Vec<String> = app
        .column("select received_at from settings order by seq")
        .await;
    assert_eq!(stamps.len(), 3);
    assert!(
        stamps
            .iter()
            .all(|s| s.as_str() >= "2099-01-01T00:00:00.000Z" && s.len() == 24),
        "{stamps:?}"
    );
    assert!(stamps.windows(2).all(|w| w[0] <= w[1]), "{stamps:?}");
}

/// DB の誤り（混んでいる以外）は、500 の InternalError で、決まった字を返す（中の誤りの字を外に出さない）。
#[tokio::test]
async fn a_db_error_is_a_500_internal_error_with_a_fixed_message() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    app.execute("alter table settings rename to settings_gone")
        .await;
    let (status, headers, answer) = app
        .put(
            "/api/settings/o-1",
            &setting(SE1, "claude-opus-5-5", "high"),
        )
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "{answer}");
    assert!(
        !message.contains("settings"),
        "中の誤りの字が出ている: {message}"
    );
}
