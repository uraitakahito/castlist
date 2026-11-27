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
async fn a_window_cuts_the_same_part_of_the_set_as_the_graph() {
    let s = sample().await;
    let all = ok(&s.app, "/api/graph").await;
    let nodes = all["nodes"].as_array().unwrap();
    // 見本の中ほど（2 人目の登録から 4 人目の登録まで）で切る。両端を含む。
    let (from, to) = (
        nodes[1]["receivedAt"].as_str().unwrap(),
        nodes[3]["receivedAt"].as_str().unwrap(),
    );
    for query in [
        format!("?from={from}&to={to}"),
        format!("?to={to}"),
        format!("?from={from}"),
        format!("?label=project:castlist&to={to}"),
    ] {
        let graph = ok(&s.app, &format!("/api/graph{query}")).await;
        let timeline = ok(&s.app, &format!("/api/timeline{query}")).await;
        assert_eq!(timeline["nodes"], graph["nodes"], "{query}");
        assert_eq!(timeline["edges"], graph["edges"], "{query}");
        let n = timeline["nodes"].as_array().unwrap().len();
        assert!(n >= 1, "{query}: {timeline}");
        // 出来事と model と effort は、切った組の点の物だけ。
        let ids = field(&timeline["nodes"], "id");
        for key in ["events", "settings"] {
            for row in timeline[key].as_array().unwrap() {
                assert!(
                    ids.contains(&row["actorId"].as_str().unwrap()),
                    "{query}: {key}"
                );
            }
        }
    }
    // to だけで切った組は、全部より少ない（窓の終わりが効いている）。
    let cut = ok(
        &s.app,
        &format!(
            "/api/timeline?to={}",
            nodes[0]["receivedAt"].as_str().unwrap()
        ),
    )
    .await;
    assert!(
        cut["nodes"].as_array().unwrap().len() < nodes.len(),
        "{cut}"
    );
    assert!(!cut["nodes"].as_array().unwrap().is_empty(), "{cut}");
}

#[tokio::test]
async fn the_events_of_the_nodes_come_in_the_order_received_with_only_the_fields_for_drawing() {
    let s = sample().await;
    let timeline = ok(&s.app, "/api/timeline").await;
    let events = timeline["events"].as_array().unwrap();
    assert!(!events.is_empty(), "{timeline}");
    // 受けた順（答えの時刻の字は、秒の下の終わりの 0 を落とすので、時刻に直して比べる）。
    let received: Vec<time::OffsetDateTime> = field(&timeline["events"], "receivedAt")
        .into_iter()
        .map(|t| {
            time::OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339).unwrap()
        })
        .collect();
    assert!(received.is_sorted(), "{received:?}");
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
    let all = ok(&s.app, "/api/events?label=project:castlist&maxResults=1000").await;
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
    // どの点のアクターも、GetActor の settings と同じ行（同じ欄・同じ順）。
    let mut seen = 0;
    for node in timeline["nodes"].as_array().unwrap() {
        let id = node["id"].as_str().unwrap();
        let rows: Vec<&Value> = timeline["settings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["actorId"] == id)
            .collect();
        let actor = ok(&s.app, &format!("/api/actors/{id}")).await;
        let want: Vec<&Value> = actor["settings"].as_array().unwrap().iter().collect();
        assert_eq!(rows, want, "{}", node["name"]);
        seen += rows.len();
    }
    assert_eq!(seen, timeline["settings"].as_array().unwrap().len());
    // 最新だけでなく、替わる前の行も入る（見本には、opus から sonnet に替わった者がいる）。
    let models = field(&timeline["settings"], "model");
    assert!(
        models.contains(&OPUS) && models.contains(&SONNET),
        "{models:?}"
    );
    let latest = timeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n.get("currentSettings").is_some())
        .count();
    assert!(
        seen > latest,
        "替わる前の行も入る: {seen} 行・最新は {latest} 行"
    );
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
