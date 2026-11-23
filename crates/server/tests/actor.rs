//! GetActor の振る舞い（処理より先に書く）。出来事・線・見た model と effort の道はまだ無いので、その行は書く接続で直に置く。
mod common;

use axum::http::StatusCode;
use common::{App, ai, at, error_type, human};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// 登録の答えの、アクターの id。
fn id(answer: &Value) -> String {
    answer["actor"]["id"].as_str().expect("id").to_owned()
}

fn t(v: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(v.as_str().unwrap_or_else(|| panic!("時刻: {v}")), &Rfc3339)
        .unwrap_or_else(|e| panic!("時刻 {v}: {e}"))
}

impl App {
    /// アクターを引き、200 であることを見て本文を返す。
    async fn actor(&self, id: &str) -> Value {
        let (status, _, answer) = self.get(&format!("/api/actors/{id}")).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        answer
    }
}

#[tokio::test]
async fn an_actor_comes_with_everything_attached_in_camel_case() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": "cmux.workspace.ref", "value": "workspace:1" }));
    body["settings"] = json!({ "model": "claude-opus-5-5", "effort": "high" });
    let se1 = id(&app.register(&body).await);
    let se2 = id(&app.register(&ai("se", "s-2")).await);
    let pm = id(&app.register(&human("pm")).await);
    app.link("l-1", "launched", &pm, &se1).await;
    app.link("l-2", "continued", &se1, &se2).await;
    app.event("e-1", &se1, "entered", r#"{"source":"startup"}"#)
        .await;
    app.observed("o-1", &se1, "claude-sonnet-5", "medium").await;

    let before = OffsetDateTime::now_utc();
    let actor = app.actor(&se1).await;
    assert_eq!(actor["id"], se1.as_str());
    assert_eq!(actor["name"], "se.1");
    assert_eq!(actor["kind"], "ai");
    assert_eq!(actor["issuer"], "tpm");
    assert_eq!(actor["clockSkew"], false);
    assert!(actor["sentAt"].as_str().unwrap().ends_with('Z'), "{actor}");
    assert!(
        actor["receivedAt"].as_str().unwrap().ends_with('Z'),
        "{actor}"
    );
    // asOf は読み始める前の castlist の時刻。
    let as_of = t(&actor["asOf"]);
    assert!(as_of >= t(&actor["receivedAt"]), "{actor}");
    assert!(as_of <= before + time::Duration::seconds(1), "{actor}");
    assert_eq!(actor["labels"], json!({ "project": "castlist" }));

    assert_eq!(actor["bindings"].as_array().unwrap().len(), 2, "{actor}");
    let key = &actor["bindings"][0];
    assert_eq!(key["issuer"], "tpm");
    assert_eq!(key["type"], "claude.session");
    assert_eq!(key["value"], "s-1");
    assert_eq!(key["isKey"], true);
    assert!(key["sentAt"].is_string() && key["receivedAt"].is_string());
    assert_eq!(actor["bindings"][1]["type"], "cmux.workspace.ref");
    assert_eq!(actor["bindings"][1]["isKey"], false);

    let event = &actor["events"][0];
    assert_eq!(event["id"], "e-1");
    assert_eq!(event["actorId"], se1.as_str());
    assert_eq!(event["type"], "entered");
    assert_eq!(event["detail"], json!({ "source": "startup" }));
    assert_eq!(event["clockSkew"], false);
    assert!(event["sentAt"].is_string() && event["receivedAt"].is_string());

    // 登録の時に頼んだ物には id が無い。見た物には送り手の id がある。
    let settings = actor["settings"].as_array().unwrap();
    assert_eq!(settings.len(), 2, "{actor}");
    assert!(settings[0].get("id").is_none(), "{actor}");
    assert_eq!(settings[0]["actorId"], se1.as_str());
    assert_eq!(settings[0]["model"], "claude-opus-5-5");
    assert_eq!(settings[0]["effort"], "high");
    assert_eq!(settings[0]["source"], "requested");
    assert_eq!(settings[0]["clockSkew"], false);
    assert_eq!(settings[1]["id"], "o-1");
    assert_eq!(settings[1]["source"], "observed");
    assert_eq!(actor["currentSettings"], settings[1], "{actor}");

    let link_in = &actor["linksIn"][0];
    assert_eq!(link_in["id"], "l-1");
    assert_eq!(link_in["type"], "launched");
    assert_eq!(link_in["from"], pm.as_str());
    assert_eq!(link_in["to"], se1.as_str());
    assert_eq!(link_in["clockSkew"], false);
    assert!(link_in["sentAt"].is_string() && link_in["receivedAt"].is_string());
    assert_eq!(actor["linksIn"].as_array().unwrap().len(), 1);
    assert_eq!(actor["linksOut"][0]["id"], "l-2");
    assert_eq!(actor["linksOut"][0]["type"], "continued");
    assert_eq!(actor["linksOut"][0]["to"], se2.as_str());
    assert_eq!(actor["linksOut"].as_array().unwrap().len(), 1);

    assert_eq!(actor["state"]["presence"], "entered");
    assert_eq!(actor["state"]["present"], true);
    assert_eq!(actor["state"]["inferred"], false);
    assert!(actor["state"]["enteredAt"].is_string(), "{actor}");
    assert!(actor["state"].get("exitedAt").is_none(), "{actor}");
}

#[tokio::test]
async fn an_actor_without_anything_attached_has_empty_lists_and_no_current_settings() {
    let app = App::start().await;
    let pm = id(&app.register(&human("pm")).await);
    let actor = app.actor(&pm).await;
    assert_eq!(actor["kind"], "human");
    assert_eq!(actor["events"], json!([]));
    assert_eq!(actor["settings"], json!([]));
    assert_eq!(actor["linksIn"], json!([]));
    assert_eq!(actor["linksOut"], json!([]));
    assert!(actor.get("currentSettings").is_none(), "{actor}");
    assert_eq!(
        actor["state"],
        json!({ "present": false, "inferred": false }),
        "{actor}"
    );
}

#[tokio::test]
async fn an_unknown_actor_is_404_not_found() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let (status, headers, answer) = app
        .get("/api/actors/019abcde-0000-7000-8000-000000000001")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(error_type(&headers), "NotFound");
    assert!(answer["message"].is_string(), "{answer}");
}

#[tokio::test]
async fn an_actor_id_of_the_wrong_form_is_400() {
    let app = App::start().await;
    let (status, headers, answer) = app.get("/api/actors/no-such-actor").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(error_type(&headers), "ValidationException");
}

#[tokio::test]
async fn a_skewed_registration_shows_clock_skew() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["sentAt"] = json!(at(-600));
    let se1 = id(&app.register(&body).await);
    let actor = app.actor(&se1).await;
    assert_eq!(actor["clockSkew"], true, "{actor}");
    // 送り手の時刻は、送った時刻のまま（UTC に直してある）。
    let off = t(&actor["sentAt"]) - t(&body["sentAt"]);
    assert!(off.abs() < time::Duration::milliseconds(1), "{actor}");
}

#[tokio::test]
async fn labels_are_by_key_and_the_rest_are_in_the_order_received() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["labels"] = json!({ "z": "1", "a": "2", "m": "3" });
    body["bindings"] = json!([
        { "type": "claude.session", "value": "s-1" },
        { "type": "z.ref", "value": "z-1" },
        { "type": "a.ref", "value": "a-1" },
    ]);
    body["settings"] = json!({ "model": "claude-opus-5-5", "effort": "high" });
    let se1 = id(&app.register(&body).await);
    // 同じ鍵で、z.ref と model と effort だけを替える。
    body["bindings"][1]["value"] = json!("z-2");
    body["settings"] = json!({ "model": "claude-opus-5-5", "effort": "max" });
    app.register(&body).await;
    // 送り手の時刻が後ろへ戻っても、並びは受けた順（seq）。
    for (n, (kind, sent)) in [
        ("entered", "2099-01-01T00:00:03.000Z"),
        ("compacted", "2099-01-01T00:00:02.000Z"),
        ("exited", "2099-01-01T00:00:01.000Z"),
    ]
    .iter()
    .enumerate()
    {
        app.event_at(&format!("e-{n}"), &se1, kind, "{}", sent)
            .await;
    }
    let other = id(&app.register(&ai("se", "s-2")).await);
    app.link("l-b", "continued", &se1, &other).await;
    app.link("l-a", "launched", &se1, &other).await;
    app.observed("o-b", &se1, "claude-sonnet-5", "low").await;
    app.observed("o-a", &se1, "claude-haiku-5", "medium").await;

    let actor = app.actor(&se1).await;
    // JSON の物の鍵の並びは意味を持たないので、中身を見る。
    assert_eq!(actor["labels"], json!({ "a": "2", "m": "3", "z": "1" }));
    let bindings: Vec<String> = actor["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            format!(
                "{}={}",
                b["type"].as_str().unwrap(),
                b["value"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        bindings,
        vec!["claude.session=s-1", "z.ref=z-1", "a.ref=a-1", "z.ref=z-2"]
    );
    // 今の付随する id は、種類ごとの最新を種類の順に。
    let current: Vec<String> = actor["currentBindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            format!(
                "{}={}",
                b["type"].as_str().unwrap(),
                b["value"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        current,
        vec!["a.ref=a-1", "claude.session=s-1", "z.ref=z-2"]
    );
    let events: Vec<&str> = actor["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(events, vec!["e-0", "e-1", "e-2"]);
    let settings: Vec<String> = actor["settings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            format!(
                "{}/{}",
                s["model"].as_str().unwrap(),
                s["effort"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        settings,
        vec![
            "claude-opus-5-5/high",
            "claude-opus-5-5/max",
            "claude-sonnet-5/low",
            "claude-haiku-5/medium",
        ]
    );
    assert_eq!(actor["currentSettings"]["id"], "o-a");
    let links: Vec<&str> = actor["linksOut"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    assert_eq!(links, vec!["l-b", "l-a"]);
}

#[tokio::test]
async fn the_detail_of_an_event_comes_back_as_the_json_that_was_sent() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    let detail = json!({
        "reason": "workspace_gone",
        "inferred": "true",
        "count": 3,
        "below": -2,
        "ratio": 0.5,
        "none": null,
        "list": [1, "a", { "deep": true }],
    });
    app.event("e-1", &se1, "reported_in", &detail.to_string())
        .await;
    app.event("e-2", &se1, "compacted", "{}").await;
    let actor = app.actor(&se1).await;
    assert_eq!(actor["events"][0]["detail"], detail, "{actor}");
    assert_eq!(actor["events"][1]["detail"], json!({}), "{actor}");
}

#[tokio::test]
async fn entering_makes_present_and_exiting_makes_absent() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    let state = async |n: &str, kind: &str, received: &str| {
        app.event_at(n, &se1, kind, "{}", received).await;
        app.actor(&se1).await["state"].clone()
    };
    let s = state("e-1", "entered", "2099-01-01T00:00:01.000Z").await;
    assert_eq!(s["presence"], "entered");
    assert_eq!(s["present"], true);
    assert_eq!(t(&s["enteredAt"]), t(&json!("2099-01-01T00:00:01Z")));
    assert!(s.get("exitedAt").is_none(), "{s}");
    // 居る・居ないを替えない種類では替わらない。
    for (n, kind) in ["compacted", "reported_in", "model_unreadable", "retracted"]
        .iter()
        .enumerate()
    {
        let s = state(&format!("n-{n}"), kind, "2099-01-01T00:00:02.000Z").await;
        assert_eq!(s["presence"], "entered", "{kind} {s}");
        assert_eq!(s["present"], true, "{kind} {s}");
    }
    let s = state("e-2", "exited", "2099-01-01T00:00:03.000Z").await;
    assert_eq!(s["presence"], "exited");
    assert_eq!(s["present"], false);
    assert_eq!(t(&s["exitedAt"]), t(&json!("2099-01-01T00:00:03Z")));
    let s = state("e-3", "reentered", "2099-01-01T00:00:04.000Z").await;
    assert_eq!(s["presence"], "reentered");
    assert_eq!(s["present"], true);
    // 登場の時刻は初めの entered だけ。退場の時刻は最後の退場（戻っても残る）。
    assert_eq!(t(&s["enteredAt"]), t(&json!("2099-01-01T00:00:01Z")));
    assert_eq!(t(&s["exitedAt"]), t(&json!("2099-01-01T00:00:03Z")));
    let s = state("e-4", "cleared", "2099-01-01T00:00:05.000Z").await;
    assert_eq!(s["presence"], "cleared");
    assert_eq!(s["present"], false);
    let s = state("e-5", "forked", "2099-01-01T00:00:06.000Z").await;
    assert_eq!(s["presence"], "forked");
    assert_eq!(s["present"], true);
    let s = state("e-6", "entered", "2099-01-01T00:00:07.000Z").await;
    assert_eq!(t(&s["enteredAt"]), t(&json!("2099-01-01T00:00:01Z")));
    let s = state("e-7", "closed", "2099-01-01T00:00:08.000Z").await;
    assert_eq!(s["presence"], "closed");
    assert_eq!(s["present"], false);
    assert_eq!(t(&s["exitedAt"]), t(&json!("2099-01-01T00:00:08Z")));
}

#[tokio::test]
async fn entered_at_counts_only_entered_not_forked_or_reentered() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    app.event("e-1", &se1, "forked", "{}").await;
    app.event("e-2", &se1, "reentered", "{}").await;
    let s = app.actor(&se1).await["state"].clone();
    assert_eq!(s["present"], true, "{s}");
    assert_eq!(s["presence"], "reentered", "{s}");
    assert!(s.get("enteredAt").is_none(), "{s}");
}

#[tokio::test]
async fn an_inferred_close_is_inferred_and_reentering_clears_it() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    let inferred = async || app.actor(&se1).await["state"].clone();
    assert_eq!(inferred().await["inferred"], false);
    app.event("i-1", &se1, "entered", "{}").await;
    assert_eq!(inferred().await["inferred"], false);
    app.event(
        "i-2",
        &se1,
        "closed",
        r#"{"inferred":true,"reason":"workspace_gone"}"#,
    )
    .await;
    let s = inferred().await;
    assert_eq!(s["present"], false, "{s}");
    assert_eq!(s["presence"], "closed", "{s}");
    assert_eq!(s["inferred"], true, "{s}");
    // 再登場で、推しの印は消える。
    app.event("i-3", &se1, "reentered", "{}").await;
    let s = inferred().await;
    assert_eq!(s["present"], true, "{s}");
    assert_eq!(s["inferred"], false, "{s}");
    // ふつうの幕引き・字の "true"・ほかの種類の inferred は、推しにしない。
    for (n, (kind, detail)) in [
        ("closed", "{}"),
        ("closed", r#"{"inferred":"true"}"#),
        ("closed", r#"{"inferred":1}"#),
        ("exited", r#"{"inferred":true}"#),
        ("cleared", r#"{"inferred":true}"#),
    ]
    .iter()
    .enumerate()
    {
        app.event(&format!("j-{n}-in"), &se1, "reentered", "{}")
            .await;
        app.event(&format!("j-{n}"), &se1, kind, detail).await;
        let s = inferred().await;
        assert_eq!(s["present"], false, "{kind} {detail} {s}");
        assert_eq!(s["inferred"], false, "{kind} {detail} {s}");
    }
}

#[tokio::test]
async fn inferred_is_decided_by_the_first_exit_after_the_last_enter() {
    let app = App::start().await;
    let lost = r#"{"inferred":true,"reason":"workspace_gone"}"#;
    // 推しの幕引きの後に、遅れた退場: 推しのまま。
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    app.event("k-1", &se1, "entered", "{}").await;
    app.event("k-2", &se1, "closed", lost).await;
    app.event("k-3", &se1, "exited", "{}").await;
    let s = app.actor(&se1).await["state"].clone();
    assert_eq!(s["presence"], "exited", "{s}");
    assert_eq!(s["present"], false, "{s}");
    assert_eq!(s["inferred"], true, "{s}");
    // 退場の後に、推しの幕引き: ふつうの停止。
    let se2 = id(&app.register(&ai("se", "s-2")).await);
    app.event("k-4", &se2, "entered", "{}").await;
    app.event("k-5", &se2, "exited", "{}").await;
    app.event("k-6", &se2, "closed", lost).await;
    let s = app.actor(&se2).await["state"].clone();
    assert_eq!(s["presence"], "closed", "{s}");
    assert_eq!(s["inferred"], false, "{s}");
    // 一度も居なかったアクターの推しの幕引きは、推し。
    let se3 = id(&app.register(&ai("se", "s-3")).await);
    app.event("k-7", &se3, "closed", lost).await;
    let s = app.actor(&se3).await["state"].clone();
    assert_eq!(s["inferred"], true, "{s}");
    // 居る間は、前の帯の推しを持ち越さない。
    app.event("k-8", &se3, "forked", "{}").await;
    app.event("k-9", &se3, "compacted", "{}").await;
    let s = app.actor(&se3).await["state"].clone();
    assert_eq!(s["present"], true, "{s}");
    assert_eq!(s["inferred"], false, "{s}");
}

/// 500 の InternalError で、中の誤りの字（`leak` を含む物）を出さないことを見る。
async fn internal_error(app: &App, id: &str, leak: &str) {
    let (status, headers, answer) = app.get(&format!("/api/actors/{id}")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "{answer}");
    assert!(!message.contains(leak), "中の誤りの字が出ている: {message}");
}

/// DB の誤り（混んでいる以外）は、500 の InternalError で、決まった字を返す。
#[tokio::test]
async fn a_db_error_is_a_500_internal_error_with_a_fixed_message() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    // 札の表を外して、札を読む所を落とす。
    app.execute("alter table labels rename to labels_gone")
        .await;
    internal_error(&app, &se1, "labels").await;
}

/// 残っている行を答えの形に直せない時（読めない時刻）も、500 の InternalError で、決まった字を返す。
#[tokio::test]
async fn a_stored_row_that_cannot_be_converted_is_a_500_internal_error() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    app.event_at("e-1", &se1, "entered", "{}", "not-a-time")
        .await;
    internal_error(&app, &se1, "not-a-time").await;
}

/// 読む接続が空かない（混んでいる）時だけは、503 の Unavailable（送り直せば通る見込み）。
#[tokio::test]
async fn waiting_too_long_for_a_read_connection_is_503_unavailable() {
    let app = App::start().await;
    let se1 = id(&app.register(&ai("se", "s-1")).await);
    // 軽い読みの接続（READS 本）を、みな握ったままにする。
    let mut held = Vec::new();
    for _ in 0..castlist_next::db::READS {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    let (status, headers, answer) = app.get(&format!("/api/actors/{se1}")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(error_type(&headers), "Unavailable");
    assert!(
        answer["message"]
            .as_str()
            .is_some_and(|m| m.contains("混んでいる")),
        "{answer}"
    );
    drop(held);
    app.actor(&se1).await;
}

/// 今の DB から移した行には、時差の付いた送り手の時刻（`+09:00`）が残る。読む所は UTC に直して返す（next.md の「データ」）。
/// 読めずに 500 になると、そのアクターの GetActor が全部読めなくなるので、ここで止める。
#[tokio::test]
async fn a_stored_time_with_an_offset_is_read_as_utc() {
    let app = App::start().await;
    let actor = app.register(&ai("se", "s-1")).await;
    let id = actor["actor"]["id"].as_str().unwrap().to_owned();
    app.event_at(
        "e-offset",
        &id,
        "entered",
        "{}",
        "2026-01-02T03:04:05.678+09:00",
    )
    .await;
    let (status, _, detail) = app.get(&format!("/api/actors/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let event = &detail["events"][0];
    assert_eq!(event["sentAt"], "2026-01-01T18:04:05.678Z", "{event}");
    assert_eq!(event["receivedAt"], "2026-01-01T18:04:05.678Z", "{event}");
}
