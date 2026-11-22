//! 道筋ごとの試験（JSON の API）。試験ごとに一時の DB を作り、Router を直に呼ぶ。

mod common;

use axum::http::{StatusCode, header};
use common::app::{App, ai, at, event, human, link, remove_db, setting, temp_db};
use serde_json::{Value, json};
use sqlx::Connection;

const SE1: &str = "claude.session:s-1";
const SE2: &str = "claude.session:s-2";
const PM: &str = "tpm.person:pm";

/// SE1・SE2 の AI と、PM の人を登録する。
async fn three(app: &App) -> (Value, Value, Value) {
    let se1 = app.register(&ai("se", "s-1")).await;
    let se2 = app.register(&ai("se", "s-2")).await;
    let pm = app.register(&human("pm")).await;
    (se1, se2, pm)
}

// --- 登録 ---

#[tokio::test]
async fn 同じ鍵の登録は_1_つのアクターになり_2_度目は_200() {
    let app = App::start().await;
    let (status, first) = app.post("/v1/actors", &ai("se", "s-1")).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["created"], true);
    assert_eq!(first["name"], "se.1");
    assert_eq!(first["kind"], "ai");
    assert!(
        first["id"].as_str().is_some_and(|id| id.len() == 36),
        "{first}"
    );

    let (status, second) = app.post("/v1/actors", &ai("se", "s-1")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["created"], false);
    assert_eq!(second["id"], first["id"]);
    assert_eq!(second["name"], "se.1");
    assert_eq!(app.count("actors").await, 1);
    assert_eq!(app.count("labels").await, 1);
    assert_eq!(app.count("bindings").await, 1);
}

#[tokio::test]
async fn 名は_接頭辞ごとに_1_から数える() {
    let app = App::start().await;
    assert_eq!(app.register(&ai("se", "s-1")).await["name"], "se.1");
    assert_eq!(app.register(&ai("se", "s-2")).await["name"], "se.2");
    assert_eq!(app.register(&ai("tpm", "s-3")).await["name"], "tpm.1");
    assert_eq!(app.register(&human("pm")).await["name"], "pm.1");
    // 出し手が違えば、名は別に数える。
    let mut other = ai("se", "s-4");
    other["issuer"] = json!("other");
    assert_eq!(app.register(&other).await["name"], "se.1");
}

#[tokio::test]
async fn 外で決めた_id_での登録を受ける道は無い() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["id"] = json!("01900000-0000-7000-8000-000000000000");
    let (status, answer) = app.post("/v1/actors", &body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(answer["error"].is_string(), "{answer}");
    for method in ["PUT", "POST"] {
        let (status, _) = app
            .call(
                method,
                "/v1/actors/01900000-0000-7000-8000-000000000000",
                Some(&ai("se", "s-2")),
            )
            .await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
    }
    assert_eq!(app.count("actors").await, 0);
}

#[tokio::test]
async fn 鍵の付随する_id_は_2_つのアクターに付かない() {
    let app = App::start().await;
    let (se1, se2, _) = three(&app).await;
    let refused = sqlx::query(
        "insert into bindings(actor_id, issuer, type, value, is_key, sent_at, received_at)
         values(?1, 'tpm', 'claude.session', 's-1', 1, ?2, ?2)",
    )
    .bind(se2["id"].as_str())
    .bind(at(0))
    .execute(app.db.write())
    .await
    .expect_err("鍵が 2 つのアクターに付いた");
    assert!(refused.to_string().contains("UNIQUE"), "{refused}");
    // 鍵でない付随する id は、同じ値を 2 つのアクターが持ってよい（workspace は使い回される）。
    let mut again = ai("se", "s-1");
    again["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": "cmux.workspace.ref", "value": "workspace:1" }));
    app.register(&again).await;
    let mut other = ai("se", "s-2");
    other["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": "cmux.workspace.ref", "value": "workspace:1" }));
    app.register(&other).await;
    assert_eq!(app.count("bindings").await, 5);
    assert_ne!(se1["id"], se2["id"]);
}

#[tokio::test]
async fn 鍵でない付随する_id_は_変わった時だけ足す() {
    let app = App::start().await;
    let with = |workspace: &str| {
        let mut body = ai("se", "s-1");
        body["bindings"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "type": "cmux.workspace.ref", "value": workspace }));
        body
    };
    app.register(&with("workspace:1")).await;
    assert_eq!(app.count("bindings").await, 2);
    app.register(&with("workspace:1")).await;
    assert_eq!(app.count("bindings").await, 2);
    app.register(&with("workspace:2")).await;
    assert_eq!(app.count("bindings").await, 3);
    app.register(&with("workspace:2")).await;
    assert_eq!(app.count("bindings").await, 3);
}

#[tokio::test]
async fn 登録の形が違えば_422() {
    let app = App::start().await;
    let cases: Vec<(&str, Value)> = vec![
        ("kind", json!("robot")),
        ("name_prefix", json!("SE")),
        ("name_prefix", json!("")),
        ("issuer", json!("")),
        ("bindings", json!([])),
        (
            "bindings",
            json!([{ "type": "claude.session", "value": "" }]),
        ),
        ("sent_at", json!("きのう")),
    ];
    for (field, value) in cases {
        let mut body = ai("se", "s-1");
        body[field] = value.clone();
        let (status, answer) = app.post("/v1/actors", &body).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{field}={value}: {answer}"
        );
        assert!(answer["error"].is_string(), "{answer}");
    }
    let (status, answer) = app
        .call("POST", "/v1/actors", Some(&json!("JSON の物でない")))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(app.count("actors").await, 0);
}

#[tokio::test]
async fn 登録の_model_と_effort_は_変わった時だけ_requested_を足す() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["settings"] = json!({ "model": "claude-opus-5-5", "effort": "high" });
    app.register(&body).await;
    assert_eq!(app.count("settings").await, 1);
    app.register(&body).await;
    assert_eq!(app.count("settings").await, 1);
    body["settings"] = json!({ "model": "claude-opus-5-5", "effort": "max" });
    app.register(&body).await;
    assert_eq!(app.count("settings").await, 2);
    let source: String = app
        .scalar("select source from settings order by seq desc limit 1")
        .await;
    assert_eq!(source, "requested");
}

// --- 引き当て ---

#[tokio::test]
async fn resolve_は知らない物に_null_を返す() {
    let app = App::start().await;
    let (se1, _, pm) = three(&app).await;
    let (status, answer) = app
        .post(
            "/v1/resolve",
            &json!({ "issuer": "tpm", "refs": [SE1, "claude.session:none", PM, "cmux.workspace.ref:workspace:1"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer["as_of"].is_string(), "{answer}");
    let results = answer["results"].as_array().expect("results");
    assert_eq!(results.len(), 4);
    assert_eq!(results[0]["ref"], SE1);
    assert_eq!(results[0]["actor"]["id"], se1["id"]);
    assert_eq!(results[0]["actor"]["name"], "se.1");
    assert_eq!(results[0]["actor"]["kind"], "ai");
    assert_eq!(results[1]["actor"], Value::Null);
    assert_eq!(results[2]["actor"]["id"], pm["id"]);
    assert_eq!(results[2]["actor"]["kind"], "human");
    assert_eq!(results[3]["actor"], Value::Null, "鍵でない物では引かない");
    // 出し手が違えば引けない。
    let (_, other) = app
        .post("/v1/resolve", &json!({ "issuer": "other", "refs": [SE1] }))
        .await;
    assert_eq!(other["results"][0]["actor"], Value::Null, "{other}");
    // 形の違う参照は 422。
    let (status, _) = app
        .post(
            "/v1/resolve",
            &json!({ "issuer": "tpm", "refs": ["colon-less"] }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// --- 線 ---

#[tokio::test]
async fn 線は_知らないアクターなら_422_同じ_id_で違えば_409() {
    let app = App::start().await;
    three(&app).await;
    let (status, answer) = app
        .put(
            "/v1/links/l-0",
            &link("launched", PM, "claude.session:none"),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    let (status, _) = app.put("/v1/links/l-0", &link("adopted", PM, SE1)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.count("links").await, 0);

    let body = link("launched", PM, SE1);
    let (status, answer) = app.put("/v1/links/l-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let (status, _) = app.put("/v1/links/l-1", &body).await;
    assert_eq!(status, StatusCode::OK);
    let (status, answer) = app.put("/v1/links/l-1", &link("continued", PM, SE1)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert!(answer["error"].is_string());
    let kind: String = app.scalar("select type from links where id = 'l-1'").await;
    assert_eq!(kind, "launched", "前の行はそのまま");
    // 起こされるのは 1 度だけ。
    let (status, _) = app.put("/v1/links/l-2", &link("launched", SE2, SE1)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app.put("/v1/links/l-3", &link("continued", SE1, SE2)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(app.count("links").await, 2);
}

// --- 出来事 ---

#[tokio::test]
async fn 出来事は_同じ_id_で同じなら_200_違えば_409_で前の行はそのまま() {
    let app = App::start().await;
    three(&app).await;
    let body = event(SE1, "entered", &at(0));
    let (status, answer) = app.put("/v1/events/e-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let (status, _) = app.put("/v1/events/e-1", &body).await;
    assert_eq!(status, StatusCode::OK);
    let mut other = body.clone();
    other["type"] = json!("exited");
    let (status, answer) = app.put("/v1/events/e-1", &other).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    let kind: String = app.scalar("select type from events where id = 'e-1'").await;
    assert_eq!(kind, "entered");
    assert_eq!(app.count("events").await, 1);

    let (status, _) = app
        .put(
            "/v1/events/e-2",
            &event("claude.session:none", "entered", &at(0)),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut detail = event(SE1, "compacted", &at(0));
    detail["detail"] = json!({ "trigger": "auto" });
    let (status, _) = app.put("/v1/events/e-3", &detail).await;
    assert_eq!(status, StatusCode::CREATED);
    detail["detail"] = json!(["配列"]);
    let (status, _) = app.put("/v1/events/e-4", &detail).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn 出来事の種類は_10_だけ() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await;
    let kinds = [
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
    ];
    for (i, kind) in kinds.iter().enumerate() {
        let (status, answer) = app
            .put(&format!("/v1/events/k-{i}"), &event(SE1, kind, &at(0)))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{kind}: {answer}");
    }
    let (status, _) = app
        .put("/v1/events/k-x", &event(SE1, "danced", &at(0)))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // DB の CHECK も断る。
    let refused = sqlx::query(
        "insert into events(id, actor_id, type, sent_at, received_at, clock_skew, body_hash)
         values('k-y', ?1, 'danced', ?2, ?2, 0, 'x')",
    )
    .bind(se1["id"].as_str())
    .bind(at(0))
    .execute(app.db.write())
    .await
    .expect_err("CHECK が断らない");
    assert!(refused.to_string().contains("CHECK"), "{refused}");
    assert_eq!(app.count("events").await, 10);
}

// --- model と effort ---

#[tokio::test]
async fn model_と_effort_は_同じ_id_で違えば_409_最新と同じなら行を足さない() {
    let app = App::start().await;
    three(&app).await;
    let body = setting(SE1, "claude-opus-5-5", "high", &at(0));
    let (status, answer) = app.put("/v1/settings/t-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let (status, _) = app.put("/v1/settings/t-1", &body).await;
    assert_eq!(status, StatusCode::OK);
    let (status, answer) = app
        .put(
            "/v1/settings/t-1",
            &setting(SE1, "claude-opus-5-5", "max", &at(0)),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    let effort: String = app
        .scalar(
            "select e.name from settings s join efforts e on e.id = s.effort_id where s.id = 't-1'",
        )
        .await;
    assert_eq!(effort, "high");
    // 違う id でも、最新と同じなら行を足さない。
    let (status, _) = app
        .put(
            "/v1/settings/t-2",
            &setting(SE1, "claude-opus-5-5", "high", &at(0)),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.count("settings").await, 1);
    let (status, _) = app
        .put(
            "/v1/settings/t-3",
            &setting(SE1, "claude-opus-5-5", "xhigh", &at(0)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(app.count("settings").await, 2);
    let (status, _) = app
        .put(
            "/v1/settings/t-4",
            &setting("claude.session:none", "claude-opus-5-5", "high", &at(0)),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn 知らない_model_と_effort_の名は_1_度だけ足し_番号で指す() {
    let app = App::start().await;
    three(&app).await;
    let models = app.count("models").await;
    let efforts = app.count("efforts").await;
    assert_eq!(efforts, 5, "low・medium・high・xhigh・max");
    let (status, answer) = app
        .put(
            "/v1/settings/u-1",
            &setting(SE1, "claude-new-9", "ultra", &at(0)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(app.count("models").await, models + 1);
    assert_eq!(app.count("efforts").await, efforts + 1);
    let (status, _) = app
        .put(
            "/v1/settings/u-2",
            &setting(SE2, "claude-new-9", "ultra", &at(0)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(app.count("models").await, models + 1, "2 度目は足さない");
    assert_eq!(app.count("efforts").await, efforts + 1);
    let pointed: i64 = app
        .scalar(
            "select count(*) from settings s
             join models m on m.id = s.model_id and m.name = 'claude-new-9'
             join efforts e on e.id = s.effort_id and e.name = 'ultra'",
        )
        .await;
    assert_eq!(pointed, 2);
}

#[tokio::test]
async fn 形の違う_model_と_effort_と_source_は_422() {
    let app = App::start().await;
    three(&app).await;
    let long = "a".repeat(65);
    let cases: Vec<(&str, &str)> = vec![
        ("model", "Opus"),
        ("model", ""),
        ("model", &long),
        ("model", "claude opus"),
        ("model", "claude_opus"),
        ("effort", "ULTRA"),
        ("effort", "高い"),
        ("source", "guessed"),
    ];
    for (i, (field, value)) in cases.iter().enumerate() {
        let mut body = setting(SE1, "claude-opus-5-5", "high", &at(0));
        body[*field] = json!(value);
        let (status, answer) = app.put(&format!("/v1/settings/v-{i}"), &body).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{field}={value}: {answer}"
        );
    }
    assert_eq!(app.count("settings").await, 0);
    let ok = "a".repeat(64);
    let (status, _) = app
        .put("/v1/settings/v-ok", &setting(SE1, &ok, "high", &at(0)))
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

// --- 順と時刻 ---

#[tokio::test]
async fn 最新は_出し手の時刻でなく_seq_で決める() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await;
    let id = se1["id"].as_str().unwrap().to_owned();
    // 1 時間先・1 時間前・今 の順に送る。最新は、最後に受けた物。
    for (sid, model, offset) in [
        ("o-1", "model-ahead", 3600),
        ("o-2", "model-behind", -3600),
        ("o-3", "model-now", 0),
    ] {
        let (status, _) = app
            .put(
                &format!("/v1/settings/{sid}"),
                &setting(SE1, model, "high", &at(offset)),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, actor) = app.get(&format!("/v1/actors/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert_eq!(actor["current_settings"]["model"], "model-now", "{actor}");

    for (eid, kind, offset) in [("p-1", "entered", 3600), ("p-2", "exited", -3600)] {
        app.put(&format!("/v1/events/{eid}"), &event(SE1, kind, &at(offset)))
            .await;
    }
    let (_, actor) = app.get(&format!("/v1/actors/{id}")).await;
    assert_eq!(actor["state"]["present"], false, "{actor}");
    assert_eq!(actor["state"]["presence"], "exited");
}

#[tokio::test]
async fn 時計が_5_分より離れていても受け_clock_skew_を立てる() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    for (eid, offset) in [
        ("c-1", 3600),
        ("c-2", -3600),
        ("c-3", 0),
        ("c-4", 240),
        ("c-5", -360),
    ] {
        let (status, _) = app
            .put(
                &format!("/v1/events/{eid}"),
                &event(SE1, "reported_in", &at(offset)),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{eid}");
    }
    let skews: Vec<i64> = sqlx::query_scalar("select clock_skew from events order by seq")
        .fetch_all(app.db.read())
        .await
        .unwrap();
    assert_eq!(skews, vec![1, 1, 0, 0, 1]);
    // 出し手の時刻は、そのまま残す。
    let mut body = event(SE1, "reported_in", "2026-01-02T03:04:05+09:00");
    body["detail"] = json!({});
    app.put("/v1/events/c-6", &body).await;
    let kept: String = app
        .scalar("select sent_at from events where id = 'c-6'")
        .await;
    assert_eq!(kept, "2026-01-02T03:04:05+09:00");
}

#[tokio::test]
async fn received_at_は戻らない() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await;
    // 先の時刻の行が既にあっても、次の行はそれより前にならない。
    sqlx::query(
        "insert into events(id, actor_id, type, sent_at, received_at, clock_skew, body_hash)
         values('future', ?1, 'reported_in', ?2, '2099-01-01T00:00:00.000Z', 1, 'x')",
    )
    .bind(se1["id"].as_str())
    .bind(at(0))
    .execute(app.db.write())
    .await
    .unwrap();
    app.put("/v1/events/r-1", &event(SE1, "reported_in", &at(0)))
        .await;
    let received: String = app
        .scalar("select received_at from events where id = 'r-1'")
        .await;
    assert!(
        received.as_str() >= "2099-01-01T00:00:00.000Z",
        "{received}"
    );
    for i in 0..5 {
        app.put(
            &format!("/v1/settings/r-{i}"),
            &setting(SE1, &format!("m-{i}"), "high", &at(0)),
        )
        .await;
    }
    for table in ["actors", "events", "settings", "bindings"] {
        let stamps: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "select received_at from {table} order by seq"
        )))
        .fetch_all(app.db.read())
        .await
        .unwrap();
        assert!(!stamps.is_empty(), "{table}");
        assert!(
            stamps.windows(2).all(|w| w[0] <= w[1]),
            "{table}: {stamps:?}"
        );
        assert!(
            stamps.iter().all(|s| s.len() == 24 && s.ends_with('Z')),
            "{table}: {stamps:?}"
        );
    }
}

// --- 居るか ---

#[tokio::test]
async fn 登場で居る_退場で居ない_戻ればまた居る() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await;
    let id = se1["id"].as_str().unwrap().to_owned();
    let present = async || {
        let (_, actor) = app.get(&format!("/v1/actors/{id}")).await;
        let view: i64 = sqlx::query_scalar("select present from actor_state where actor_id = ?1")
            .bind(&id)
            .fetch_one(app.db.read())
            .await
            .unwrap();
        assert_eq!(actor["state"]["present"], view == 1, "{actor}");
        actor
    };
    assert_eq!(present().await["state"]["present"], false);
    app.put("/v1/events/s-1", &event(SE1, "entered", &at(0)))
        .await;
    let actor = present().await;
    assert_eq!(actor["state"]["present"], true);
    assert_eq!(actor["state"]["presence"], "entered");
    assert!(actor["state"]["entered_at"].is_string());
    app.put("/v1/events/s-2", &event(SE1, "compacted", &at(0)))
        .await;
    assert_eq!(
        present().await["state"]["present"],
        true,
        "compacted では替わらない"
    );
    app.put("/v1/events/s-3", &event(SE1, "exited", &at(0)))
        .await;
    let actor = present().await;
    assert_eq!(actor["state"]["present"], false);
    assert!(actor["state"]["exited_at"].is_string());
    app.put("/v1/events/s-4", &event(SE1, "reentered", &at(0)))
        .await;
    assert_eq!(present().await["state"]["present"], true);
}

// --- 見る ---

#[tokio::test]
async fn アクターを_id_で引くと_付随する物がみな付く() {
    let app = App::start().await;
    let (se1, _, pm) = three(&app).await;
    let id = se1["id"].as_str().unwrap();
    app.put("/v1/links/g-1", &link("launched", PM, SE1)).await;
    app.put("/v1/links/g-2", &link("continued", SE1, SE2)).await;
    app.put("/v1/events/g-3", &event(SE1, "entered", &at(0)))
        .await;
    app.put(
        "/v1/settings/g-4",
        &setting(SE1, "claude-opus-5-5", "high", &at(0)),
    )
    .await;
    let (status, actor) = app.get(&format!("/v1/actors/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert!(actor["as_of"].is_string());
    assert_eq!(actor["id"], se1["id"]);
    assert_eq!(actor["name"], "se.1");
    assert_eq!(actor["kind"], "ai");
    assert_eq!(actor["issuer"], "tpm");
    assert_eq!(actor["labels"]["project"], "castlist");
    assert_eq!(actor["bindings"][0]["type"], "claude.session");
    assert_eq!(actor["bindings"][0]["value"], "s-1");
    assert_eq!(actor["bindings"][0]["is_key"], true);
    assert_eq!(actor["events"][0]["type"], "entered");
    assert_eq!(actor["settings"][0]["model"], "claude-opus-5-5");
    assert_eq!(actor["settings"][0]["effort"], "high");
    assert_eq!(actor["current_settings"]["effort"], "high");
    assert_eq!(actor["links_in"][0]["from"], pm["id"]);
    assert_eq!(actor["links_in"][0]["type"], "launched");
    assert_eq!(actor["links_out"][0]["type"], "continued");
    assert_eq!(actor["state"]["present"], true);

    let (status, answer) = app.get("/v1/actors/no-such-actor").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert!(answer["error"].is_string());
}

#[tokio::test]
async fn 付随する_id_と時刻で_その時のアクターを引く() {
    let app = App::start().await;
    let workspace = |session: &str| {
        let mut body = ai("se", session);
        body["bindings"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "type": "cmux.workspace.ref", "value": "workspace:1" }));
        body
    };
    let first = app.register(&workspace("s-1")).await;
    let then: String = app
        .scalar("select received_at from bindings where type = 'cmux.workspace.ref'")
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let second = app.register(&workspace("s-2")).await;

    let (status, answer) = app
        .get(&format!(
            "/v1/actors?binding=cmux.workspace.ref:workspace:1&at={then}"
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer["as_of"].is_string());
    assert_eq!(answer["actors"][0]["id"], first["id"], "{answer}");
    assert_eq!(answer["actors"].as_array().unwrap().len(), 1);
    let (_, answer) = app
        .get("/v1/actors?binding=cmux.workspace.ref:workspace:1")
        .await;
    assert_eq!(answer["actors"][0]["id"], second["id"], "{answer}");
    let (_, answer) = app
        .get("/v1/actors?binding=cmux.workspace.ref:workspace:1&at=2000-01-01T00:00:00Z")
        .await;
    assert_eq!(answer["actors"], json!([]), "{answer}");

    let (status, answer) = app.get("/v1/actors?label=project:castlist").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let names: Vec<&str> = answer["actors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["se.1", "se.2"]);
    // 問い合わせの引数の誤りは 400（監査の検索と同じ。docs/src/api.md）。
    let (status, _) = app.get("/v1/actors?at=きのう&binding=a:b").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn 推しの幕引きの後は_停止で推しの印が立ち_再登場で消える() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await;
    let id = se1["id"].as_str().unwrap().to_owned();
    let inferred = async || {
        let (_, actor) = app.get(&format!("/v1/actors/{id}")).await;
        let (_, list) = app.get("/v1/actors?label=project:castlist").await;
        let item = list["actors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id.as_str())
            .unwrap()
            .clone();
        let (_, graph) = app.get("/v1/graph?label=project:castlist").await;
        let node = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id.as_str())
            .unwrap()
            .clone();
        assert_eq!(item["inferred"], actor["state"]["inferred"], "{item}");
        assert_eq!(
            node["state"]["inferred"], actor["state"]["inferred"],
            "{node}"
        );
        actor
    };
    assert_eq!(inferred().await["state"]["inferred"], false);
    app.put("/v1/events/i-1", &event(SE1, "entered", &at(0)))
        .await;
    assert_eq!(inferred().await["state"]["inferred"], false);
    let lost = json!({ "issuer": "tpm", "by": SE1, "type": "closed",
        "detail": { "inferred": true, "reason": "workspace_gone" }, "sent_at": at(0) });
    let (status, _) = app.put("/v1/events/i-2", &lost).await;
    assert_eq!(status, StatusCode::CREATED);
    let actor = inferred().await;
    assert_eq!(actor["state"]["present"], false);
    assert_eq!(actor["state"]["presence"], "closed");
    assert_eq!(actor["state"]["inferred"], true, "{actor}");
    // 再登場で、推しの印は消える。
    app.put("/v1/events/i-3", &event(SE1, "reentered", &at(0)))
        .await;
    let actor = inferred().await;
    assert_eq!(actor["state"]["present"], true);
    assert_eq!(actor["state"]["inferred"], false, "{actor}");
    // ふつうの幕引き・字の "true"・ほかの種類の inferred は、推しにしない。
    for (n, body) in [
        json!({ "issuer": "tpm", "by": SE1, "type": "closed", "sent_at": at(0) }),
        json!({ "issuer": "tpm", "by": SE1, "type": "closed", "detail": { "inferred": "true" }, "sent_at": at(0) }),
        json!({ "issuer": "tpm", "by": SE1, "type": "exited", "detail": { "inferred": true }, "sent_at": at(0) }),
    ]
    .iter()
    .enumerate()
    {
        app.put(&format!("/v1/events/j-{n}-in"), &event(SE1, "reentered", &at(0)))
            .await;
        let (status, _) = app.put(&format!("/v1/events/j-{n}"), body).await;
        assert_eq!(status, StatusCode::CREATED);
        let actor = inferred().await;
        assert_eq!(actor["state"]["present"], false);
        assert_eq!(actor["state"]["inferred"], false, "{body} {actor}");
    }
}

#[tokio::test]
async fn 推しの印は_今の帯を終わらせた出来事で決め_停止中の退場では替わらない() {
    let app = App::start().await;
    let lost = |id: &str| {
        json!({ "issuer": "tpm", "by": id, "type": "closed",
        "detail": { "inferred": true, "reason": "workspace_gone" }, "sent_at": at(0) })
    };
    // 推しの幕引きの後に、遅れた退場（推しが誤りで、その session が後で SessionEnd を送った）: 推しのまま。
    let se1 = app.register(&ai("se", "s-1")).await;
    app.put("/v1/events/k-1", &event(SE1, "entered", &at(0)))
        .await;
    app.put("/v1/events/k-2", &lost(SE1)).await;
    app.put("/v1/events/k-3", &event(SE1, "exited", &at(0)))
        .await;
    let (_, actor) = app
        .get(&format!("/v1/actors/{}", se1["id"].as_str().unwrap()))
        .await;
    assert_eq!(actor["state"]["present"], false);
    assert_eq!(actor["state"]["inferred"], true, "{actor}");
    // 退場の後に、推しの幕引き（控えを読んだ後に SessionEnd が割り込んだ）: ふつうの停止。
    let se2 = app.register(&ai("se", "s-2")).await;
    app.put("/v1/events/k-4", &event(SE2, "entered", &at(0)))
        .await;
    app.put("/v1/events/k-5", &event(SE2, "exited", &at(0)))
        .await;
    app.put("/v1/events/k-6", &lost(SE2)).await;
    let (_, actor) = app
        .get(&format!("/v1/actors/{}", se2["id"].as_str().unwrap()))
        .await;
    assert_eq!(actor["state"]["present"], false);
    assert_eq!(actor["state"]["inferred"], false, "{actor}");
}

#[tokio::test]
async fn 図の点と線は_絞った外の一歩先も含める() {
    let app = App::start().await;
    let (se1, se2, pm) = three(&app).await;
    app.put("/v1/links/h-1", &link("launched", PM, SE1)).await;
    app.put("/v1/links/h-2", &link("launched", SE1, SE2)).await;
    app.put("/v1/events/h-3", &event(SE1, "entered", &at(0)))
        .await;
    app.put(
        "/v1/settings/h-4",
        &setting(SE1, "claude-opus-5-5", "high", &at(0)),
    )
    .await;
    let (status, graph) = app.get("/v1/graph?label=project:castlist").await;
    assert_eq!(status, StatusCode::OK, "{graph}");
    assert!(graph["as_of"].is_string());
    let nodes = graph["nodes"].as_array().unwrap();
    let ids: Vec<&Value> = nodes.iter().map(|n| &n["id"]).collect();
    assert_eq!(nodes.len(), 3, "{graph}");
    for actor in [&se1, &se2, &pm] {
        assert!(ids.contains(&&actor["id"]), "{graph}");
    }
    let node = nodes.iter().find(|n| n["id"] == se1["id"]).unwrap();
    assert_eq!(node["name"], "se.1");
    assert_eq!(node["state"]["present"], true);
    assert_eq!(node["current_settings"]["model"], "claude-opus-5-5");
    let edges = graph["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 2, "{graph}");
    assert!(
        edges
            .iter()
            .any(|e| e["from"] == pm["id"] && e["to"] == se1["id"])
    );

    // 時刻の窓の外なら、何も無い。
    let (status, graph) = app
        .get("/v1/graph?label=project:castlist&from=2000-01-01T00:00:00Z&to=2000-01-02T00:00:00Z")
        .await;
    assert_eq!(status, StatusCode::OK, "{graph}");
    assert_eq!(graph["nodes"], json!([]));
    assert_eq!(graph["edges"], json!([]));

    // 問い合わせの引数の誤りは 400 で、訳に引数の名が入る（監査の検索と同じ。docs/src/api.md）。
    for (uri, name) in [
        ("/v1/graph?colour=red", "colour"),
        ("/v1/graph?from=きのう", "from"),
        ("/v1/graph?to=2026-13-01", "to"),
        ("/v1/graph?label=nocolon", "label"),
        ("/v1/graph?label=a:b&label=c:d", "label"),
    ] {
        let (status, answer) = app.get(uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
        let message = answer["error"].as_str().unwrap_or_default();
        assert!(message.contains(name), "{uri}: {message}");
    }
}

// --- そのほか ---

#[tokio::test]
async fn healthz_は今も答える() {
    let app = App::start().await;
    let (status, answer) = app.get("/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["status"], "ok");
}

#[tokio::test]
async fn backup_は_外のキーの検査に通る_sqlite_の_file_を返す() {
    let app = App::start().await;
    three(&app).await;
    app.put("/v1/links/b-1", &link("launched", PM, SE1)).await;
    app.put("/v1/events/b-2", &event(SE1, "entered", &at(0)))
        .await;
    let (status, headers, bytes) = app.raw("GET", "/v1/backup", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/vnd.sqlite3")
    );
    assert!(
        bytes.starts_with(b"SQLite format 3\0"),
        "{} bytes",
        bytes.len()
    );
    let path = temp_db("backup");
    tokio::fs::write(&path, &bytes).await.unwrap();
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}?mode=ro", path.display()))
        .await
        .unwrap();
    let broken: Vec<(String,)> = sqlx::query_as("pragma foreign_key_check")
        .fetch_all(&mut conn)
        .await
        .unwrap();
    assert!(broken.is_empty(), "{broken:?}");
    let actors: i64 = sqlx::query_scalar("select count(*) from actors")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(actors, 3);
    conn.close().await.unwrap();
    remove_db(&path);
}

// --- 送り手の時刻の名（sent_at） ---

/// 答えのどこにも `at` の鍵が無いか（送り手の時刻は `sent_at`）。
fn assert_no_at(path: &str, v: &Value) {
    match v {
        Value::Object(map) => {
            assert!(!map.contains_key("at"), "{path}: 答えに at の鍵がある: {v}");
            for (k, child) in map {
                assert_no_at(&format!("{path}.{k}"), child);
            }
        }
        Value::Array(items) => items.iter().for_each(|c| assert_no_at(path, c)),
        _ => {}
    }
}

#[tokio::test]
async fn 送り手の時刻は_sent_at_で受け_答えも_sent_at_で返す() {
    let app = App::start().await;
    let (se1, _, pm) = three(&app).await;
    let id = se1["id"].as_str().unwrap();
    let sent = "2026-11-22T13:00:00.123+09:00";
    let mut answers = Vec::new();
    for (path, mut body) in [
        ("/v1/links/s-1", link("launched", PM, SE1)),
        ("/v1/links/s-2", link("continued", SE1, SE2)),
        ("/v1/events/s-3", event(SE1, "entered", sent)),
        (
            "/v1/settings/s-4",
            setting(SE1, "claude-opus-5-5", "high", sent),
        ),
    ] {
        body["sent_at"] = json!(sent);
        let (status, answer) = app.put(path, &body).await;
        assert_eq!(status, StatusCode::CREATED, "{path}: {answer}");
        assert_eq!(
            answer["sent_at"], sent,
            "{path}: 送った字のまま返す: {answer}"
        );
        answers.push((path.to_owned(), answer));
    }
    for path in [
        format!("/v1/actors/{id}"),
        format!("/v1/actors/{id}/lineage"),
        format!("/v1/actors/{id}/settings"),
        "/v1/actors?label=project:castlist".to_owned(),
        "/v1/events?type=entered".to_owned(),
        "/v1/settings?source=observed".to_owned(),
        "/v1/graph".to_owned(),
    ] {
        let (status, answer) = app.get(&path).await;
        assert_eq!(status, StatusCode::OK, "{path}: {answer}");
        answers.push((path, answer));
    }
    for (path, answer) in &answers {
        assert_no_at(path, answer);
    }
    let (_, actor) = app.get(&format!("/v1/actors/{id}")).await;
    assert!(actor["sent_at"].is_string(), "{actor}");
    assert!(actor["bindings"][0]["sent_at"].is_string(), "{actor}");
    assert!(
        actor["current_bindings"][0]["sent_at"].is_string(),
        "{actor}"
    );
    assert_eq!(actor["events"][0]["sent_at"], sent);
    assert_eq!(actor["current_settings"]["sent_at"], sent);
    assert_eq!(actor["links_in"][0]["from"], pm["id"]);
    assert!(actor["links_in"][0]["sent_at"].is_string(), "{actor}");
}

#[tokio::test]
async fn 移る間は_古い名_at_も受け_同じ_id_の送り直しは_どちらの名でも_200() {
    let app = App::start().await;
    let mut old = ai("se", "s-1");
    let sent = old.as_object_mut().unwrap().remove("sent_at").unwrap();
    old["at"] = sent;
    let (status, answer) = app.post("/v1/actors", &old).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    app.register(&human("pm")).await;
    let t = at(0);
    for (path, body) in [
        ("/v1/events/o-1", event(SE1, "entered", &t)),
        ("/v1/links/o-2", link("launched", PM, SE1)),
        (
            "/v1/settings/o-3",
            setting(SE1, "claude-opus-5-5", "high", &t),
        ),
    ] {
        // 前の版の送り手は at で送る。
        let mut old = body.clone();
        let sent = old.as_object_mut().unwrap().remove("sent_at").unwrap();
        old["at"] = sent;
        let (status, answer) = app.put(path, &old).await;
        assert_eq!(status, StatusCode::CREATED, "{path}: {answer}");
        assert_eq!(answer["sent_at"], old["at"], "{path}: {answer}");
        // 新しい名で同じ物を送り直しても、同じ物（409 にしない）。
        let (status, answer) = app.put(path, &body).await;
        assert_eq!(status, StatusCode::OK, "{path}: {answer}");
    }
}

#[tokio::test]
async fn 書く_4_つの道は_sent_at_と_at_を両方付けると_422_で_at_が読めなくても_422() {
    let app = App::start().await;
    three(&app).await;
    for (path, body) in [
        ("/v1/actors", ai("se", "s-9")),
        ("/v1/events/b-1", event(SE1, "entered", &at(0))),
        ("/v1/links/b-2", link("launched", PM, SE1)),
        (
            "/v1/settings/b-3",
            setting(SE1, "claude-opus-5-5", "high", &at(0)),
        ),
    ] {
        let method = if path == "/v1/actors" { "POST" } else { "PUT" };
        let mut both = body.clone();
        both["at"] = both["sent_at"].clone();
        let (status, answer) = app.call(method, path, Some(&both)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path}: {answer}");
        // 移る間の送り手は at で送る。読めない時の字は、送った名 at も指す。
        let mut old = body.clone();
        old.as_object_mut().unwrap().remove("sent_at");
        old["at"] = json!("きのう");
        let (status, answer) = app.call(method, path, Some(&old)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path}: {answer}");
        let text = answer.to_string();
        assert!(
            text.contains("sent_at（0.1.x の名 at） は RFC 3339 の時刻"),
            "{path}: {answer}"
        );
    }
}

/// 0.1.3 の castlist が、下の本文（送り手の時刻の名は at）で DB に入れた本文の写し（body_hash）。
/// v0.1.3 の tag の試験で、同じ本文を PUT して DB から読んだ値（0.2.0 で手で作らない）。
const V013_HASHES: [(&str, &str); 3] = [
    (
        "events",
        "01a219acb1f8304fa333ad65982caaa6276a1ac2903c98d6667ecf19be2acaaf",
    ),
    (
        "links",
        "0f75eada811ff0d8f2ec1af823eb5ba1918929a0745fc2125df7fa0115f6d5d9",
    ),
    (
        "settings",
        "561b9c80aab0c3618398f7c684ea2047c50eb3308d2f9c620ef157402b54739b",
    ),
];

#[tokio::test]
async fn 本文の写しは_0_1_x_が作った値と同じで_前の版で受けた物を_sent_at_で送り直しても_409_にならない()
 {
    let t = "2026-11-22T13:00:00.000+09:00";
    let bodies = [
        (
            "/v1/events/h-1",
            json!({ "issuer": "tpm", "by": SE1, "type": "entered", "detail": {"k": "v"}, "sent_at": t }),
        ),
        (
            "/v1/links/h-2",
            json!({ "issuer": "tpm", "type": "launched", "from": PM, "to": SE1, "sent_at": t }),
        ),
        (
            "/v1/settings/h-3",
            json!({ "issuer": "tpm", "by": SE1, "model": "claude-opus-5-5", "effort": "high", "source": "observed", "sent_at": t }),
        ),
    ];
    // 新しい名でも古い名でも、写しは 0.1.3 が作った値と同じ。
    for name in ["sent_at", "at"] {
        let app = App::start().await;
        three(&app).await;
        for ((path, body), (table, want)) in bodies.iter().zip(V013_HASHES) {
            let mut body = body.clone();
            if name == "at" {
                let sent = body.as_object_mut().unwrap().remove("sent_at").unwrap();
                body["at"] = sent;
            }
            let (status, answer) = app.put(path, &body).await;
            assert_eq!(status, StatusCode::CREATED, "{name} {path}: {answer}");
            let id = path.rsplit('/').next().unwrap();
            let got: String = app
                .scalar(&format!("select body_hash from {table} where id = '{id}'"))
                .await;
            assert_eq!(got, want, "{name} {path}: 0.1.3 の写しと違う");
        }
    }
}

#[tokio::test]
async fn 時点の問い_at_の答えは_頭の_at_に問いの時点を返し_行には_at_の鍵が無い() {
    let app = App::start().await;
    let (se1, _, _) = three(&app).await;
    let id = se1["id"].as_str().unwrap();
    let (status, answer) = app
        .put(
            "/v1/settings/p-1",
            &setting(SE1, "claude-opus-5-5", "high", &at(0)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let when = at(60).replace('+', "%2B");
    let (status, answer) = app
        .get(&format!("/v1/actors/{id}/settings?at={when}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer["at"].is_string(), "{answer}");
    assert!(answer["setting"]["sent_at"].is_string(), "{answer}");
    let mut rest = answer.clone();
    rest.as_object_mut().unwrap().remove("at");
    assert_no_at("settings?at=", &rest);
}
