//! 監査の検索の見本（探す操作の試験で共に使う）。Auditor が台帳の件から持てる物（project・Key・役・workspace・時刻）で
//! 引けるよう、0.4.x の server の tests/search.rs と同じ記録を、書く操作（RegisterActor・PutEvent・PutLink・PutSetting）だけで作る。
use std::collections::BTreeMap;
use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{App, at};

pub const KEY: &str = "t-23zs";
pub const OPUS: &str = "claude-opus-5-5";
pub const SONNET: &str = "claude-sonnet-5";

pub const OWNER: &str = "tpm.person:owner";
pub const PM: &str = "tpm.person:pm";
pub const TPM: &str = "claude.session:s-tpm";
pub const SE1: &str = "claude.session:s-se1";
pub const SE2: &str = "claude.session:s-se2";
pub const DEVOPS: &str = "claude.session:s-devops";
pub const AUDITOR: &str = "claude.session:s-auditor";
pub const SE3: &str = "claude.session:s-se3";

/// 受けた時刻（ミリ秒まで）が、次の段と重ならないように少し待つ。
pub async fn tick() {
    tokio::time::sleep(Duration::from_millis(5)).await;
}

/// 席のアクターの登録の本文（鍵は claude.session、workspace も付ける。頼んだ model と effort は opus と high）。
pub fn seat(prefix: &str, session: &str, role: &str, workspace: &str) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "ai",
        "namePrefix": prefix,
        "labels": { "project": "castlist", "tpm.key": KEY, "tpm.role": role },
        "bindings": [
            { "type": "claude.session", "value": session },
            { "type": "cmux.workspace.ref", "value": workspace },
        ],
        "settings": { "model": OPUS, "effort": "high" },
        "sentAt": at(0),
    })
}

/// 人のアクターの登録の本文（鍵は tpm.person）。
pub fn person(prefix: &str, person: &str) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "human",
        "namePrefix": prefix,
        "labels": { "project": "castlist", "tpm.role": "product-manager" },
        "bindings": [{ "type": "tpm.person", "value": person }],
        "sentAt": at(0),
    })
}

/// 出来事の本文（送り手の時刻を決める）。
pub fn ev(by: &str, kind: &str, detail: Value, sent_at: &str) -> Value {
    json!({ "issuer": "tpm", "by": by, "type": kind, "detail": detail, "sentAt": sent_at })
}

pub fn line(kind: &str, from: &str, to: &str) -> Value {
    json!({ "issuer": "tpm", "type": kind, "from": from, "to": to, "sentAt": at(0) })
}

pub fn observed(by: &str, model: &str, effort: &str) -> Value {
    json!({ "issuer": "tpm", "by": by, "model": model, "effort": effort, "source": "observed", "sentAt": at(0) })
}

/// 送って 201 か 200 であることを見て、答えを返す。
pub async fn put(app: &App, uri: &str, body: &Value) -> Value {
    let (status, _, answer) = app.put(uri, body).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "{uri}: {status} {answer}"
    );
    answer
}

/// 送って、受けた時刻（答えの receivedAt）を返す。
async fn received(app: &App, uri: &str, body: &Value) -> String {
    put(app, uri, body).await["receivedAt"]
        .as_str()
        .expect("receivedAt")
        .to_owned()
}

/// 見本の記録。名から id を引く物と、段の間の時刻の印。
pub struct Sample {
    pub app: App,
    pub ids: BTreeMap<&'static str, String>,
    /// se.1 が登場した時（workspace:3 は se.1 の物）。
    pub se1_entered: String,
    /// se.2 が登場した時（頼んだ opus のまま）。
    pub se2_entered: String,
    /// se.2 の /model（見た sonnet）を受けた時。
    pub model_changed: String,
    /// se.2 と devops.1 が退いた後、se.3 と tpm.1 が退く前の印（auditor.1 の reported_in）。
    pub marker: String,
    /// 印の後、最後の段の前に引いた答えの asOf。
    pub as_of_before_last: String,
}

impl Sample {
    pub fn id(&self, name: &str) -> &str {
        self.ids
            .get(name)
            .unwrap_or_else(|| panic!("{name} が無い"))
    }
}

/// 人の owner → PM → TPM → SWE×2・DevOps・Auditor。se.1 は /clear して se.3 に継がれる。
/// se.2 は /model で sonnet に替わる（見た行）。tpm.1 は頼んだ high と違う max を見られ、後で high に戻る。
/// auditor.1 は退かず、送り手の時計が 1 時間ずれた出来事を 1 つ持つ。devops.1 と auditor.1 は model を読めなかった（訳は detail.reason）。
pub async fn sample() -> Sample {
    let app = App::start().await;
    let mut ids = BTreeMap::new();
    let mut reg = async |name: &'static str, body: Value| {
        let answer = app.register(&body).await;
        assert_eq!(answer["actor"]["name"], name, "{answer}");
        ids.insert(name, answer["actor"]["id"].as_str().unwrap().to_owned());
        tick().await;
    };
    reg("owner.1", person("owner", "owner")).await;
    reg("pm.1", person("pm", "pm")).await;
    put(&app, "/api/links/l-owner-pm", &line("launched", OWNER, PM)).await;
    reg("tpm.1", seat("tpm", "s-tpm", "tpm", "workspace:1")).await;
    put(&app, "/api/links/l-pm-tpm", &line("launched", PM, TPM)).await;
    put(
        &app,
        "/api/events/e-tpm-in",
        &ev(TPM, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;

    reg(
        "se.1",
        seat("se", "s-se1", "software-engineer", "workspace:3"),
    )
    .await;
    put(&app, "/api/links/l-tpm-se1", &line("launched", TPM, SE1)).await;
    let se1_entered = received(
        &app,
        "/api/events/e-se1-in",
        &ev(SE1, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;
    reg(
        "se.2",
        seat("se", "s-se2", "software-engineer", "workspace:4"),
    )
    .await;
    put(&app, "/api/links/l-tpm-se2", &line("launched", TPM, SE2)).await;
    let se2_entered = received(
        &app,
        "/api/events/e-se2-in",
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
        "/api/links/l-tpm-devops",
        &line("launched", TPM, DEVOPS),
    )
    .await;
    put(
        &app,
        "/api/events/e-devops-in",
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
        "/api/links/l-tpm-auditor",
        &line("launched", TPM, AUDITOR),
    )
    .await;
    put(
        &app,
        "/api/events/e-auditor-in",
        &ev(AUDITOR, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;

    // /model: se.2 が sonnet に替わったのを見た（頼んだ opus と違う）。
    let model_changed = received(
        &app,
        "/api/settings/t-model",
        &observed(SE2, SONNET, "high"),
    )
    .await;
    tick().await;
    // tpm.1: 頼んだ high と違う max を見て、後で high に戻ったのを見た。
    put(&app, "/api/settings/t-max", &observed(TPM, OPUS, "max")).await;
    tick().await;
    put(&app, "/api/settings/t-back", &observed(TPM, OPUS, "high")).await;
    tick().await;
    put(
        &app,
        "/api/events/e-unread-1",
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
        "/api/events/e-skew",
        &ev(AUDITOR, "reported_in", json!({}), &at(3600)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/api/events/e-unread-2",
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
        "/api/events/e-se1-clear",
        &ev(SE1, "cleared", json!({}), &at(0)),
    )
    .await;
    tick().await;
    reg(
        "se.3",
        seat("se", "s-se3", "software-engineer", "workspace:3"),
    )
    .await;
    put(&app, "/api/links/l-se1-se3", &line("continued", SE1, SE3)).await;
    put(
        &app,
        "/api/events/e-se3-in",
        &ev(SE3, "entered", json!({ "source": "clear" }), &at(0)),
    )
    .await;
    tick().await;

    put(
        &app,
        "/api/events/e-se2-out",
        &ev(SE2, "exited", json!({}), &at(0)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/api/events/e-devops-out",
        &ev(DEVOPS, "closed", json!({ "inferred": true }), &at(0)),
    )
    .await;
    tick().await;
    let marker = received(
        &app,
        "/api/events/e-marker",
        &ev(AUDITOR, "reported_in", json!({}), &at(0)),
    )
    .await;
    tick().await;
    let (_, _, answer) = app.get("/api/actors?label=project:castlist").await;
    let as_of_before_last = answer["asOf"].as_str().unwrap_or_default().to_owned();
    tick().await;

    put(
        &app,
        "/api/events/e-se3-out",
        &ev(SE3, "exited", json!({}), &at(0)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/api/events/e-tpm-out",
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

/// 200 で答え、asOf が付いていることを見て、答えを返す。
pub async fn ok(app: &App, uri: &str) -> Value {
    let (status, _, answer) = app.get(uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {answer}");
    assert!(answer["asOf"].is_string(), "{uri}: asOf が無い {answer}");
    answer
}

pub fn names(items: &Value) -> Vec<&str> {
    items
        .as_array()
        .unwrap_or_else(|| panic!("配列でない: {items}"))
        .iter()
        .map(|a| a["name"].as_str().unwrap_or("?"))
        .collect()
}

pub fn field<'a>(items: &'a Value, key: &str) -> Vec<&'a str> {
    items
        .as_array()
        .unwrap_or_else(|| panic!("配列でない: {items}"))
        .iter()
        .map(|a| a[key].as_str().unwrap_or("null"))
        .collect()
}
