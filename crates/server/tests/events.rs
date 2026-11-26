//! PutEvent の振る舞い（処理より先に書く）。試験ごとに一時の DB を作り、Router を通して要求を送り、DB の行を見る。
mod common;

use axum::http::StatusCode;
use common::{App, SE1, SE2, ai, at, error_type, event, human, scalar};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// 出来事を送り、状態と本文を返す。
async fn put(app: &App, id: &str, body: &Value) -> (StatusCode, Value) {
    let (status, _, answer) = app.put(&format!("/api/events/{id}"), body).await;
    (status, answer)
}

/// 送って 400 の ValidationException であることを見て、message を返す。
async fn invalid(app: &App, id: &str, body: &Value) -> String {
    let (status, headers, answer) = app.put(&format!("/api/events/{id}"), body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body} {answer}");
    assert_eq!(error_type(&headers), "ValidationException", "{body}");
    answer["message"].as_str().unwrap_or_default().to_owned()
}

/// 送って 409 の Conflict であることを見る。
async fn conflict(app: &App, id: &str, body: &Value) {
    let (status, headers, answer) = app.put(&format!("/api/events/{id}"), body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body} {answer}");
    assert_eq!(error_type(&headers), "Conflict", "{body}");
    assert!(answer["message"].is_string(), "{answer}");
}

fn id(answer: &Value) -> String {
    answer["actor"]["id"].as_str().expect("id").to_owned()
}

#[tokio::test]
async fn a_new_event_is_201_and_returns_the_stored_row_in_the_same_shape_as_get_actor() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    let mut body = event(SE1, "entered");
    body["detail"] = json!({ "source": "startup" });
    let (status, answer) = put(&app, "e-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["id"], "e-1");
    assert_eq!(answer["actorId"], se1.as_str());
    assert_eq!(answer["type"], "entered");
    assert_eq!(answer["detail"], json!({ "source": "startup" }));
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
    assert_eq!(actor["events"], json!([answer]), "{actor}");
    assert_eq!(actor["state"]["presence"], "entered", "{actor}");
}

#[tokio::test]
async fn the_same_id_and_the_same_body_again_is_200_with_the_identical_answer() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let mut body = event(SE1, "compacted");
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    body["detail"] = json!({ "trigger": "auto", "n": 1 });
    let (status, first) = put(&app, "e-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let (status, again) = put(&app, "e-1", &body).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, first);
    // 同じ時刻の違う書き方と、detail の鍵の並びの違いは、同じ本文。
    let mut same = body.clone();
    same["sentAt"] = json!("2099-01-01T00:00:00.000Z");
    same["detail"] = serde_json::from_str(r#"{"n":1,"trigger":"auto"}"#).unwrap();
    let (status, answer) = put(&app, "e-1", &same).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer, first);
    assert_eq!(app.count("events").await, 1);
}

#[tokio::test]
async fn no_detail_is_the_same_as_an_empty_object() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let body = event(SE1, "entered");
    let (status, answer) = put(&app, "e-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["detail"], json!({}));
    let mut empty = body.clone();
    empty["detail"] = json!({});
    let (status, again) = put(&app, "e-1", &empty).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, answer);
    let stored: String = scalar(&app, "select detail from events where id = 'e-1'").await;
    assert_eq!(stored, "{}");
}

#[tokio::test]
async fn the_same_id_with_a_different_body_is_409_and_the_old_row_stays() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    app.register(&ai("se", "s-2")).await;
    let mut body = event(SE1, "entered");
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    let (status, first) = put(&app, "e-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    for (field, value) in [
        ("type", json!("exited")),
        ("detail", json!({ "reason": "x" })),
        ("sentAt", json!("2099-01-01T00:00:00.001Z")),
        ("by", json!(SE2)),
        ("issuer", json!("other")),
    ] {
        let mut other = body.clone();
        other[field] = value;
        conflict(&app, "e-1", &other).await;
    }
    assert_eq!(app.count("events").await, 1);
    let (status, again) = put(&app, "e-1", &body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, first, "前の行はそのまま");
}

/// 同じ id かは、by を引くより先に見る（引けない by でも、同じ id なら 409）。
#[tokio::test]
async fn the_same_id_is_checked_before_by_is_resolved() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    put(&app, "e-1", &event(SE1, "entered")).await;
    conflict(&app, "e-1", &event("claude.session:none", "entered")).await;
}

#[tokio::test]
async fn by_is_resolved_by_the_issuers_key_only_and_unknown_is_400() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": "cmux.workspace.ref", "value": "workspace:1" }));
    app.register(&body).await;
    // 知らない物・鍵でない付随する id・ほかの出し手の鍵では引かない。
    let message = invalid(&app, "e-1", &event("claude.session:none", "entered")).await;
    assert!(message.contains("claude.session:none"), "{message}");
    invalid(
        &app,
        "e-2",
        &event("cmux.workspace.ref:workspace:1", "entered"),
    )
    .await;
    let mut other = event(SE1, "entered");
    other["issuer"] = json!("other");
    invalid(&app, "e-3", &other).await;
    assert_eq!(app.count("events").await, 0);
}

#[tokio::test]
async fn the_ten_event_types_are_taken_and_an_eleventh_is_400() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    for (i, kind) in [
        "entered",
        "reentered",
        "forked",
        "compacted",
        "cleared",
        "exited",
        "closed",
        "reported_in",
        "model_unreadable",
        "retracted",
    ]
    .iter()
    .enumerate()
    {
        let (status, answer) = put(&app, &format!("k-{i}"), &event(SE1, kind)).await;
        assert_eq!(status, StatusCode::CREATED, "{kind}: {answer}");
        assert_eq!(answer["type"], *kind);
    }
    invalid(&app, "k-x", &event(SE1, "danced")).await;
    assert_eq!(app.count("events").await, 10);
}

#[tokio::test]
async fn a_detail_that_is_not_a_json_object_is_400() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    for (i, detail) in [json!(["配列"]), json!("字"), json!(1), json!(true)]
        .into_iter()
        .enumerate()
    {
        let mut body = event(SE1, "compacted");
        body["detail"] = detail;
        let message = invalid(&app, &format!("d-{i}"), &body).await;
        assert!(message.contains("detail"), "{message}");
    }
    assert_eq!(app.count("events").await, 0);
}

/// 推しの幕引き（detail.inferred が JSON の true の closed）を PutEvent で送ると、GetActor の状態が推しになる。
#[tokio::test]
async fn an_inferred_close_sent_by_put_event_shows_inferred_in_get_actor() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    let state = async || app.get(&format!("/api/actors/{se1}")).await.2["state"].clone();
    put(&app, "i-1", &event(SE1, "entered")).await;
    let mut close = event(SE1, "closed");
    close["detail"] = json!({ "inferred": true, "reason": "workspace_gone" });
    let (status, answer) = put(&app, "i-2", &close).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let s = state().await;
    assert_eq!(s["presence"], "closed", "{s}");
    assert_eq!(s["present"], false, "{s}");
    assert_eq!(s["inferred"], true, "{s}");
    // 字の "true" は推しにしない。
    put(&app, "i-3", &event(SE1, "reentered")).await;
    let mut close = event(SE1, "closed");
    close["detail"] = json!({ "inferred": "true" });
    put(&app, "i-4", &close).await;
    let s = state().await;
    assert_eq!(s["present"], false, "{s}");
    assert_eq!(s["inferred"], false, "{s}");
}

#[tokio::test]
async fn clock_skew_is_set_when_sent_at_is_more_than_5_minutes_away_and_sent_at_is_kept_in_utc() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    for (i, offset) in [3600, -3600, 0, 240, -360].into_iter().enumerate() {
        let mut body = event(SE1, "reported_in");
        body["sentAt"] = json!(at(offset));
        let (status, answer) = put(&app, &format!("c-{i}"), &body).await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    let skews: Vec<i64> = app
        .column("select clock_skew from events order by seq")
        .await;
    assert_eq!(skews, vec![1, 1, 0, 0, 1]);
    let mut body = event(SE1, "reported_in");
    body["sentAt"] = json!("2099-01-01T08:59:59.500Z");
    put(&app, "c-utc", &body).await;
    let kept: String = scalar(&app, "select sent_at from events where id = 'c-utc'").await;
    assert_eq!(kept, "2099-01-01T08:59:59.5Z");
}

#[tokio::test]
async fn received_at_never_goes_backwards() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    // 先の時刻の行が既にあっても、次の行はそれより前にならない（どの表の行でも）。
    app.event_at(
        "future",
        &se1,
        "reported_in",
        "{}",
        "2099-01-01T00:00:00.000Z",
    )
    .await;
    for i in 0..3 {
        put(&app, &format!("r-{i}"), &event(SE1, "reported_in")).await;
    }
    let stamps: Vec<String> = app
        .column("select received_at from events order by seq")
        .await;
    assert_eq!(stamps.len(), 4);
    assert!(stamps.windows(2).all(|w| w[0] <= w[1]), "{stamps:?}");
    assert!(
        stamps.iter().all(|s| s.len() == 24 && s.ends_with('Z')),
        "{stamps:?}"
    );
}

/// 本文の写し（body_hash）は、docs/src/api.md の「データ」に書いた形の SHA-256。
/// detail は鍵を並べた形で残す。
#[tokio::test]
async fn the_body_hash_is_sha256_of_the_canonical_body() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let body = json!({
        "sentAt": "2099-01-01T00:00:00.500Z",
        "type": "compacted",
        "issuer": "tpm",
        "detail": { "z": { "c": null, "b": true }, "a": 1, "list": [2, "あ\"", -1, 0.5] },
        "by": SE1,
    });
    let (status, answer) = put(&app, "h-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let canonical = concat!(
        r#"{"by":"claude.session:s-1","#,
        r#""detail":{"a":1,"list":[2,"あ\"",-1,0.5],"z":{"b":true,"c":null}},"#,
        r#""issuer":"tpm","sentAt":"2099-01-01T00:00:00.5Z","type":"compacted"}"#
    );
    let want: String = Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let stored: String = scalar(&app, "select body_hash from events where id = 'h-1'").await;
    assert_eq!(stored, want);
    let detail: String = scalar(&app, "select detail from events where id = 'h-1'").await;
    assert_eq!(
        detail,
        r#"{"a":1,"list":[2,"あ\"",-1,0.5],"z":{"b":true,"c":null}}"#
    );
}

#[tokio::test]
async fn malformed_events_are_400_and_write_nothing() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    // id の形（空白の無い印字できる ASCII で 128 字まで）。
    for bad in ["a%20b".to_owned(), "%E3%81%82".to_owned(), "a".repeat(129)] {
        let (status, _, answer) = app
            .put(&format!("/api/events/{bad}"), &event(SE1, "entered"))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {answer}");
    }
    for (field, value) in [
        ("issuer", json!("TPM")),
        ("by", json!("no-colon")),
        ("by", json!("claude.session:")),
    ] {
        let mut body = event(SE1, "entered");
        body[field] = value;
        invalid(&app, "m-1", &body).await;
    }
    let mut no_sent_at = event(SE1, "entered");
    no_sent_at.as_object_mut().unwrap().remove("sentAt");
    let (status, _, answer) = app.put("/api/events/m-2", &no_sent_at).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    let ok = "a".repeat(128);
    let (status, _, answer) = app
        .put(&format!("/api/events/{ok}"), &event(SE1, "entered"))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(app.count("events").await, 1);
}

#[tokio::test]
async fn waiting_too_long_for_the_write_connection_is_503_unavailable() {
    let app = App::start().await;
    app.register(&human("pm")).await;
    app.register(&ai("se", "s-1")).await;
    let held = app.db.write().acquire().await.expect("書く接続");
    let (status, headers, answer) = app.put("/api/events/e-1", &event(SE1, "entered")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(held);
    assert_eq!(app.count("events").await, 0);
}

/// DB の誤り（混んでいる以外）は、500 の InternalError で、決まった字を返す（中の誤りの字を外に出さない）。
#[tokio::test]
async fn a_db_error_is_a_500_internal_error_with_a_fixed_message() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    app.execute("alter table events rename to events_gone")
        .await;
    let (status, headers, answer) = app.put("/api/events/e-1", &event(SE1, "entered")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "{answer}");
    assert!(
        !message.contains("events"),
        "中の誤りの字が出ている: {message}"
    );
}
