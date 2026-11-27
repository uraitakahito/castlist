//! GetTimeline（タイムラインを描く元の値）の振る舞い（処理より先に書く）。点と線は GetGraph と同じ組で、
//! その点の出来事（帯と印に要る欄だけ）と、model と effort の行の全部を足す。並べる計算は、呼ぶ側（画面）がする。
mod common;

use axum::http::StatusCode;
use common::sample::{KEY, OPUS, SONNET, field, names, ok, sample};
use common::{App, ai, error_type};
use serde_json::{Value, json};

/// あるアクターの出来事の種類（受けた順）。
fn kinds<'a>(timeline: &'a Value, actor: &str) -> Vec<&'a str> {
    timeline["events"]
        .as_array()
        .unwrap_or_else(|| panic!("events が無い: {timeline}"))
        .iter()
        .filter(|e| e["actorId"] == actor)
        .map(|e| e["type"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn the_nodes_and_edges_are_the_same_set_as_the_graph() {
    let s = sample().await;
    for query in [
        "",
        "?label=tpm.role:software-engineer",
        "?from=9000-01-01T00:00:00Z",
    ] {
        let graph = ok(&s.app, &format!("/api/graph{query}")).await;
        let timeline = ok(&s.app, &format!("/api/timeline{query}")).await;
        assert_eq!(timeline["nodes"], graph["nodes"], "{query}");
        assert_eq!(timeline["edges"], graph["edges"], "{query}");
        assert!(timeline["asOf"].is_string(), "{timeline}");
    }
}

#[tokio::test]
async fn the_events_of_the_nodes_come_in_the_order_received_with_only_the_fields_for_drawing() {
    let s = sample().await;
    let timeline = ok(&s.app, "/api/timeline").await;
    let events = timeline["events"].as_array().unwrap();
    assert!(!events.is_empty(), "{timeline}");
    // 受けた順（時刻は UTC の字なので、字の順で比べられる）。
    let received = field(&timeline["events"], "receivedAt");
    let mut sorted = received.clone();
    sorted.sort_unstable();
    assert_eq!(received, sorted);
    // 欄は、帯と印に要る物だけ（detail の全部は出さない）。
    for event in events {
        let mut keys: Vec<&str> = event
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.retain(|k| *k != "reason");
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["actorId", "inferred", "receivedAt", "type"],
            "{event}"
        );
    }
    // 出来事の数は、点のアクターの出来事の全部（ListEvents で数える）。
    let all = ok(
        &s.app,
        &format!("/api/events?label=project:castlist&maxResults=1000"),
    )
    .await;
    assert_eq!(
        events.len(),
        all["events"].as_array().unwrap().len(),
        "{timeline}"
    );
    // se.1 は /clear で退き、se.3 が継いだ。
    assert_eq!(kinds(&timeline, s.id("se.1")).last(), Some(&"cleared"));
}

#[tokio::test]
async fn a_label_narrows_the_events_and_settings_to_the_nodes() {
    let s = sample().await;
    let timeline = ok(&s.app, "/api/timeline?label=tpm.role:software-engineer").await;
    let nodes: Vec<&str> = field(&timeline["nodes"], "id");
    assert_eq!(names(&timeline["nodes"]), ["tpm.1", "se.1", "se.2", "se.3"]);
    for key in ["events", "settings"] {
        for row in timeline[key].as_array().unwrap() {
            assert!(
                nodes.contains(&row["actorId"].as_str().unwrap()),
                "{key}: {row}"
            );
        }
    }
    // 線の向こうの端（tpm.1）の出来事も入る（帯を描くため）。
    assert!(!kinds(&timeline, s.id("tpm.1")).is_empty(), "{timeline}");
    assert!(kinds(&timeline, s.id("pm.1")).is_empty(), "{timeline}");
}

#[tokio::test]
async fn the_settings_are_every_row_in_the_order_received_in_the_shape_of_get_actor() {
    let s = sample().await;
    let timeline = ok(&s.app, &format!("/api/timeline?label=tpm.key:{KEY}")).await;
    let se1 = s.id("se.1");
    let rows: Vec<&Value> = timeline["settings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["actorId"] == se1)
        .collect();
    let actor = ok(&s.app, &format!("/api/actors/{se1}")).await;
    let want: Vec<&Value> = actor["settings"].as_array().unwrap().iter().collect();
    assert!(want.len() >= 2, "{actor}");
    assert_eq!(rows, want, "{timeline}");
    assert_eq!(rows[0]["model"], OPUS);
    assert!(rows.iter().any(|r| r["model"] == SONNET), "{rows:?}");
}

#[tokio::test]
async fn an_inferred_close_carries_the_mark_and_the_reason() {
    let app = App::start().await;
    let answer = app.register(&ai("se", "s-1")).await;
    let id = answer["actor"]["id"].as_str().unwrap().to_owned();
    let by = "claude.session:s-1";
    let put = |uri: &'static str, body: Value| {
        let app = &app;
        async move {
            let (status, _, text) = app.send(common::put(uri, &body)).await;
            assert!(status.is_success(), "{uri}: {text}");
        }
    };
    put("/api/events/e-1", common::event(by, "entered")).await;
    let mut close = common::event(by, "closed");
    close["detail"] = json!({"inferred": true, "reason": "workspace_gone", "more": {"x": 1}});
    put("/api/events/e-2", close).await;
    let timeline = ok(&app, "/api/timeline").await;
    assert_eq!(kinds(&timeline, &id), ["entered", "closed"]);
    let events = timeline["events"].as_array().unwrap();
    assert_eq!(events[0]["inferred"], false);
    assert!(events[0].get("reason").is_none(), "{}", events[0]);
    assert_eq!(events[1]["inferred"], true);
    assert_eq!(events[1]["reason"], "workspace_gone");
    assert!(events[1].get("detail").is_none(), "{}", events[1]);
}

#[tokio::test]
async fn with_no_actor_every_list_is_empty() {
    let app = App::start().await;
    let timeline = ok(&app, "/api/timeline").await;
    for key in ["nodes", "edges", "events", "settings"] {
        assert_eq!(timeline[key], json!([]), "{key}");
    }
}

#[tokio::test]
async fn a_malformed_parameter_is_400_like_the_graph() {
    let app = App::start().await;
    for query in [
        "?label=nocolon",
        "?from=yesterday",
        "?to=2026-11-24T00:00:00%2B09:00",
    ] {
        let (graph_status, _, _) = app.send(common::get(&format!("/api/graph{query}"))).await;
        let (status, headers, body) = app
            .send(common::get(&format!("/api/timeline{query}")))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(status, graph_status, "{query}");
        assert!(!error_type(&headers).is_empty(), "{query}: {body}");
    }
}
