//! GetLineage（起こした・継いだ線の上り下り）の振る舞い（処理より先に書く）。0.4.x の server の tests/search.rs の問いを、新しい道と欄の名で引く。
mod common;

use axum::http::StatusCode;
use common::sample::{KEY, field, line, names, ok, put, sample};
use common::{App, ai, error_type, human};
use serde_json::{Value, json};

fn id(answer: &Value) -> String {
    answer["actor"]["id"].as_str().expect("id").to_owned()
}

#[tokio::test]
async fn who_launched_whom_up_to_the_human_root() {
    let s = sample().await;
    let lineage = |name: &str, q: &str| format!("/api/actors/{}/lineage?{q}", s.id(name));
    let answer = ok(&s.app, &lineage("se.3", "direction=up")).await;
    assert_eq!(answer["actorId"], s.id("se.3"));
    // 人の pm.1 で止まる（owner.1 へは上らない）。点は初めの登場の順（登場していない人が先）。
    assert_eq!(
        names(&answer["nodes"]),
        ["pm.1", "tpm.1", "se.1", "se.3"],
        "{answer}"
    );
    // 線は受けた順。
    assert_eq!(
        field(&answer["edges"], "id"),
        ["l-pm-tpm", "l-tpm-se1", "l-se1-se3"],
        "{answer}"
    );
    let edge = &answer["edges"][2];
    assert_eq!(edge["type"], "continued");
    assert_eq!(edge["from"], s.id("se.1"));
    assert_eq!(edge["to"], s.id("se.3"));
    assert!(edge["receivedAt"].is_string(), "{edge}");
    assert!(edge["sentAt"].is_string(), "{edge}");
    assert_eq!(edge["clockSkew"], false);
    // 点は一覧の形。
    let node = answer["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "se.3")
        .unwrap();
    assert_eq!(node["url"], format!("/actors/{}", s.id("se.3")));
    assert_eq!(node["labels"]["tpm.key"], KEY);
    assert_eq!(node["present"], false);
    assert_eq!(node["lastExitType"], "exited");
}

#[tokio::test]
async fn down_is_one_step_by_default_and_depth_applies() {
    let s = sample().await;
    let lineage = |name: &str, q: &str| format!("/api/actors/{}/lineage?{q}", s.id(name));
    let answer = ok(&s.app, &lineage("tpm.1", "direction=down")).await;
    assert_eq!(
        names(&answer["nodes"]),
        ["tpm.1", "se.1", "se.2", "devops.1", "auditor.1"],
        "{answer}"
    );
    assert_eq!(
        field(&answer["edges"], "id"),
        ["l-tpm-se1", "l-tpm-se2", "l-tpm-devops", "l-tpm-auditor"],
        "{answer}"
    );
    let answer = ok(&s.app, &lineage("tpm.1", "direction=down&depth=2")).await;
    assert!(names(&answer["nodes"]).contains(&"se.3"), "{answer}");
    // 線の種類は下りも絞る。
    let answer = ok(
        &s.app,
        &lineage("tpm.1", "direction=down&depth=2&type=launched"),
    )
    .await;
    assert!(!names(&answer["nodes"]).contains(&"se.3"), "{answer}");
    // 深さを付ければ、上りにもそれを使う。
    let answer = ok(&s.app, &lineage("se.3", "direction=up&depth=1")).await;
    assert_eq!(names(&answer["nodes"]), ["se.1", "se.3"], "{answer}");
}

#[tokio::test]
async fn both_directions_by_default() {
    let s = sample().await;
    let answer = ok(&s.app, &format!("/api/actors/{}/lineage", s.id("se.1"))).await;
    assert_eq!(
        names(&answer["nodes"]),
        ["pm.1", "tpm.1", "se.1", "se.3"],
        "{answer}"
    );
    assert_eq!(
        field(&answer["edges"], "id"),
        ["l-pm-tpm", "l-tpm-se1", "l-se1-se3"],
        "{answer}"
    );
    // 上 20 段・下 1 段: tpm.1 からは、上の pm.1 と、下の 1 段だけ。
    let answer = ok(
        &s.app,
        &format!("/api/actors/{}/lineage?direction=both", s.id("tpm.1")),
    )
    .await;
    assert_eq!(
        names(&answer["nodes"]),
        ["pm.1", "tpm.1", "se.1", "se.2", "devops.1", "auditor.1"],
        "{answer}"
    );
}

#[tokio::test]
async fn a_human_does_not_go_up_and_type_narrows_up() {
    let s = sample().await;
    let lineage = |name: &str, q: &str| format!("/api/actors/{}/lineage?{q}", s.id(name));
    // 人からは上らない。
    let answer = ok(&s.app, &lineage("pm.1", "direction=up")).await;
    assert_eq!(names(&answer["nodes"]), ["pm.1"], "{answer}");
    assert_eq!(answer["edges"], json!([]));
    // 線の種類で絞る: se.3 は起こされていない（継いだだけ）。
    let answer = ok(&s.app, &lineage("se.3", "direction=up&type=launched")).await;
    assert_eq!(names(&answer["nodes"]), ["se.3"], "{answer}");
    assert_eq!(answer["edges"], json!([]));
    let answer = ok(&s.app, &lineage("se.3", "direction=up&type=continued")).await;
    assert_eq!(names(&answer["nodes"]), ["se.1", "se.3"], "{answer}");
}

/// 線が輪になっていても、点と線は重ならない。
#[tokio::test]
async fn a_cycle_has_no_duplicate_nodes_or_edges() {
    let app = App::start().await;
    id(&app.register(&human("pm")).await);
    let a = id(&app.register(&ai("se", "s-1")).await);
    id(&app.register(&ai("se", "s-2")).await);
    put(
        &app,
        "/api/links/l-1",
        &line("launched", "tpm.person:pm", "claude.session:s-1"),
    )
    .await;
    put(
        &app,
        "/api/links/l-2",
        &line("continued", "claude.session:s-1", "claude.session:s-2"),
    )
    .await;
    put(
        &app,
        "/api/links/l-3",
        &line("continued", "claude.session:s-2", "claude.session:s-1"),
    )
    .await;
    let answer = ok(&app, &format!("/api/actors/{a}/lineage?depth=20")).await;
    assert_eq!(
        names(&answer["nodes"]),
        ["pm.1", "se.1", "se.2"],
        "{answer}"
    );
    assert_eq!(
        field(&answer["edges"], "id"),
        ["l-1", "l-2", "l-3"],
        "{answer}"
    );
}

#[tokio::test]
async fn an_actor_without_links_is_only_itself() {
    let app = App::start().await;
    let a = id(&app.register(&ai("se", "s-1")).await);
    let answer = ok(&app, &format!("/api/actors/{a}/lineage")).await;
    assert_eq!(names(&answer["nodes"]), ["se.1"], "{answer}");
    assert_eq!(answer["edges"], json!([]));
}

#[tokio::test]
async fn an_unknown_actor_is_404_not_found() {
    let app = App::start().await;
    let (status, headers, answer) = app
        .get("/api/actors/019abcde-0000-7000-8000-000000000009/lineage")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(error_type(&headers), "NotFound");
}

#[tokio::test]
async fn parameters_of_the_wrong_form_are_400() {
    let app = App::start().await;
    let a = id(&app.register(&ai("se", "s-1")).await);
    for uri in [
        "/api/actors/no-such/lineage".to_owned(),
        format!("/api/actors/{a}/lineage?depth=21"),
        format!("/api/actors/{a}/lineage?depth=0"),
        format!("/api/actors/{a}/lineage?direction=sideways"),
        format!("/api/actors/{a}/lineage?type=married"),
    ] {
        let (status, _, answer) = app.get(&uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    }
}

#[tokio::test]
async fn a_db_error_is_a_500_internal_error_with_a_fixed_message() {
    let app = App::start().await;
    let a = id(&app.register(&ai("se", "s-1")).await);
    app.execute("alter table links rename to links_gone").await;
    let (status, headers, answer) = app.get(&format!("/api/actors/{a}/lineage")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(
        !message.is_empty() && !message.contains("links"),
        "{message}"
    );
}
