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
    // 同じ鍵の登録し直しで、workspace が替わった（外部の id の履歴）。
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
async fn 一覧の行は_名_種類_状態_最後の出来事_ラベル_model_と_effort_時刻を出す() {
    let s = sample().await;
    let body = html(&s.app, "/").await;
    let se1 = row(&body, s.id("se.1"));
    for want in [
        ">se.1<",
        "AI",
        "停止",
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
    for want in [">pm.1<", "人", "稼働中", "登場", "product-manager"] {
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
async fn プロフィールは_名_種類_id_issuer_ラベル_今の状態を出す() {
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
    for want in ["停止", "交代", OPUS, "high", "<time datetime=\""] {
        assert!(state.contains(want), "今の状態に {want} が無い\n{state}");
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
            "外部の id に {want} が無い\n{bindings}"
        );
    }
}

#[tokio::test]
async fn プロフィールは_起動した人を人の起点まで_起動したアクター_引き継ぎを出す() {
    let s = sample().await;
    let body = html(&s.app, &format!("/actors/{}", s.id("se.2"))).await;
    // se.2 は se.1 を引き継いだ。se.1 は tpm.1 が、tpm.1 は人の pm.1 が起動した。
    let chain = listed(section(&body, "launched-by"));
    assert_eq!(chain, s.ids_of(&["pm.1", "tpm.1", "se.1"]), "{body}");
    assert_eq!(
        listed(section(&body, "continued")),
        s.ids_of(&["se.1"]),
        "引き継ぎ元"
    );
    assert!(section(&body, "continued").contains("引き継ぎ元"));

    let body = html(&s.app, &format!("/actors/{}", s.id("se.1"))).await;
    assert_eq!(
        listed(section(&body, "continued")),
        s.ids_of(&["se.2"]),
        "引き継ぎ先"
    );
    assert!(section(&body, "continued").contains("引き継ぎ先"));
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
async fn プロフィールの_model_と_effort_の履歴は_指定と違う実際の行に食い違いの印を付ける() {
    let s = sample().await;
    let body = html(&s.app, &format!("/actors/{}", s.id("se.1"))).await;
    let settings = section(&body, "settings");
    assert_eq!(
        settings.matches("<tr").count() - 1,
        3,
        "見出しと 3 行\n{settings}"
    );
    for want in ["指定", "実際", OPUS, SONNET, "high", "<time datetime=\""] {
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
async fn プロフィールの出来事は_日本語の名と理由と送り手の時刻と_時刻ずれの印を出す() {
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

// --- 系図（図 A） ---

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
async fn 系図の箱は_名_種類と短い_model_と_effort_状態と出来事を出す() {
    let s = sample().await;
    let body = html(&s.app, "/graph/tree").await;
    let svg = svg(&body);
    let se1 = tree_node(svg, s.id("se.1"));
    for want in [">se.1<", "AI", "opus-5-5 · high", "停止", "交代"] {
        assert!(se1.contains(want), "se.1 の箱に {want} が無い\n{se1}");
    }
    assert!(!se1.contains("claude-"), "claude- を落とす\n{se1}");
    assert!(se1.contains("class=\"node ai\""), "{se1}");
    let pm = tree_node(svg, s.id("pm.1"));
    for want in [">pm.1<", "人", "稼働中", "登場", "class=\"node human\""] {
        assert!(pm.contains(want), "pm.1 の箱に {want} が無い\n{pm}");
    }
    let other = tree_node(svg, s.id("other.1"));
    assert!(other.contains("haiku-5 · low"), "{other}");
}

#[tokio::test]
async fn 系図の絞りは_図の_api_と同じ引数で狭める() {
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
    assert!(body.contains("条件に合うアクターはいません"), "{body}");
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

// --- タイムライン（図 B） ---

/// タイムラインの、そのアクターの行（`<g class="lane …">` 〜 `</g>`）。
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
async fn 一覧と系図からタイムラインへ行け_タイムラインから系図と一覧へ戻れる() {
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
async fn タイムラインは_アクターごとに_1_行で_人を上に初めの登場の順に並べ_名はプロフィールへの_link()
 {
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
async fn タイムラインは_model_と_effort_の替わり目で区切り_model_ごとの色の_class_を付ける() {
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
async fn タイムラインの区切りは_送り手の時刻でなく_受けた順() {
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
async fn タイムラインの凡例は_model_と線と出来事の印を出す() {
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
async fn タイムラインは_起こした線と継いだ線の矢印と_出来事の印と_居る者の開いた端を描く() {
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
async fn タイムラインの絞りは_図の_api_と同じ引数で狭める() {
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
async fn タイムラインに当たる者が無ければ_そう書き_svg_を出さない() {
    let s = sample().await;
    let body = html(&s.app, &format!("/graph/timeline?from={}", s.end)).await;
    assert!(!body.contains("<svg"), "{body}");
    assert!(body.contains("条件に合うアクターはいません"), "{body}");
}

#[tokio::test]
async fn タイムラインの引数の誤りは_引数の名を書いた_400_の画面() {
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
async fn タイムラインも利用者の字を_html_で逃がす() {
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

// --- 相関図（図 C） ---

/// 取り込んだ cytoscape の path（static/vendor/SOURCES.md の file と同じ）。
const CYTOSCAPE: &str = "/static/vendor/cytoscape-3.35.0.min.js";

/// 相関図の画面: 200 の HTML で CSP が付き、script は castlist が配る 2 つの file だけ（中身の無い `<script src>`）で、
/// inline の script と style が無く、castlist の CSS を読むことを見る。
async fn network_html(app: &App, uri: &str) -> String {
    let (status, headers, body) = page(app, uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    let vendor = format!("<script src=\"{CYTOSCAPE}\"></script>");
    assert_eq!(
        body.matches(&vendor).count(),
        1,
        "{uri}: cytoscape を読まない\n{body}"
    );
    let ours = body
        .split_once("<script src=\"/static/network.js?v=")
        .and_then(|(_, rest)| rest.split_once("\"></script>"))
        .map(|(v, _)| v)
        .unwrap_or_else(|| panic!("{uri}: network.js を読まない\n{body}"));
    assert!(
        !ours.is_empty() && ours.chars().all(|c| c.is_ascii_hexdigit()),
        "{uri}: network.js の ?v= が中身の写しでない: {ours}"
    );
    // 2 つの script の印を除けば、ほかの画面と同じ決まり（<script も style も無い）。
    let rest = body.replace(&vendor, "").replace(
        &format!("<script src=\"/static/network.js?v={ours}\"></script>"),
        "",
    );
    assert_html(uri, &headers, &rest);
    body
}

/// 相関図が絵を描く場所（`id="network"` の要素）の、data を引く URL（`data-src`。HTML の逃がしを戻す）。
fn data_src(body: &str) -> String {
    let at = body
        .find("id=\"network\"")
        .unwrap_or_else(|| panic!("id=\"network\" が無い\n{body}"));
    let tag = &body[body[..at].rfind('<').expect("<")..at + body[at..].find('>').expect(">")];
    let rest = tag
        .split_once("data-src=\"")
        .unwrap_or_else(|| panic!("data-src が無い\n{tag}"))
        .1;
    unescape(rest.split_once('"').expect("\"").0)
}

#[tokio::test]
async fn 一覧と系図とタイムラインから相関図へ行け_相関図から三つへ戻れる() {
    let s = sample().await;
    for uri in ["/", "/graph/tree", "/graph/timeline"] {
        let body = html(&s.app, uri).await;
        assert!(body.contains("href=\"/graph/network\""), "{uri}\n{body}");
    }
    let body = network_html(&s.app, "/graph/network").await;
    for want in [
        "href=\"/\"",
        "href=\"/graph/tree\"",
        "href=\"/graph/timeline\"",
    ] {
        assert!(body.contains(want), "{want} が無い\n{body}");
    }
}

#[tokio::test]
async fn 相関図は_castlist_が配る_script_だけを読み_inline_の_script_と_style_が無い() {
    let s = sample().await;
    let body = network_html(&s.app, "/graph/network").await;
    // 凡例（model の色・人の形・線の形）と、model と effort の絞り。
    for want in [
        "m-opus", "m-sonnet", "m-haiku", "m-other", "m-none", "実線", "点線", "人",
    ] {
        assert!(body.contains(want), "{want} が無い\n{body}");
    }
    for want in ["id=\"f-model\"", "id=\"f-effort\""] {
        assert!(body.contains(want), "{want} が無い\n{body}");
    }
}

#[tokio::test]
async fn 相関図は_script_を切った_browser_に系図とタイムラインを勧める() {
    let s = sample().await;
    let body = network_html(&s.app, "/graph/network?label=tpm.key:t-23zs").await;
    let noscript = body
        .split_once("<noscript>")
        .and_then(|(_, rest)| rest.split_once("</noscript>"))
        .map(|(inner, _)| inner)
        .unwrap_or_else(|| panic!("noscript が無い\n{body}"));
    // 同じ絞りのまま、系図とタイムラインへ。
    for want in [
        "href=\"/graph/tree?label=tpm.key:t-23zs\"",
        "href=\"/graph/timeline?label=tpm.key:t-23zs\"",
        "JavaScript",
    ] {
        assert!(noscript.contains(want), "{want} が無い\n{noscript}");
    }
}

#[tokio::test]
async fn 相関図は_絞りを図の_api_へそのまま渡す() {
    let s = sample().await;
    let cases = [
        ("/graph/network".to_owned(), "/v1/graph".to_owned()),
        (
            "/graph/network?label=tpm.key:t-99&from=&to=".to_owned(),
            "/v1/graph?label=tpm.key:t-99".to_owned(),
        ),
        (
            format!("/graph/network?to={}&label=tpm.key:{KEY}", s.se1_entered),
            format!(
                "/v1/graph?to={}&label=tpm.key:{KEY}",
                s.se1_entered.replace('+', "%2B")
            ),
        ),
    ];
    for (uri, want) in cases {
        let body = network_html(&s.app, &uri).await;
        let src = data_src(&body);
        assert_eq!(src, want, "{uri}");
        // 渡す URL は、API が受ける。
        let (status, answer) = s.app.get(&src).await;
        assert_eq!(status, StatusCode::OK, "{src}: {answer}");
    }
    // form には今の値が残る。
    let body = network_html(&s.app, "/graph/network?label=tpm.key:t-99").await;
    assert!(body.contains("value=\"tpm.key:t-99\""), "{body}");
}

#[tokio::test]
async fn 相関図の引数の誤りは_引数の名を書いた_400_の画面() {
    let s = sample().await;
    for (uri, name) in [
        ("/graph/network?colour=red", "colour"),
        ("/graph/network?from=きのう", "from"),
        ("/graph/network?to=きのう", "to"),
        ("/graph/network?label=nocolon", "label"),
        ("/graph/network?label=a:b&label=c:d", "label"),
    ] {
        let (status, headers, body) = page(&s.app, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_html(uri, &headers, &body);
        assert!(body.contains(name), "{uri}: {name} を書かない\n{body}");
    }
}

#[tokio::test]
async fn 相関図も利用者の字を_html_で逃がす() {
    let s = sample().await;
    let uri = "/graph/network?label=tpm.key:%3Cscript%3Ealert(1)%3C/script%3E";
    // 生の字は無く（network_html() が <script を見る）、逃がしを戻すと、form の欄に元の字がある。
    let body = network_html(&s.app, uri).await;
    assert!(
        unescape(&body).contains("<script>alert(1)</script>"),
        "{body}"
    );
    assert_eq!(
        data_src(&body),
        "/v1/graph?label=tpm.key:%3Cscript%3Ealert%281%29%3C%2Fscript%3E"
    );
}

/// script の file を引き、本文と header を返す（`application/javascript` で、長く持たせてよいことを見る）。
async fn script(app: &App, uri: &str) -> Vec<u8> {
    let (status, headers, bytes) = app.raw("GET", uri, None).await;
    assert_eq!(status, StatusCode::OK, "{uri}");
    let get = |name| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    };
    assert!(
        get(header::CONTENT_TYPE).starts_with("application/javascript"),
        "{uri}: {headers:?}"
    );
    assert_eq!(
        get(header::CACHE_CONTROL),
        "public, max-age=31536000, immutable",
        "{uri}"
    );
    assert_eq!(get(header::X_CONTENT_TYPE_OPTIONS), "nosniff", "{uri}");
    assert!(!bytes.is_empty(), "{uri}");
    bytes
}

#[tokio::test]
async fn 相関図の_script_は_castlist_が_application_javascript_で配る() {
    let s = sample().await;
    let body = network_html(&s.app, "/graph/network").await;
    let ours = body
        .split_once("<script src=\"/static/network.js")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(query, _)| format!("/static/network.js{query}"))
        .expect("network.js");
    let js = String::from_utf8(script(&s.app, &ours).await).expect("UTF-8");
    // data は画面の data-src（図の API）から引き、押した点はプロフィールへ。
    for want in ["dataset.src", "/actors/", "cytoscape("] {
        assert!(js.contains(want), "network.js に {want} が無い");
    }
    script(&s.app, CYTOSCAPE).await;
}

#[tokio::test]
async fn 取り込んだ_cytoscape_は_sources_md_に書いた_sha256_と同じ物を配る() {
    use sha2::{Digest, Sha256};
    let s = sample().await;
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/static/vendor/SOURCES.md");
    let sources = tokio::fs::read_to_string(path)
        .await
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    let field = |name: &str| {
        sources
            .lines()
            .find_map(|l| l.strip_prefix(&format!("| {name} | `")))
            .and_then(|rest| rest.strip_suffix("` |"))
            .unwrap_or_else(|| panic!("SOURCES.md に {name} が無い"))
            .to_owned()
    };
    assert_eq!(format!("/{}", field("file")), CYTOSCAPE);
    let bytes = script(&s.app, CYTOSCAPE).await;
    let got: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(got, field("sha256"));
}

#[tokio::test]
async fn どの画面も_鍵と値の組を_ラベルと呼び_札とは書かない() {
    let s = sample().await;
    let id = s.id("se.1");
    let mut pages = Vec::new();
    for path in [
        "/".to_owned(),
        format!("/actors/{id}"),
        "/graph/tree".to_owned(),
        "/graph/timeline".to_owned(),
    ] {
        pages.push((path.clone(), html(&s.app, &path).await));
    }
    // 相関図は castlist が配る script を読むので、それを許す道で読む。
    pages.push((
        "/graph/network".to_owned(),
        network_html(&s.app, "/graph/network").await,
    ));
    for (path, body) in pages {
        assert!(body.contains("ラベル"), "{path} に「ラベル」が無い\n{body}");
        assert!(!body.contains('札'), "{path} に「札」がある\n{body}");
    }
}

// --- 画面の字（Product Manager の答えた字の一覧: 状態は「稼働中・停止」、図は系図・タイムライン・相関図、説明は「です・ます」） ---

/// どの画面にも出る、下の字。
const FOOTER: &str = "時刻は日本時間（UTC+9）で、castlist が受け取った時刻です。時刻にマウスを重ねると ISO 8601 の形で表示します。";

/// 前の字。どの画面にも出さない（出来事の名と、利用者の字は当たらない形にしてある）。
const OLD: &[&str] = &[
    "居るか",
    "居た",
    ">いる<",
    ">いない<",
    "時間の帯",
    "関係の網",
    "時計のずれた行",
    "絞る",
    "絞りを緩める",
    "どれも",
    "当たるアクター",
    "答えた時",
    "受けた時",
    "日本の時刻",
    "起こした",
    "（継がせた者）",
    "（継いだ者）",
    "頼んだ",
    "見た行",
    "名を押す",
    "頁",
    "出し手",
    "今の様子",
    "付随する id",
    "前と後",
    "訳（reason）",
    "元（source）",
    "外す",
    "の窓",
];

fn assert_words(path: &str, body: &str, want: &[&str]) {
    for w in want.iter().chain([&FOOTER]) {
        assert!(body.contains(w), "{path} に「{w}」が無い\n{body}");
    }
    for old in OLD {
        assert!(
            !body.contains(old),
            "{path} に前の字「{old}」がある\n{body}"
        );
    }
}

#[tokio::test]
async fn 一覧は_状態を稼働中と停止で書き_絞り込みの欄と説明を決めた字で書く() {
    let s = sample().await;
    let body = html(&s.app, "/").await;
    assert_words(
        "/",
        &body,
        &[
            "<a href=\"/graph/tree\">系図</a> ・ <a href=\"/graph/timeline\">タイムライン</a> ・ <a href=\"/graph/network\">相関図</a>",
            "<legend>絞り込み（すべての条件に合うアクターを表示します。空の欄は使いません）</legend>",
            "状態 <select name=\"active\">",
            "<option value=\"\">すべて</option>",
            ">稼働中</option>",
            ">停止</option>",
            "稼働した期間（から）",
            "（まで）",
            "時刻ずれあり",
            ">絞り込む</button>",
            ">条件を消す</a>",
            "条件が無いので、この 7 日に稼働したアクター（登場の記録があるアクター）を新しい順に表示しています。登場の記録が無いアクターは、ラベルなどで絞り込むと表示されます。",
            "<th>名前</th><th>種類</th><th>状態</th><th>課題の Key・役割</th><th>model · effort</th><th>最初の登場</th><th>最後の退場</th>",
            ">稼働中</span>",
            ">停止</span>",
            ">時刻ずれ</span>",
            "表示した時刻:",
        ],
    );
    let body = html(&s.app, &format!("/?label=tpm.key:{KEY}&limit=1")).await;
    assert!(body.contains(">次のページ</a>"), "{body}");
    let body = html(&s.app, "/?label=tpm.key:t-none").await;
    assert!(body.contains("条件に合うアクターはいません。"), "{body}");
    for old in OLD {
        assert!(!body.contains(old), "前の字「{old}」がある\n{body}");
    }
}

#[tokio::test]
async fn プロフィールは_欄と節を決めた字で書く() {
    let s = sample().await;
    let path = format!("/actors/{}", s.id("se.1"));
    let body = html(&s.app, &path).await;
    assert_words(
        &path,
        &body,
        &[
            "<dt>発行元（issuer）</dt>",
            "<dt>登録した時刻</dt>",
            "<h2>今の状態</h2>",
            "<dt>状態</dt>",
            ">停止</span>",
            "<dt>最初の登場</dt>",
            "<h2>外部の id</h2>",
            "<th>受け取った時刻</th>",
            "<h2>起動した人</h2>",
            "<h2>起動したアクター</h2>",
            "<h2>引き継ぎ</h2>",
            "<dt>引き継ぎ元</dt>",
            "<dt>引き継ぎ先</dt>",
            "<th>指定・実際</th>",
            ">指定</td>",
            ">実際</td>",
            ">食い違い</span>",
            "食い違い: 実際の値のうち、その前に最後に指定した値と model か effort が違う物です。",
            "<th>理由（reason）</th>",
            "<th>きっかけ（source）</th>",
            "<th>送り手の時刻</th>",
            "起動した",
        ],
    );
    // se.2 は送り手の時計がずれている。印は「時刻ずれ」。
    let path = format!("/actors/{}", s.id("se.2"));
    let body = html(&s.app, &path).await;
    assert_words(
        &path,
        &body,
        &[">時刻ずれ</span>", ">稼働中</span>", "引き継がせた"],
    );
    // 人の根は、起動した人がいない。設定の記録も無い。
    let path = format!("/actors/{}", s.id("pm.1"));
    let body = html(&s.app, &path).await;
    assert_words(&path, &body, &["ありません（起点）。", "ありません。"]);
}

#[tokio::test]
async fn 系図は_見出しと行き来と絞り込みと凡例を決めた字で書く() {
    let s = sample().await;
    let body = html(&s.app, "/graph/tree").await;
    assert_words(
        "/graph/tree",
        &body,
        &[
            "<title>系図 ・ castlist</title>",
            "<a href=\"/\">一覧へ</a> ・ <a href=\"/graph/timeline\">タイムラインへ</a> ・ <a href=\"/graph/network\">相関図へ</a>",
            "<legend>絞り込み（条件に合うアクターと、その 1 本先の線の相手を表示します。空の欄は使いません）</legend>",
            "登録した期間（から）",
            "（まで）",
            ">絞り込む</button>",
            ">条件を消す</a>",
            ">稼働中</tspan>",
            ">停止</tspan>",
            "実線: 起動した（上が起動したアクター）",
            "点線: 引き継いだ（/clear の交代・fork の分身。引き継ぎ元の右に置きます）",
            "名前を押すとプロフィールを表示します。5 のアクター。",
            "表示した時刻:",
        ],
    );
    let body = html(&s.app, &format!("/graph/tree?from={}", s.end)).await;
    assert_words(
        "/graph/tree（空）",
        &body,
        &["条件に合うアクターはいません。条件を減らすか、"],
    );
}

#[tokio::test]
async fn タイムラインは_見出しと行き来と説明と凡例を決めた字で書く() {
    let s = sample().await;
    let body = html(&s.app, "/graph/timeline").await;
    assert_words(
        "/graph/timeline",
        &body,
        &[
            "<title>タイムライン ・ castlist</title>",
            "<h1>タイムライン <span class=\"kind\"><a href=\"/\">一覧へ</a> ・ <a href=\"/graph/tree\">系図へ</a> ・ <a href=\"/graph/network\">相関図へ</a>",
            "<legend>絞り込み（系図と同じ条件です。期間を付けると、横の時刻もその期間にします）</legend>",
            "登録した期間（から）",
            "横は castlist が受け取った時刻（日本時間）です。行は 1 つのアクターで、人を上に、ほかは最初の登場の順に並べます。帯は稼働していた期間で、model と effort が替わった所で区切ります。帯にマウスを重ねると、値と時刻を表示します。名前を押すとプロフィールを表示します。",
            "右端が尖った帯: まだ稼働中（か、表示する期間の外へ続く）",
            "実線: 起動した（起動した時刻から、起動されたアクターの帯の始まりへ）",
            "点線: 引き継いだ（引き継ぎ元の帯の終わりから、引き継ぎ先の帯の始まりへ）",
            "<title>今（表示した時刻）</title>",
            "aria-label=\"タイムライン\"",
            "（指定）",
            "（実際・指定と違う）",
        ],
    );
    let body = html(&s.app, &format!("/graph/timeline?from={}", s.end)).await;
    assert_words(
        "/graph/timeline（空）",
        &body,
        &["条件に合うアクターはいません。条件を減らすか、"],
    );
}

#[tokio::test]
async fn 相関図は_見出しと行き来と説明と凡例と_script_を切った時の字を決めた字で書く() {
    let s = sample().await;
    let body = network_html(&s.app, "/graph/network").await;
    assert_words(
        "/graph/network",
        &body,
        &[
            "<title>相関図 ・ castlist</title>",
            "<h1>相関図 <span class=\"kind\"><a href=\"/\">一覧へ</a> ・ <a href=\"/graph/tree\">系図へ</a> ・ <a href=\"/graph/timeline\">タイムラインへ</a>",
            "<legend>表示する点を選ぶ（このページの中だけで、合わない点とその線を隠します）</legend>",
            "<option value=\"\">すべて</option>",
            "点は 1 つのアクターで、名前を下に書きます。色は最新の model、形は種類です。薄い点は停止したアクターです。点を押すとプロフィールを、名前にマウスを重ねると model · effort と状態を表示します。点はドラッグで動かせます（元の並びに戻すには、ページを読み直します）。",
            "角のある形: 人",
            "実線の矢印: 起動した",
            "点線の矢印: 引き継いだ（/clear の交代・fork の分身）",
            "この画面は JavaScript で描きます。JavaScript を切っている時は、同じ条件の",
            "タイムライン</a>をお使いください（どちらも JavaScript を使いません）。",
            "aria-label=\"相関図\"",
        ],
    );
    let ours = body
        .split_once("<script src=\"/static/network.js")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(query, _)| format!("/static/network.js{query}"))
        .expect("network.js");
    let js = String::from_utf8(script(&s.app, &ours).await).expect("UTF-8");
    for want in ["'稼働中'", "'停止'", "条件に合うアクターはいません"] {
        assert!(js.contains(want), "network.js に {want} が無い");
    }
    for old in [
        "'いる'",
        "'いない'",
        "当たるアクター",
        "関係の網",
        "時間の帯",
    ] {
        assert!(!js.contains(old), "network.js に前の字 {old} がある");
    }
}

// --- 期間の欄（browser の日時の欄と、期間をすぐ選ぶ link） ---

/// 期間の欄を持つ画面と、その欄の名（から・まで）。
const PERIOD_PAGES: [(&str, &str, &str); 4] = [
    ("/", "active_from", "active_to"),
    ("/graph/tree", "from", "to"),
    ("/graph/timeline", "from", "to"),
    ("/graph/network", "from", "to"),
];

/// 期間をすぐ選ぶ link の名（並ぶ順）。
const QUICK: [&str; 4] = ["直近 1 時間", "今日", "昨日", "直近 7 日"];

/// 画面を引く（相関図は script を許す道で）。
async fn period_page(app: &App, uri: &str) -> String {
    if uri.starts_with("/graph/network") {
        network_html(app, uri).await
    } else {
        html(app, uri).await
    }
}

/// `name="…"` の `<input>` の tag。
fn input<'a>(body: &'a str, name: &str) -> &'a str {
    let at = body
        .find(&format!("name=\"{name}\""))
        .unwrap_or_else(|| panic!("{name} の欄が無い\n{body}"));
    let start = body[..at].rfind("<input").expect("<input");
    &body[start..at + body[at..].find('>').expect(">")]
}

/// 名の付いた link（`>名</a>`）の URL。HTML の逃がしを戻す。
fn link_to(body: &str, name: &str) -> String {
    let at = body
        .find(&format!(">{name}</a>"))
        .unwrap_or_else(|| panic!("「{name}」の link が無い\n{body}"));
    let rest = body[..at]
        .rsplit_once("href=\"")
        .unwrap_or_else(|| panic!("「{name}」に href が無い"))
        .1;
    unescape(rest.split_once('"').expect("\"").0)
}

/// URL の引数（名と値。`%XX` を戻す）。
fn params_of(uri: &str) -> Vec<(String, String)> {
    let Some((_, query)) = uri.split_once('?') else {
        return Vec::new();
    };
    let decode = |s: &str| {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).expect("%XX"));
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).expect("UTF-8")
    };
    query
        .split('&')
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn param<'a>(params: &'a [(String, String)], name: &str) -> &'a str {
    params
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("{name} が無い: {params:?}"))
}

fn instant(iso: &str) -> time::OffsetDateTime {
    time::OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|e| panic!("{iso}: {e}"))
}

/// 時刻 `iso` の `secs` 秒あとを、秒まで（下は切る）の日本時間で: 日時の欄の字と、RFC 3339（`+09:00`）の字。
fn jst(iso: &str, secs: i64) -> (String, String) {
    let t = (instant(iso) + time::Duration::seconds(secs))
        .to_offset(time::UtcOffset::from_hms(9, 0, 0).unwrap())
        .replace_nanosecond(0)
        .unwrap();
    let rfc = t
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    assert!(rfc.ends_with("+09:00"), "{rfc}");
    (rfc[..19].to_owned(), rfc)
}

#[tokio::test]
async fn 期間の欄は_browser_の日時の欄で_今の期間を日本時間で出す() {
    let s = sample().await;
    for (path, from, to) in PERIOD_PAGES {
        // 何も付けなければ、空の日時の欄。
        let body = period_page(&s.app, path).await;
        for name in [from, to] {
            let tag = input(&body, name);
            assert!(tag.contains("type=\"datetime-local\""), "{path}: {tag}");
            assert!(tag.contains("value=\"\""), "{path}: {tag}");
            // 秒やミリ秒の付いた値も、送れる（`step` が分の刻みだと、browser が送るのを断る）。
            assert!(tag.contains("step=\"any\""), "{path}: {tag}");
        }
        // RFC 3339 の期間（前の link の形）は、日本時間の欄の字で出す。送り直すと同じ時刻。
        let uri =
            format!("{path}?{from}=2026-11-22T02:00:00Z&{to}=2026-11-22T12:00:07.120%2B09:00");
        let body = period_page(&s.app, &uri).await;
        assert!(
            input(&body, from).contains("value=\"2026-11-22T11:00\""),
            "{uri}: {}",
            input(&body, from)
        );
        assert!(
            input(&body, to).contains("value=\"2026-11-22T12:00:07.120\""),
            "{uri}: {}",
            input(&body, to)
        );
        // 日時の欄が送る字（日本時間）は、そのまま欄に戻る。
        let uri = format!("{path}?{from}=2026-11-22T11:00&{to}=2026-11-22T12:30:05");
        let body = period_page(&s.app, &uri).await;
        assert!(
            input(&body, from).contains("value=\"2026-11-22T11:00\""),
            "{uri}: {}",
            input(&body, from)
        );
        assert!(
            input(&body, to).contains("value=\"2026-11-22T12:30:05\""),
            "{uri}: {}",
            input(&body, to)
        );
    }
}

#[tokio::test]
async fn 日時の欄が送る日本時間の値は_同じ時刻の_rfc_3339_と同じ結果() {
    let s = sample().await;
    // 終わりの印の 1 秒あと（秒まで）: 稼働中の pm.1・tpm.1・se.2 だけが、その後も稼働している。
    let (local, rfc) = jst(&s.end, 1);
    let rfc = rfc.replace('+', "%2B");
    let cases: Vec<(String, String, Vec<&str>)> = vec![
        (
            format!("/?active_from={local}"),
            format!("/?active_from={rfc}"),
            vec!["pm.1", "tpm.1", "se.2"],
        ),
        // 分までの字（秒を送らない browser の形）。
        (
            "/?label=tpm.key:t-23zs&active_to=2000-01-01T09:00".to_owned(),
            "/?label=tpm.key:t-23zs&active_to=2000-01-01T09:00:00%2B09:00".to_owned(),
            vec![],
        ),
        (
            format!("/graph/tree?to={local}"),
            format!("/graph/tree?to={rfc}"),
            vec!["pm.1", "tpm.1", "se.1", "se.2", "other.1"],
        ),
        (
            format!("/graph/timeline?to={local}"),
            format!("/graph/timeline?to={rfc}"),
            vec!["pm.1", "tpm.1", "se.1", "se.2", "other.1"],
        ),
        (
            "/graph/tree?label=tpm.key:t-99&from=2000-01-01T09:00".to_owned(),
            "/graph/tree?label=tpm.key:t-99&from=2000-01-01T09:00:00%2B09:00".to_owned(),
            vec!["other.1"],
        ),
    ];
    for (local_uri, rfc_uri, want) in cases {
        let by_local = sorted(s.listed(&local_uri).await);
        let by_rfc = sorted(s.listed(&rfc_uri).await);
        assert_eq!(by_local, by_rfc, "{local_uri} と {rfc_uri}");
        assert_eq!(by_local, sorted(s.ids_of(&want)), "{local_uri}");
    }
    // 相関図は、RFC 3339 にした期間を図の API に渡す（API は RFC 3339 だけを受ける）。
    let body = network_html(
        &s.app,
        "/graph/network?label=tpm.key:t-99&from=2026-11-22T11:00",
    )
    .await;
    let src = data_src(&body);
    assert_eq!(
        src,
        "/v1/graph?label=tpm.key:t-99&from=2026-11-22T11:00:00%2B09:00"
    );
    let (status, answer) = s.app.get(&src).await;
    assert_eq!(status, StatusCode::OK, "{src}: {answer}");
    // 次のページの link も、RFC 3339 の期間を持つ（証拠に貼れる形）。
    let body = html(
        &s.app,
        &format!("/?label=tpm.key:{KEY}&limit=1&active_from=2000-01-01T09:00"),
    )
    .await;
    let next = next_link(&body).expect("次のページ");
    assert_eq!(
        param(&params_of(&next), "active_from"),
        "2000-01-01T09:00:00+09:00",
        "{next}"
    );
}

#[tokio::test]
async fn 期間をすぐ選ぶ_link_は_今の条件のまま_期間だけを日本時間の_rfc_3339_で替える() {
    let s = sample().await;
    // 一覧は、次のページの URL（after を持つ）から。
    let first = html(&s.app, &format!("/?label=tpm.key:{KEY}&kind=ai&limit=2")).await;
    let list = next_link(&first).expect("次のページ");
    assert!(list.contains("after="), "{list}");
    let uris = [
        format!("{list}&active_to=2000-01-01T09:00"),
        format!("/graph/tree?label=tpm.key:{KEY}&from=2000-01-01T09:00"),
        format!("/graph/timeline?to=2030-01-01T00:00:00Z&label=tpm.key:{KEY}"),
        format!("/graph/network?label=tpm.key:{KEY}&from=2000-01-01T09:00"),
    ];
    for ((path, from, to), uri) in PERIOD_PAGES.into_iter().zip(uris) {
        let before = time::OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .unwrap();
        let body = period_page(&s.app, &uri).await;
        // 直近の期間の終わりは、今の秒を切り上げた時刻（今の秒の中に来た記録も入れる）。
        let after = time::OffsetDateTime::now_utc() + time::Duration::SECOND;
        assert!(
            body.contains("期間をすぐ選ぶ（今の条件のまま、期間だけを替えます）:"),
            "{uri}\n{body}"
        );
        // 並ぶ順。
        let at: Vec<usize> = QUICK
            .iter()
            .map(|q| body.find(&format!(">{q}</a>")).unwrap_or(usize::MAX))
            .collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{uri}: {at:?}");
        let mut ranges = Vec::new();
        for name in QUICK {
            let href = link_to(&body, name);
            assert!(
                href.starts_with(&format!("{path}?")),
                "{uri}: {name}: {href}"
            );
            let params = params_of(&href);
            // ほかの条件は、そのまま残し、次のページの after は落とす。
            let kept: Vec<&(String, String)> = params
                .iter()
                .filter(|(n, _)| n != from && n != to)
                .collect();
            let want: Vec<(String, String)> = params_of(&uri)
                .into_iter()
                .filter(|(n, _)| n != from && n != to && n != "after")
                .collect();
            assert_eq!(
                kept,
                want.iter().collect::<Vec<_>>(),
                "{uri}: {name}: {href}"
            );
            assert!(!href.contains("after="), "{href}");
            let (f, t) = (param(&params, from), param(&params, to));
            for v in [f, t] {
                assert!(v.ends_with("+09:00"), "{name}: {v}");
            }
            assert_eq!(
                params.iter().filter(|(n, _)| n == from || n == to).count(),
                2,
                "{href}"
            );
            ranges.push((instant(f), instant(t), f.to_owned(), t.to_owned()));
        }
        let hour = time::Duration::hours(1);
        let day = time::Duration::days(1);
        let (h, today, yesterday, week) = (&ranges[0], &ranges[1], &ranges[2], &ranges[3]);
        assert!(h.1 >= before && h.1 <= after, "{uri}: {} は今", h.3);
        assert_eq!(h.1 - h.0, hour, "{uri}");
        assert_eq!(week.1 - week.0, time::Duration::days(7), "{uri}");
        assert!(
            week.1 >= before && week.1 <= after,
            "{uri}: {} は今",
            week.3
        );
        assert!(today.2.ends_with("T00:00:00+09:00"), "{}", today.2);
        assert_eq!(today.1 - today.0, day, "{uri}");
        assert!(today.0 <= h.1 && h.1 < today.1, "{uri}: 今日は今を含む");
        assert_eq!(yesterday.1, today.0, "{uri}");
        assert_eq!(yesterday.1 - yesterday.0, day, "{uri}");
    }
    // link を辿ると、その期間で絞った画面（記録は今の物なので、直近 1 時間に入る）。
    let body = html(&s.app, &format!("/?label=tpm.key:{KEY}")).await;
    let href = link_to(&body, "直近 1 時間");
    assert_eq!(
        sorted(s.listed(&href).await),
        sorted(s.ids_of(&["tpm.1", "se.1", "se.2"])),
        "{href}"
    );
    let body = html(&s.app, "/graph/tree?label=tpm.key:t-99").await;
    let href = link_to(&body, "直近 7 日");
    assert_eq!(s.listed(&href).await, s.ids_of(&["other.1"]), "{href}");
}

#[tokio::test]
async fn 日時の欄の読めない値は_引数の名を書いた_400_の画面() {
    let s = sample().await;
    for (uri, name) in [
        ("/?active_from=2026-11-22T25:00", "active_from"),
        ("/?active_to=2026-11-22T11", "active_to"),
        ("/graph/tree?from=2026-13-01T00:00", "from"),
        ("/graph/timeline?to=2026-11-22%2011:00", "to"),
        ("/graph/network?from=2026-11-22T11:00Z", "from"),
    ] {
        let (status, headers, body) = page(&s.app, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_html(uri, &headers, &body);
        assert!(body.contains(name), "{uri}: {name} を書かない\n{body}");
    }
}
