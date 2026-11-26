//! GetGraph（図の点と線）の振る舞い（処理より先に書く）。 0.4.x の server の `GET /v1/graph` の問い（tests/api.rs の「図の点と線」）を、
//! 新しい道と欄の名で引く。見本は書く操作だけで作る（common::sample）。
mod common;

use axum::http::StatusCode;
use castlist::graph::LOOKUPS;
use common::sample::{OPUS, SONNET, field, names, ok, sample};
use common::{App, ai, error_type};
use serde_json::{Value, json};

/// 点の id から、その点を引く。
fn node<'a>(graph: &'a Value, id: &str) -> &'a Value {
    graph["nodes"]
        .as_array()
        .unwrap_or_else(|| panic!("nodes が無い: {graph}"))
        .iter()
        .find(|n| n["id"] == id)
        .unwrap_or_else(|| panic!("{id} の点が無い: {graph}"))
}

#[tokio::test]
async fn without_parameters_every_actor_and_every_link_in_the_order_received() {
    let s = sample().await;
    let graph = ok(&s.app, "/api/graph").await;
    // 点は登録を受けた順（seq）。
    assert_eq!(
        names(&graph["nodes"]),
        [
            "owner.1",
            "pm.1",
            "tpm.1",
            "se.1",
            "se.2",
            "devops.1",
            "auditor.1",
            "se.3"
        ],
        "{graph}"
    );
    // 線も受けた順。
    assert_eq!(
        field(&graph["edges"], "id"),
        [
            "l-owner-pm",
            "l-pm-tpm",
            "l-tpm-se1",
            "l-tpm-se2",
            "l-tpm-devops",
            "l-tpm-auditor",
            "l-se1-se3"
        ],
        "{graph}"
    );
    let edge = &graph["edges"][6];
    assert_eq!(edge["type"], "continued");
    assert_eq!(edge["from"], s.id("se.1"));
    assert_eq!(edge["to"], s.id("se.3"));
    assert!(
        edge["sentAt"].is_string() && edge["receivedAt"].is_string(),
        "{edge}"
    );
    assert_eq!(edge["clockSkew"], false);
}

#[tokio::test]
async fn a_label_selects_the_set_and_the_links_touching_it_bring_one_step_beyond() {
    let s = sample().await;
    let graph = ok(&s.app, "/api/graph?label=tpm.role:software-engineer").await;
    // 組は se.1・se.2・se.3。線の向こうの端の tpm.1 も入るが、その先の pm.1 は入らない（一歩だけ）。
    assert_eq!(
        names(&graph["nodes"]),
        ["tpm.1", "se.1", "se.2", "se.3"],
        "{graph}"
    );
    // 組に触れる線だけ（tpm.1 から devops.1・auditor.1 への線と、pm.1 から tpm.1 への線は入らない）。
    assert_eq!(
        field(&graph["edges"], "id"),
        ["l-tpm-se1", "l-tpm-se2", "l-se1-se3"],
        "{graph}"
    );
    // 人の根は、組の中から一歩で届く時だけ入る。
    let graph = ok(&s.app, "/api/graph?label=tpm.role:tpm").await;
    assert_eq!(
        names(&graph["nodes"]),
        ["pm.1", "tpm.1", "se.1", "se.2", "devops.1", "auditor.1"],
        "{graph}"
    );
    // 当たらないラベルなら、点も線も無い。
    let graph = ok(&s.app, "/api/graph?label=tpm.key:t-none").await;
    assert_eq!(graph["nodes"], json!([]), "{graph}");
    assert_eq!(graph["edges"], json!([]), "{graph}");
}

#[tokio::test]
async fn the_window_is_on_the_time_the_registration_was_received_with_both_ends_included() {
    let s = sample().await;
    // se.1 の登場の時まで: 組は owner.1・pm.1・tpm.1・se.1（se.2 から後は、その後に登録した）。
    let graph = ok(
        &s.app,
        &format!(
            "/api/graph?label=tpm.role:software-engineer&to={}",
            s.se1_entered
        ),
    )
    .await;
    assert_eq!(names(&graph["nodes"]), ["tpm.1", "se.1", "se.3"], "{graph}");
    assert_eq!(
        field(&graph["edges"], "id"),
        ["l-tpm-se1", "l-se1-se3"],
        "{graph}"
    );
    // 窓の外なら、何も無い。
    let graph = ok(
        &s.app,
        "/api/graph?label=project:castlist&from=2000-01-01T00:00:00Z&to=2000-01-02T00:00:00Z",
    )
    .await;
    assert_eq!(graph["nodes"], json!([]), "{graph}");
    assert_eq!(graph["edges"], json!([]), "{graph}");
    let graph = ok(&s.app, &format!("/api/graph?from={}", s.marker)).await;
    assert_eq!(graph["nodes"], json!([]), "{graph}");
    // 両端を含む: 登録を受けた時刻ちょうどの窓で、そのアクターが入る。
    let (_, _, se3) = s.app.get(&format!("/api/actors/{}", s.id("se.3"))).await;
    let registered = se3["receivedAt"].as_str().unwrap();
    let graph = ok(
        &s.app,
        &format!("/api/graph?label=tpm.role:software-engineer&from={registered}&to={registered}"),
    )
    .await;
    assert_eq!(names(&graph["nodes"]), ["se.1", "se.3"], "{graph}");
    assert_eq!(field(&graph["edges"], "id"), ["l-se1-se3"], "{graph}");
}

#[tokio::test]
async fn a_node_has_the_registration_the_state_and_the_latest_model_and_effort() {
    let s = sample().await;
    let graph = ok(&s.app, "/api/graph").await;
    for n in graph["nodes"].as_array().unwrap() {
        let id = n["id"].as_str().unwrap();
        let (status, _, actor) = s.app.get(&format!("/api/actors/{id}")).await;
        assert_eq!(status, StatusCode::OK, "{actor}");
        // 点の欄は、GetActor の同じ名の欄と同じ（state と currentSettings も同じ形）。
        for key in [
            "id",
            "name",
            "kind",
            "issuer",
            "sentAt",
            "receivedAt",
            "clockSkew",
            "state",
        ] {
            assert_eq!(n[key], actor[key], "{key}: {n}");
        }
        assert_eq!(
            n.get("currentSettings"),
            actor.get("currentSettings"),
            "{n}"
        );
        // ほかの欄は無い（札・付随する id・出来事は、図の答えに入れない）。
        let mut keys: Vec<&str> = n.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut want = vec![
            "clockSkew",
            "id",
            "issuer",
            "kind",
            "name",
            "receivedAt",
            "sentAt",
            "state",
        ];
        if n.get("currentSettings").is_some() {
            want.push("currentSettings");
        }
        want.sort_unstable();
        assert_eq!(keys, want, "{n}");
    }
    let owner = node(&graph, s.id("owner.1"));
    assert_eq!(owner["kind"], "human");
    assert!(owner.get("currentSettings").is_none(), "{owner}");
    assert_eq!(
        owner["state"],
        json!({ "present": false, "inferred": false })
    );
    // se.2 は /model で sonnet に替わった（見た行）。
    let se2 = node(&graph, s.id("se.2"));
    assert_eq!(se2["currentSettings"]["model"], SONNET, "{se2}");
    assert_eq!(se2["currentSettings"]["source"], "observed");
    assert_eq!(se2["currentSettings"]["id"], "t-model");
    assert_eq!(se2["state"]["presence"], "exited");
    // tpm.1 は max を見られた後、high に戻った。
    let tpm = node(&graph, s.id("tpm.1"));
    assert_eq!(tpm["currentSettings"]["model"], OPUS);
    assert_eq!(tpm["currentSettings"]["effort"], "high");
    assert_eq!(tpm["currentSettings"]["id"], "t-back");
    let auditor = node(&graph, s.id("auditor.1"));
    assert_eq!(auditor["state"]["present"], true, "{auditor}");
    assert!(auditor["state"]["enteredAt"].is_string(), "{auditor}");
}

#[tokio::test]
async fn an_actor_closed_by_inference_is_marked_inferred_in_its_state() {
    let s = sample().await;
    let graph = ok(&s.app, "/api/graph").await;
    // devops.1 は、roster が落ちたと推して閉じた（推しの幕引き）。ほかの停止は推しでない。
    let devops = node(&graph, s.id("devops.1"));
    assert_eq!(devops["state"]["present"], false, "{devops}");
    assert_eq!(devops["state"]["presence"], "closed");
    assert_eq!(devops["state"]["inferred"], true, "{devops}");
    for name in ["se.1", "se.2", "se.3", "tpm.1", "auditor.1"] {
        assert_eq!(
            node(&graph, s.id(name))["state"]["inferred"],
            false,
            "{name}"
        );
    }
    // また登場すれば、推しの印は消える。
    let (status, _, answer) = s
        .app
        .put(
            "/api/events/e-devops-back",
            &common::event("claude.session:s-devops", "reentered"),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let graph = ok(&s.app, "/api/graph?label=tpm.role:devops").await;
    let devops = node(&graph, s.id("devops.1"));
    assert_eq!(devops["state"]["present"], true, "{devops}");
    assert_eq!(devops["state"]["inferred"], false, "{devops}");
}

/// 値の形の誤りは、生成した層が 400 で断る。
#[tokio::test]
async fn values_of_the_wrong_form_are_400() {
    let s = sample().await;
    for uri in [
        "/api/graph?label=nocolon",
        "/api/graph?label=Project:castlist",
        "/api/graph?from=きのう",
        "/api/graph?to=2026-13-01",
        "/api/graph?from=2026-11-24T09:00:00%2B09:00",
    ] {
        let (status, _, answer) = s.app.get(uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    }
}

/// restJson1 は知らない引数を読み飛ばし、1 つだけの引数を重ねれば初めの物を使う（0.4.x の server は 400。next.md）。
#[tokio::test]
async fn unknown_and_repeated_single_parameters_are_not_refused() {
    let s = sample().await;
    let graph = ok(&s.app, "/api/graph?label=tpm.role:auditor&colour=red").await;
    assert_eq!(names(&graph["nodes"]), ["tpm.1", "auditor.1"], "{graph}");
    let graph = ok(
        &s.app,
        "/api/graph?label=tpm.role:auditor&label=tpm.role:devops",
    )
    .await;
    assert_eq!(names(&graph["nodes"]), ["tpm.1", "auditor.1"], "{graph}");
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
    app.register(&ai("se", "s-1")).await;
    app.execute("alter table links rename to links_gone").await;
    internal_error(&app, "/api/graph", "links").await;
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
    internal_error(&app, "/api/graph", "not-a-time").await;
}

/// 図は重い読みの接続だけを使う: 軽い読みの接続をみな握っていても答え、重い読みの接続をみな握れば 503 の Unavailable。
/// 重い読みの接続が空かない間も、軽い読み（GetActor）は待たずに答える。
#[tokio::test]
async fn the_graph_reads_only_through_the_heavy_read_connections() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await["actor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut light = Vec::new();
    for _ in 0..castlist::db::READS {
        light.push(app.db.read().acquire().await.expect("軽い読みの接続"));
    }
    let graph = ok(&app, "/api/graph").await;
    assert_eq!(names(&graph["nodes"]), ["se.1"], "{graph}");
    drop(light);

    let mut heavy = Vec::new();
    for _ in 0..castlist::db::HEAVY_READS {
        heavy.push(app.db.heavy().acquire().await.expect("重い読みの接続"));
    }
    let started = std::time::Instant::now();
    let (status, _, actor) = app.get(&format!("/api/actors/{se1}")).await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    let (status, headers, answer) = app.get("/api/graph").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(heavy);
    ok(&app, "/api/graph").await;
}

// --- 索引 ---

#[tokio::test]
async fn the_graph_sql_does_not_scan_a_big_table() {
    let s = sample().await;
    let names: Vec<&str> = LOOKUPS.iter().map(|(name, _)| *name).collect();
    for want in [
        "graph_label",
        "graph_links",
        "graph_actors",
        "graph_state",
        "graph_settings",
        "timeline_events",
        "timeline_settings",
    ] {
        assert!(names.contains(&want), "{want} が LOOKUPS に無い: {names:?}");
    }
    for (name, sql) in LOOKUPS {
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
