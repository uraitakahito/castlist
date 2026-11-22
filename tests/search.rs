//! 監査の検索の試験。Auditor が台帳の件から持てる物（project・Key・役・workspace・時刻）だけで、
//! 問いごとに 1 つずつ引く。見本の記録は、公の API だけで作る。

mod common;

use axum::http::StatusCode;
use castlist::search::plans;
use common::app::{App, at, request};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use tower::ServiceExt;

const KEY: &str = "t-23zs";
const OPUS: &str = "claude-opus-5-5";
const SONNET: &str = "claude-sonnet-5";

/// 受けた時刻（ミリ秒まで）が、次の段と重ならないように少し待つ。
async fn tick() {
    tokio::time::sleep(Duration::from_millis(5)).await;
}

/// 席のアクターの登録の本文（鍵は claude.session、workspace も付ける）。
fn seat(prefix: &str, session: &str, role: &str, workspace: &str) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "ai",
        "name_prefix": prefix,
        "labels": { "project": "castlist", "tpm.key": KEY, "tpm.role": role },
        "bindings": [
            { "type": "claude.session", "value": session },
            { "type": "cmux.workspace.ref", "value": workspace },
        ],
        "settings": { "model": OPUS, "effort": "high" },
        "sent_at": at(0),
    })
}

fn person(prefix: &str, person: &str) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "human",
        "name_prefix": prefix,
        "labels": { "project": "castlist", "tpm.role": "product-manager" },
        "bindings": [{ "type": "tpm.person", "value": person }],
        "sent_at": at(0),
    })
}

fn ev(by: &str, kind: &str, detail: Value, at: &str) -> Value {
    json!({ "issuer": "tpm", "by": by, "type": kind, "detail": detail, "sent_at": at })
}

fn line(kind: &str, from: &str, to: &str) -> Value {
    json!({ "issuer": "tpm", "type": kind, "from": from, "to": to, "sent_at": at(0) })
}

fn observed(by: &str, model: &str, effort: &str) -> Value {
    json!({ "issuer": "tpm", "by": by, "model": model, "effort": effort, "source": "observed", "sent_at": at(0) })
}

const OWNER: &str = "tpm.person:owner";
const PM: &str = "tpm.person:pm";
const TPM: &str = "claude.session:s-tpm";
const SE1: &str = "claude.session:s-se1";
const SE2: &str = "claude.session:s-se2";
const DEVOPS: &str = "claude.session:s-devops";
const AUDITOR: &str = "claude.session:s-auditor";
const SE3: &str = "claude.session:s-se3";

/// 見本の記録。名から id を引く物と、段の間の時刻の印。
struct Sample {
    app: App,
    ids: std::collections::BTreeMap<&'static str, String>,
    /// se.1 が登場した時（workspace:3 は se.1 の物）。
    se1_entered: String,
    /// se.2 の登場の時（/model の前）。
    se2_entered: String,
    /// se.2 の /model（見た sonnet）を受けた時。
    model_changed: String,
    /// se.2 と devops.1 が退いた後、se.3 と tpm.1 が退く前の印（auditor.1 の reported_in）。
    marker: String,
    /// 印の後、最後の段の前に引いた答えの as_of。
    as_of_before_last: String,
}

impl Sample {
    fn id(&self, name: &str) -> &str {
        self.ids
            .get(name)
            .unwrap_or_else(|| panic!("{name} が無い"))
    }
}

/// 送って 201 か 200 であることを見て、答えを返す。
async fn put(app: &App, uri: &str, body: &Value) -> Value {
    let (status, answer) = app.put(uri, body).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "{uri}: {status} {answer}"
    );
    answer
}

async fn received(app: &App, uri: &str, body: &Value) -> String {
    put(app, uri, body).await["received_at"]
        .as_str()
        .expect("received_at")
        .to_owned()
}

/// 人の PM → TPM → SWE×2・DevOps・Auditor。se.1 は /clear して se.3 に継がれる。
/// se.2 は /model で sonnet に替わる（見た行）。tpm.1 は頼んだ high と違う max を見られ、後で high に戻る。
/// auditor.1 は退かず、送り手の時計が 1 時間ずれた出来事を 1 つ持つ。
async fn sample() -> Sample {
    let app = App::start().await;
    let mut ids = std::collections::BTreeMap::new();
    let mut reg = async |name: &'static str, body: Value| {
        let answer = app.register(&body).await;
        assert_eq!(answer["name"], name, "{answer}");
        ids.insert(name, answer["id"].as_str().unwrap().to_owned());
        tick().await;
    };
    reg("owner.1", person("owner", "owner")).await;
    reg("pm.1", person("pm", "pm")).await;
    put(&app, "/v1/links/l-owner-pm", &line("launched", OWNER, PM)).await;
    reg("tpm.1", seat("tpm", "s-tpm", "tpm", "workspace:1")).await;
    put(&app, "/v1/links/l-pm-tpm", &line("launched", PM, TPM)).await;
    put(
        &app,
        "/v1/events/e-tpm-in",
        &ev(TPM, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;

    reg(
        "se.1",
        seat("se", "s-se1", "software-engineer", "workspace:3"),
    )
    .await;
    put(&app, "/v1/links/l-tpm-se1", &line("launched", TPM, SE1)).await;
    let se1_entered = received(
        &app,
        "/v1/events/e-se1-in",
        &ev(SE1, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;
    reg(
        "se.2",
        seat("se", "s-se2", "software-engineer", "workspace:4"),
    )
    .await;
    put(&app, "/v1/links/l-tpm-se2", &line("launched", TPM, SE2)).await;
    let se2_entered = received(
        &app,
        "/v1/events/e-se2-in",
        &ev(SE2, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;
    reg(
        "devops.1",
        seat("devops", "s-devops", "devops", "workspace:5"),
    )
    .await;
    put(
        &app,
        "/v1/links/l-tpm-devops",
        &line("launched", TPM, DEVOPS),
    )
    .await;
    put(
        &app,
        "/v1/events/e-devops-in",
        &ev(DEVOPS, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;
    reg(
        "auditor.1",
        seat("auditor", "s-auditor", "auditor", "workspace:6"),
    )
    .await;
    put(
        &app,
        "/v1/links/l-tpm-auditor",
        &line("launched", TPM, AUDITOR),
    )
    .await;
    put(
        &app,
        "/v1/events/e-auditor-in",
        &ev(AUDITOR, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;

    // /model: se.2 が sonnet に替わったのを見た（頼んだ opus と違う）。
    let model_changed =
        received(&app, "/v1/settings/t-model", &observed(SE2, SONNET, "high")).await;
    tick().await;
    // tpm.1: 頼んだ high と違う max を見て、後で high に戻ったのを見た。
    put(&app, "/v1/settings/t-max", &observed(TPM, OPUS, "max")).await;
    tick().await;
    put(&app, "/v1/settings/t-back", &observed(TPM, OPUS, "high")).await;
    tick().await;
    put(
        &app,
        "/v1/events/e-unread-1",
        &ev(
            DEVOPS,
            "model_unreadable",
            json!({ "reason": "statusline-missing" }),
            &at(0),
        ),
    )
    .await;
    tick().await;
    // 送り手の時計が 1 時間先。
    put(
        &app,
        "/v1/events/e-skew",
        &ev(AUDITOR, "reported_in", json!({}), &at(3600)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/v1/events/e-unread-2",
        &ev(
            AUDITOR,
            "model_unreadable",
            json!({ "reason": "timeout" }),
            &at(0),
        ),
    )
    .await;
    tick().await;

    // /clear: se.1 が退き、同じ workspace の se.3 が継ぐ。
    put(
        &app,
        "/v1/events/e-se1-clear",
        &ev(SE1, "cleared", json!({}), &at(0)),
    )
    .await;
    tick().await;
    reg(
        "se.3",
        seat("se", "s-se3", "software-engineer", "workspace:3"),
    )
    .await;
    put(&app, "/v1/links/l-se1-se3", &line("continued", SE1, SE3)).await;
    put(
        &app,
        "/v1/events/e-se3-in",
        &ev(SE3, "entered", json!({ "source": "clear" }), &at(0)),
    )
    .await;
    tick().await;

    put(
        &app,
        "/v1/events/e-se2-out",
        &ev(SE2, "exited", json!({}), &at(0)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/v1/events/e-devops-out",
        &ev(DEVOPS, "closed", json!({}), &at(0)),
    )
    .await;
    tick().await;
    let marker = received(
        &app,
        "/v1/events/e-marker",
        &ev(AUDITOR, "reported_in", json!({}), &at(0)),
    )
    .await;
    tick().await;
    let (_, answer) = app.get("/v1/events?type=entered&limit=1").await;
    let as_of_before_last = answer["as_of"].as_str().unwrap_or_default().to_owned();
    tick().await;

    put(
        &app,
        "/v1/events/e-se3-out",
        &ev(SE3, "exited", json!({}), &at(0)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/v1/events/e-tpm-out",
        &ev(TPM, "exited", json!({}), &at(0)),
    )
    .await;
    Sample {
        app,
        ids,
        se1_entered,
        se2_entered,
        model_changed,
        marker,
        as_of_before_last,
    }
}

/// 200 で答え、as_of が付いていることを見て、答えを返す。
async fn ok(app: &App, uri: &str) -> Value {
    let (status, answer) = app.get(uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {answer}");
    assert!(answer["as_of"].is_string(), "{uri}: as_of が無い {answer}");
    answer
}

fn names(items: &Value) -> Vec<&str> {
    items
        .as_array()
        .unwrap_or_else(|| panic!("配列でない: {items}"))
        .iter()
        .map(|a| a["name"].as_str().unwrap_or("?"))
        .collect()
}

fn field<'a>(items: &'a Value, key: &str) -> Vec<&'a str> {
    items
        .as_array()
        .unwrap_or_else(|| panic!("配列でない: {items}"))
        .iter()
        .map(|a| a[key].as_str().unwrap_or("null"))
        .collect()
}

fn sorted(mut v: Vec<&str>) -> Vec<&str> {
    v.sort_unstable();
    v
}

// --- 問いごと ---

#[tokio::test]
async fn 問い_workspace_w_に時刻_t_に居たのは誰か() {
    let s = sample().await;
    let uri = |at: &str| format!("/v1/actors?binding=cmux.workspace.ref:workspace:3&at={at}");
    let answer = ok(&s.app, &uri(&s.se1_entered)).await;
    assert_eq!(names(&answer["actors"]), ["se.1"], "{answer}");
    // 一覧の形は、ほかの検索と同じ（url で横顔の頁へ）。
    assert_eq!(
        answer["actors"][0]["url"],
        format!("/actors/{}", s.id("se.1"))
    );
    let answer = ok(&s.app, &uri(&s.marker)).await;
    assert_eq!(names(&answer["actors"]), ["se.3"], "{answer}");
    let answer = ok(&s.app, "/v1/actors?binding=cmux.workspace.ref:workspace:3").await;
    assert_eq!(names(&answer["actors"]), ["se.3"], "{answer}");
    let answer = ok(&s.app, &uri("2000-01-01T00:00:00Z")).await;
    assert_eq!(answer["actors"], json!([]), "{answer}");
    assert_eq!(answer["next"], Value::Null);
}

#[tokio::test]
async fn 問い_key_のアクター_clear_で足された者も() {
    let s = sample().await;
    let answer = ok(&s.app, &format!("/v1/actors?label=tpm.key:{KEY}")).await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "se.1", "se.2", "devops.1", "auditor.1", "se.3"],
        "初めの登場の順: {answer}"
    );
    assert_eq!(answer["next"], Value::Null);
    // ラベルを 2 つ重ねれば、どちらも持つ者だけ。
    let answer = ok(
        &s.app,
        &format!("/v1/actors?label=tpm.key:{KEY}&label=tpm.role:software-engineer"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["se.1", "se.2", "se.3"],
        "{answer}"
    );
}

#[tokio::test]
async fn 答えの一つ一つの形() {
    let s = sample().await;
    let answer = ok(&s.app, "/v1/actors?label=tpm.role:auditor").await;
    let item = &answer["actors"][0];
    let id = s.id("auditor.1");
    assert_eq!(item["id"], id);
    assert_eq!(item["name"], "auditor.1");
    assert_eq!(item["kind"], "ai");
    assert_eq!(item["url"], format!("/actors/{id}"));
    assert_eq!(
        item["labels"],
        json!({ "project": "castlist", "tpm.key": KEY, "tpm.role": "auditor" })
    );
    assert!(
        item["first_entered_at"]
            .as_str()
            .is_some_and(|t| t.len() == 24),
        "{item}"
    );
    assert_eq!(item["last_exited_at"], Value::Null);
    assert_eq!(item["last_exit_type"], Value::Null);
    assert_eq!(item["present"], true);
    assert_eq!(item["model"], OPUS);
    assert_eq!(item["effort"], "high");
    let answer = ok(&s.app, "/v1/actors?label=tpm.role:devops").await;
    let item = &answer["actors"][0];
    assert_eq!(item["present"], false);
    assert_eq!(item["last_exit_type"], "closed");
    assert!(
        item["last_exited_at"]
            .as_str()
            .is_some_and(|t| t.len() == 24),
        "{item}"
    );
}

#[tokio::test]
async fn 問い_時刻の窓に居たアクター() {
    let s = sample().await;
    let window = |from: &str, to: &str| format!("/v1/actors?active_from={from}&active_to={to}");
    // 印の時: se.1 は /clear で、se.2 と devops.1 は退いた後。退かない auditor.1 は今まで居る。
    let answer = ok(&s.app, &window(&s.marker, &s.marker)).await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "auditor.1", "se.3"],
        "{answer}"
    );
    // /model の時: 初めの 5 席が居た（se.3 はまだ）。
    let answer = ok(&s.app, &window(&s.model_changed, &s.model_changed)).await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "se.1", "se.2", "devops.1", "auditor.1"],
        "{answer}"
    );
    // 窓の片方だけでもよい。
    let answer = ok(&s.app, &format!("/v1/actors?active_from={}", s.marker)).await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "auditor.1", "se.3"],
        "{answer}"
    );
    let answer = ok(&s.app, &format!("/v1/actors?active_to={}", s.se1_entered)).await;
    assert_eq!(names(&answer["actors"]), ["tpm.1", "se.1"], "{answer}");
    let answer = ok(
        &s.app,
        &window("2000-01-01T00:00:00Z", "2000-01-02T00:00:00Z"),
    )
    .await;
    assert_eq!(answer["actors"], json!([]));
}

#[tokio::test]
async fn 問い_誰が起こしたか_人の根まで_と_誰を起こしたか() {
    let s = sample().await;
    let lineage = |name: &str, q: &str| format!("/v1/actors/{}/lineage?{q}", s.id(name));
    let answer = ok(&s.app, &lineage("se.3", "direction=up")).await;
    assert_eq!(
        sorted(names(&answer["nodes"])),
        ["pm.1", "se.1", "se.3", "tpm.1"],
        "人の pm.1 で止まる（owner.1 へは上らない）: {answer}"
    );
    let edges: BTreeSet<(String, String, String)> = answer["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            assert!(e["received_at"].is_string(), "{e}");
            (
                e["type"].as_str().unwrap().to_owned(),
                e["from"].as_str().unwrap().to_owned(),
                e["to"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let edge = |kind: &str, from: &str, to: &str| {
        (kind.to_owned(), s.id(from).to_owned(), s.id(to).to_owned())
    };
    assert_eq!(
        edges,
        BTreeSet::from([
            edge("continued", "se.1", "se.3"),
            edge("launched", "tpm.1", "se.1"),
            edge("launched", "pm.1", "tpm.1"),
        ])
    );
    let node = answer["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "se.3")
        .unwrap();
    assert_eq!(node["url"], format!("/actors/{}", s.id("se.3")));
    assert_eq!(node["labels"]["tpm.key"], KEY);

    // 下へは、既定で 1 段。
    let answer = ok(&s.app, &lineage("tpm.1", "direction=down")).await;
    assert_eq!(
        sorted(names(&answer["nodes"])),
        ["auditor.1", "devops.1", "se.1", "se.2", "tpm.1"],
        "{answer}"
    );
    assert_eq!(answer["edges"].as_array().unwrap().len(), 4, "{answer}");
    let answer = ok(&s.app, &lineage("tpm.1", "direction=down&depth=2")).await;
    assert!(names(&answer["nodes"]).contains(&"se.3"), "{answer}");
    let answer = ok(
        &s.app,
        &lineage("tpm.1", "direction=down&depth=2&type=launched"),
    )
    .await;
    assert!(!names(&answer["nodes"]).contains(&"se.3"), "{answer}");
    // 既定は上下とも。
    let answer = ok(&s.app, &lineage("se.1", "")).await;
    assert_eq!(
        sorted(names(&answer["nodes"])),
        ["pm.1", "se.1", "se.3", "tpm.1"],
        "{answer}"
    );
    // 人からは上らない。
    let answer = ok(&s.app, &lineage("pm.1", "direction=up")).await;
    assert_eq!(names(&answer["nodes"]), ["pm.1"], "{answer}");
    assert_eq!(answer["edges"], json!([]));
    // 線の種類で絞る: se.3 は起こされていない（継いだだけ）。
    let answer = ok(&s.app, &lineage("se.3", "direction=up&type=launched")).await;
    assert_eq!(names(&answer["nodes"]), ["se.3"], "{answer}");

    let (status, answer) = s.app.get("/v1/actors/no-such/lineage").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    // 起こした者で、アクターを探す。
    let answer = ok(&s.app, &format!("/v1/actors?launched_by={}", s.id("tpm.1"))).await;
    assert_eq!(
        names(&answer["actors"]),
        ["se.1", "se.2", "devops.1", "auditor.1"],
        "{answer}"
    );
}

#[tokio::test]
async fn 問い_役と時刻で絞る() {
    let s = sample().await;
    let role = "label=tpm.role:software-engineer";
    let answer = ok(
        &s.app,
        &format!("/v1/actors?{role}&active_from={0}&active_to={0}", s.marker),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["se.3"], "{answer}");
    let answer = ok(
        &s.app,
        &format!(
            "/v1/actors?{role}&active_from={0}&active_to={0}",
            s.model_changed
        ),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["se.1", "se.2"], "{answer}");
    let answer = ok(&s.app, &format!("/v1/actors?{role}&kind=human")).await;
    assert_eq!(answer["actors"], json!([]));
    let answer = ok(&s.app, "/v1/actors?label=project:castlist&kind=human").await;
    assert_eq!(names(&answer["actors"]), ["owner.1", "pm.1"], "{answer}");
}

#[tokio::test]
async fn 問い_model_m_か_effort_e_で動いたことのあるアクター() {
    let s = sample().await;
    let answer = ok(&s.app, &format!("/v1/actors?model={SONNET}")).await;
    assert_eq!(names(&answer["actors"]), ["se.2"], "{answer}");
    // 一覧の model は最新の物。
    assert_eq!(answer["actors"][0]["model"], SONNET);
    let answer = ok(&s.app, "/v1/actors?effort=max").await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1"],
        "今は high に戻っていても: {answer}"
    );
    assert_eq!(answer["actors"][0]["effort"], "high");
    let answer = ok(
        &s.app,
        &format!("/v1/actors?model={OPUS}&label=tpm.key:{KEY}"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "se.1", "se.2", "devops.1", "auditor.1", "se.3"],
        "{answer}"
    );
    let answer = ok(&s.app, "/v1/actors?model=no-such-model").await;
    assert_eq!(answer["actors"], json!([]));
}

#[tokio::test]
async fn 問い_x_の時刻_t_の_model_と_effort() {
    let s = sample().await;
    let uri = |q: &str| format!("/v1/actors/{}/settings{q}", s.id("se.2"));
    let answer = ok(&s.app, &uri(&format!("?at={}", s.se2_entered))).await;
    let row = &answer["setting"];
    assert_eq!(row["model"], OPUS, "{answer}");
    assert_eq!(row["effort"], "high");
    assert_eq!(row["source"], "requested");
    assert!(
        row["model_id"].is_i64() && row["effort_id"].is_i64(),
        "{row}"
    );
    assert!(
        row["sent_at"].is_string() && row["received_at"].is_string(),
        "{row}"
    );
    assert_eq!(row["clock_skew"], false);
    let answer = ok(&s.app, &uri(&format!("?at={}", s.model_changed))).await;
    assert_eq!(answer["setting"]["model"], SONNET, "{answer}");
    assert_eq!(answer["setting"]["source"], "observed");
    assert_eq!(answer["setting"]["id"], "t-model");
    let answer = ok(&s.app, &uri("?at=2000-01-01T00:00:00Z")).await;
    assert_eq!(answer["setting"], Value::Null, "{answer}");
    // at が無ければ、全部を受けた順で。
    let answer = ok(&s.app, &uri("")).await;
    assert_eq!(
        field(&answer["settings"], "model"),
        [OPUS, SONNET],
        "{answer}"
    );
    assert_eq!(
        field(&answer["settings"], "source"),
        ["requested", "observed"]
    );
    let (status, _) = s.app.get("/v1/actors/no-such/settings").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 問い_見た物が頼んだ物と違う行() {
    let s = sample().await;
    let answer = ok(&s.app, "/v1/settings?mismatch=true").await;
    assert_eq!(
        field(&answer["settings"], "id"),
        ["t-model", "t-max"],
        "{answer}"
    );
    assert_eq!(field(&answer["settings"], "actor_name"), ["se.2", "tpm.1"]);
    assert_eq!(answer["settings"][0]["actor_id"], s.id("se.2"));
    assert_eq!(answer["next"], Value::Null);
    // ほかの絞り: model・effort・source・時刻の窓。
    let answer = ok(&s.app, "/v1/settings?effort=max").await;
    assert_eq!(field(&answer["settings"], "id"), ["t-max"], "{answer}");
    let answer = ok(
        &s.app,
        &format!("/v1/settings?model={SONNET}&source=observed"),
    )
    .await;
    assert_eq!(field(&answer["settings"], "id"), ["t-model"], "{answer}");
    let answer = ok(
        &s.app,
        &format!("/v1/settings?source=observed&from={}", s.model_changed),
    )
    .await;
    assert_eq!(
        field(&answer["settings"], "id"),
        ["t-model", "t-max", "t-back"],
        "{answer}"
    );
    let answer = ok(
        &s.app,
        &format!("/v1/settings?to={}&source=observed", s.se2_entered),
    )
    .await;
    assert_eq!(answer["settings"], json!([]), "{answer}");
}

#[tokio::test]
async fn 問い_退かないアクターと_退き方() {
    let s = sample().await;
    let answer = ok(
        &s.app,
        &format!("/v1/actors?label=tpm.key:{KEY}&active=true"),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["auditor.1"], "{answer}");
    let answer = ok(
        &s.app,
        &format!("/v1/actors?label=tpm.key:{KEY}&active=false"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "se.1", "se.2", "devops.1", "se.3"],
        "{answer}"
    );
    // 最後の退場の順。退いていない者は先（昇順）。
    let answer = ok(
        &s.app,
        &format!("/v1/actors?label=tpm.key:{KEY}&sort=exited"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["auditor.1", "se.1", "se.2", "devops.1", "se.3", "tpm.1"],
        "{answer}"
    );
    assert_eq!(
        field(&answer["actors"], "last_exit_type"),
        ["null", "cleared", "exited", "closed", "exited", "exited"]
    );
    let answer = ok(
        &s.app,
        &format!("/v1/actors?label=tpm.key:{KEY}&sort=exited&order=desc"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "se.3", "devops.1", "se.2", "se.1", "auditor.1"],
        "{answer}"
    );
}

#[tokio::test]
async fn 問い_出来事を種類と時刻で_アクターをまたいで() {
    let s = sample().await;
    let answer = ok(&s.app, "/v1/events?type=model_unreadable").await;
    assert_eq!(
        field(&answer["events"], "id"),
        ["e-unread-1", "e-unread-2"],
        "{answer}"
    );
    assert_eq!(
        field(&answer["events"], "actor_name"),
        ["devops.1", "auditor.1"]
    );
    assert_eq!(answer["events"][0]["actor_id"], s.id("devops.1"));
    assert_eq!(
        answer["events"][0]["detail"]["reason"],
        "statusline-missing"
    );
    assert!(answer["events"][0]["sent_at"].is_string());
    assert!(answer["events"][0]["received_at"].is_string());
    assert_eq!(answer["events"][0]["clock_skew"], false);
    assert_eq!(answer["next"], Value::Null);
    let answer = ok(&s.app, "/v1/events?type=model_unreadable&reason=timeout").await;
    assert_eq!(field(&answer["events"], "id"), ["e-unread-2"], "{answer}");
    let answer = ok(&s.app, "/v1/events?type=cleared&type=exited&type=closed").await;
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
    let answer = ok(&s.app, &format!("/v1/events?actor={se3}")).await;
    assert_eq!(
        field(&answer["events"], "id"),
        ["e-se3-in", "e-se3-out"],
        "{answer}"
    );
    let answer = ok(&s.app, &format!("/v1/events?actor={se3}&source=clear")).await;
    assert_eq!(field(&answer["events"], "id"), ["e-se3-in"], "{answer}");
    let answer = ok(
        &s.app,
        &format!("/v1/events?type=entered&to={}", s.se1_entered),
    )
    .await;
    assert_eq!(
        field(&answer["events"], "id"),
        ["e-tpm-in", "e-se1-in"],
        "{answer}"
    );
}

#[tokio::test]
async fn 問い_時刻_t_から後に新しく起きた物() {
    let s = sample().await;
    let since = &s.as_of_before_last;
    let answer = ok(&s.app, &format!("/v1/events?from={since}")).await;
    assert_eq!(
        field(&answer["events"], "id"),
        ["e-se3-out", "e-tpm-out"],
        "{answer}"
    );
    let answer = ok(&s.app, &format!("/v1/settings?from={since}")).await;
    assert_eq!(answer["settings"], json!([]), "{answer}");
    let answer = ok(&s.app, &format!("/v1/actors?active_from={since}")).await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "auditor.1", "se.3"],
        "{answer}"
    );
}

#[tokio::test]
async fn 問い_送り手の時計がずれた行() {
    let s = sample().await;
    let answer = ok(&s.app, "/v1/events?skewed=true").await;
    assert_eq!(field(&answer["events"], "id"), ["e-skew"], "{answer}");
    let row = &answer["events"][0];
    assert_eq!(row["clock_skew"], true);
    assert_ne!(row["sent_at"], row["received_at"]);
    let answer = ok(
        &s.app,
        &format!("/v1/actors?skewed=true&label=tpm.key:{KEY}"),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["auditor.1"], "{answer}");
    let answer = ok(&s.app, "/v1/settings?skewed=true").await;
    assert_eq!(answer["settings"], json!([]), "{answer}");
}

// --- 頁を分ける ---

/// limit=1 で after を辿る。頁の間に新しいアクターを足しても、元の組に穴も重なりも無く、終わりで next が null。
async fn walk(app: &App, query: &str, insert_from: usize) -> Vec<String> {
    let mut seen = Vec::new();
    let mut after: Option<String> = None;
    for page in 0..100 {
        let uri = match &after {
            Some(cursor) => format!("/v1/actors?{query}&limit=1&after={cursor}"),
            None => format!("/v1/actors?{query}&limit=1"),
        };
        let answer = ok(app, &uri).await;
        let actors = answer["actors"].as_array().unwrap();
        assert!(actors.len() <= 1, "{answer}");
        seen.extend(actors.iter().map(|a| a["id"].as_str().unwrap().to_owned()));
        // 頁の間に、登場する者と、まだ登場しない者を足す。
        if page < 6 {
            let session = format!("s-new-{insert_from}-{page}");
            app.register(&seat("new", &session, "software-engineer", "workspace:9"))
                .await;
            if page % 2 == 0 {
                put(
                    app,
                    &format!("/v1/events/e-new-{insert_from}-{page}"),
                    &ev(
                        &format!("claude.session:{session}"),
                        "entered",
                        json!({}),
                        &at(0),
                    ),
                )
                .await;
            }
            tick().await;
        }
        match answer["next"].as_str() {
            Some(cursor) => after = Some(cursor.to_owned()),
            None => {
                assert!(answer["next"].is_null(), "{answer}");
                return seen;
            }
        }
    }
    panic!("100 頁で終わらない");
}

#[tokio::test]
async fn 頁を_after_で辿ると_足されても穴も重なりも無い() {
    let s = sample().await;
    let query = "label=project:castlist";
    for (n, order) in [("entered", "asc"), ("exited", "desc")]
        .into_iter()
        .enumerate()
    {
        let (sort, order) = order;
        let q = format!("{query}&sort={sort}&order={order}");
        let before = ok(&s.app, &format!("/v1/actors?{q}")).await;
        let want: Vec<String> = before["actors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap().to_owned())
            .collect();
        // 見本の 8 と、前の辿りで足した 6 つずつ。
        assert_eq!(want.len(), 8 + 6 * n, "{before}");
        let seen = walk(&s.app, &q, n).await;
        let original: Vec<&String> = seen.iter().filter(|id| want.contains(id)).collect();
        assert_eq!(original, want.iter().collect::<Vec<_>>(), "{sort} {order}");
        let unique: BTreeSet<&String> = seen.iter().collect();
        assert_eq!(unique.len(), seen.len(), "重なり: {seen:?}");
    }
}

#[tokio::test]
async fn 出来事と_model_と_effort_の頁も_after_で辿れる() {
    let s = sample().await;
    for (path, key) in [
        ("/v1/events?type=entered", "events"),
        ("/v1/settings?source=requested", "settings"),
    ] {
        let all = ok(&s.app, path).await;
        let want = field(&all[key], "id")
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut seen = Vec::new();
        let mut uri = format!("{path}&limit=2");
        loop {
            let answer = ok(&s.app, &uri).await;
            seen.extend(field(&answer[key], "id").into_iter().map(str::to_owned));
            match answer["next"].as_str() {
                Some(cursor) => uri = format!("{path}&limit=2&after={cursor}"),
                None => break,
            }
        }
        assert_eq!(seen, want, "{path}");
    }
}

// --- 誤り ---

#[tokio::test]
async fn 知らない引数_読めない時刻_大きすぎる_limit_読めない_after_は_400_で引数の名を言う() {
    let s = sample().await;
    let se1 = s.id("se.1").to_owned();
    let cases = [
        (
            "/v1/actors?label=project:castlist&colour=red".to_owned(),
            "colour",
        ),
        (
            "/v1/actors?label=project:castlist&active_from=きのう".to_owned(),
            "active_from",
        ),
        (
            "/v1/actors?binding=cmux.workspace.ref:workspace:3&at=2026-13-01".to_owned(),
            "at",
        ),
        (
            "/v1/actors?label=project:castlist&limit=1001".to_owned(),
            "limit",
        ),
        (
            "/v1/actors?label=project:castlist&limit=0".to_owned(),
            "limit",
        ),
        (
            "/v1/actors?label=project:castlist&after=garbage".to_owned(),
            "after",
        ),
        (
            "/v1/actors?label=project:castlist&sort=name".to_owned(),
            "sort",
        ),
        (
            "/v1/actors?label=project:castlist&active=yes".to_owned(),
            "active",
        ),
        ("/v1/actors?label=castlist".to_owned(), "label"),
        ("/v1/actors?kind=ai".to_owned(), "label"),
        (format!("/v1/actors/{se1}/lineage?depth=21"), "depth"),
        (
            format!("/v1/actors/{se1}/lineage?direction=sideways"),
            "direction",
        ),
        (format!("/v1/actors/{se1}/lineage?type=married"), "type"),
        (format!("/v1/actors/{se1}/lineage?colour=red"), "colour"),
        (format!("/v1/actors/{se1}/settings?at=noon"), "at"),
        (format!("/v1/actors/{se1}/settings?colour=red"), "colour"),
        ("/v1/settings?mismatch=true&colour=red".to_owned(), "colour"),
        ("/v1/settings?from=noon".to_owned(), "from"),
        ("/v1/settings?source=guessed".to_owned(), "source"),
        ("/v1/settings?limit=1001".to_owned(), "limit"),
        ("/v1/settings?after=zz".to_owned(), "after"),
        ("/v1/events?type=entered&colour=red".to_owned(), "colour"),
        ("/v1/events?type=danced".to_owned(), "type"),
        ("/v1/events?to=noon".to_owned(), "to"),
        ("/v1/events?limit=1001".to_owned(), "limit"),
        ("/v1/events?after=%E3%81%82".to_owned(), "after"),
    ];
    for (uri, name) in cases {
        let (status, answer) = s.app.get(&uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
        let message = answer["error"].as_str().unwrap_or_default();
        assert!(message.contains(name), "{uri}: {name} を言わない: {answer}");
        assert!(
            message
                .chars()
                .any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)),
            "{uri}: 日本語でない: {answer}"
        );
    }
    // 別の検索の after は使えない。
    let events = ok(&s.app, "/v1/events?type=entered&limit=1").await;
    let cursor = events["next"].as_str().unwrap().to_owned();
    let (status, answer) = s
        .app
        .get(&format!("/v1/actors?label=project:castlist&after={cursor}"))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert!(
        answer["error"]
            .as_str()
            .unwrap_or_default()
            .contains("after")
    );
}

// --- 索引 ---

#[tokio::test]
async fn 検索の_sql_は大きな表を端から読まない() {
    let s = sample().await;
    let plans = plans();
    let names: Vec<&str> = plans.iter().map(|(name, _)| *name).collect();
    for want in [
        "actors_label",
        "actors_labels",
        "actors_binding",
        "actors_window",
        "actors_model",
        "actors_effort",
        "actors_skewed",
        "actors_launched_by",
        "actors_active",
        "actors_after",
        "actors_exited_desc",
        "actors_all",
        "actor_items",
        "actor_labels",
        "lineage_kind",
        "lineage_up",
        "lineage_down",
        "actor_settings_at",
        "actor_settings",
        "settings",
        "settings_model",
        "settings_effort",
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
    ] {
        assert!(names.contains(&want), "{want} が plans に無い: {names:?}");
    }
    for (name, sql) in &plans {
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

// --- 待たせない ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 書く鍵を_2_秒握っている間も_検索は_100_ms_で答える() {
    let s = sample().await;
    let app = &s.app;
    let se2 = s.id("se.2").to_owned();
    let tpm = s.id("tpm.1").to_owned();
    let searches: Vec<String> = vec![
        format!("/v1/actors?label=tpm.key:{KEY}"),
        format!("/v1/actors?active_from={0}&active_to={0}", s.marker),
        "/v1/actors?binding=cmux.workspace.ref:workspace:3".to_owned(),
        format!("/v1/actors/{se2}/lineage"),
        format!("/v1/actors/{se2}/settings?at={}", s.model_changed),
        "/v1/settings?mismatch=true".to_owned(),
        format!("/v1/settings?model={SONNET}"),
        "/v1/events?type=model_unreadable".to_owned(),
        format!("/v1/events?actor={tpm}"),
        format!("/v1/actors?launched_by={tpm}&sort=exited&order=desc"),
    ];
    let mut tx = app.db.write().begin().await.unwrap();
    sqlx::query("insert into models(name, first_seen_at) values('lock-holder', ?1)")
        .bind(at(0))
        .execute(&mut *tx)
        .await
        .unwrap();
    let held = Instant::now();
    let mut slowest = Duration::ZERO;
    for round in 0..10 {
        let tasks: Vec<_> = (0..10)
            .map(|i| {
                let uri = searches[(round + i) % searches.len()].clone();
                let router = app.app.clone();
                tokio::spawn(async move {
                    let started = Instant::now();
                    let response = router.oneshot(request("GET", &uri, None)).await.unwrap();
                    (uri, response.status(), started.elapsed())
                })
            })
            .collect();
        for task in tasks {
            let (uri, status, took) = task.await.unwrap();
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert!(took < Duration::from_millis(100), "{uri}: {took:?}");
            slowest = slowest.max(took);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    assert!(held.elapsed() < Duration::from_secs(2) + Duration::from_millis(500));
    tokio::time::sleep(Duration::from_secs(2).saturating_sub(held.elapsed())).await;
    tx.rollback().await.unwrap();
    println!("一番遅い検索: {slowest:?}");
}
