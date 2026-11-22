//! 画面の試験: 一覧（`/`）とプロフィール（`/actors/{id}`）と、それが読む CSS（`/static/site.css`）。
//! 見本の記録は、公の API だけで作る。HTML は字で見る（決まった印: 行の link・`rel="next"`・`<section id>`）。

mod common;

use axum::http::{HeaderMap, StatusCode, header};
use common::app::{App, at};
use serde_json::{Value, json};
use std::time::Duration;

const KEY: &str = "t-23zs";
const OPUS: &str = "claude-opus-5-5";
const SONNET: &str = "claude-sonnet-5";

const PM: &str = "tpm.person:pm";
const TPM: &str = "claude.session:s-tpm";
const SE1: &str = "claude.session:s-se1";
const SE2: &str = "claude.session:s-se2";
const OTHER: &str = "claude.session:s-other";

async fn tick() {
    tokio::time::sleep(Duration::from_millis(5)).await;
}

fn seat(prefix: &str, session: &str, key: &str, role: &str, workspace: &str) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "ai",
        "name_prefix": prefix,
        "labels": { "project": "castlist", "tpm.key": key, "tpm.role": role },
        "bindings": [
            { "type": "claude.session", "value": session },
            { "type": "cmux.workspace.ref", "value": workspace },
        ],
        "settings": { "model": OPUS, "effort": "high" },
        "at": at(0),
    })
}

fn ev(by: &str, kind: &str, detail: Value, at: &str) -> Value {
    json!({ "issuer": "tpm", "by": by, "type": kind, "detail": detail, "at": at })
}

fn line(kind: &str, from: &str, to: &str) -> Value {
    json!({ "issuer": "tpm", "type": kind, "from": from, "to": to, "at": at(0) })
}

fn observed(by: &str, model: &str, effort: &str) -> Value {
    json!({ "issuer": "tpm", "by": by, "model": model, "effort": effort, "source": "observed", "at": at(0) })
}

async fn put(app: &App, uri: &str, body: &Value) -> Value {
    let (status, answer) = app.put(uri, body).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "{uri}: {status} {answer}"
    );
    answer
}

struct Sample {
    app: App,
    ids: std::collections::BTreeMap<&'static str, String>,
    /// se.1 の登場を受けた時（se.2 と other.1 は、まだ登場していない）。
    se1_entered: String,
    /// 終わりの印（このとき居るのは pm.1・tpm.1・se.2）。
    end: String,
}

impl Sample {
    fn id(&self, name: &str) -> &str {
        self.ids
            .get(name)
            .unwrap_or_else(|| panic!("{name} が無い"))
    }
}

/// 人の pm.1 → tpm.1 → se.1。se.1 は /model で sonnet を見られ（頼んだ opus と違う）、/clear で退き、se.2 が継ぐ。
/// se.2 は送り手の時計が 1 時間ずれた出来事を持つ。other.1 は別の Key（t-99・haiku・low）で、退いた。
async fn sample() -> Sample {
    let app = App::start().await;
    let mut ids = std::collections::BTreeMap::new();
    let mut reg = async |name: &'static str, body: Value| {
        let answer = app.register(&body).await;
        assert_eq!(answer["name"], name, "{answer}");
        ids.insert(name, answer["id"].as_str().unwrap().to_owned());
        tick().await;
    };
    reg(
        "pm.1",
        json!({
            "issuer": "tpm",
            "kind": "human",
            "name_prefix": "pm",
            "labels": { "project": "castlist", "tpm.role": "product-manager" },
            "bindings": [{ "type": "tpm.person", "value": "pm" }],
            "at": at(0),
        }),
    )
    .await;
    put(
        &app,
        "/v1/events/e-pm-in",
        &ev(PM, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;
    reg("tpm.1", seat("tpm", "s-tpm", KEY, "tpm", "workspace:1")).await;
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
        seat("se", "s-se1", KEY, "software-engineer", "workspace:3"),
    )
    .await;
    put(&app, "/v1/links/l-tpm-se1", &line("launched", TPM, SE1)).await;
    let se1_entered = put(
        &app,
        "/v1/events/e-se1-in",
        &ev(SE1, "entered", json!({}), &at(0)),
    )
    .await["received_at"]
        .as_str()
        .unwrap()
        .to_owned();
    tick().await;
    put(
        &app,
        "/v1/settings/t-se1-sonnet",
        &observed(SE1, SONNET, "high"),
    )
    .await;
    tick().await;
    put(
        &app,
        "/v1/settings/t-se1-opus",
        &observed(SE1, OPUS, "high"),
    )
    .await;
    tick().await;
    put(
        &app,
        "/v1/events/e-se1-compact",
        &ev(SE1, "compacted", json!({}), &at(0)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/v1/events/e-se1-unread",
        &ev(
            SE1,
            "model_unreadable",
            json!({ "reason": "timeout" }),
            &at(0),
        ),
    )
    .await;
    tick().await;
    // 同じ鍵の登録し直しで、workspace が替わった（付随する id の履歴）。
    reg(
        "se.1",
        seat("se", "s-se1", KEY, "software-engineer", "workspace:7"),
    )
    .await;
    put(
        &app,
        "/v1/events/e-se1-clear",
        &ev(SE1, "cleared", json!({ "reason": "clear" }), &at(0)),
    )
    .await;
    tick().await;
    reg(
        "se.2",
        seat("se", "s-se2", KEY, "software-engineer", "workspace:7"),
    )
    .await;
    put(&app, "/v1/links/l-se1-se2", &line("continued", SE1, SE2)).await;
    put(
        &app,
        "/v1/events/e-se2-in",
        &ev(SE2, "entered", json!({ "source": "clear" }), &at(0)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/v1/events/e-se2-skew",
        &ev(SE2, "reported_in", json!({}), &at(3600)),
    )
    .await;
    tick().await;
    let mut other = seat(
        "other",
        "s-other",
        "t-99",
        "software-engineer",
        "workspace:9",
    );
    other["settings"] = json!({ "model": "claude-haiku-5", "effort": "low" });
    reg("other.1", other).await;
    put(
        &app,
        "/v1/events/e-other-in",
        &ev(OTHER, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;
    put(
        &app,
        "/v1/events/e-other-out",
        &ev(OTHER, "exited", json!({}), &at(0)),
    )
    .await;
    tick().await;
    let end = castlist::db::now();
    Sample {
        app,
        ids,
        se1_entered,
        end,
    }
}

/// 画面を引き、状態・header・本文を返す。
async fn page(app: &App, uri: &str) -> (StatusCode, HeaderMap, String) {
    let (status, headers, bytes) = app.raw("GET", uri, None).await;
    (status, headers, String::from_utf8(bytes).expect("UTF-8"))
}

/// 200 の HTML で、CSP が付き、inline の script と style が無く、castlist の CSS を読むことを見る。
async fn html(app: &App, uri: &str) -> String {
    let (status, headers, body) = page(app, uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    assert_html(uri, &headers, &body);
    body
}

fn assert_html(uri: &str, headers: &HeaderMap, body: &str) {
    let get = |name| headers.get(name).and_then(|v| v.to_str().ok());
    assert!(
        get(header::CONTENT_TYPE).is_some_and(|v| v.starts_with("text/html")),
        "{uri}: {headers:?}"
    );
    assert_eq!(
        get(header::CONTENT_SECURITY_POLICY),
        Some(castlist::CSP),
        "{uri}"
    );
    for bad in ["<script", "style=", "<style"] {
        assert!(!body.contains(bad), "{uri}: {bad} がある\n{body}");
    }
    assert!(
        body.contains("<link rel=\"stylesheet\" href=\"/static/site.css"),
        "{uri}: CSS を読まない\n{body}"
    );
}

/// 一覧の行に出たアクターの id（出た順）。行は `href="/actors/<id>"` の link を 1 つずつ持つ。
fn listed(body: &str) -> Vec<String> {
    body.split("href=\"/actors/")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(id, _)| id.to_owned()))
        .collect()
}

/// HTML の字の逃がしを戻す（名の形 `&amp;` と、番号の形 `&#38;` の両方）。
fn unescape(s: &str) -> String {
    [
        ("&lt;", "<"),
        ("&#60;", "<"),
        ("&gt;", ">"),
        ("&#62;", ">"),
        ("&quot;", "\""),
        ("&#34;", "\""),
        ("&#39;", "'"),
        ("&amp;", "&"),
        ("&#38;", "&"),
    ]
    .iter()
    .fold(s.to_owned(), |s, (from, to)| s.replace(from, to))
}

/// 次の頁の link（`rel="next"`）。HTML の逃がしを戻す。
fn next_link(body: &str) -> Option<String> {
    let rest = body.split_once("rel=\"next\" href=\"")?.1;
    Some(unescape(rest.split_once('"')?.0))
}

/// そのアクターの行（`<tr>` 〜 `</tr>`）。
fn row<'a>(body: &'a str, id: &str) -> &'a str {
    let at = body
        .find(&format!("href=\"/actors/{id}\""))
        .unwrap_or_else(|| panic!("{id} の行が無い\n{body}"));
    let start = body[..at].rfind("<tr").expect("<tr");
    let end = at + body[at..].find("</tr>").expect("</tr>");
    &body[start..end]
}

/// `<section id="…">` 〜 `</section>`。
fn section<'a>(body: &'a str, id: &str) -> &'a str {
    let start = body
        .find(&format!("<section id=\"{id}\""))
        .unwrap_or_else(|| panic!("section {id} が無い\n{body}"));
    let end = start + body[start..].find("</section>").expect("</section>");
    &body[start..end]
}

impl Sample {
    async fn listed(&self, uri: &str) -> Vec<String> {
        listed(&html(&self.app, uri).await)
    }

    fn ids_of(&self, names: &[&str]) -> Vec<String> {
        names.iter().map(|n| self.id(n).to_owned()).collect()
    }
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

// --- 一覧 ---

#[tokio::test]
async fn 一覧は_条件が無ければ_この_7_日に居た者を新しい順に出し_そう書く() {
    let s = sample().await;
    let body = html(&s.app, "/").await;
    assert_eq!(
        listed(&body),
        s.ids_of(&["other.1", "se.2", "se.1", "tpm.1", "pm.1"]),
        "{body}"
    );
    assert!(body.contains("この 7 日"), "既定の条件を書く\n{body}");
}

#[tokio::test]
async fn 一覧の行は_名_種類_居るか_最後の出来事_札_model_と_effort_時刻を出す() {
    let s = sample().await;
    let body = html(&s.app, "/").await;
    let se1 = row(&body, s.id("se.1"));
    for want in [
        ">se.1<",
        "AI",
        "いない",
        "交代",
        KEY,
        "software-engineer",
        OPUS,
        "high",
        "<time datetime=\"",
    ] {
        assert!(se1.contains(want), "se.1 の行に {want} が無い\n{se1}");
    }
    let pm = row(&body, s.id("pm.1"));
    for want in [">pm.1<", "人", "いる", "登場", "product-manager"] {
        assert!(pm.contains(want), "pm.1 の行に {want} が無い\n{pm}");
    }
    let other = row(&body, s.id("other.1"));
    for want in ["退場", "claude-haiku-5", "low", "t-99"] {
        assert!(
            other.contains(want),
            "other.1 の行に {want} が無い\n{other}"
        );
    }
    // 送り手の時計がずれた行を持つ者だけに、ずれの印。
    assert!(row(&body, s.id("se.2")).contains("class=\"skew\""));
    assert!(!se1.contains("class=\"skew\""), "{se1}");
}

#[tokio::test]
async fn 一覧の絞りは_検索の_api_と同じ引数で狭める() {
    let s = sample().await;
    let cases: Vec<(String, Vec<&str>)> = vec![
        (
            format!("/?label=tpm.key:{KEY}"),
            vec!["tpm.1", "se.1", "se.2"],
        ),
        (
            format!("/?label=tpm.key:{KEY}&label=tpm.role:software-engineer"),
            vec!["se.1", "se.2"],
        ),
        ("/?active=true".to_owned(), vec!["pm.1", "tpm.1", "se.2"]),
        ("/?active=false".to_owned(), vec!["se.1", "other.1"]),
        (format!("/?model={SONNET}"), vec!["se.1"]),
        ("/?effort=low".to_owned(), vec!["other.1"]),
        ("/?skewed=true".to_owned(), vec!["se.2"]),
        (
            format!("/?active_to={}", s.se1_entered),
            vec!["pm.1", "tpm.1", "se.1"],
        ),
        (
            format!("/?active_from={}", s.end),
            vec!["pm.1", "tpm.1", "se.2"],
        ),
        // 画面の form は空の欄も送る。空の値は、付けなかったのと同じ。
        (
            format!("/?label=tpm.key:{KEY}&label=&active=&model=&effort=&active_from=&active_to="),
            vec!["tpm.1", "se.1", "se.2"],
        ),
    ];
    for (uri, want) in cases {
        let got = s.listed(&uri).await;
        assert_eq!(sorted(got), sorted(s.ids_of(&want)), "{uri}");
    }
}

#[tokio::test]
async fn 一覧の次の頁の_link_を辿ると_穴も重なりも無く終わる() {
    let s = sample().await;
    let mut uri = format!("/?label=tpm.key:{KEY}&limit=1");
    let mut seen = Vec::new();
    for _ in 0..10 {
        let body = html(&s.app, &uri).await;
        let ids = listed(&body);
        assert!(ids.len() <= 1, "{uri}: {ids:?}");
        seen.extend(ids);
        match next_link(&body) {
            Some(next) => {
                assert!(next.starts_with("/?"), "{next}");
                assert!(next.contains("after="), "{next}");
                uri = next;
            }
            None => break,
        }
    }
    // 新しい順（初めの登場の降順）。
    assert_eq!(seen, s.ids_of(&["se.2", "se.1", "tpm.1"]));
}

#[tokio::test]
async fn 一覧の引数の誤りは_引数の名を書いた_400_の画面() {
    let s = sample().await;
    for (uri, name) in [
        ("/?colour=red", "colour"),
        ("/?active_from=きのう", "active_from"),
        ("/?label=nocolon", "label"),
        ("/?after=zz", "after"),
    ] {
        let (status, headers, body) = page(&s.app, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_html(uri, &headers, &body);
        assert!(body.contains(name), "{uri}: {name} を書かない\n{body}");
    }
}

// --- プロフィール ---

#[tokio::test]
async fn プロフィールは_名_種類_id_issuer_札_今の様子を出す() {
    let s = sample().await;
    let id = s.id("se.1");
    let body = html(&s.app, &format!("/actors/{id}")).await;
    for want in [
        "se.1",
        "AI",
        id,
        "tpm",
        KEY,
        "software-engineer",
        "castlist",
    ] {
        assert!(body.contains(want), "{want} が無い\n{body}");
    }
    let state = section(&body, "state");
    for want in ["いない", "交代", OPUS, "high", "<time datetime=\""] {
        assert!(state.contains(want), "今の様子に {want} が無い\n{state}");
    }
    let bindings = section(&body, "bindings");
    for want in [
        "claude.session",
        "s-se1",
        "cmux.workspace.ref",
        "workspace:3",
        "workspace:7",
    ] {
        assert!(
            bindings.contains(want),
            "付随する id に {want} が無い\n{bindings}"
        );
    }
}

#[tokio::test]
async fn プロフィールは_起こした人を人の根まで_起こした者_前と後を出す() {
    let s = sample().await;
    let body = html(&s.app, &format!("/actors/{}", s.id("se.2"))).await;
    // se.2 は se.1 を継いだ。se.1 は tpm.1 が、tpm.1 は人の pm.1 が起こした。
    let chain = listed(section(&body, "launched-by"));
    assert_eq!(chain, s.ids_of(&["pm.1", "tpm.1", "se.1"]), "{body}");
    assert_eq!(
        listed(section(&body, "continued")),
        s.ids_of(&["se.1"]),
        "前"
    );
    assert!(section(&body, "continued").contains("前"));

    let body = html(&s.app, &format!("/actors/{}", s.id("se.1"))).await;
    assert_eq!(
        listed(section(&body, "continued")),
        s.ids_of(&["se.2"]),
        "後"
    );
    assert!(section(&body, "continued").contains("後"));
    assert_eq!(
        listed(section(&body, "launched-by")),
        s.ids_of(&["pm.1", "tpm.1"])
    );

    let body = html(&s.app, &format!("/actors/{}", s.id("tpm.1"))).await;
    assert_eq!(listed(section(&body, "children")), s.ids_of(&["se.1"]));
    assert_eq!(listed(section(&body, "launched-by")), s.ids_of(&["pm.1"]));

    // 人の根には、起こした人がいない。
    let body = html(&s.app, &format!("/actors/{}", s.id("pm.1"))).await;
    assert!(listed(section(&body, "launched-by")).is_empty());
    assert_eq!(listed(section(&body, "children")), s.ids_of(&["tpm.1"]));
}

#[tokio::test]
async fn プロフィールの_model_と_effort_の履歴は_頼んだ物と違う見た行に印を付ける() {
    let s = sample().await;
    let body = html(&s.app, &format!("/actors/{}", s.id("se.1"))).await;
    let settings = section(&body, "settings");
    assert_eq!(
        settings.matches("<tr").count() - 1,
        3,
        "見出しと 3 行\n{settings}"
    );
    for want in ["頼んだ", "見た", OPUS, SONNET, "high", "<time datetime=\""] {
        assert!(settings.contains(want), "{want} が無い\n{settings}");
    }
    // sonnet を見た行だけが、頼んだ opus と違う。
    assert_eq!(
        settings.matches("class=\"mismatch\"").count(),
        1,
        "{settings}"
    );
    let sonnet_row = settings
        .split("<tr")
        .find(|r| r.contains(SONNET))
        .expect("sonnet の行");
    assert!(sonnet_row.contains("class=\"mismatch\""), "{sonnet_row}");
}

#[tokio::test]
async fn プロフィールの出来事は_日本語の名と訳と送り手の時刻と_ずれの印を出す() {
    let s = sample().await;
    let body = html(&s.app, &format!("/actors/{}", s.id("se.1"))).await;
    let events = section(&body, "events");
    for want in ["登場", "要約", "model 不明", "timeout", "交代", "clear"] {
        assert!(events.contains(want), "{want} が無い\n{events}");
    }
    assert!(!events.contains("class=\"skew\""), "{events}");

    let body = html(&s.app, &format!("/actors/{}", s.id("se.2"))).await;
    let events = section(&body, "events");
    for want in ["登場", "名乗り", "clear", "class=\"skew\""] {
        assert!(events.contains(want), "{want} が無い\n{events}");
    }
}

#[tokio::test]
async fn 知らない_id_のプロフィールは_404_の画面() {
    let s = sample().await;
    let (status, headers, body) = page(&s.app, "/actors/no-such-actor").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_html("/actors/no-such-actor", &headers, &body);
    assert!(body.contains("無い"), "{body}");
}

// --- 守りと CSS ---

#[tokio::test]
async fn css_は_castlist_が_text_css_で配り_明るい色と暗い色の両方を持つ() {
    let s = sample().await;
    let (status, headers, bytes) = s.app.raw("GET", "/static/site.css", None).await;
    assert_eq!(status, StatusCode::OK);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.starts_with("text/css"), "{content_type}");
    let css = String::from_utf8(bytes).expect("UTF-8");
    assert!(css.contains("prefers-color-scheme: dark"), "{css}");
    assert!(headers.get(header::CACHE_CONTROL).is_some(), "{headers:?}");
}

#[tokio::test]
async fn 利用者の字は_html_で逃がす() {
    let s = sample().await;
    let mut body = seat(
        "x",
        "s-x",
        "<script>alert(1)</script>",
        "a\"b",
        "workspace:1",
    );
    body["labels"]["note"] = json!("<b>太</b>&");
    let x = s.app.register(&body).await;
    let x_ref = "claude.session:s-x";
    put(
        &s.app,
        "/v1/events/e-x-in",
        &ev(
            x_ref,
            "entered",
            json!({ "reason": "<img src=x onerror=alert(1)>" }),
            &at(0),
        ),
    )
    .await;
    let id = x["id"].as_str().unwrap();
    for uri in [
        "/?label=tpm.key:%3Cscript%3Ealert(1)%3C/script%3E".to_owned(),
        format!("/actors/{id}"),
    ] {
        let page = html(&s.app, &uri).await;
        assert!(page.contains(id), "{uri}\n{page}");
        // 生の字は無く（html() が <script を見る）、逃がしを戻すと元の字になる。
        assert!(
            unescape(&page).contains("<script>alert(1)</script>"),
            "{uri}\n{page}"
        );
        assert!(!page.contains("<b>"), "{uri}\n{page}");
        assert!(!page.contains("<img"), "{uri}\n{page}");
    }
}

// --- 系図（相関図 A） ---

/// 系図の SVG（`<svg` 〜 `</svg>`）。
fn svg(body: &str) -> &str {
    let start = body
        .find("<svg")
        .unwrap_or_else(|| panic!("svg が無い\n{body}"));
    let end = start + body[start..].find("</svg>").expect("</svg>");
    &body[start..end]
}

/// 系図の、そのアクターの箱（`<g class="node …">` 〜 `</g>`）。
fn tree_node<'a>(svg: &'a str, id: &str) -> &'a str {
    let at = svg
        .find(&format!("href=\"/actors/{id}\""))
        .unwrap_or_else(|| panic!("{id} の箱が無い\n{svg}"));
    let start = svg[..at].rfind("<g class=\"node").expect("<g class=\"node");
    let end = at + svg[at..].find("</g>").expect("</g>");
    &svg[start..end]
}

#[tokio::test]
async fn 一覧の頭から系図へ行ける() {
    let s = sample().await;
    let body = html(&s.app, "/").await;
    assert!(body.contains("href=\"/graph/tree\""), "{body}");
}

#[tokio::test]
async fn 系図は_点ごとに_1_つの_link_と_起こした線と継いだ線を描く() {
    let s = sample().await;
    let body = html(&s.app, "/graph/tree").await;
    let svg = svg(&body);
    assert_eq!(
        sorted(listed(svg)),
        sorted(s.ids_of(&["pm.1", "tpm.1", "se.1", "se.2", "other.1"])),
        "{svg}"
    );
    // pm.1 → tpm.1 → se.1 は起こした（実線）、se.1 → se.2 は継いだ（点線）。
    assert_eq!(svg.matches("class=\"edge launched\"").count(), 2, "{svg}");
    assert_eq!(svg.matches("class=\"edge continued\"").count(), 1, "{svg}");
    // 凡例。
    for want in ["実線", "点線"] {
        assert!(body.contains(want), "{want} が無い\n{body}");
    }
}

#[tokio::test]
async fn 系図の箱は_名_種類と短い_model_と_effort_居るかと出来事を出す() {
    let s = sample().await;
    let body = html(&s.app, "/graph/tree").await;
    let svg = svg(&body);
    let se1 = tree_node(svg, s.id("se.1"));
    for want in [">se.1<", "AI", "opus-5-5 · high", "いない", "交代"] {
        assert!(se1.contains(want), "se.1 の箱に {want} が無い\n{se1}");
    }
    assert!(!se1.contains("claude-"), "claude- を落とす\n{se1}");
    assert!(se1.contains("class=\"node ai\""), "{se1}");
    let pm = tree_node(svg, s.id("pm.1"));
    for want in [">pm.1<", "人", "いる", "登場", "class=\"node human\""] {
        assert!(pm.contains(want), "pm.1 の箱に {want} が無い\n{pm}");
    }
    let other = tree_node(svg, s.id("other.1"));
    assert!(other.contains("haiku-5 · low"), "{other}");
}

#[tokio::test]
async fn 系図の絞りは_相関図の_api_と同じ引数で狭める() {
    let s = sample().await;
    let cases: Vec<(String, Vec<&str>, usize, usize)> = vec![
        // Key で絞った組と、そこから出る線の一歩先（人の根 pm.1）。
        (
            format!("/graph/tree?label=tpm.key:{KEY}"),
            vec!["pm.1", "tpm.1", "se.1", "se.2"],
            2,
            1,
        ),
        (
            "/graph/tree?label=tpm.key:t-99".to_owned(),
            vec!["other.1"],
            0,
            0,
        ),
        (
            format!("/graph/tree?to={}", s.se1_entered),
            vec!["pm.1", "tpm.1", "se.1", "se.2"],
            2,
            1,
        ),
        // 画面の form は空の欄も送る。空の値は、付けなかったのと同じ。
        (
            format!("/graph/tree?label=tpm.key:{KEY}&from=&to="),
            vec!["pm.1", "tpm.1", "se.1", "se.2"],
            2,
            1,
        ),
    ];
    for (uri, want, launched, continued) in cases {
        let body = html(&s.app, &uri).await;
        let svg = svg(&body);
        assert_eq!(sorted(listed(svg)), sorted(s.ids_of(&want)), "{uri}");
        assert_eq!(
            svg.matches("class=\"edge launched\"").count(),
            launched,
            "{uri}"
        );
        assert_eq!(
            svg.matches("class=\"edge continued\"").count(),
            continued,
            "{uri}"
        );
    }
}

#[tokio::test]
async fn 系図に当たる者が無ければ_そう書き_svg_を出さない() {
    let s = sample().await;
    let body = html(&s.app, &format!("/graph/tree?from={}", s.end)).await;
    assert!(!body.contains("<svg"), "{body}");
    assert!(body.contains("当たるアクターはいない"), "{body}");
}

#[tokio::test]
async fn 系図の引数の誤りは_引数の名を書いた_400_の画面() {
    let s = sample().await;
    for (uri, name) in [
        ("/graph/tree?colour=red", "colour"),
        ("/graph/tree?from=きのう", "from"),
        ("/graph/tree?to=きのう", "to"),
        ("/graph/tree?label=nocolon", "label"),
        ("/graph/tree?label=a:b&label=c:d", "label"),
    ] {
        let (status, headers, body) = page(&s.app, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_html(uri, &headers, &body);
        assert!(body.contains(name), "{uri}: {name} を書かない\n{body}");
    }
}

#[tokio::test]
async fn 系図も利用者の字を_html_で逃がす() {
    let s = sample().await;
    let x = s
        .app
        .register(&seat(
            "x",
            "s-x",
            "<script>alert(1)</script>",
            "a\"b",
            "workspace:1",
        ))
        .await;
    let id = x["id"].as_str().unwrap();
    let page = html(
        &s.app,
        "/graph/tree?label=tpm.key:%3Cscript%3Ealert(1)%3C/script%3E",
    )
    .await;
    assert_eq!(listed(svg(&page)), vec![id.to_owned()], "{page}");
    // 生の字は無く（html() が <script を見る）、逃がしを戻すと、form の欄に元の字がある。
    assert!(
        unescape(&page).contains("<script>alert(1)</script>"),
        "{page}"
    );
}

// --- 時間の帯（相関図 B） ---

/// 時間の帯の、そのアクターの行（`<g class="lane …">` 〜 `</g>`）。
fn lane<'a>(svg: &'a str, id: &str) -> &'a str {
    let at = svg
        .find(&format!("href=\"/actors/{id}\""))
        .unwrap_or_else(|| panic!("{id} の行が無い\n{svg}"));
    let start = svg[..at].rfind("<g class=\"lane").expect("<g class=\"lane");
    let end = at + svg[at..].find("</g>").expect("</g>");
    &svg[start..end]
}

/// 行の帯の切れの class（`seg m-…`）を、出た順に。
fn segs(lane: &str) -> Vec<&str> {
    lane.split("<rect class=\"seg ")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(class, _)| class))
        .collect()
}

#[tokio::test]
async fn 一覧と系図から時間の帯へ行け_時間の帯から系図と一覧へ戻れる() {
    let s = sample().await;
    for uri in ["/", "/graph/tree"] {
        let body = html(&s.app, uri).await;
        assert!(body.contains("href=\"/graph/timeline\""), "{uri}\n{body}");
    }
    let body = html(&s.app, "/graph/timeline").await;
    assert!(body.contains("href=\"/graph/tree\""), "{body}");
    assert!(body.contains("href=\"/\""), "{body}");
}

#[tokio::test]
async fn 時間の帯は_アクターごとに_1_行で_人を上に初めの登場の順に並べ_名はプロフィールへの_link() {
    let s = sample().await;
    let body = html(&s.app, "/graph/timeline").await;
    let svg = svg(&body);
    assert!(svg.contains("class=\"timeline\""), "{svg}");
    assert_eq!(
        listed(svg),
        s.ids_of(&["pm.1", "tpm.1", "se.1", "se.2", "other.1"]),
        "{svg}"
    );
    assert_eq!(svg.matches("<g class=\"lane").count(), 5, "{svg}");
    let pm = lane(svg, s.id("pm.1"));
    assert!(pm.contains(">pm.1<") && pm.contains(">人<"), "{pm}");
    assert!(pm.contains("class=\"lane human\""), "{pm}");
    let se1 = lane(svg, s.id("se.1"));
    assert!(se1.contains(">se.1<") && se1.contains(">AI<"), "{se1}");
    // 目盛り（時刻の字）がある。
    assert!(svg.contains("class=\"tick\""), "{svg}");
}

#[tokio::test]
async fn 時間の帯は_model_と_effort_の替わり目で区切り_model_ごとの色の_class_を付ける() {
    let s = sample().await;
    let body = html(&s.app, "/graph/timeline").await;
    let svg = svg(&body);
    // se.1 は 頼んだ opus → 見た sonnet（頼んだ物と違う）→ 見た opus で、3 つに切れる。ほかは 1 つずつ。
    assert_eq!(
        segs(lane(svg, s.id("se.1"))),
        vec!["m-opus", "m-sonnet", "m-opus"],
        "{svg}"
    );
    assert_eq!(segs(lane(svg, s.id("tpm.1"))), vec!["m-opus"]);
    assert_eq!(segs(lane(svg, s.id("se.2"))), vec!["m-opus"]);
    assert_eq!(segs(lane(svg, s.id("other.1"))), vec!["m-haiku"]);
    // 人の pm.1 には model と effort が無い。
    assert_eq!(segs(lane(svg, s.id("pm.1"))), vec!["m-none"]);
    assert_eq!(svg.matches("<rect class=\"seg ").count(), 7, "{svg}");
    // 切れの title に、短い model と effort。
    let se1 = lane(svg, s.id("se.1"));
    for want in ["opus-5-5 · high", "sonnet-5 · high"] {
        assert!(se1.contains(want), "se.1 に {want} が無い\n{se1}");
    }
    assert!(!svg.contains("claude-"), "claude- を落とす\n{svg}");
}

#[tokio::test]
async fn 時間の帯の区切りは_送り手の時刻でなく_受けた順() {
    let s = sample().await;
    let x = s
        .app
        .register(&seat("x", "s-x", "t-x", "software-engineer", "workspace:2"))
        .await;
    let x_ref = "claude.session:s-x";
    put(
        &s.app,
        "/v1/events/e-x-in",
        &ev(x_ref, "entered", json!({}), &at(0)),
    )
    .await;
    tick().await;
    // 送り手の時刻は、後に送った物ほど前（時計が戻った）。
    for (id, model, secs) in [
        ("t-x-1", SONNET, 3600),
        ("t-x-2", "claude-haiku-5", 0),
        ("t-x-3", OPUS, -3600),
    ] {
        let mut body = observed(x_ref, model, "high");
        body["at"] = json!(at(secs));
        put(&s.app, &format!("/v1/settings/{id}"), &body).await;
        tick().await;
    }
    let body = html(&s.app, "/graph/timeline?label=tpm.key:t-x").await;
    let svg = svg(&body);
    let id = x["id"].as_str().unwrap();
    assert_eq!(
        segs(lane(svg, id)),
        vec!["m-opus", "m-sonnet", "m-haiku", "m-opus"],
        "{svg}"
    );
}

#[tokio::test]
async fn 時間の帯の凡例は_model_と線と出来事の印を出す() {
    let s = sample().await;
    let body = html(&s.app, "/graph/timeline").await;
    let legend = section(&body, "legend");
    for want in [
        "class=\"swatch m-opus\"",
        "opus-5-5",
        "class=\"swatch m-sonnet\"",
        "sonnet-5",
        "class=\"swatch m-haiku\"",
        "haiku-5",
        "実線",
        "点線",
        "要約",
        "名乗り",
        "model 不明",
    ] {
        assert!(legend.contains(want), "凡例に {want} が無い\n{legend}");
    }
    assert!(!legend.contains("claude-"), "{legend}");
}

#[tokio::test]
async fn 時間の帯は_起こした線と継いだ線の矢印と_出来事の印と_居る者の開いた端を描く() {
    let s = sample().await;
    let body = html(&s.app, "/graph/timeline").await;
    let svg = svg(&body);
    assert_eq!(svg.matches("class=\"link launched\"").count(), 2, "{svg}");
    assert_eq!(svg.matches("class=\"link continued\"").count(), 1, "{svg}");
    let se1 = lane(svg, s.id("se.1"));
    assert!(se1.contains("class=\"mark compacted\""), "{se1}");
    assert!(se1.contains("class=\"mark model_unreadable\""), "{se1}");
    assert!(se1.contains("<title>要約"), "{se1}");
    assert!(se1.contains("<title>model 不明"), "{se1}");
    let se2 = lane(svg, s.id("se.2"));
    assert!(se2.contains("class=\"mark reported_in\""), "{se2}");
    assert!(se2.contains("<title>名乗り"), "{se2}");
    // 居る者（pm.1・tpm.1・se.2）だけ、帯の端を開く。
    for (name, open) in [
        ("pm.1", true),
        ("tpm.1", true),
        ("se.1", false),
        ("se.2", true),
        ("other.1", false),
    ] {
        let lane = lane(svg, s.id(name));
        assert_eq!(lane.contains("class=\"open-end\""), open, "{name}\n{lane}");
    }
}

#[tokio::test]
async fn 時間の帯の絞りは_相関図の_api_と同じ引数で狭める() {
    let s = sample().await;
    let cases: Vec<(String, Vec<&str>)> = vec![
        (
            format!("/graph/timeline?label=tpm.key:{KEY}"),
            vec!["pm.1", "tpm.1", "se.1", "se.2"],
        ),
        (
            "/graph/timeline?label=tpm.key:t-99".to_owned(),
            vec!["other.1"],
        ),
        (
            format!("/graph/timeline?to={}", s.se1_entered),
            vec!["pm.1", "tpm.1", "se.1", "se.2"],
        ),
        (
            format!("/graph/timeline?label=tpm.key:{KEY}&from=&to="),
            vec!["pm.1", "tpm.1", "se.1", "se.2"],
        ),
    ];
    for (uri, want) in cases {
        let body = html(&s.app, &uri).await;
        let svg = svg(&body);
        assert_eq!(listed(svg), s.ids_of(&want), "{uri}");
    }
}

#[tokio::test]
async fn 時間の帯に当たる者が無ければ_そう書き_svg_を出さない() {
    let s = sample().await;
    let body = html(&s.app, &format!("/graph/timeline?from={}", s.end)).await;
    assert!(!body.contains("<svg"), "{body}");
    assert!(body.contains("当たるアクターはいない"), "{body}");
}

#[tokio::test]
async fn 時間の帯の引数の誤りは_引数の名を書いた_400_の画面() {
    let s = sample().await;
    for (uri, name) in [
        ("/graph/timeline?colour=red", "colour"),
        ("/graph/timeline?from=きのう", "from"),
        ("/graph/timeline?to=きのう", "to"),
        ("/graph/timeline?label=nocolon", "label"),
        ("/graph/timeline?label=a:b&label=c:d", "label"),
    ] {
        let (status, headers, body) = page(&s.app, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_html(uri, &headers, &body);
        assert!(body.contains(name), "{uri}: {name} を書かない\n{body}");
    }
}

#[tokio::test]
async fn 時間の帯も利用者の字を_html_で逃がす() {
    let s = sample().await;
    let x = s
        .app
        .register(&seat(
            "x",
            "s-x",
            "<script>alert(1)</script>",
            "a\"b",
            "workspace:1",
        ))
        .await;
    put(
        &s.app,
        "/v1/events/e-x-in",
        &ev("claude.session:s-x", "entered", json!({}), &at(0)),
    )
    .await;
    let id = x["id"].as_str().unwrap();
    let page = html(
        &s.app,
        "/graph/timeline?label=tpm.key:%3Cscript%3Ealert(1)%3C/script%3E",
    )
    .await;
    assert_eq!(listed(svg(&page)), vec![id.to_owned()], "{page}");
    assert!(
        unescape(&page).contains("<script>alert(1)</script>"),
        "{page}"
    );
}
