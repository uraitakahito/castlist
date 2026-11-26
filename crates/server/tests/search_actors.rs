//! ListActors（監査の検索）の振る舞い（処理より先に書く）。 0.4.x の server（v0.4.0 の tag）の tests/search.rs の問いを、新しい道と欄の名で引く。
//! 見本は書く操作だけで作り（common::sample）、受けた時刻を決めたい所だけ DB に直に置く。
mod common;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use castlist::search::plans;
use common::sample::{KEY, OPUS, SONNET, Sample, ev, field, names, ok, put, sample, seat, tick};
use common::{App, ai, at, error_type};
use serde_json::{Value, json};

/// 送って 400 の ValidationException であることを見て、message を返す。
async fn invalid(app: &App, uri: &str) -> String {
    let (status, headers, answer) = app.get(uri).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    assert_eq!(error_type(&headers), "ValidationException", "{uri}");
    answer["message"].as_str().unwrap_or_default().to_owned()
}

/// 頁の印の中身を、server と同じ形（16 進）にする（ほかの検索の印を作る試験のため）。
fn token(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

// --- 問いごと ---

#[tokio::test]
async fn who_was_in_workspace_w_at_time_t() {
    let s = sample().await;
    let uri = |at: &str| format!("/api/actors?binding=cmux.workspace.ref:workspace:3&at={at}");
    let answer = ok(&s.app, &uri(&s.se1_entered)).await;
    assert_eq!(names(&answer["actors"]), ["se.1"], "{answer}");
    assert_eq!(
        answer["actors"][0]["url"],
        format!("/actors/{}", s.id("se.1"))
    );
    let answer = ok(&s.app, &uri(&s.marker)).await;
    assert_eq!(names(&answer["actors"]), ["se.3"], "{answer}");
    // at が無ければ asOf の時。
    let answer = ok(&s.app, "/api/actors?binding=cmux.workspace.ref:workspace:3").await;
    assert_eq!(names(&answer["actors"]), ["se.3"], "{answer}");
    let answer = ok(&s.app, &uri("2000-01-01T00:00:00Z")).await;
    assert_eq!(answer["actors"], json!([]), "{answer}");
    assert_eq!(answer["nextToken"], Value::Null);
}

/// 付随する id は、出し手と鍵かを問わず、その値の最新の行のアクター 1 つ。
#[tokio::test]
async fn a_binding_is_the_latest_row_across_issuers_and_keys() {
    let s = sample().await;
    let other = s
        .app
        .register(&json!({
            "issuer": "pagetray",
            "kind": "ai",
            "namePrefix": "pt",
            "bindings": [{ "type": "cmux.workspace.ref", "value": "workspace:3" }],
            "sentAt": at(0),
        }))
        .await;
    let answer = ok(&s.app, "/api/actors?binding=cmux.workspace.ref:workspace:3").await;
    assert_eq!(
        field(&answer["actors"], "id"),
        [other["actor"]["id"].as_str().unwrap()]
    );
    assert_eq!(answer["actors"][0]["issuer"], "pagetray", "{answer}");
    // 前の時刻なら、前の行のアクター。
    let answer = ok(
        &s.app,
        &format!(
            "/api/actors?binding=cmux.workspace.ref:workspace:3&at={}",
            s.marker
        ),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["se.3"], "{answer}");
}

#[tokio::test]
async fn the_actors_of_a_key_including_those_added_by_clear() {
    let s = sample().await;
    let answer = ok(&s.app, &format!("/api/actors?label=tpm.key:{KEY}")).await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "se.1", "se.2", "devops.1", "auditor.1", "se.3"],
        "初めの登場の順: {answer}"
    );
    assert_eq!(answer["nextToken"], Value::Null);
    // ラベルを 2 つ重ねれば、どちらも持つ者だけ。
    let answer = ok(
        &s.app,
        &format!("/api/actors?label=tpm.key:{KEY}&label=tpm.role:software-engineer"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["se.1", "se.2", "se.3"],
        "{answer}"
    );
}

#[tokio::test]
async fn each_row_has_the_list_shape_in_camel_case() {
    let s = sample().await;
    let answer = ok(&s.app, "/api/actors?label=tpm.role:auditor").await;
    let item = &answer["actors"][0];
    let id = s.id("auditor.1");
    assert_eq!(item["id"], id);
    assert_eq!(item["name"], "auditor.1");
    assert_eq!(item["kind"], "ai");
    assert_eq!(item["issuer"], "tpm");
    assert_eq!(item["clockSkew"], false);
    assert!(
        item["sentAt"].as_str().is_some_and(|t| t.ends_with('Z')),
        "{item}"
    );
    assert!(
        item["receivedAt"]
            .as_str()
            .is_some_and(|t| t.ends_with('Z')),
        "{item}"
    );
    assert_eq!(item["url"], format!("/actors/{id}"));
    assert_eq!(
        item["labels"],
        json!({ "project": "castlist", "tpm.key": KEY, "tpm.role": "auditor" })
    );
    assert!(
        item["firstEnteredAt"]
            .as_str()
            .is_some_and(|t| t.ends_with('Z')),
        "{item}"
    );
    assert_eq!(item["lastExitedAt"], Value::Null);
    assert_eq!(item["lastExitType"], Value::Null);
    assert_eq!(item["present"], true);
    assert_eq!(item["inferred"], false);
    assert_eq!(item["model"], OPUS);
    assert_eq!(item["effort"], "high");
    let answer = ok(&s.app, "/api/actors?label=tpm.role:devops").await;
    let item = &answer["actors"][0];
    assert_eq!(item["present"], false);
    assert_eq!(item["lastExitType"], "closed");
    // devops.1 は推しの幕引き（detail.inferred が true の closed）で退いた。
    assert_eq!(item["inferred"], true, "{item}");
    assert!(
        item["lastExitedAt"]
            .as_str()
            .is_some_and(|t| t.ends_with('Z')),
        "{item}"
    );
    // 人は登場していないので、初めの登場も model も無い。
    let answer = ok(&s.app, "/api/actors?label=tpm.role:product-manager").await;
    let item = &answer["actors"][0];
    assert_eq!(item["firstEnteredAt"], Value::Null, "{item}");
    assert_eq!(item["model"], Value::Null, "{item}");
    assert_eq!(item["effort"], Value::Null, "{item}");
    assert_eq!(item["present"], false, "{item}");
}

#[tokio::test]
async fn the_actors_active_in_a_window() {
    let s = sample().await;
    let window = |from: &str, to: &str| format!("/api/actors?activeFrom={from}&activeTo={to}");
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
    let answer = ok(&s.app, &format!("/api/actors?activeFrom={}", s.marker)).await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "auditor.1", "se.3"],
        "{answer}"
    );
    let answer = ok(&s.app, &format!("/api/actors?activeTo={}", s.se1_entered)).await;
    assert_eq!(names(&answer["actors"]), ["tpm.1", "se.1"], "{answer}");
    let answer = ok(
        &s.app,
        &window("2000-01-01T00:00:00Z", "2000-01-02T00:00:00Z"),
    )
    .await;
    assert_eq!(answer["actors"], json!([]));
}

#[tokio::test]
async fn the_actors_launched_by_an_actor() {
    let s = sample().await;
    let answer = ok(&s.app, &format!("/api/actors?launchedBy={}", s.id("tpm.1"))).await;
    assert_eq!(
        names(&answer["actors"]),
        ["se.1", "se.2", "devops.1", "auditor.1"],
        "{answer}"
    );
    // 継いだ（continued）線は数えない。
    let answer = ok(&s.app, &format!("/api/actors?launchedBy={}", s.id("se.1"))).await;
    assert_eq!(answer["actors"], json!([]), "{answer}");
}

#[tokio::test]
async fn narrowing_by_role_and_time_and_kind() {
    let s = sample().await;
    let role = "label=tpm.role:software-engineer";
    let answer = ok(
        &s.app,
        &format!("/api/actors?{role}&activeFrom={0}&activeTo={0}", s.marker),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["se.3"], "{answer}");
    let answer = ok(
        &s.app,
        &format!(
            "/api/actors?{role}&activeFrom={0}&activeTo={0}",
            s.model_changed
        ),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["se.1", "se.2"], "{answer}");
    let answer = ok(&s.app, &format!("/api/actors?{role}&kind=human")).await;
    assert_eq!(answer["actors"], json!([]));
    let answer = ok(&s.app, "/api/actors?label=project:castlist&kind=human").await;
    assert_eq!(names(&answer["actors"]), ["owner.1", "pm.1"], "{answer}");
}

#[tokio::test]
async fn the_actors_that_ever_ran_with_model_m_or_effort_e() {
    let s = sample().await;
    let answer = ok(&s.app, &format!("/api/actors?model={SONNET}")).await;
    assert_eq!(names(&answer["actors"]), ["se.2"], "{answer}");
    // 一覧の model は最新の物。
    assert_eq!(answer["actors"][0]["model"], SONNET);
    let answer = ok(&s.app, "/api/actors?effort=max").await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1"],
        "今は high に戻っていても: {answer}"
    );
    assert_eq!(answer["actors"][0]["effort"], "high");
    let answer = ok(
        &s.app,
        &format!("/api/actors?model={OPUS}&label=tpm.key:{KEY}"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "se.1", "se.2", "devops.1", "auditor.1", "se.3"],
        "{answer}"
    );
    let answer = ok(&s.app, "/api/actors?model=no-such-model").await;
    assert_eq!(answer["actors"], json!([]));
}

#[tokio::test]
async fn active_actors_and_how_they_exited() {
    let s = sample().await;
    let answer = ok(
        &s.app,
        &format!("/api/actors?label=tpm.key:{KEY}&active=true"),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["auditor.1"], "{answer}");
    // active=true は、それだけで絞る条件になる。
    let answer = ok(&s.app, "/api/actors?active=true").await;
    assert_eq!(names(&answer["actors"]), ["auditor.1"], "{answer}");
    let answer = ok(
        &s.app,
        &format!("/api/actors?label=tpm.key:{KEY}&active=false"),
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
        &format!("/api/actors?label=tpm.key:{KEY}&sort=exited"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["auditor.1", "se.1", "se.2", "devops.1", "se.3", "tpm.1"],
        "{answer}"
    );
    assert_eq!(
        field(&answer["actors"], "lastExitType"),
        ["null", "cleared", "exited", "closed", "exited", "exited"]
    );
    // 降順では、退いていない者は後。
    let answer = ok(
        &s.app,
        &format!("/api/actors?label=tpm.key:{KEY}&sort=exited&order=desc"),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "se.3", "devops.1", "se.2", "se.1", "auditor.1"],
        "{answer}"
    );
    // 初めの登場の降順。登場していない人は後で、同じ値（''）は受けた順の降順。
    let answer = ok(&s.app, "/api/actors?label=project:castlist&order=desc").await;
    assert_eq!(
        names(&answer["actors"]),
        [
            "se.3",
            "auditor.1",
            "devops.1",
            "se.2",
            "se.1",
            "tpm.1",
            "pm.1",
            "owner.1"
        ],
        "{answer}"
    );
}

#[tokio::test]
async fn what_became_active_after_time_t() {
    let s = sample().await;
    let answer = ok(
        &s.app,
        &format!("/api/actors?activeFrom={}", s.as_of_before_last),
    )
    .await;
    assert_eq!(
        names(&answer["actors"]),
        ["tpm.1", "auditor.1", "se.3"],
        "{answer}"
    );
}

#[tokio::test]
async fn the_actors_with_skewed_rows() {
    let s = sample().await;
    let answer = ok(
        &s.app,
        &format!("/api/actors?skewed=true&label=tpm.key:{KEY}"),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["auditor.1"], "{answer}");
    // skewed=true は、それだけで絞る条件になる。
    let answer = ok(&s.app, "/api/actors?skewed=true").await;
    assert_eq!(names(&answer["actors"]), ["auditor.1"], "{answer}");
}

/// 時刻ずれは、登録と model と effort の行でも数える（線は見ない）。
#[tokio::test]
async fn skewed_counts_registrations_and_settings_but_not_links() {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["sentAt"] = json!(at(-3600));
    app.register(&body).await;
    app.register(&ai("se", "s-2")).await;
    app.register(&ai("se", "s-3")).await;
    app.register(&ai("se", "s-4")).await;
    let mut setting = common::setting("claude.session:s-2", OPUS, "high");
    setting["sentAt"] = json!(at(3600));
    put(&app, "/api/settings/o-1", &setting).await;
    let mut link = common::link("continued", "claude.session:s-3", "claude.session:s-4");
    link["sentAt"] = json!(at(3600));
    put(&app, "/api/links/l-1", &link).await;
    let answer = ok(&app, "/api/actors?skewed=true").await;
    assert_eq!(names(&answer["actors"]), ["se.1", "se.2"], "{answer}");
}

/// 時刻の条件は、DB に残す字（UTC・ミリ秒まで・24 字）に直して比べる（字のまま比べると `…00Z` が `…00.500Z` より後になる）。
#[tokio::test]
async fn query_times_are_compared_in_the_stored_form() {
    let app = App::start().await;
    let se1 = app.register(&ai("se", "s-1")).await["actor"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    app.event_at("e-1", &se1, "entered", "{}", "2000-01-01T00:00:00.500Z")
        .await;
    let answer = ok(&app, "/api/actors?activeTo=2000-01-01T00:00:00Z").await;
    assert_eq!(answer["actors"], json!([]), "{answer}");
    let answer = ok(&app, "/api/actors?activeTo=2000-01-01T00:00:00.500Z").await;
    assert_eq!(names(&answer["actors"]), ["se.1"], "{answer}");
    // ミリ秒より下は切る。
    let answer = ok(&app, "/api/actors?activeTo=2000-01-01T00:00:00.5009Z").await;
    assert_eq!(names(&answer["actors"]), ["se.1"], "{answer}");
    let answer = ok(&app, "/api/actors?activeTo=2000-01-01T00:00:00.499Z").await;
    assert_eq!(answer["actors"], json!([]), "{answer}");
}

// --- 頁を分ける ---

/// maxResults=1 で nextToken を辿る。頁の間に新しいアクターを足しても、元の組に穴も重なりも無く、終わりで nextToken が無い。
async fn walk(app: &App, query: &str, insert_from: usize) -> Vec<String> {
    let mut seen = Vec::new();
    let mut token: Option<String> = None;
    for page in 0..100 {
        let uri = match &token {
            Some(t) => format!("/api/actors?{query}&maxResults=1&nextToken={t}"),
            None => format!("/api/actors?{query}&maxResults=1"),
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
                    &format!("/api/events/e-new-{insert_from}-{page}"),
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
        match answer["nextToken"].as_str() {
            Some(t) => token = Some(t.to_owned()),
            None => {
                assert!(answer["nextToken"].is_null(), "{answer}");
                return seen;
            }
        }
    }
    panic!("100 頁で終わらない");
}

#[tokio::test]
async fn walking_the_pages_has_no_holes_or_overlaps_even_with_inserts() {
    let s = sample().await;
    let query = "label=project:castlist";
    for (n, (sort, order)) in [("entered", "asc"), ("exited", "desc")]
        .into_iter()
        .enumerate()
    {
        let q = format!("{query}&sort={sort}&order={order}");
        let before = ok(&s.app, &format!("/api/actors?{q}")).await;
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
async fn max_results_defaults_to_100_and_cuts_the_page() {
    let app = App::start().await;
    for i in 0..101 {
        app.register(&ai("se", &format!("s-{i}"))).await;
    }
    let answer = ok(&app, "/api/actors?label=project:castlist").await;
    assert_eq!(answer["actors"].as_array().unwrap().len(), 100);
    let next = answer["nextToken"].as_str().expect("nextToken").to_owned();
    let answer = ok(
        &app,
        &format!("/api/actors?label=project:castlist&nextToken={next}"),
    )
    .await;
    assert_eq!(names(&answer["actors"]), ["se.101"], "{answer}");
    assert_eq!(answer["nextToken"], Value::Null);
}

/// 頁の印は、同じ sort と order の答えの物だけを受ける。崩れた物と、ほかの検索の物も断る。
#[tokio::test]
async fn a_next_token_of_another_sort_order_or_query_is_400() {
    let s: Sample = sample().await;
    let first = ok(&s.app, "/api/actors?label=project:castlist&maxResults=1").await;
    let next = first["nextToken"].as_str().expect("nextToken").to_owned();
    for uri in [
        format!("/api/actors?label=project:castlist&sort=exited&nextToken={next}"),
        format!("/api/actors?label=project:castlist&order=desc&nextToken={next}"),
        "/api/actors?label=project:castlist&nextToken=garbage".to_owned(),
        "/api/actors?label=project:castlist&nextToken=abc".to_owned(),
        format!(
            "/api/actors?label=project:castlist&nextToken={}",
            token("events\n5")
        ),
        format!(
            "/api/actors?label=project:castlist&nextToken={}",
            token("actors\nentered\nasc\nx\nnot-a-seq")
        ),
    ] {
        let message = invalid(&s.app, &uri).await;
        assert!(message.contains("nextToken"), "{uri}: {message}");
    }
    // ほかの条件は印に結ばない（同じ sort と order なら通る）。
    ok(
        &s.app,
        &format!("/api/actors?label=tpm.key:{KEY}&nextToken={next}"),
    )
    .await;
}

// --- 誤り ---

#[tokio::test]
async fn at_without_binding_is_400() {
    let s = sample().await;
    let message = invalid(
        &s.app,
        "/api/actors?label=project:castlist&at=2026-11-24T00:00:00Z",
    )
    .await;
    assert!(
        message.contains("at") && message.contains("binding"),
        "{message}"
    );
}

/// 索引で引ける条件が 1 つも無ければ、アクターの表を端から読むことになるので断る（kind と active=false は数えない）。
#[tokio::test]
async fn a_search_without_an_indexed_condition_is_400() {
    let s = sample().await;
    for uri in [
        "/api/actors",
        "/api/actors?kind=ai",
        "/api/actors?active=false",
        "/api/actors?kind=human&active=false&sort=exited&order=desc&maxResults=5",
    ] {
        let message = invalid(&s.app, uri).await;
        assert!(message.contains("label"), "{uri}: {message}");
    }
}

#[tokio::test]
async fn skewed_false_is_400() {
    let s = sample().await;
    let message = invalid(&s.app, "/api/actors?label=project:castlist&skewed=false").await;
    assert!(message.contains("skewed"), "{message}");
}

/// 値の形の誤りは、生成した層が 400 で断る。
#[tokio::test]
async fn values_of_the_wrong_form_are_400() {
    let s = sample().await;
    for uri in [
        "/api/actors?label=castlist",
        "/api/actors?label=Project:castlist",
        "/api/actors?binding=workspace",
        "/api/actors?label=project:castlist&activeFrom=きのう",
        "/api/actors?label=project:castlist&activeTo=2026-11-24T09:00:00%2B09:00",
        "/api/actors?binding=cmux.workspace.ref:workspace:3&at=2026-13-01",
        "/api/actors?label=project:castlist&maxResults=1001",
        "/api/actors?label=project:castlist&maxResults=0",
        "/api/actors?label=project:castlist&maxResults=ten",
        "/api/actors?label=project:castlist&sort=name",
        "/api/actors?label=project:castlist&order=up",
        "/api/actors?label=project:castlist&active=yes",
        "/api/actors?label=project:castlist&kind=robot",
        "/api/actors?launchedBy=not-an-id",
        "/api/actors?model=Opus",
    ] {
        let (status, _, answer) = s.app.get(uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {answer}");
    }
}

/// restJson1 は知らない引数を読み飛ばし、1 つだけの引数を重ねれば初めの物を使う（0.4.x の server は 400。next.md）。
#[tokio::test]
async fn unknown_and_repeated_single_parameters_are_not_refused() {
    let s = sample().await;
    let answer = ok(&s.app, "/api/actors?label=tpm.role:auditor&colour=red").await;
    assert_eq!(names(&answer["actors"]), ["auditor.1"], "{answer}");
    let answer = ok(
        &s.app,
        &format!("/api/actors?label=tpm.key:{KEY}&sort=exited&sort=entered"),
    )
    .await;
    assert_eq!(names(&answer["actors"])[0], "auditor.1", "{answer}");
}

/// 500 の InternalError で、中の誤りの字（`leak` を含む物）を出さないことを見る。
async fn internal_error(app: &App, uri: &str, leak: &str) {
    let (status, headers, answer) = app.get(uri).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = answer["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "{answer}");
    assert!(!message.contains(leak), "中の誤りの字が出ている: {message}");
}

#[tokio::test]
async fn a_db_error_is_a_500_internal_error_with_a_fixed_message() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    app.execute("alter table labels rename to labels_gone")
        .await;
    internal_error(&app, "/api/actors?label=project:castlist", "labels").await;
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
    internal_error(&app, "/api/actors?label=project:castlist", "not-a-time").await;
}

#[tokio::test]
async fn waiting_too_long_for_a_read_connection_is_503_unavailable() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let mut held = Vec::new();
    for _ in 0..castlist::db::READS {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    let (status, headers, answer) = app.get("/api/actors?label=project:castlist").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(held);
    ok(&app, "/api/actors?label=project:castlist").await;
}

// --- 索引 ---

#[tokio::test]
async fn the_search_sql_does_not_scan_a_big_table() {
    let s = sample().await;
    let plans = plans();
    let names: Vec<&str> = plans.iter().map(|(name, _)| *name).collect();
    for want in [
        "actors_label",
        "actors_labels",
        "actors_binding",
        "actors_window",
        "actors_window_from",
        "actors_model",
        "actors_effort",
        "actors_skewed",
        "actors_launched_by",
        "actors_active",
        "actors_kind",
        "actors_after",
        "actors_exited_desc",
        "actors_all",
        "actor_items",
        "actor_labels",
        "lineage_kind",
        "lineage_up",
        "lineage_down",
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
