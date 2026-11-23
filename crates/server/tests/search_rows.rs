//! ListActorSettings・GetActorSettingAt・ListSettings・ListEvents（監査の検索の残り）の振る舞い（処理より先に書く）。
//! 今の server の tests/search.rs の問いを、新しい道と欄の名で引く。見本は書く操作だけで作り（common::sample）、
//! 受けた時刻を決めたい所だけ DB に直に置く。
mod common;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use castlist_next::search::plans;
use common::sample::{OPUS, SONNET, ev, field, observed, ok, put, sample, seat, tick};
use common::{App, ai, at, error_type};
use serde_json::{Value, json};

/// 送って 400 の ValidationException であることを見て、message を返す。
async fn invalid(app: &App, uri: &str) -> String {
    let (status, headers, answer) = app.get(uri).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    assert_eq!(error_type(&headers), "ValidationException", "{uri}");
    answer["message"].as_str().unwrap_or_default().to_owned()
}

/// 頁の印の中身を、server と同じ形（16 進）にする（崩れた印を作る試験のため）。
fn token(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

/// 形は正しいが、どのアクターでもない id。
const NO_SUCH: &str = "019abcde-0000-7000-8000-00000000ffff";

// --- ListActorSettings と GetActorSettingAt ---

#[tokio::test]
async fn the_model_and_effort_of_x_at_time_t() {
    let s = sample().await;
    let se2 = s.id("se.2").to_owned();
    let uri = |time: &str| format!("/api/actors/{se2}/settings/at?time={time}");
    let answer = ok(&s.app, &uri(&s.se2_entered)).await;
    assert_eq!(answer["actorId"], se2.as_str(), "{answer}");
    assert_eq!(answer["at"], s.se2_entered.as_str(), "{answer}");
    let row = &answer["setting"];
    assert_eq!(row["model"], OPUS, "{answer}");
    assert_eq!(row["effort"], "high");
    assert_eq!(row["source"], "requested");
    assert_eq!(row["actorId"], se2.as_str());
    // 登録の時に頼んだ物には id が無い。
    assert!(row.get("id").is_none(), "{row}");
    assert!(
        row["sentAt"].is_string() && row["receivedAt"].is_string(),
        "{row}"
    );
    assert_eq!(row["clockSkew"], false);
    let answer = ok(&s.app, &uri(&s.model_changed)).await;
    assert_eq!(answer["setting"]["model"], SONNET, "{answer}");
    assert_eq!(answer["setting"]["source"], "observed");
    assert_eq!(answer["setting"]["id"], "t-model");
    // その時までに行が無ければ、setting が無い。
    let answer = ok(&s.app, &uri("2000-01-01T00:00:00Z")).await;
    assert!(answer.get("setting").is_none(), "{answer}");
    assert_eq!(answer["at"], "2000-01-01T00:00:00Z");
}

#[tokio::test]
async fn all_the_model_and_effort_rows_of_an_actor_in_order() {
    let s = sample().await;
    let se2 = s.id("se.2");
    let answer = ok(&s.app, &format!("/api/actors/{se2}/settings")).await;
    assert_eq!(answer["actorId"], se2, "{answer}");
    assert_eq!(
        field(&answer["settings"], "model"),
        [OPUS, SONNET],
        "{answer}"
    );
    assert_eq!(
        field(&answer["settings"], "source"),
        ["requested", "observed"]
    );
    assert_eq!(field(&answer["settings"], "id"), ["null", "t-model"]);
    // 行の形は GetActor の model と effort と同じ（アクターの名は付けない）。
    assert!(answer["settings"][0].get("actorName").is_none(), "{answer}");
    let tpm = s.id("tpm.1");
    let answer = ok(&s.app, &format!("/api/actors/{tpm}/settings")).await;
    assert_eq!(
        field(&answer["settings"], "effort"),
        ["high", "max", "high"],
        "{answer}"
    );
    // 人は model と effort を持たない。
    let pm = s.id("pm.1");
    let answer = ok(&s.app, &format!("/api/actors/{pm}/settings")).await;
    assert_eq!(answer["settings"], json!([]), "{answer}");
}

#[tokio::test]
async fn the_settings_of_an_unknown_actor_are_404() {
    let s = sample().await;
    for uri in [
        format!("/api/actors/{NO_SUCH}/settings"),
        format!("/api/actors/{NO_SUCH}/settings/at?time=2026-11-24T00:00:00Z"),
    ] {
        let (status, headers, answer) = s.app.get(&uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {answer}");
        assert_eq!(error_type(&headers), "NotFound", "{uri}");
    }
}

/// time は DB に残す字（UTC・ミリ秒まで）に直して比べ、直した物を at で返す。
#[tokio::test]
async fn the_time_is_compared_and_returned_in_the_stored_form() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await["actor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    app.execute(&format!(
        "insert into models(name, first_seen_at) values('{OPUS}', '2000-01-01T00:00:00.000Z')"
    ))
    .await;
    app.execute(&format!(
        "insert into settings(id, actor_id, model_id, effort_id, source, sent_at, received_at, clock_skew, body_hash)
         values('t-1', '{se1}', (select id from models where name = '{OPUS}'), (select id from efforts where name = 'high'),
                'observed', '2000-01-01T00:00:00.500Z', '2000-01-01T00:00:00.500Z', 0, 'x')"
    ))
    .await;
    let uri = |time: &str| format!("/api/actors/{se1}/settings/at?time={time}");
    let answer = ok(&app, &uri("2000-01-01T00:00:00Z")).await;
    assert!(answer.get("setting").is_none(), "{answer}");
    let answer = ok(&app, &uri("2000-01-01T00:00:00.499Z")).await;
    assert!(answer.get("setting").is_none(), "{answer}");
    // ミリ秒より下は切る。
    let answer = ok(&app, &uri("2000-01-01T00:00:00.5009Z")).await;
    assert_eq!(answer["setting"]["id"], "t-1", "{answer}");
    assert_eq!(answer["at"], "2000-01-01T00:00:00.5Z", "{answer}");
}

#[tokio::test]
async fn a_missing_or_wrong_time_is_400() {
    let s = sample().await;
    let se1 = s.id("se.1");
    for uri in [
        format!("/api/actors/{se1}/settings/at"),
        format!("/api/actors/{se1}/settings/at?time=noon"),
        format!("/api/actors/{se1}/settings/at?time=2026-11-24T09:00:00%2B09:00"),
        "/api/actors/not-an-id/settings".to_owned(),
        "/api/actors/not-an-id/settings/at?time=2026-11-24T00:00:00Z".to_owned(),
    ] {
        let (status, _, answer) = s.app.get(&uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    }
}

// --- ListSettings ---

#[tokio::test]
async fn the_rows_where_what_was_observed_differs_from_what_was_requested() {
    let s = sample().await;
    let answer = ok(&s.app, "/api/settings?mismatch=true").await;
    assert_eq!(
        field(&answer["settings"], "id"),
        ["t-model", "t-max"],
        "{answer}"
    );
    assert_eq!(field(&answer["settings"], "actorName"), ["se.2", "tpm.1"]);
    assert_eq!(answer["settings"][0]["actorId"], s.id("se.2"));
    assert!(answer.get("nextToken").is_none(), "{answer}");
    // ほかの絞り: model・effort・source・時刻の窓。
    let answer = ok(&s.app, "/api/settings?effort=max").await;
    assert_eq!(field(&answer["settings"], "id"), ["t-max"], "{answer}");
    let answer = ok(
        &s.app,
        &format!("/api/settings?model={SONNET}&source=observed"),
    )
    .await;
    assert_eq!(field(&answer["settings"], "id"), ["t-model"], "{answer}");
    let answer = ok(
        &s.app,
        &format!("/api/settings?source=observed&from={}", s.model_changed),
    )
    .await;
    assert_eq!(
        field(&answer["settings"], "id"),
        ["t-model", "t-max", "t-back"],
        "{answer}"
    );
    let answer = ok(
        &s.app,
        &format!("/api/settings?to={}&source=observed", s.se2_entered),
    )
    .await;
    assert_eq!(answer["settings"], json!([]), "{answer}");
    // 条件が無くてもよい（受けた順の全部。頼んだ 6 と見た 3）。
    let answer = ok(&s.app, "/api/settings").await;
    assert_eq!(answer["settings"].as_array().unwrap().len(), 9, "{answer}");
}

#[tokio::test]
async fn each_setting_row_has_the_setting_shape_and_the_actor_name() {
    let s = sample().await;
    let answer = ok(&s.app, &format!("/api/settings?model={SONNET}")).await;
    let row = &answer["settings"][0];
    assert_eq!(row["id"], "t-model", "{row}");
    assert_eq!(row["actorId"], s.id("se.2"));
    assert_eq!(row["actorName"], "se.2");
    assert_eq!(row["model"], SONNET);
    assert_eq!(row["effort"], "high");
    assert_eq!(row["source"], "observed");
    assert_eq!(row["receivedAt"], s.model_changed.as_str());
    assert!(row["sentAt"].as_str().is_some_and(|t| t.ends_with('Z')));
    assert_eq!(row["clockSkew"], false);
    // 頼んだ行には id が無い。
    let answer = ok(&s.app, "/api/settings?source=requested&maxResults=1").await;
    assert!(answer["settings"][0].get("id").is_none(), "{answer}");
    assert_eq!(answer["settings"][0]["actorName"], "tpm.1", "{answer}");
}

/// 前に頼んだ行の無い見た行は、違いに入らない（後で頼んだ行とも比べない）。
#[tokio::test]
async fn an_observed_row_without_an_earlier_request_is_not_a_mismatch() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    put(
        &app,
        "/api/settings/o-1",
        &observed("claude.session:s-1", SONNET, "low"),
    )
    .await;
    tick().await;
    let mut body = ai("se", "s-1");
    body["settings"] = json!({ "model": OPUS, "effort": "high" });
    app.register(&body).await;
    let answer = ok(&app, "/api/settings").await;
    assert_eq!(
        field(&answer["settings"], "source"),
        ["observed", "requested"],
        "{answer}"
    );
    let answer = ok(&app, "/api/settings?mismatch=true").await;
    assert_eq!(answer["settings"], json!([]), "{answer}");
    // 頼んだ後に違う物を見れば入る。同じ物は入らない。
    put(
        &app,
        "/api/settings/o-2",
        &observed("claude.session:s-1", OPUS, "max"),
    )
    .await;
    put(
        &app,
        "/api/settings/o-3",
        &observed("claude.session:s-1", OPUS, "high"),
    )
    .await;
    let answer = ok(&app, "/api/settings?mismatch=true").await;
    assert_eq!(field(&answer["settings"], "id"), ["o-2"], "{answer}");
}

#[tokio::test]
async fn mismatch_false_and_skewed_false_are_400() {
    let s = sample().await;
    for (uri, name) in [
        ("/api/settings?mismatch=false", "mismatch"),
        ("/api/settings?skewed=false", "skewed"),
        ("/api/events?skewed=false", "skewed"),
    ] {
        let message = invalid(&s.app, uri).await;
        assert!(message.contains(name), "{uri}: {message}");
    }
}

// --- ListEvents ---

#[tokio::test]
async fn events_by_type_and_time_across_actors() {
    let s = sample().await;
    let answer = ok(&s.app, "/api/events?type=model_unreadable").await;
    assert_eq!(
        field(&answer["events"], "id"),
        ["e-unread-1", "e-unread-2"],
        "{answer}"
    );
    assert_eq!(
        field(&answer["events"], "actorName"),
        ["devops.1", "auditor.1"]
    );
    let row = &answer["events"][0];
    assert_eq!(row["actorId"], s.id("devops.1"));
    assert_eq!(row["type"], "model_unreadable");
    assert_eq!(row["detail"], json!({ "reason": "statusline-missing" }));
    assert!(row["sentAt"].as_str().is_some_and(|t| t.ends_with('Z')));
    assert!(row["receivedAt"].as_str().is_some_and(|t| t.ends_with('Z')));
    assert_eq!(row["clockSkew"], false);
    assert!(answer.get("nextToken").is_none(), "{answer}");
    let answer = ok(&s.app, "/api/events?type=model_unreadable&reason=timeout").await;
    assert_eq!(field(&answer["events"], "id"), ["e-unread-2"], "{answer}");
    // 種類を重ねれば、どれか。
    let answer = ok(&s.app, "/api/events?type=cleared&type=exited&type=closed").await;
    assert_eq!(
        field(&answer["events"], "id"),
        [
            "e-se1-clear",
            "e-se2-out",
            "e-devops-out",
            "e-se3-out",
            "e-tpm-out"
        ],
        "{answer}"
    );
    let se3 = s.id("se.3");
    let answer = ok(&s.app, &format!("/api/events?actor={se3}")).await;
    assert_eq!(
        field(&answer["events"], "id"),
        ["e-se3-in", "e-se3-out"],
        "{answer}"
    );
    let answer = ok(&s.app, &format!("/api/events?actor={se3}&source=clear")).await;
    assert_eq!(field(&answer["events"], "id"), ["e-se3-in"], "{answer}");
    // 窓は両端を含む。
    let answer = ok(
        &s.app,
        &format!("/api/events?type=entered&to={}", s.se1_entered),
    )
    .await;
    assert_eq!(
        field(&answer["events"], "id"),
        ["e-tpm-in", "e-se1-in"],
        "{answer}"
    );
    let answer = ok(&s.app, &format!("/api/events?from={0}&to={0}", s.marker)).await;
    assert_eq!(field(&answer["events"], "id"), ["e-marker"], "{answer}");
    let answer = ok(&s.app, &format!("/api/events?actor={NO_SUCH}")).await;
    assert_eq!(answer["events"], json!([]), "{answer}");
}

#[tokio::test]
async fn what_happened_after_a_previous_as_of() {
    let s = sample().await;
    let since = &s.as_of_before_last;
    let answer = ok(&s.app, &format!("/api/events?from={since}")).await;
    assert_eq!(
        field(&answer["events"], "id"),
        ["e-se3-out", "e-tpm-out"],
        "{answer}"
    );
    let answer = ok(&s.app, &format!("/api/settings?from={since}")).await;
    assert_eq!(answer["settings"], json!([]), "{answer}");
}

#[tokio::test]
async fn the_rows_with_a_skewed_sender_clock() {
    let s = sample().await;
    let answer = ok(&s.app, "/api/events?skewed=true").await;
    assert_eq!(field(&answer["events"], "id"), ["e-skew"], "{answer}");
    let row = &answer["events"][0];
    assert_eq!(row["clockSkew"], true);
    assert_ne!(row["sentAt"], row["receivedAt"]);
    let answer = ok(&s.app, "/api/settings?skewed=true").await;
    assert_eq!(answer["settings"], json!([]), "{answer}");
}

// --- 頁を分ける ---

/// maxResults=2 で nextToken を辿る。初めの 3 頁の間に行を足しても、元の組に穴も重なりも無く、足した行は後ろに続き、
/// 終わりで nextToken が無い。
#[tokio::test]
async fn walking_the_pages_of_events_and_settings_has_no_holes_or_overlaps() {
    let s = sample().await;
    for (n, (path, key)) in [
        ("/api/events?type=entered", "events"),
        ("/api/settings?source=requested", "settings"),
    ]
    .into_iter()
    .enumerate()
    {
        let all = ok(&s.app, path).await;
        let want: Vec<String> = all[key]
            .as_array()
            .unwrap()
            .iter()
            .map(Value::to_string)
            .collect();
        assert!(want.len() > 4, "{all}");
        let mut seen = Vec::new();
        let mut uri = format!("{path}&maxResults=2");
        for page in 0..100 {
            let answer = ok(&s.app, &uri).await;
            let rows = answer[key].as_array().unwrap();
            assert!(rows.len() <= 2, "{answer}");
            seen.extend(rows.iter().map(Value::to_string));
            // 頁の間に、登場するアクター（頼んだ行と、登場の出来事）を足す。
            if page < 3 {
                let session = format!("s-new-{n}-{page}");
                s.app
                    .register(&seat("new", &session, "software-engineer", "workspace:9"))
                    .await;
                put(
                    &s.app,
                    &format!("/api/events/e-new-{n}-{page}"),
                    &ev(
                        &format!("claude.session:{session}"),
                        "entered",
                        json!({}),
                        &at(0),
                    ),
                )
                .await;
                tick().await;
            }
            match answer["nextToken"].as_str() {
                Some(t) => uri = format!("{path}&maxResults=2&nextToken={t}"),
                None => break,
            }
        }
        assert_eq!(seen[..want.len()], want[..], "{path}");
        assert_eq!(
            seen.len(),
            want.len() + 3,
            "足した 3 つが後ろに続く: {path}"
        );
        let unique: BTreeSet<&String> = seen.iter().collect();
        assert_eq!(unique.len(), seen.len(), "重なり: {path}");
    }
}

#[tokio::test]
async fn max_results_defaults_to_100_and_cuts_the_page() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await["actor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for i in 0..101 {
        app.event(&format!("e-{i}"), &se1, "reported_in", "{}")
            .await;
    }
    let answer = ok(&app, "/api/events").await;
    assert_eq!(answer["events"].as_array().unwrap().len(), 100);
    let next = answer["nextToken"].as_str().expect("nextToken").to_owned();
    let answer = ok(&app, &format!("/api/events?nextToken={next}")).await;
    assert_eq!(field(&answer["events"], "id"), ["e-100"], "{answer}");
    assert!(answer.get("nextToken").is_none(), "{answer}");
}

/// 頁の印は、同じ検索の答えの物だけを受ける。ほかの検索の物と、崩れた物は断る。
#[tokio::test]
async fn a_next_token_of_another_query_is_400() {
    let s = sample().await;
    let next = |answer: Value| answer["nextToken"].as_str().expect("nextToken").to_owned();
    let actors = next(ok(&s.app, "/api/actors?label=project:castlist&maxResults=1").await);
    let events = next(ok(&s.app, "/api/events?maxResults=1").await);
    let settings = next(ok(&s.app, "/api/settings?maxResults=1").await);
    for uri in [
        format!("/api/events?nextToken={actors}"),
        format!("/api/events?nextToken={settings}"),
        format!("/api/settings?nextToken={actors}"),
        format!("/api/settings?nextToken={events}"),
        format!("/api/actors?label=project:castlist&nextToken={events}"),
        format!("/api/actors?label=project:castlist&nextToken={settings}"),
        "/api/events?nextToken=garbage".to_owned(),
        "/api/settings?nextToken=abc".to_owned(),
        format!("/api/events?nextToken={}", token("events\nnot-a-seq")),
        format!("/api/settings?nextToken={}", token("settings\n1\n2")),
    ] {
        let message = invalid(&s.app, &uri).await;
        assert!(message.contains("nextToken"), "{uri}: {message}");
    }
    // ほかの条件は印に結ばない（同じ検索なら通る）。
    ok(
        &s.app,
        &format!("/api/events?type=entered&nextToken={events}"),
    )
    .await;
}

// --- 誤り ---

/// 値の形の誤りは、生成した層が 400 で断る（種類は 10 種のどれか・reason と source は空でない 256 字まで）。
#[tokio::test]
async fn values_of_the_wrong_form_are_400() {
    let s = sample().await;
    let long = "x".repeat(257);
    for uri in [
        "/api/events?type=danced".to_owned(),
        "/api/events?type=entered&type=Entered".to_owned(),
        "/api/events?actor=not-an-id".to_owned(),
        "/api/events?reason=".to_owned(),
        format!("/api/events?source={long}"),
        "/api/events?to=noon".to_owned(),
        "/api/events?from=2026-11-24T09:00:00%2B09:00".to_owned(),
        "/api/events?maxResults=1001".to_owned(),
        "/api/events?skewed=yes".to_owned(),
        "/api/settings?source=guessed".to_owned(),
        "/api/settings?model=Opus".to_owned(),
        "/api/settings?from=noon".to_owned(),
        "/api/settings?maxResults=0".to_owned(),
        "/api/settings?mismatch=yes".to_owned(),
    ] {
        let (status, _, answer) = s.app.get(&uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    }
    // 256 字までは受ける。
    ok(&s.app, &format!("/api/events?source={}", "x".repeat(256))).await;
}

/// 500 の InternalError で、中の誤りの字（`leak` を含む物）を出さないことを見る。
async fn internal_error(app: &App, uri: &str, leak: &str) {
    let (status, headers, answer) = app.get(uri).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{uri}: {answer}");
    assert_eq!(error_type(&headers), "InternalError", "{uri}");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "{answer}");
    assert!(!message.contains(leak), "中の誤りの字が出ている: {message}");
}

#[tokio::test]
async fn a_db_error_is_a_500_internal_error_with_a_fixed_message() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await["actor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    app.execute("alter table models rename to models_gone")
        .await;
    app.execute("alter table events rename to events_gone")
        .await;
    for uri in [
        format!("/api/actors/{se1}/settings"),
        format!("/api/actors/{se1}/settings/at?time=2026-11-24T00:00:00Z"),
        "/api/settings".to_owned(),
    ] {
        internal_error(&app, &uri, "models").await;
    }
    internal_error(&app, "/api/events", "events").await;
}

#[tokio::test]
async fn a_stored_row_that_cannot_be_converted_is_a_500_internal_error() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await["actor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    app.event_at("e-1", &se1, "entered", "{}", "not-a-time")
        .await;
    internal_error(&app, "/api/events", "not-a-time").await;
}

#[tokio::test]
async fn waiting_too_long_for_a_read_connection_is_503_unavailable() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await["actor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let uris = [
        format!("/api/actors/{se1}/settings"),
        format!("/api/actors/{se1}/settings/at?time=2026-11-24T00:00:00Z"),
        "/api/settings".to_owned(),
        "/api/events".to_owned(),
    ];
    let mut held = Vec::new();
    for _ in 0..castlist_next::db::READS {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    // 4 つを同時に送る（どれも接続を 5 秒待つので、順に送ると 20 秒かかる）。
    let [a, b, c, d] = &uris;
    let answers = tokio::join!(app.get(a), app.get(b), app.get(c), app.get(d));
    for (uri, (status, headers, answer)) in uris
        .iter()
        .zip([answers.0, answers.1, answers.2, answers.3])
    {
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}: {answer}");
        assert_eq!(error_type(&headers), "Unavailable", "{uri}");
    }
    drop(held);
    for uri in &uris {
        ok(&app, uri).await;
    }
}

// --- 索引 ---

#[tokio::test]
async fn the_search_sql_does_not_scan_a_big_table() {
    let s = sample().await;
    let plans = plans();
    let names: Vec<&str> = plans.iter().map(|(name, _)| *name).collect();
    let want = [
        "actor_settings",
        "actor_settings_at",
        "settings",
        "settings_model",
        "settings_effort",
        "settings_source",
        "settings_window",
        "settings_mismatch",
        "settings_skewed",
        "settings_all",
        "events",
        "events_types",
        "events_actor",
        "events_window",
        "events_detail",
        "events_skewed",
        "events_all",
    ];
    for name in want {
        assert!(names.contains(&name), "{name} が plans に無い: {names:?}");
    }
    for (name, sql) in plans.iter().filter(|(name, _)| want.contains(name)) {
        let plan: Vec<(i64, i64, i64, String)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!("explain query plan {sql}")))
                .fetch_all(s.app.db.read())
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
    }
}
