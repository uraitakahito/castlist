//! PutUsage・ListUsage（アクターごとのトークンの数と費え）の振る舞い（処理より先に書く）。試験ごとに一時の DB を作り、
//! Router を通して要求を送り、DB の行を見る。受けた時刻を決めたい所だけ DB に直に置く。
mod common;

use axum::http::StatusCode;
use castlist::db::LOOKUPS;
use castlist::search::plans;
use common::sample::{ok, seat};
use common::{App, SE1, SE2, ai, at, error_type, event, scalar};
use serde_json::{Value, json};

/// 区間の時（UTC の 2026-11-24 の 10 時）の内の時刻。
const T1: &str = "2026-11-24T10:05:00Z";
const T2: &str = "2026-11-24T10:55:00Z";
/// 上の時を含む 1 日の窓。
const DAY: &str = "from=2026-11-24T00:00:00Z&to=2026-11-25T00:00:00Z";

/// トークンの数と費えの本文（出し手は tpm・送り手の時刻は今。本体の回なので agent は無い）。
fn usage(by: &str, from_at: &str, to_at: &str) -> Value {
    json!({
        "issuer": "tpm",
        "by": by,
        "model": "claude-opus-5-5",
        "speed": "standard",
        "serviceTier": "standard",
        "fromAt": from_at,
        "toAt": to_at,
        "turns": 3,
        "inputTokens": 30,
        "outputTokens": 1200,
        "cacheReadTokens": 90000,
        "cacheWrite5mTokens": 0,
        "cacheWrite1hTokens": 4000,
        "webSearchRequests": 2,
        "webFetchRequests": 5,
        "costMicroUsd": 250_000,
        "maxContext": 61000,
        "buckets": [
            { "bucket": 1, "turns": 1, "costMicroUsd": 50000 },
            { "bucket": 2, "turns": 2, "costMicroUsd": 200_000 },
        ],
        "sentAt": at(0),
    })
}

/// 送り、状態と本文を返す。
async fn put(app: &App, id: &str, body: &Value) -> (StatusCode, Value) {
    let (status, _, answer) = app.put(&format!("/api/usage/{id}"), body).await;
    (status, answer)
}

/// 送って 201 であることを見て、答えを返す。
async fn created(app: &App, id: &str, body: &Value) -> Value {
    let (status, answer) = put(app, id, body).await;
    assert_eq!(status, StatusCode::CREATED, "{id}: {body} {answer}");
    answer
}

/// 送って 400 の ValidationException であることを見て、message を返す。
async fn invalid(app: &App, id: &str, body: &Value) -> String {
    let (status, headers, answer) = app.put(&format!("/api/usage/{id}"), body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body} {answer}");
    assert_eq!(error_type(&headers), "ValidationException", "{body}");
    answer["message"].as_str().unwrap_or_default().to_owned()
}

/// 送って 409 の Conflict であることを見る。
async fn conflict(app: &App, id: &str, body: &Value) {
    let (status, headers, answer) = app.put(&format!("/api/usage/{id}"), body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body} {answer}");
    assert_eq!(error_type(&headers), "Conflict", "{body}");
    assert!(answer["message"].is_string(), "{answer}");
}

/// 読んで 400 の ValidationException であることを見て、message を返す。
async fn invalid_query(app: &App, uri: &str) -> String {
    let (status, headers, answer) = app.get(uri).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    assert_eq!(error_type(&headers), "ValidationException", "{uri}");
    answer["message"].as_str().unwrap_or_default().to_owned()
}

fn id(answer: &Value) -> String {
    answer["actor"]["id"].as_str().expect("id").to_owned()
}

/// 答えの行の id（受けた順）。
fn ids(answer: &Value) -> Vec<&str> {
    answer["usage"]
        .as_array()
        .unwrap_or_else(|| panic!("usage が配列でない: {answer}"))
        .iter()
        .map(|row| row["id"].as_str().unwrap_or("?"))
        .collect()
}

// --- PutUsage ---

#[tokio::test]
async fn a_new_usage_is_201_and_the_stored_row_reads_back() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    let (status, answer) = put(&app, "u-1", &usage(SE1, T1, T2)).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["id"], "u-1");
    assert_eq!(answer["actorId"], se1.as_str());
    assert_eq!(answer["model"], "claude-opus-5-5");
    assert!(
        answer.get("agent").is_none(),
        "本体の回には agent が無い: {answer}"
    );
    assert_eq!(answer["speed"], "standard");
    assert_eq!(answer["serviceTier"], "standard");
    assert_eq!(answer["fromAt"], T1);
    assert_eq!(answer["toAt"], T2);
    assert_eq!(answer["turns"], 3);
    assert_eq!(answer["inputTokens"], 30);
    assert_eq!(answer["outputTokens"], 1200);
    assert_eq!(answer["cacheReadTokens"], 90000);
    assert_eq!(answer["cacheWrite5mTokens"], 0);
    assert_eq!(answer["cacheWrite1hTokens"], 4000);
    assert_eq!(answer["webSearchRequests"], 2);
    assert_eq!(answer["webFetchRequests"], 5);
    assert_eq!(answer["costMicroUsd"], 250_000);
    assert_eq!(answer["maxContext"], 61000);
    assert_eq!(
        answer["buckets"],
        json!([
            { "bucket": 1, "turns": 1, "costMicroUsd": 50000 },
            { "bucket": 2, "turns": 2, "costMicroUsd": 200_000 },
        ])
    );
    assert_eq!(answer["clockSkew"], false);
    assert!(
        answer["sentAt"].as_str().unwrap().ends_with('Z'),
        "{answer}"
    );
    assert!(
        answer["receivedAt"].as_str().unwrap().ends_with('Z'),
        "{answer}"
    );
    // 読む口の行は、書く口の答えと同じ形。
    let listed = ok(&app, &format!("/api/usage?{DAY}")).await;
    assert_eq!(listed["usage"], json!([answer]), "{listed}");
    assert_eq!(app.count("usage").await, 1);
}

#[tokio::test]
async fn the_agent_of_a_subagent_turn_is_kept_and_big_numbers_survive() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let mut body = usage(SE1, T1, T2);
    body["agent"] = json!("Explore");
    body["speed"] = json!("fast");
    body["serviceTier"] = json!("priority");
    // 32 bit に収まらない数（Long）。段は両端（0 と 40）。
    body["cacheReadTokens"] = json!(9_000_000_000_i64);
    body["costMicroUsd"] = json!(5_000_000_000_i64);
    body["webSearchRequests"] = json!(0);
    body["webFetchRequests"] = json!(6_000_000_000_i64);
    body["maxContext"] = json!(1_000_000);
    body["buckets"] = json!([
        { "bucket": 0, "turns": 2, "costMicroUsd": 1 },
        { "bucket": 40, "turns": 1, "costMicroUsd": 4_999_999_999_i64 },
    ]);
    let answer = created(&app, "u-1", &body).await;
    assert_eq!(answer["agent"], "Explore");
    assert_eq!(answer["speed"], "fast");
    assert_eq!(answer["serviceTier"], "priority");
    assert_eq!(answer["cacheReadTokens"], 9_000_000_000_i64);
    assert_eq!(answer["costMicroUsd"], 5_000_000_000_i64);
    assert_eq!(answer["webSearchRequests"], 0);
    assert_eq!(answer["webFetchRequests"], 6_000_000_000_i64);
    assert_eq!(answer["buckets"], body["buckets"]);
    let listed = ok(&app, &format!("/api/usage?{DAY}")).await;
    assert_eq!(listed["usage"], json!([answer]), "{listed}");
}

#[tokio::test]
async fn the_same_id_and_the_same_body_again_is_200_and_the_row_stays_one() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let mut body = usage(SE1, T1, T2);
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    let (status, first) = put(&app, "u-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let (status, again) = put(&app, "u-1", &body).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, first);
    // 同じ時刻の違う書き方と、段の並びの違いは、同じ本文。
    let mut same = body.clone();
    same["sentAt"] = json!("2099-01-01T00:00:00.000Z");
    same["fromAt"] = json!("2026-11-24T10:05:00.000Z");
    same["buckets"] = json!([
        { "bucket": 2, "turns": 2, "costMicroUsd": 200_000 },
        { "bucket": 1, "turns": 1, "costMicroUsd": 50000 },
    ]);
    let (status, answer) = put(&app, "u-1", &same).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer, first);
    assert_eq!(app.count("usage").await, 1);
}

#[tokio::test]
async fn the_same_id_with_a_different_body_is_409_and_the_old_row_stays() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    app.register(&ai("se", "s-2")).await;
    let mut body = usage(SE1, T1, T2);
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    let (_, first) = put(&app, "u-1", &body).await;
    for (field, value) in [
        ("model", json!("claude-sonnet-5")),
        ("agent", json!("Explore")),
        ("speed", json!("fast")),
        ("serviceTier", json!("priority")),
        ("by", json!(SE2)),
        ("fromAt", json!("2026-11-24T10:05:01Z")),
        ("toAt", json!("2026-11-24T10:55:01Z")),
        ("inputTokens", json!(31)),
        ("outputTokens", json!(1201)),
        ("cacheReadTokens", json!(90001)),
        ("cacheWrite5mTokens", json!(1)),
        ("cacheWrite1hTokens", json!(4001)),
        ("webSearchRequests", json!(3)),
        ("webFetchRequests", json!(6)),
        ("maxContext", json!(61001)),
        (
            "buckets",
            json!([{ "bucket": 3, "turns": 3, "costMicroUsd": 250_000 }]),
        ),
        ("sentAt", json!("2099-01-01T00:00:01Z")),
        ("issuer", json!("other")),
    ] {
        let mut other = body.clone();
        other[field] = value;
        conflict(&app, "u-1", &other).await;
    }
    let input: i64 = scalar(&app, "select input_tokens from usage where id = 'u-1'").await;
    assert_eq!(input, 30, "前の行はそのまま");
    assert_eq!(app.count("usage").await, 1);
    let (status, again) = put(&app, "u-1", &body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, first);
}

/// 同じ id かは、by を引くより先に見る。
#[tokio::test]
async fn the_same_id_is_checked_before_by_is_resolved() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    created(&app, "u-1", &usage(SE1, T1, T2)).await;
    conflict(&app, "u-1", &usage("claude.session:none", T1, T2)).await;
}

/// PutSetting と同じ: by は出し手の中の鍵だけで引き、引けなければ 400。
#[tokio::test]
async fn by_is_resolved_by_the_issuers_key_only_and_unknown_is_400() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let message = invalid(&app, "u-1", &usage("claude.session:none", T1, T2)).await;
    assert!(message.contains("claude.session:none"), "{message}");
    let mut other = usage(SE1, T1, T2);
    other["issuer"] = json!("other");
    invalid(&app, "u-2", &other).await;
    assert_eq!(app.count("usage").await, 0);
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
async fn an_unknown_model_name_is_added_to_the_list_once() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let models = app.count("models").await;
    for (i, by) in [SE1, SE1].into_iter().enumerate() {
        let mut body = usage(by, T1, T2);
        body["model"] = json!("claude-new-9");
        created(&app, &format!("u-{i}"), &body).await;
    }
    assert_eq!(app.count("models").await, models + 1, "2 度目は足さない");
    let pointed: i64 = scalar(
        &app,
        "select count(*) from usage u join models m on m.id = u.model_id and m.name = 'claude-new-9'",
    )
    .await;
    assert_eq!(pointed, 2);
}

/// 同じアクター・同じ時・同じ種類の行は、2 つ以上在りうる（送り手が、増えた分を新しい行で送る）。
/// PutSetting の「最新と同じなら足さない」は、ここには無い。
#[tokio::test]
async fn two_rows_of_the_same_actor_hour_and_kind_are_both_kept_and_returned() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let mut body = usage(SE1, T1, T2);
    body["sentAt"] = json!("2099-01-01T00:00:00Z");
    let first = created(&app, "u-1", &body).await;
    // id のほかは、1 字も違わない本文。
    let second = created(&app, "u-2", &body).await;
    assert_eq!(second["id"], "u-2");
    assert_eq!(app.count("usage").await, 2);
    let listed = ok(&app, &format!("/api/usage?{DAY}")).await;
    assert_eq!(listed["usage"], json!([first, second]), "{listed}");
}

#[tokio::test]
async fn bodies_that_break_the_rules_of_a_row_are_400() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let with = |changes: &[(&str, Value)]| {
        let mut body = usage(SE1, T1, T2);
        for (field, value) in changes {
            body[*field] = value.clone();
        }
        body
    };
    // 区間: 同じ UTC の時の内で、始めが終わりより後でない。
    let message = invalid(
        &app,
        "v-hour",
        &with(&[("toAt", json!("2026-11-24T11:00:00Z"))]),
    )
    .await;
    assert!(message.contains("fromAt"), "{message}");
    invalid(
        &app,
        "v-day",
        &with(&[
            ("fromAt", json!("2026-11-23T10:05:00Z")),
            ("toAt", json!("2026-11-24T10:05:00Z")),
        ]),
    )
    .await;
    let message = invalid(
        &app,
        "v-order",
        &with(&[("fromAt", json!(T2)), ("toAt", json!(T1))]),
    )
    .await;
    assert!(message.contains("fromAt"), "{message}");
    // 回の数は 1 以上。
    invalid(
        &app,
        "v-turns-0",
        &with(&[("turns", json!(0)), ("buckets", json!([]))]),
    )
    .await;
    invalid(
        &app,
        "v-turns-0b",
        &with(&[
            ("turns", json!(0)),
            ("costMicroUsd", json!(0)),
            (
                "buckets",
                json!([{ "bucket": 0, "turns": 0, "costMicroUsd": 0 }]),
            ),
        ]),
    )
    .await;
    // 負の数。
    for field in [
        "turns",
        "inputTokens",
        "outputTokens",
        "cacheReadTokens",
        "cacheWrite5mTokens",
        "cacheWrite1hTokens",
        "webSearchRequests",
        "webFetchRequests",
        "costMicroUsd",
        "maxContext",
    ] {
        invalid(
            &app,
            &format!("v-neg-{field}"),
            &with(&[(field, json!(-1))]),
        )
        .await;
    }
    for (i, bucket) in [
        json!({ "bucket": 1, "turns": -1, "costMicroUsd": 250_000 }),
        json!({ "bucket": 1, "turns": 3, "costMicroUsd": -1 }),
        // 段は 0〜40。
        json!({ "bucket": 41, "turns": 3, "costMicroUsd": 250_000 }),
        json!({ "bucket": -1, "turns": 3, "costMicroUsd": 250_000 }),
    ]
    .into_iter()
    .enumerate()
    {
        invalid(
            &app,
            &format!("v-bucket-{i}"),
            &with(&[("buckets", json!([bucket]))]),
        )
        .await;
    }
    // 同じ段は 1 度だけ。
    let message = invalid(
        &app,
        "v-twice",
        &with(&[(
            "buckets",
            json!([
                { "bucket": 1, "turns": 1, "costMicroUsd": 50000 },
                { "bucket": 1, "turns": 2, "costMicroUsd": 200_000 },
            ]),
        )]),
    )
    .await;
    assert!(message.contains("bucket"), "{message}");
    // 段の和が、行の数と合う。
    let message = invalid(&app, "v-sum-turns", &with(&[("turns", json!(4))])).await;
    assert!(message.contains("turns"), "{message}");
    let message = invalid(
        &app,
        "v-sum-cost",
        &with(&[("costMicroUsd", json!(250_001))]),
    )
    .await;
    assert!(message.contains("costMicroUsd"), "{message}");
    // 要る欄が無い・形の違う名。
    for field in [
        "model",
        "speed",
        "serviceTier",
        "fromAt",
        "toAt",
        "buckets",
        "webSearchRequests",
        "webFetchRequests",
    ] {
        let mut body = usage(SE1, T1, T2);
        body.as_object_mut().unwrap().remove(field);
        invalid(&app, &format!("v-missing-{field}"), &body).await;
    }
    invalid(&app, "v-model", &with(&[("model", json!("Opus 5"))])).await;
    invalid(&app, "v-agent", &with(&[("agent", json!(""))])).await;
    assert_eq!(app.count("usage").await, 0);
    // 境: 始めと終わりが同じ時刻・時の初めと終わりは受ける。
    created(
        &app,
        "v-ok-1",
        &with(&[("fromAt", json!(T1)), ("toAt", json!(T1))]),
    )
    .await;
    created(
        &app,
        "v-ok-2",
        &with(&[
            ("fromAt", json!("2026-11-24T10:00:00Z")),
            ("toAt", json!("2026-11-24T10:59:59.999Z")),
        ]),
    )
    .await;
}

/// 受けた時刻は、ほかの表の最新より前にならず、ほかの表も、この表の最新より前にならない。
#[tokio::test]
async fn received_at_never_goes_backwards_across_the_tables() {
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
    let answer = created(&app, "u-1", &usage(SE1, T1, T2)).await;
    let received: String = scalar(&app, "select received_at from usage where id = 'u-1'").await;
    assert!(
        received.as_str() >= "2099-01-01T00:00:00.000Z" && received.len() == 24,
        "{received}"
    );
    // 送り手の時刻（今）とは 5 分より離れているので、時計のずれの印が立つ。
    assert_eq!(answer["clockSkew"], true, "{answer}");
    // この表の最新が先の時刻なら、後の出来事も、それより前にならない。
    app.usage_at(
        "u-far",
        &se1,
        "2026-11-24T10:00:00.000Z",
        "2199-01-01T00:00:00.000Z",
    )
    .await;
    let (status, _, answer) = app.put("/api/events/e-1", &event(SE1, "reported_in")).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let received: String = scalar(&app, "select received_at from events where id = 'e-1'").await;
    assert!(
        received.as_str() >= "2199-01-01T00:00:00.000Z",
        "{received}"
    );
}

/// 500 の InternalError で、中の誤りの字（`leak` を含む物）を出さないことを見る。
fn internal_error(
    (status, headers, answer): (StatusCode, axum::http::HeaderMap, Value),
    leak: &str,
) {
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "{answer}");
    assert!(!message.contains(leak), "中の誤りの字が出ている: {message}");
}

/// 答えの形に直せない行と、DB の誤り（混んでいる以外）は、500 の InternalError で、決まった字を返す。
#[tokio::test]
async fn a_row_that_cannot_be_converted_and_a_db_error_are_500_with_a_fixed_message() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    // 受けた時刻が時刻でない行を、直に置く。
    app.usage_at("u-bad", &se1, "2026-11-24T10:00:00.000Z", "not-a-time")
        .await;
    internal_error(app.get(&format!("/api/usage?{DAY}")).await, "not-a-time");
    app.execute("alter table usage rename to usage_gone").await;
    internal_error(
        app.put("/api/usage/u-1", &usage(SE1, T1, T2)).await,
        "usage",
    );
    internal_error(app.get(&format!("/api/usage?{DAY}")).await, "usage");
}

// --- ListUsage ---

/// 窓は区間の終わりの時刻（toAt）で切る: `from <= toAt < to`。
#[tokio::test]
async fn the_window_includes_from_and_excludes_to() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    for (uid, to_at) in [
        ("u-before", "2026-11-24T09:59:59.999Z"),
        ("u-from", "2026-11-24T10:00:00Z"),
        ("u-last", "2026-11-24T11:59:59.999Z"),
        ("u-to", "2026-11-24T12:00:00Z"),
    ] {
        created(&app, uid, &usage(SE1, to_at, to_at)).await;
    }
    let answer = ok(
        &app,
        "/api/usage?from=2026-11-24T10:00:00Z&to=2026-11-24T12:00:00Z",
    )
    .await;
    assert_eq!(ids(&answer), ["u-from", "u-last"], "{answer}");
    assert!(answer.get("nextToken").is_none(), "{answer}");
    // 隣り合う窓は、重ならず、抜けも無い。
    let answer = ok(
        &app,
        "/api/usage?from=2026-11-24T12:00:00Z&to=2026-11-24T13:00:00Z",
    )
    .await;
    assert_eq!(ids(&answer), ["u-to"], "{answer}");
    // 窓は、受けた時刻でも送り手の時刻でもなく、区間の終わりで切る（どの行も、今受けた）。
    let answer = ok(
        &app,
        &format!("/api/usage?from={}&to={}", at(-3600), at(3600)),
    )
    .await;
    assert!(ids(&answer).is_empty(), "{answer}");
    // 空の窓は空の答え。
    let answer = ok(
        &app,
        "/api/usage?from=2026-11-24T10:00:00Z&to=2026-11-24T10:00:00Z",
    )
    .await;
    assert!(ids(&answer).is_empty(), "{answer}");
    assert_eq!(answer["actors"], json!([]), "{answer}");
}

#[tokio::test]
async fn from_and_to_are_required_and_the_window_is_31_days_at_most() {
    let app = App::start().await;
    for uri in [
        "/api/usage",
        "/api/usage?from=2026-11-24T00:00:00Z",
        "/api/usage?to=2026-11-25T00:00:00Z",
    ] {
        invalid_query(&app, uri).await;
    }
    // ちょうど 31 日は受け、1 ミリ秒でも越えれば断る。
    ok(
        &app,
        "/api/usage?from=2026-11-01T00:00:00Z&to=2026-12-02T00:00:00Z",
    )
    .await;
    let message = invalid_query(
        &app,
        "/api/usage?from=2026-11-01T00:00:00Z&to=2026-12-02T00:00:00.001Z",
    )
    .await;
    assert!(message.contains("31"), "{message}");
    // 形の違う値。
    for uri in [
        format!("/api/usage?{DAY}&maxResults=0"),
        format!("/api/usage?{DAY}&maxResults=1001"),
        format!("/api/usage?{DAY}&actorId=se.1"),
        format!("/api/usage?{DAY}&label=project"),
        "/api/usage?from=noon&to=2026-11-25T00:00:00Z".to_owned(),
    ] {
        let (status, _, answer) = app.get(&uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    }
}

/// 見本: 3 つの席（a と b は software-engineer、c は auditor）が、同じ時に 2・1・1 行。受けた順は a・b・a・c。
async fn seats() -> (App, [String; 3]) {
    let app = App::start().await;
    let a = id(&app
        .register(&seat("se", "s-a", "software-engineer", "workspace:1"))
        .await);
    let b = id(&app
        .register(&seat("se", "s-b", "software-engineer", "workspace:2"))
        .await);
    let c = id(&app
        .register(&seat("auditor", "s-c", "auditor", "workspace:3"))
        .await);
    for (uid, session) in [
        ("u-a1", "s-a"),
        ("u-b1", "s-b"),
        ("u-a2", "s-a"),
        ("u-c1", "s-c"),
    ] {
        created(
            &app,
            uid,
            &usage(&format!("claude.session:{session}"), T1, T2),
        )
        .await;
    }
    (app, [a, b, c])
}

#[tokio::test]
async fn rows_are_narrowed_by_labels_and_by_the_actor() {
    let (app, [a, b, c]) = seats().await;
    let all = ok(&app, &format!("/api/usage?{DAY}")).await;
    assert_eq!(ids(&all), ["u-a1", "u-b1", "u-a2", "u-c1"], "{all}");
    // ラベルは ListActors の label と同じ書き方（鍵:値）で、重ねれば、どれも持つアクターの行。
    let answer = ok(
        &app,
        &format!("/api/usage?{DAY}&label=tpm.role:software-engineer"),
    )
    .await;
    assert_eq!(ids(&answer), ["u-a1", "u-b1", "u-a2"], "{answer}");
    let answer = ok(
        &app,
        &format!("/api/usage?{DAY}&label=project:castlist&label=tpm.role:auditor"),
    )
    .await;
    assert_eq!(ids(&answer), ["u-c1"], "{answer}");
    assert_eq!(answer["usage"][0]["actorId"], c.as_str());
    let answer = ok(&app, &format!("/api/usage?{DAY}&label=tpm.role:devops")).await;
    assert!(ids(&answer).is_empty(), "{answer}");
    // アクターの id。
    let answer = ok(&app, &format!("/api/usage?{DAY}&actorId={a}")).await;
    assert_eq!(ids(&answer), ["u-a1", "u-a2"], "{answer}");
    // どちらも付ければ AND。
    let answer = ok(
        &app,
        &format!("/api/usage?{DAY}&actorId={b}&label=tpm.role:software-engineer"),
    )
    .await;
    assert_eq!(ids(&answer), ["u-b1"], "{answer}");
    let answer = ok(
        &app,
        &format!("/api/usage?{DAY}&actorId={b}&label=tpm.role:auditor"),
    )
    .await;
    assert!(ids(&answer).is_empty(), "{answer}");
    // 形は正しいが、どのアクターでもない id は、空の答え。
    let answer = ok(
        &app,
        &format!("/api/usage?{DAY}&actorId=019abcde-0000-7000-8000-00000000ffff"),
    )
    .await;
    assert!(ids(&answer).is_empty(), "{answer}");
}

/// 答えの actors は、その頁に出てきたアクターを 1 度ずつ（行に初めて出た順。id・名・ラベル）。
#[tokio::test]
async fn the_answer_has_each_actor_of_the_page_once() {
    let (app, [a, b, c]) = seats().await;
    let all = ok(&app, &format!("/api/usage?{DAY}")).await;
    let labels =
        |role: &str| json!({ "project": "castlist", "tpm.key": "t-23zs", "tpm.role": role });
    assert_eq!(
        all["actors"],
        json!([
            { "id": a, "name": "se.1", "labels": labels("software-engineer") },
            { "id": b, "name": "se.2", "labels": labels("software-engineer") },
            { "id": c, "name": "auditor.1", "labels": labels("auditor") },
        ]),
        "{all}"
    );
    // 頁に出てこないアクターは入らない。
    let page = ok(&app, &format!("/api/usage?{DAY}&maxResults=1")).await;
    assert_eq!(ids(&page), ["u-a1"]);
    assert_eq!(
        page["actors"],
        json!([{ "id": a, "name": "se.1", "labels": labels("software-engineer") }]),
        "{page}"
    );
    let answer = ok(&app, &format!("/api/usage?{DAY}&actorId={a}")).await;
    assert_eq!(
        answer["actors"].as_array().map(Vec::len),
        Some(1),
        "{answer}"
    );
}

#[tokio::test]
async fn max_results_and_next_token_walk_the_pages_in_the_order_received() {
    let (app, _) = seats().await;
    let path = format!("/api/usage?{DAY}&maxResults=3");
    let first = ok(&app, &path).await;
    assert_eq!(ids(&first), ["u-a1", "u-b1", "u-a2"], "{first}");
    let token = first["nextToken"].as_str().expect("nextToken").to_owned();
    // 辿る間に足された行は、後ろに続く。
    created(&app, "u-late", &usage("claude.session:s-b", T1, T2)).await;
    let second = ok(&app, &format!("{path}&nextToken={token}")).await;
    assert_eq!(ids(&second), ["u-c1", "u-late"], "{second}");
    assert!(second.get("nextToken").is_none(), "{second}");
    // ちょうど 1 頁で終わる時は、nextToken が無い。
    let exact = ok(&app, &format!("/api/usage?{DAY}&maxResults=5")).await;
    assert_eq!(ids(&exact).len(), 5);
    assert!(exact.get("nextToken").is_none(), "{exact}");
    // 崩れた印と、ほかの検索の印は 400。
    app.put("/api/events/e-1", &event("claude.session:s-a", "entered"))
        .await;
    app.put("/api/events/e-2", &event("claude.session:s-a", "exited"))
        .await;
    let events = ok(&app, "/api/events?maxResults=1").await;
    let events = events["nextToken"].as_str().expect("nextToken");
    for uri in [
        format!("/api/usage?{DAY}&nextToken={events}"),
        format!("/api/usage?{DAY}&nextToken=garbage"),
    ] {
        let message = invalid_query(&app, &uri).await;
        assert!(message.contains("nextToken"), "{uri}: {message}");
    }
    let message = invalid_query(&app, &format!("/api/events?nextToken={token}")).await;
    assert!(message.contains("nextToken"), "{message}");
}

#[tokio::test]
async fn max_results_defaults_to_100() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    let now = castlist::db::now();
    for i in 0..101 {
        app.usage_at(&format!("u-{i:03}"), &se1, "2026-11-24T10:00:00.000Z", &now)
            .await;
    }
    let answer = ok(&app, &format!("/api/usage?{DAY}")).await;
    assert_eq!(ids(&answer).len(), 100, "{answer}");
    let token = answer["nextToken"].as_str().expect("nextToken");
    let rest = ok(&app, &format!("/api/usage?{DAY}&nextToken={token}")).await;
    assert_eq!(ids(&rest), ["u-100"], "{rest}");
}

/// 軽い読みの接続（ListSettings・ListEvents と同じ側）が塞がっていれば 503、重い読みの接続が塞がっていても答える。
#[tokio::test]
async fn the_list_uses_the_light_read_connections() {
    let app = App::start().await;
    let uri = format!("/api/usage?{DAY}");
    let mut heavy = Vec::new();
    for _ in 0..castlist::db::HEAVY_READS {
        heavy.push(app.db.heavy().acquire().await.expect("重い読みの接続"));
    }
    ok(&app, &uri).await;
    drop(heavy);
    let mut held = Vec::new();
    for _ in 0..castlist::db::READS {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    let (status, headers, answer) = app.get(&uri).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(held);
    ok(&app, &uri).await;
}

// --- DB の決まり（足すだけ・外のキー・trigger の一覧は tests/db.rs、移行の file は tests/migrations.rs） ---

/// usage の表の列（seq のほか）。
const COLUMNS: [&str; 23] = [
    "id",
    "actor_id",
    "model_id",
    "agent",
    "speed",
    "service_tier",
    "from_at",
    "to_at",
    "turns",
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_write_5m_tokens",
    "cache_write_1h_tokens",
    "web_search_requests",
    "web_fetch_requests",
    "cost_micro_usd",
    "max_context",
    "buckets",
    "sent_at",
    "received_at",
    "clock_skew",
    "body_hash",
];

/// 表の CHECK: 数は 0 以上（回の数は 1 以上）、段の並びは JSON の配列。
#[tokio::test]
async fn the_table_refuses_negative_numbers_and_buckets_that_are_not_a_json_array() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    app.usage_at(
        "u-ok",
        &se1,
        "2026-11-24T10:00:00.000Z",
        &castlist::db::now(),
    )
    .await;
    // u-ok の行の写しを、1 つの列だけ替えて足す SQL（id は u-bad。`column` が id なら、id も替えた値にする）。
    let insert = |column: &str, value: &str| {
        let values: Vec<&str> = COLUMNS
            .iter()
            .map(|c| match *c {
                c if c == column => value,
                "id" => "'u-bad'",
                c => c,
            })
            .collect();
        format!(
            "insert into usage({}) select {} from usage where id = 'u-ok'",
            COLUMNS.join(", "),
            values.join(", ")
        )
    };
    for (column, value) in [
        ("turns", "0"),
        ("turns", "-1"),
        ("input_tokens", "-1"),
        ("output_tokens", "-1"),
        ("cache_read_tokens", "-1"),
        ("cache_write_5m_tokens", "-1"),
        ("cache_write_1h_tokens", "-1"),
        ("web_search_requests", "-1"),
        ("web_fetch_requests", "-1"),
        ("cost_micro_usd", "-1"),
        ("max_context", "-1"),
        ("buckets", "'{}'"),
        ("buckets", "'not json'"),
        ("agent", "''"),
        ("clock_skew", "2"),
    ] {
        let sql = insert(column, value);
        let refused = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .execute(app.db.write())
            .await
            .expect_err(&format!("断らない: {column} = {value}\n{sql}"));
        assert!(
            refused.to_string().contains("CHECK"),
            "{column} = {value}: {refused}"
        );
    }
    // 同じ id は 2 度足せない。
    let refused = sqlx::query(sqlx::AssertSqlSafe(insert("id", "id")))
        .execute(app.db.write())
        .await
        .expect_err("同じ id を足せた");
    assert!(refused.to_string().contains("UNIQUE"), "{refused}");
    assert_eq!(app.count("usage").await, 1);
}

// --- 索引 ---

/// 書く口と読む口の SQL は、索引で引く（大きな表を端から読まない）。
#[tokio::test]
async fn the_usage_sql_uses_an_index_and_does_not_scan_a_big_table() {
    let (app, _) = seats().await;
    let mut sqls: Vec<(&str, String)> = LOOKUPS
        .iter()
        .filter(|(name, _)| name.starts_with("usage"))
        .map(|(name, sql)| (*name, (*sql).to_owned()))
        .collect();
    sqls.extend(
        plans()
            .into_iter()
            .filter(|(name, _)| name.starts_with("usage")),
    );
    let names: Vec<&str> = sqls.iter().map(|(name, _)| *name).collect();
    // 名と、その SQL が使うはずの索引（無ければ、端から読まないことだけを見る）。
    let want = [
        ("usage", Some("sqlite_autoindex_usage_1")),
        ("usage_hash", Some("sqlite_autoindex_usage_1")),
        ("usage_window", Some("usage_to")),
        ("usage_actor", None),
        // ラベルで絞る時も、窓の索引で引く（広いラベルのアクターの行を、初めから全部辿らない）。
        ("usage_labels", Some("usage_to")),
        ("usage_all", None),
        ("usage_actors", None),
    ];
    for (name, _) in want {
        assert!(names.contains(&name), "{name} が無い: {names:?}");
    }
    for (name, sql) in &sqls {
        let plan: Vec<(i64, i64, i64, String)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!("explain query plan {sql}")))
                .fetch_all(app.db.read())
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}\n{sql}"));
        let details: Vec<&str> = plan.iter().map(|row| row.3.as_str()).collect();
        // tests/db.rs と同じ: 端から読んでよいのは、小さな master と JSON の配列だけ。
        let scans: Vec<&&str> = details
            .iter()
            .filter(|d| {
                d.strip_prefix("SCAN ")
                    .and_then(|rest| rest.split_whitespace().next())
                    .is_some_and(|target| {
                        !["models", "efforts", "json_each", "CONSTANT"].contains(&target)
                    })
            })
            .collect();
        assert!(scans.is_empty(), "{name} が端から読む: {details:?}\n{sql}");
        if let Some((_, Some(index))) = want.iter().find(|(n, _)| n == name) {
            assert!(
                details.iter().any(|d| d.contains(index)),
                "{name} が索引 {index} を使わない: {details:?}\n{sql}"
            );
        }
    }
}
