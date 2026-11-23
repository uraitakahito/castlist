//! RegisterActor の振る舞い（処理より先に書く）。試験ごとに一時の DB を作り、Router を通して要求を送り、DB の行を見る。
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{App, ai, at, error_type, human};
use serde_json::{Value, json};

/// 本文の付随する id に、鍵でない物を足す。
fn with_binding(mut body: Value, kind: &str, value: &str) -> Value {
    body["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": kind, "value": value }));
    body
}

#[tokio::test]
async fn a_new_key_is_201_and_the_same_key_again_is_200_with_the_same_actor() {
    let app = App::start().await;
    let (status, _, first) = app.post("/api/actors", &ai("se", "s-1")).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["created"], true);
    assert_eq!(first["actor"]["name"], "se.1");
    assert_eq!(first["actor"]["kind"], "ai");
    let id = first["actor"]["id"].as_str().expect("id");
    // UUID v7（版の字は 15 字目）。
    assert_eq!(id.len(), 36, "{id}");
    assert_eq!(&id[14..15], "7", "{id}");

    let (status, _, second) = app.post("/api/actors", &ai("se", "s-1")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["created"], false);
    assert_eq!(second["actor"], first["actor"]);
    assert_eq!(app.count("actors").await, 1);
    assert_eq!(app.count("labels").await, 1);
    assert_eq!(app.count("bindings").await, 1);
    assert_eq!(app.count("settings").await, 0);
}

#[tokio::test]
async fn an_id_from_outside_is_ignored() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["id"] = json!("01900000-0000-7000-8000-000000000000");
    let (status, _, answer) = app.post("/api/actors", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_ne!(answer["actor"]["id"], body["id"]);
    let ids: Vec<String> = app.column("select id from actors").await;
    assert_eq!(
        ids,
        vec![answer["actor"]["id"].as_str().unwrap().to_owned()]
    );
}

#[tokio::test]
async fn names_count_from_1_per_issuer_and_prefix() {
    let app = App::start().await;
    let name = |answer: Value| answer["actor"]["name"].clone();
    assert_eq!(name(app.register(&ai("se", "s-1")).await), "se.1");
    assert_eq!(name(app.register(&ai("se", "s-2")).await), "se.2");
    assert_eq!(name(app.register(&ai("tpm", "s-3")).await), "tpm.1");
    assert_eq!(name(app.register(&human("pm")).await), "pm.1");
    assert_eq!(name(app.register(&ai("se", "s-4")).await), "se.3");
    // 出し手が違えば、名は別に数える。
    let mut other = ai("se", "s-5");
    other["issuer"] = json!("other");
    assert_eq!(name(app.register(&other).await), "se.1");
    let rows: Vec<String> = app
        .column(
            "select issuer || ' ' || prefix || ' ' || n || ' ' || name from actors order by seq",
        )
        .await;
    assert_eq!(
        rows,
        vec![
            "tpm se 1 se.1",
            "tpm se 2 se.2",
            "tpm tpm 1 tpm.1",
            "tpm pm 1 pm.1",
            "tpm se 3 se.3",
            "other se 1 se.1",
        ]
    );
}

#[tokio::test]
async fn the_key_is_looked_up_within_the_issuer() {
    let app = App::start().await;
    let first = app.register(&ai("se", "s-1")).await;
    let mut other = ai("se", "s-1");
    other["issuer"] = json!("other");
    let (status, _, second) = app.post("/api/actors", &other).await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_ne!(second["actor"]["id"], first["actor"]["id"]);
    assert_eq!(app.count("actors").await, 2);
}

#[tokio::test]
async fn an_existing_key_ignores_kind_prefix_and_labels() {
    let app = App::start().await;
    let first = app.register(&ai("se", "s-1")).await;
    let mut again = ai("other", "s-1");
    again["kind"] = json!("human");
    again["labels"] = json!({ "project": "pagetray", "role": "pm" });
    let (status, _, second) = app.post("/api/actors", &again).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["actor"], first["actor"]);
    let actors: Vec<String> = app
        .column("select kind || ' ' || prefix || ' ' || name from actors")
        .await;
    assert_eq!(actors, vec!["ai se se.1"]);
    let labels: Vec<String> = app
        .column("select key || '=' || value from labels order by seq")
        .await;
    assert_eq!(labels, vec!["project=castlist"]);
}

#[tokio::test]
async fn labels_are_written_once_at_creation_in_key_order() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["labels"] = json!({ "role": "se", "project": "castlist", "team": "ai" });
    let answer = app.register(&body).await;
    let labels: Vec<String> = app
        .column("select actor_id || ' ' || key || '=' || value from labels order by seq")
        .await;
    let id = answer["actor"]["id"].as_str().unwrap();
    assert_eq!(
        labels,
        vec![
            format!("{id} project=castlist"),
            format!("{id} role=se"),
            format!("{id} team=ai"),
        ]
    );
    // labels の無い登録も受ける。
    let mut bare = ai("se", "s-2");
    bare.as_object_mut().unwrap().remove("labels");
    let (status, _, answer) = app.post("/api/actors", &bare).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(app.count("labels").await, 3);
}

#[tokio::test]
async fn a_key_binding_belongs_to_one_actor_only() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let se2 = app.register(&ai("se", "s-2")).await;
    let refused = sqlx::query(
        "insert into bindings(actor_id, issuer, type, value, is_key, sent_at, received_at)
         values(?1, 'tpm', 'claude.session', 's-1', 1, ?2, ?2)",
    )
    .bind(se2["actor"]["id"].as_str())
    .bind(at(0))
    .execute(app.db.write())
    .await
    .expect_err("鍵が 2 つのアクターに付いた");
    assert!(refused.to_string().contains("UNIQUE"), "{refused}");
    // 鍵でない付随する id は、同じ値を 2 つのアクターが持ってよい（workspace は使い回される）。
    app.register(&with_binding(
        ai("se", "s-1"),
        "cmux.workspace.ref",
        "workspace:1",
    ))
    .await;
    app.register(&with_binding(
        ai("se", "s-2"),
        "cmux.workspace.ref",
        "workspace:1",
    ))
    .await;
    let rows: Vec<String> = app
        .column("select type || ':' || value || ' ' || is_key from bindings order by seq")
        .await;
    assert_eq!(
        rows,
        vec![
            "claude.session:s-1 1",
            "claude.session:s-2 1",
            "cmux.workspace.ref:workspace:1 0",
            "cmux.workspace.ref:workspace:1 0",
        ]
    );
}

#[tokio::test]
async fn non_key_bindings_are_appended_only_when_they_differ_from_the_latest_of_the_type() {
    let app = App::start().await;
    let with = |workspace: &str| {
        with_binding(
            with_binding(ai("se", "s-1"), "cmux.workspace.ref", workspace),
            "tpm.role",
            "se",
        )
    };
    // 新しく作る時も、鍵でない物を足す。
    app.register(&with("workspace:1")).await;
    app.register(&with("workspace:1")).await;
    app.register(&with("workspace:2")).await;
    app.register(&with("workspace:2")).await;
    // 前の値に戻れば、最新と違うので足す。
    app.register(&with("workspace:1")).await;
    let rows: Vec<String> = app
        .column("select type || ':' || value || ' ' || is_key from bindings order by seq")
        .await;
    assert_eq!(
        rows,
        vec![
            "claude.session:s-1 1",
            "cmux.workspace.ref:workspace:1 0",
            "tpm.role:se 0",
            "cmux.workspace.ref:workspace:2 0",
            "cmux.workspace.ref:workspace:1 0",
        ]
    );
}

#[tokio::test]
async fn requested_settings_are_appended_only_when_model_or_effort_changes() {
    let app = App::start().await;
    let with = |model: &str, effort: &str| {
        let mut body = ai("se", "s-1");
        body["settings"] = json!({ "model": model, "effort": effort });
        body
    };
    app.register(&with("claude-opus-5-5", "high")).await;
    app.register(&with("claude-opus-5-5", "high")).await;
    app.register(&with("claude-opus-5-5", "max")).await;
    app.register(&with("claude-sonnet-5", "max")).await;
    app.register(&with("claude-sonnet-5", "max")).await;
    // settings の無い登録は、何も足さない。
    app.register(&ai("se", "s-1")).await;
    let rows: Vec<String> = app
        .column(
            "select m.name || ' ' || f.name || ' ' || s.source || ' ' || coalesce(s.id, '-') || ' ' || coalesce(s.body_hash, '-')
             from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id order by s.seq",
        )
        .await;
    assert_eq!(
        rows,
        vec![
            "claude-opus-5-5 high requested - -",
            "claude-opus-5-5 max requested - -",
            "claude-sonnet-5 max requested - -",
        ]
    );
}

#[tokio::test]
async fn unknown_model_and_effort_names_are_added_to_the_lists_once() {
    let app = App::start().await;
    let efforts: Vec<String> = app.column("select name from efforts order by id").await;
    assert_eq!(efforts, vec!["low", "medium", "high", "xhigh", "max"]);
    assert_eq!(app.count("models").await, 0);
    for session in ["s-1", "s-2"] {
        let mut body = ai("se", session);
        body["settings"] = json!({ "model": "claude-brand-new", "effort": "turbo" });
        app.register(&body).await;
    }
    let models: Vec<String> = app.column("select name from models order by id").await;
    assert_eq!(models, vec!["claude-brand-new"]);
    let efforts: Vec<String> = app.column("select name from efforts order by id").await;
    assert_eq!(
        efforts,
        vec!["low", "medium", "high", "xhigh", "max", "turbo"]
    );
    let seen: Vec<String> = app
        .column("select first_seen_at from models union all select first_seen_at from efforts where name = 'turbo'")
        .await;
    assert!(
        seen.iter().all(|s| s.len() == 24 && s.ends_with('Z')),
        "{seen:?}"
    );
    assert_eq!(app.count("settings").await, 2);
}

#[tokio::test]
async fn malformed_registrations_are_400_validation_exceptions() {
    let app = App::start().await;
    let long = "x".repeat(257);
    let cases: Vec<(&str, Value)> = vec![
        ("kind", json!("robot")),
        ("namePrefix", json!("se.x")),
        ("namePrefix", json!("SE")),
        ("namePrefix", json!("")),
        ("namePrefix", json!("x".repeat(33))),
        ("issuer", json!("")),
        ("issuer", json!("TPM")),
        ("bindings", json!([])),
        (
            "bindings",
            json!([{ "type": "claude.session", "value": "" }]),
        ),
        (
            "bindings",
            json!([{ "type": "claude.session", "value": long }]),
        ),
        (
            "bindings",
            json!([{ "type": "claude.session", "value": "a\u{7}b" }]),
        ),
        ("bindings", json!([{ "type": "Claude", "value": "s-1" }])),
        ("labels", json!({ "Project": "castlist" })),
        ("labels", json!({ "project": "" })),
        ("settings", json!({ "model": "Opus 5", "effort": "high" })),
        (
            "settings",
            json!({ "model": "claude-opus-5-5", "effort": "" }),
        ),
    ];
    for (field, value) in cases {
        let mut body = ai("se", "s-1");
        body[field] = value.clone();
        let (status, headers, answer) = app.post("/api/actors", &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}={value}: {answer}");
        assert_eq!(
            error_type(&headers),
            "ValidationException",
            "{field}={value}"
        );
        assert!(answer["message"].is_string(), "{answer}");
    }
    // 256 字ちょうどは受ける（字は Unicode の字で数える）。
    let mut body = ai("se", "s-1");
    body["bindings"] = json!([{ "type": "claude.session", "value": "あ".repeat(256) }]);
    let (status, _, answer) = app.post("/api/actors", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(app.count("actors").await, 1);
}

#[tokio::test]
async fn a_malformed_sent_at_is_400_and_nothing_is_written() {
    let app = App::start().await;
    // 時刻は生成した層が JSON を読む所で断る（定義の形を見る前なので、誤りの種類は SerializationException）。
    // Smithy の date-time は UTC（末尾 Z）だけを受け、+09:00 などの差の付いた時刻も断る。
    for sent_at in [
        json!("きのう"),
        json!("2026-01-02T03:04:05+09:00"),
        json!(0),
    ] {
        let mut body = ai("se", "s-1");
        body["sentAt"] = sent_at.clone();
        let (status, headers, answer) = app.post("/api/actors", &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{sent_at}: {answer}");
        assert_eq!(error_type(&headers), "SerializationException", "{sent_at}");
    }
    assert_eq!(app.count("actors").await, 0);
}

#[tokio::test]
async fn received_at_never_goes_backwards_and_is_24_chars() {
    let app = App::start().await;
    let first = app.register(&ai("se", "s-1")).await;
    // 先の時刻の行が既にあっても、次の行はそれより前にならない（どの表の行でも）。
    app.execute(&format!(
        "insert into labels(actor_id, key, value, received_at)
         values('{}', 'future', 'x', '2099-01-01T00:00:00.000Z')",
        first["actor"]["id"].as_str().unwrap()
    ))
    .await;
    for i in 2..=4 {
        let mut body = with_binding(ai("se", &format!("s-{i}")), "cmux.workspace.ref", "w");
        body["settings"] = json!({ "model": format!("m-{i}"), "effort": "high" });
        app.register(&body).await;
    }
    let after: Vec<String> = app
        .column("select received_at from actors where seq > 1 order by seq")
        .await;
    assert_eq!(after.len(), 3);
    assert!(
        after
            .iter()
            .all(|s| s.as_str() >= "2099-01-01T00:00:00.000Z"),
        "{after:?}"
    );
    for table in ["actors", "labels", "bindings", "settings"] {
        let stamps: Vec<String> = app
            .column(&format!("select received_at from {table} order by seq"))
            .await;
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

#[tokio::test]
async fn clock_skew_is_set_when_sent_at_is_more_than_5_minutes_away() {
    let app = App::start().await;
    let mut want = Vec::new();
    for (i, (offset, skewed)) in [(3600, 1), (-3600, 1), (0, 0), (240, 0), (-360, 1)]
        .into_iter()
        .enumerate()
    {
        let mut body = ai("se", &format!("s-{i}"));
        body["sentAt"] = json!(at(offset));
        body["settings"] = json!({ "model": "claude-opus-5-5", "effort": "high" });
        app.register(&body).await;
        want.push(skewed);
    }
    let actors: Vec<i64> = app
        .column("select clock_skew from actors order by seq")
        .await;
    assert_eq!(actors, want);
    let settings: Vec<i64> = app
        .column("select clock_skew from settings order by seq")
        .await;
    assert_eq!(settings, want);
}

#[tokio::test]
async fn clock_skew_of_exactly_5_minutes_is_not_set() {
    let app = App::start().await;
    let first = app.register(&ai("se", "s-0")).await;
    // 受けた時刻を 2099-01-01T00:05:00.000Z に止める（先の時刻の行があれば、受けた時刻はそれになる）。
    app.execute(&format!(
        "insert into labels(actor_id, key, value, received_at)
         values('{}', 'future', 'x', '2099-01-01T00:05:00.000Z')",
        first["actor"]["id"].as_str().unwrap()
    ))
    .await;
    for (i, sent_at) in [
        "2099-01-01T00:00:00Z",
        "2099-01-01T00:10:00Z",
        "2098-12-31T23:59:59.999Z",
        "2099-01-01T00:10:00.001Z",
    ]
    .into_iter()
    .enumerate()
    {
        let mut body = ai("se", &format!("s-{}", i + 1));
        body["sentAt"] = json!(sent_at);
        app.register(&body).await;
    }
    let rows: Vec<String> = app
        .column("select sent_at || ' ' || received_at || ' ' || clock_skew from actors where seq > 1 order by seq")
        .await;
    assert_eq!(
        rows,
        vec![
            "2099-01-01T00:00:00Z 2099-01-01T00:05:00.000Z 0",
            "2099-01-01T00:10:00Z 2099-01-01T00:05:00.000Z 0",
            "2098-12-31T23:59:59.999Z 2099-01-01T00:05:00.000Z 1",
            "2099-01-01T00:10:00.001Z 2099-01-01T00:05:00.000Z 1",
        ]
    );
}

#[tokio::test]
async fn a_registration_waiting_too_long_for_the_write_connection_is_503_unavailable() {
    let app = App::start().await;
    // 書く接続（1 本だけ）を握ったままにする。
    let held = app.db.write().acquire().await.expect("書く接続");
    let (status, headers, answer) = app.post("/api/actors", &ai("se", "s-1")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(error_type(&headers), "Unavailable");
    assert!(
        answer["message"]
            .as_str()
            .is_some_and(|m| m.contains("混んでいる")),
        "{answer}"
    );
    drop(held);
    assert_eq!(app.count("actors").await, 0);
}

/// DB の誤り（混んでいる以外）は、500 の InternalError で、決まった字を返す（中の誤りの字を外に出さない）。
/// 503（@retryable の Unavailable）にすると、直らない誤りで送り手が送り直し続け、roster の guard が Bash を止めるため。
#[tokio::test]
async fn a_db_error_is_a_500_internal_error_with_a_fixed_message() {
    let app = App::start().await;
    // 札の表を外して、札のある登録の insert を落とす。
    app.execute("alter table labels rename to labels_gone")
        .await;
    let mut body = ai("se", "s-1");
    body["labels"] = json!({ "project": "castlist" });
    let (status, headers, answer) = app.post("/api/actors", &body).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(
        !message.contains("labels"),
        "中の誤りの字が出ている: {message}"
    );
    assert_eq!(app.count("actors").await, 0, "transaction ごと戻る");
}

/// 本文の形が崩れた時（JSON でない・要る欄が無い）も 400 で、何も書かない。制御文字は ValidationException。
#[tokio::test]
async fn broken_bodies_are_400_and_write_nothing() {
    let app = App::start().await;
    let not_json = Request::post("/api/actors")
        .header("host", "127.0.0.1:8802")
        .header("content-type", "application/json")
        .body(Body::from("JSON の物でない"))
        .unwrap();
    let (status, _, answer) = app.send(not_json).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");

    let mut no_bindings = ai("se", "s-1");
    no_bindings.as_object_mut().unwrap().remove("bindings");
    let (status, _, answer) = app.post("/api/actors", &no_bindings).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");

    let mut c1 = ai("se", "s-1");
    c1["bindings"] = json!([{ "type": "claude.session", "value": "a\u{85}b" }]);
    let (status, headers, answer) = app.post("/api/actors", &c1).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(error_type(&headers), "ValidationException");

    assert_eq!(app.count("actors").await, 0);
}
