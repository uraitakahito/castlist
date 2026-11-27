//! 画面: アクターの一覧（`/`）とプロフィール（`/actors/{id}`）とタイムライン（`/graph/timeline`）と
//! 相関図（`/graph/network`）と、それが読む CSS（`/static/site.css`）と script（`/static/network.js` と、取り込んだ cytoscape）。
//!
//! - サーバーが HTML を組んで返す（askama。`templates/`）。字はみな HTML として逃がす。
//!   script を使うのは相関図だけで、castlist が配る file だけを読む（inline の script は無い）。
//! - CSP（[`crate::CSP`]）が inline の script と style を断るので、style は castlist が配る CSS の file だけ。
//! - 一覧の絞りは、ListActors（`GET /api/actors`）と同じ条件を、画面の引数（snake_case の `active_from`・`launched_by`・`limit`・`after` など）で
//!   URL に置く（[`params::actor_query`] で読む）。相関図は、点と線を `GET /api/graph` から引く。
//! - DB は新しい DB の所（[`crate::db`]・[`crate::search`]・[`crate::graph`]）で読む。図（タイムライン）は重い読みの接続だけを使う。
//! - 時刻は、castlist が受けた時刻（UTC）を日本の時刻（[`OFFSET`]）で出し、ISO の字を `<time datetime>` と `title` に置く。
//! - 期間の欄は browser の日時の欄（`datetime-local`）。送られた字は日本時間として RFC 3339 にしてから、API と同じ引数で読む
//!   （[`period`]。API の `/api/` は RFC 3339 だけを受けるまま）。欄の横に、期間をすぐ選ぶ link（[`quick_links`]）を置く。

mod params;

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, LazyLock};

use askama::Template;
use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

use crate::db::{self, ActorDetail, Db};
use crate::graph;
use crate::search::{ActorItem, MAX_DEPTH};
use crate::timeline::{self, END_ROOM, MARGIN, Presence, ROW_H, text_width};
use params::{GRAPH_PARAMS, PageError, Pairs, Params, bad_request};

/// 画面に出す時刻のずれ（日本の時刻）。
const OFFSET: UtcOffset = match UtcOffset::from_hms(9, 0, 0) {
    Ok(offset) => offset,
    Err(_) => UtcOffset::UTC,
};

/// 条件が無い時の一覧: この日数の間に居た者（`active_from` を今からこの日数前にする）。
const DEFAULT_DAYS: i64 = 7;

/// 一覧の 1 頁の数（引数の `limit` が無い時）。
const PAGE_SIZE: &str = "100";

/// 出来事の種類と、画面に出す日本語の名（id は英語のまま。名は画面だけ）。
const EVENT_NAMES: [(&str, &str); 10] = [
    ("entered", "登場"),
    ("reentered", "再登場"),
    ("forked", "分身"),
    ("compacted", "要約"),
    ("cleared", "交代"),
    ("exited", "退場"),
    ("closed", "幕引き"),
    ("reported_in", "名乗り"),
    ("model_unreadable", "model 不明"),
    ("retracted", "取り消し"),
];

/// 出来事の種類の、画面に出す日本語の名。知らない種類は id のまま。
fn event_name(kind: &str) -> &str {
    EVENT_NAMES
        .iter()
        .find(|(id, _)| *id == kind)
        .map_or(kind, |(_, name)| name)
}

// --- CSS と script ---

// 配る file は repo の static/ の物を、binary に埋め込む。
const SITE_CSS: &str = include_str!("../../../static/site.css");

/// 相関図（`/graph/network`）が使う cytoscape（npm の物をそのまま取り込んだ。static/vendor/SOURCES.md）。
const CYTOSCAPE_JS: &str = include_str!("../../../static/vendor/cytoscape-3.35.0.min.js");

/// cytoscape の path。版が名に入るので、中身が替われば path も替わる。
const CYTOSCAPE_PATH: &str = "/static/vendor/cytoscape-3.35.0.min.js";

/// 相関図を描く、castlist の script。
const NETWORK_JS: &str = include_str!("../../../static/network.js");

/// 中身の写しの頭を `?v=` に付けた path。中身が替われば browser は読み直す（長く持たせてよい）。
fn versioned(path: &str, body: &str) -> String {
    let digest = Sha256::digest(body.as_bytes());
    let v: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("{path}?v={v}")
}

static CSS_HREF: LazyLock<String> = LazyLock::new(|| versioned("/static/site.css", SITE_CSS));
static NETWORK_JS_HREF: LazyLock<String> =
    LazyLock::new(|| versioned("/static/network.js", NETWORK_JS));

/// binary に埋め込んだ file を、長く持たせてよい答えで返す（path が中身ごとに替わるので）。
fn asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        body,
    )
        .into_response()
}

/// `GET /static/site.css`: 画面の CSS。
async fn site_css() -> Response {
    asset("text/css; charset=utf-8", SITE_CSS)
}

/// `GET /static/vendor/cytoscape-3.35.0.min.js`: 相関図が使う cytoscape。
async fn cytoscape_js() -> Response {
    asset("application/javascript; charset=utf-8", CYTOSCAPE_JS)
}

/// `GET /static/network.js`: 相関図を描く script。
async fn network_js() -> Response {
    asset("application/javascript; charset=utf-8", NETWORK_JS)
}

// --- 画面に出す形 ---

/// 時刻: 画面の字（日本の時刻）と、元の ISO の字。
struct Time {
    iso: String,
    local: String,
}

fn time(iso: &str) -> Time {
    let local = OffsetDateTime::parse(iso, &Rfc3339)
        .map(|t| {
            let t = t.to_offset(OFFSET);
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                t.year(),
                u8::from(t.month()),
                t.day(),
                t.hour(),
                t.minute(),
                t.second()
            )
        })
        .unwrap_or_else(|_| iso.to_owned());
    Time {
        iso: iso.to_owned(),
        local,
    }
}

// --- 期間の欄 ---

/// 一覧の期間の欄（稼働した期間）と、図の期間の欄（登録した期間）の名。
const LIST_PERIOD: [&str; 2] = ["active_from", "active_to"];
const GRAPH_PERIOD: [&str; 2] = ["from", "to"];

/// 期間の欄の引数の名と、画面の見出しの字（読めない値の 400 の画面に書く）。
const PERIOD_LABELS: [(&str, &str); 4] = [
    ("active_from", "稼働した期間（から）"),
    ("active_to", "稼働した期間（まで）"),
    ("from", "登録した期間（から）"),
    ("to", "登録した期間（まで）"),
];

/// 期間の欄の値を RFC 3339 にする。browser の日時の欄（`datetime-local`）が送る
/// `YYYY-MM-DDTHH:MM`（`:SS`・`:SS.sss` も）は日本時間として `+09:00` を付ける。
/// RFC 3339 の値（前の link）は、そのまま。どちらでもない値もそのまま返す（[`period`] が 400 にする）。
fn from_local(value: &str) -> String {
    if OffsetDateTime::parse(value, &Rfc3339).is_ok() {
        return value.to_owned();
    }
    let local = value.len() >= 16
        && value.as_bytes()[10] == b'T'
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || b"-:T.".contains(&b));
    let seconds = if value.len() == 16 { ":00" } else { "" };
    let iso = format!("{value}{seconds}+09:00");
    if local && OffsetDateTime::parse(&iso, &Rfc3339).is_ok() {
        iso
    } else {
        value.to_owned()
    }
}

/// RFC 3339 の値を、日時の欄（`datetime-local`）に入れる日本時間の字にする（form を送り直しても同じ時刻）。
/// 秒が 0 なら分まで、ミリ秒が 0 なら秒まで。読めない値は、そのまま。
fn to_local(value: &str) -> String {
    let Ok(t) = OffsetDateTime::parse(value, &Rfc3339) else {
        return value.to_owned();
    };
    let t = t.to_offset(OFFSET);
    let mut out = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute()
    );
    if t.second() != 0 || t.millisecond() != 0 {
        out.push_str(&format!(":{:02}", t.second()));
    }
    if t.millisecond() != 0 {
        out.push_str(&format!(".{:03}", t.millisecond()));
    }
    out
}

/// 引数のうち、期間の欄（`names`）の値を RFC 3339 にそろえる（[`from_local`]）。読めない値は、欄の見出しと
/// 直し方を書いた 400（API の字「RFC 3339 の時刻」は、欄から打つ人には硬いので、画面では使わない）。
fn period(pairs: &mut Pairs, names: [&str; 2]) -> Result<(), PageError> {
    for (name, value) in pairs.iter_mut() {
        if names.contains(&name.as_str()) {
            *value = from_local(value);
            if OffsetDateTime::parse(value, &Rfc3339).is_err() {
                let label = PERIOD_LABELS
                    .iter()
                    .find(|(n, _)| n == name)
                    .map_or(name.as_str(), |(_, l)| l);
                return Err(bad_request(format!(
                    "「{label}」の日時を読めません。日時の欄で選ぶか、2026-11-22T13:00 の形で入れてください。"
                )));
            }
        }
    }
    Ok(())
}

/// 期間をすぐ選ぶ link の名と、その期間（RFC 3339・`+09:00`。秒まで）。`now` から決める。
/// 直近 1 時間・直近 7 日は今まで（今の秒を切り上げる。今の秒の中に来た記録も入れる）、
/// 今日・昨日は日本時間の 0 時から次の 0 時まで。
fn quick_ranges(now: OffsetDateTime) -> [(&'static str, String, String); 4] {
    let now = now.to_offset(OFFSET);
    let now = if now.nanosecond() == 0 {
        now
    } else {
        now.truncate_to_second() + time::Duration::SECOND
    };
    let today = now.replace_time(time::Time::MIDNIGHT);
    let (hour, day) = (time::Duration::HOUR, time::Duration::DAY);
    let iso = |t: OffsetDateTime| t.format(&Rfc3339).unwrap_or_default();
    [
        ("直近 1 時間", now - hour, now),
        ("今日", today, today + day),
        ("昨日", today - day, today),
        ("直近 7 日", now - day * 7, now),
    ]
    .map(|(name, from, to)| (name, iso(from), iso(to)))
}

/// 期間をすぐ選ぶ link（名と URL）。今の引数のまま、期間（`names`）だけを替え、次のページの `after` は落とす。
fn quick_links(
    path: &str,
    pairs: &Pairs,
    names: [&str; 2],
    now: OffsetDateTime,
) -> Vec<(&'static str, String)> {
    let kept = pairs
        .iter()
        .filter(|(n, _)| n != "after" && !names.contains(&n.as_str()));
    quick_ranges(now)
        .into_iter()
        .map(|(name, from, to)| {
            let mut link: Pairs = kept.clone().cloned().collect();
            link.push((names[0].to_owned(), from));
            link.push((names[1].to_owned(), to));
            (name, format!("{path}?{}", query(&link)))
        })
        .collect()
}

/// 種類の印。人（Product Manager）にだけ「人」を付け、AI には付けない（アクターはほとんど AI なので。Product Manager の頼み）。
fn kind_name(kind: &str) -> &'static str {
    if kind == "human" { "人" } else { "" }
}

/// アクターを指す link（名と種類）。
struct ActorLink {
    href: String,
    name: String,
    kind: &'static str,
}

fn actor_link(id: &str, nodes: &BTreeMap<&str, &ActorItem>) -> ActorLink {
    let node = nodes.get(id);
    ActorLink {
        href: format!("/actors/{id}"),
        name: node.map_or_else(|| id.to_owned(), |n| n.name.clone()),
        kind: node.map_or("", |n| kind_name(&n.kind)),
    }
}

/// 一覧の行と、プロフィールの「今の状態」に出す物。
struct Row {
    href: String,
    name: String,
    kind: &'static str,
    present: bool,
    /// 今の状態を決めた出来事が、推しの幕引き（「停止（推し）」と書く）。
    inferred: bool,
    /// 居る・居ないを替える出来事の最新（日本語の名・id）。
    presence: Option<(String, String)>,
    key: Option<String>,
    role: Option<String>,
    /// ラベルの全部（鍵・値）。
    labels: Vec<(String, String)>,
    /// 最新の model と effort。
    setting: Option<(String, String)>,
    first_entered: Option<Time>,
    last_exited: Option<Time>,
    skewed: bool,
}

fn row(item: &ActorItem) -> Row {
    Row {
        href: format!("/actors/{}", item.id),
        name: item.name.clone(),
        kind: kind_name(&item.kind),
        present: item.present,
        inferred: item.inferred,
        presence: item
            .presence
            .as_deref()
            .map(|p| (event_name(p).to_owned(), p.to_owned())),
        key: item.labels.get("tpm.key").cloned(),
        role: item.labels.get("tpm.role").cloned(),
        labels: item
            .labels
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        setting: item.model.clone().zip(item.effort.clone()),
        first_entered: item.first_entered_at.as_deref().map(time),
        last_exited: item.last_exited_at.as_deref().map(time),
        skewed: item.skewed,
    }
}

// --- 頁 ---

#[derive(Template)]
#[template(path = "list.html")]
struct ListPage {
    css: &'static str,
    rows: Vec<Row>,
    /// 条件が無かったので、既定の窓（この 7 日）で出したか。
    defaulted: bool,
    days: i64,
    form: Form,
    /// 期間をすぐ選ぶ link（名と URL）。
    quick: Vec<(&'static str, String)>,
    next: Option<String>,
    as_of: Time,
}

/// 絞りの form に、今の値を入れて出す。
#[derive(Default)]
struct Form {
    labels: Vec<String>,
    active: String,
    kind: String,
    model: String,
    effort: String,
    active_from: String,
    active_to: String,
    skewed: bool,
    /// form に欄の無い引数（binding・at・launched_by・sort・order・limit）。そのまま送り直す。
    hidden: Vec<(String, String)>,
}

impl Form {
    fn new(pairs: &Pairs) -> Self {
        let mut form = Self::default();
        for (name, value) in pairs {
            let value = value.clone();
            match name.as_str() {
                "label" => form.labels.push(value),
                "active" => form.active = value,
                "kind" => form.kind = value,
                "model" => form.model = value,
                "effort" => form.effort = value,
                "active_from" => form.active_from = to_local(&value),
                "active_to" => form.active_to = to_local(&value),
                "skewed" => form.skewed = value == "true",
                "after" => {}
                _ => form.hidden.push((name.clone(), value)),
            }
        }
        form
    }
}

#[derive(Template)]
#[template(path = "profile.html")]
struct ProfilePage {
    css: &'static str,
    id: String,
    issuer: String,
    registered: Time,
    me: Row,
    current: Option<CurrentSetting>,
    bindings: Vec<BindingRow>,
    /// 起こした人（根の人から、このアクターの手前まで）。線の種類の名も。
    chain: Vec<(ActorLink, &'static str)>,
    children: Vec<ActorLink>,
    before: Vec<ActorLink>,
    after: Vec<ActorLink>,
    settings: Vec<SettingRow>,
    events: Vec<EventRow>,
    as_of: Time,
}

struct CurrentSetting {
    source: &'static str,
    received: Time,
}

struct BindingRow {
    kind: String,
    value: String,
    is_key: bool,
    current: bool,
    received: Time,
}

struct SettingRow {
    received: Time,
    model: String,
    effort: String,
    source: &'static str,
    /// 実際の行（observed）が、その前に最後に指定した物（requested）と違う（画面の「食い違い」）。
    mismatch: bool,
    skewed: bool,
}

struct EventRow {
    received: Time,
    name: String,
    kind: String,
    reason: Option<String>,
    source: Option<String>,
    /// reason・source の外の detail（JSON の字。無ければ空）。
    rest: String,
    sent: Time,
    skewed: bool,
}

fn source_name(source: &str) -> &'static str {
    if source == "requested" {
        "指定"
    } else {
        "実際"
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage {
    css: &'static str,
    status: u16,
    message: String,
}

fn render(status: StatusCode, page: &impl Template) -> Response {
    match page.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(e) => {
            tracing::error!("画面を表示できません: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "画面を表示できません").into_response()
        }
    }
}

/// 誤りの画面（0.4.x の server の API の誤りと同じ状態と訳）。
fn error_page(e: PageError) -> Response {
    let PageError(status, message) = e;
    render(
        status,
        &ErrorPage {
            css: &CSS_HREF,
            status: status.as_u16(),
            message,
        },
    )
}

// --- 道 ---

/// 画面の道（手の道。Smithy の定義の外）。どれも `GET`（`HEAD` も受ける）。
pub fn routes(db: Arc<Db>) -> Router {
    Router::new()
        .route("/", get(list))
        .route("/actors/{id}", get(profile))
        .route("/graph/timeline", get(timeline))
        .route("/graph/network", get(network))
        .route("/static/site.css", get(site_css))
        .route("/static/network.js", get(network_js))
        .route(CYTOSCAPE_PATH, get(cytoscape_js))
        .with_state(db)
}

/// `GET /`: アクターの一覧。引数は ListActors と同じ条件の、画面の名（snake_case）。空の値は付けなかったのと同じ（form は空の欄も送る）。
/// 既定は新しい順（`order=desc`）。絞る条件が無ければ、この 7 日に居た者を出す。
async fn list(State(db): State<Arc<Db>>, q: Result<Query<Pairs>, QueryRejection>) -> Response {
    match list_page(db, q).await {
        Ok(page) => render(StatusCode::OK, &page),
        Err(e) => error_page(e),
    }
}

async fn list_page(
    db: Arc<Db>,
    q: Result<Query<Pairs>, QueryRejection>,
) -> Result<ListPage, PageError> {
    let mut pairs = params::pairs(q)?;
    pairs.retain(|(_, v)| !v.is_empty());
    period(&mut pairs, LIST_PERIOD)?;
    let form = Form::new(&pairs);
    let now = OffsetDateTime::now_utc();
    let quick = quick_links("/", &pairs, LIST_PERIOD, now);
    let mut effective = pairs.clone();
    if !effective.iter().any(|(n, _)| n == "order") {
        effective.push(("order".to_owned(), "desc".to_owned()));
    }
    if !effective.iter().any(|(n, _)| n == "limit") {
        effective.push(("limit".to_owned(), PAGE_SIZE.to_owned()));
    }
    let p = Params::new(effective.clone(), params::ACTOR_PARAMS, &["label"])?;
    let as_of = db::stamp(now);
    let mut query = params::actor_query(&p, &as_of)?;
    let defaulted = !query.narrowed();
    if defaulted {
        let from = db::stamp(now - time::Duration::days(DEFAULT_DAYS));
        query.active_from = Some(from.clone());
        // 次の頁も、同じ窓で続ける。
        effective.push(("active_from".to_owned(), from));
    }
    let page = db.search_actors(&query).await?;
    let next = params::actor_next(&query, &page).map(|after| {
        let mut next: Vec<(String, String)> = effective
            .iter()
            .filter(|(n, _)| n != "after")
            .cloned()
            .collect();
        next.push(("after".to_owned(), after));
        href(&next)
    });
    Ok(ListPage {
        css: &CSS_HREF,
        rows: page.rows.iter().map(row).collect(),
        defaulted,
        days: DEFAULT_DAYS,
        form,
        quick,
        next,
        as_of: time(&as_of),
    })
}

/// `GET /actors/{id}`: 1 つのアクターのプロフィール。無いアクターは 404 の画面。
async fn profile(State(db): State<Arc<Db>>, Path(id): Path<String>) -> Response {
    match profile_page(db, &id).await {
        Ok(Some(page)) => render(StatusCode::OK, &page),
        Ok(None) => error_page(params::not_found()),
        Err(e) => error_page(e),
    }
}

async fn profile_page(db: Arc<Db>, id: &str) -> Result<Option<ProfilePage>, PageError> {
    let as_of = db::now();
    let Some(detail) = db.actor(id).await? else {
        return Ok(None);
    };
    // 上は人の根まで、下は 1 段（起こした者と、後を継いだ者）。
    let Some(lineage) = db.lineage(id, MAX_DEPTH, 1, None).await? else {
        return Ok(None);
    };
    let nodes: BTreeMap<&str, &ActorItem> =
        lineage.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let Some(me) = nodes.get(id) else {
        return Ok(None);
    };
    let chain = chain(id, &lineage.edges, &nodes);
    let ActorDetail {
        actor,
        bindings,
        current_bindings,
        events,
        settings,
        current_settings,
        links_in,
        links_out,
        ..
    } = detail;
    let ends = |links: &[db::Link], kind: &str, outward: bool| -> Vec<ActorLink> {
        links
            .iter()
            .filter(|l| l.kind == kind)
            .map(|l| {
                let end = if outward { &l.to_actor } else { &l.from_actor };
                actor_link(end, &nodes)
            })
            .collect()
    };
    let current: HashSet<(&str, &str, &str)> = current_bindings
        .iter()
        .map(|b| (b.kind.as_str(), b.value.as_str(), b.received_at.as_str()))
        .collect();
    let bindings = bindings
        .iter()
        .map(|b| BindingRow {
            kind: b.kind.clone(),
            value: b.value.clone(),
            is_key: b.is_key,
            current: current.contains(&(b.kind.as_str(), b.value.as_str(), b.received_at.as_str())),
            received: time(&b.received_at),
        })
        .collect();
    // 見た行を、その前の最新の頼んだ行と比べる（監査の検索の mismatch と同じ決め方）。
    let mut requested: Option<(&str, &str)> = None;
    let settings = settings
        .iter()
        .map(|s| {
            let pair = (s.model.as_str(), s.effort.as_str());
            let mismatch = s.source == "observed" && requested.is_some_and(|r| r != pair);
            if s.source == "requested" {
                requested = Some(pair);
            }
            SettingRow {
                received: time(&s.received_at),
                model: s.model.clone(),
                effort: s.effort.clone(),
                source: source_name(&s.source),
                mismatch,
                skewed: s.clock_skew,
            }
        })
        .collect();
    let events = events
        .iter()
        .map(|e| {
            // detail は DB に残した JSON の物の字（鍵を並べた形）。
            let mut detail: Value =
                serde_json::from_str(&e.detail).unwrap_or_else(|_| Value::String(e.detail.clone()));
            let mut take = |key: &str| {
                detail
                    .as_object_mut()
                    .and_then(|o| o.remove(key))
                    .map(|v| match v {
                        Value::String(s) => s,
                        other => other.to_string(),
                    })
            };
            let reason = take("reason");
            let source = take("source");
            let rest = match &detail {
                Value::Object(o) if o.is_empty() => String::new(),
                other => other.to_string(),
            };
            EventRow {
                received: time(&e.received_at),
                name: event_name(&e.kind).to_owned(),
                kind: e.kind.clone(),
                reason,
                source,
                rest,
                sent: time(&e.sent_at),
                skewed: e.clock_skew,
            }
        })
        .collect();
    Ok(Some(ProfilePage {
        css: &CSS_HREF,
        id: actor.id.clone(),
        issuer: actor.issuer.clone(),
        registered: time(&actor.received_at),
        me: row(me),
        current: current_settings.map(|c| CurrentSetting {
            source: source_name(&c.source),
            received: time(&c.received_at),
        }),
        bindings,
        children: ends(&links_out, "launched", true),
        before: ends(&links_in, "continued", false),
        after: ends(&links_out, "continued", true),
        chain,
        settings,
        events,
        as_of: time(&as_of),
    }))
}

/// 図（タイムライン・相関図）の絞りの form（今の値）。
#[derive(Default)]
struct GraphForm {
    label: String,
    from: String,
    to: String,
}

impl GraphForm {
    fn new(pairs: &Pairs) -> Self {
        let mut form = Self::default();
        for (name, value) in pairs {
            match name.as_str() {
                "label" => form.label.clone_from(value),
                "from" => form.from = to_local(value),
                "to" => form.to = to_local(value),
                _ => {}
            }
        }
        form
    }
}

/// 図の線 1 本（SVG の path の class と `d`）。
struct ArrowView {
    class: &'static str,
    d: String,
}

/// 名の字の大きさ（太字）と、種類の字の大きさ（px。site.css の `.timeline` と同じ）。
const NAME_PX: u32 = 13;
const SUB_PX: u32 = 11;

/// model の名を短くする（`claude-opus-5-5` → `opus-5-5`）。
fn short_model(model: &str) -> &str {
    model.strip_prefix("claude-").unwrap_or(model)
}

/// `GET /graph/network?label=…&from=…&to=…`: 相関図。引数はタイムラインと同じ（空の値は付けなかったのと同じ）。
/// script を使う唯一の画面。HTML は枠だけで、点と線は `static/network.js` が `GET /api/graph`（GetGraph）から引いて描く。
/// ここでは引数を確かめ（誤りは 400 の画面）、同じ絞りの API の URL を `data-src` に置く。期間は UTC の字（ミリ秒まで）にして渡す
/// （API の時刻は時差の付いた字を受けないので）。
async fn network(q: Result<Query<Pairs>, QueryRejection>) -> Response {
    match network_page(q) {
        Ok(page) => render(StatusCode::OK, &page),
        Err(e) => error_page(e),
    }
}

fn network_page(q: Result<Query<Pairs>, QueryRejection>) -> Result<NetworkPage, PageError> {
    let mut pairs = params::pairs(q)?;
    pairs.retain(|(_, v)| !v.is_empty());
    period(&mut pairs, GRAPH_PERIOD)?;
    let form = GraphForm::new(&pairs);
    let quick = quick_links(
        "/graph/network",
        &pairs,
        GRAPH_PERIOD,
        OffsetDateTime::now_utc(),
    );
    // 確かめた引数を、そのままの順でタイムラインと API の URL に付ける（API には、期間を UTC の字にして）。
    let with = |pairs: &[(String, String)]| match query(pairs) {
        q if q.is_empty() => q,
        q => format!("?{q}"),
    };
    let api: Pairs = pairs
        .iter()
        .map(|(name, value)| {
            let value = if GRAPH_PERIOD.contains(&name.as_str()) {
                OffsetDateTime::parse(value, &Rfc3339).map_or_else(|_| value.clone(), db::stamp)
            } else {
                value.clone()
            };
            (name.clone(), value)
        })
        .collect();
    let (timeline, data_src) = (with(&pairs), with(&api));
    params::graph_query(&Params::new(pairs, GRAPH_PARAMS, &[])?)?;
    Ok(NetworkPage {
        css: &CSS_HREF,
        cytoscape: CYTOSCAPE_PATH,
        script: &NETWORK_JS_HREF,
        data_src: format!("/api/graph{data_src}"),
        timeline: format!("/graph/timeline{timeline}"),
        form,
        quick,
    })
}

#[derive(Template)]
#[template(path = "network.html")]
struct NetworkPage {
    css: &'static str,
    cytoscape: &'static str,
    script: &'static str,
    form: GraphForm,
    quick: Vec<(&'static str, String)>,
    /// network.js が点と線を引く URL（同じ絞りの `GET /api/graph`）。
    data_src: String,
    /// script を切った browser に勧める、同じ絞りのタイムライン。
    timeline: String,
}

/// `GET /graph/timeline?label=…&from=…&to=…`: タイムライン。引数は GetGraph（`GET /api/graph`）と同じ（空の値は付けなかったのと同じ）。
/// `from`・`to` を付ければ、横の時刻もその窓にする（無ければ記録の初めから終わりまで）。
/// 並べる計算と HTML を組むのは、spawn_blocking の糸で行う。
async fn timeline(State(db): State<Arc<Db>>, q: Result<Query<Pairs>, QueryRejection>) -> Response {
    match timeline_page(db, q).await {
        Ok(response) => response,
        Err(e) => error_page(e),
    }
}

async fn timeline_page(
    db: Arc<Db>,
    q: Result<Query<Pairs>, QueryRejection>,
) -> Result<Response, PageError> {
    let mut pairs = params::pairs(q)?;
    pairs.retain(|(_, v)| !v.is_empty());
    period(&mut pairs, GRAPH_PERIOD)?;
    let form = GraphForm::new(&pairs);
    let now = OffsetDateTime::now_utc();
    let quick = quick_links("/graph/timeline", &pairs, GRAPH_PERIOD, now);
    let p = Params::new(pairs, GRAPH_PARAMS, &[])?;
    let query = params::graph_query(&p)?;
    let (from, to) = (p.time("from")?, p.time("to")?);
    let as_of = db::stamp(now);
    let data = db.timeline(&query).await?;
    tokio::task::spawn_blocking(move || {
        let window = timeline::Window {
            from: from.as_deref().and_then(millis),
            to: to.as_deref().and_then(millis),
            now: millis(&as_of).unwrap_or_default(),
            offset_s: i64::from(OFFSET.whole_seconds()),
        };
        let page = TimelinePage {
            css: &CSS_HREF,
            count: data.graph.nodes.len(),
            legend: legend(&data.settings),
            svg: timeline_svg(&data, window),
            form,
            quick,
            as_of: time(&as_of),
        };
        render(StatusCode::OK, &page)
    })
    .await
    .map_err(|e| {
        tracing::error!("タイムラインを表示できません: {e}");
        PageError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "タイムラインを表示できません".to_owned(),
        )
    })
}

#[derive(Template)]
#[template(path = "timeline.html")]
struct TimelinePage {
    css: &'static str,
    form: GraphForm,
    quick: Vec<(&'static str, String)>,
    count: usize,
    /// 凡例の model（短い名と色の class）。
    legend: Vec<(String, &'static str)>,
    /// 点が無ければ無い。
    svg: Option<TimelineSvg>,
    as_of: Time,
}

struct TimelineSvg {
    width: u32,
    height: u32,
    /// 目盛りの字の下の線と、目盛りの縦の線の上と下。
    tick_y: u32,
    grid_y0: u32,
    grid_y1: u32,
    row_h: u32,
    ticks: Vec<timeline::Tick>,
    /// 今の時刻の縦の線（窓の中なら）。
    now: Option<u32>,
    /// 上からの順。
    lanes: Vec<LaneView>,
    arrows: Vec<ArrowView>,
}

/// タイムラインの行 1 つ。
struct LaneView {
    href: String,
    name: String,
    kind: &'static str,
    human: bool,
    /// 行の上と、名の字の下の線と、種類の字の左。
    y: u32,
    text_y: u32,
    kind_x: u32,
    segs: Vec<SegView>,
    marks: Vec<MarkView>,
}

struct SegView {
    class: String,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    title: String,
    /// 帯に書く字と、その置き場（収まらなければ無い）。
    label: Option<(u32, u32, String)>,
    /// 開いた端の印の path（居るまま・窓の外へ続く）。
    open_end: Option<String>,
    open_start: Option<String>,
}

struct MarkView {
    kind: String,
    x: u32,
    y0: u32,
    y1: u32,
    title: String,
}

/// 推しの幕引きの訳（`detail.reason`）の字。知らない訳と、訳の無い物は、推した者だけを書く。
fn lost_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("workspace_gone") => "居場所が消えた",
        Some("superseded") => "居場所に別の session が入った",
        _ => "roster が落ちたと推した",
    }
}

/// 印を付ける出来事。
const MARK_EVENTS: [&str; 3] = ["compacted", "reported_in", "model_unreadable"];

/// model の名から、帯の色の class（同じ名はいつも同じ class）。知らない種類はみな `m-other`。
fn tone(model: &str) -> &'static str {
    ["opus", "sonnet", "haiku"]
        .into_iter()
        .zip(["m-opus", "m-sonnet", "m-haiku"])
        .find(|(family, _)| model.contains(family))
        .map_or("m-other", |(_, class)| class)
}

/// 凡例の model: 出てくる model の短い名（名の順）と色の class。
fn legend(settings: &[db::Setting]) -> Vec<(String, &'static str)> {
    let names: std::collections::BTreeSet<&str> =
        settings.iter().map(|s| short_model(&s.model)).collect();
    names.into_iter().map(|m| (m.to_owned(), tone(m))).collect()
}

/// RFC 3339 の時刻を、UNIX の時からのミリ秒に。
fn millis(iso: &str) -> Option<i64> {
    let t = OffsetDateTime::parse(iso, &Rfc3339).ok()?;
    i64::try_from(t.unix_timestamp_nanos() / 1_000_000).ok()
}

/// タイムラインの SVG に置く物。点が無ければ None。
fn timeline_svg(data: &graph::Timeline, window: timeline::Window) -> Option<TimelineSvg> {
    let nodes = &data.graph.nodes;
    if nodes.is_empty() {
        return None;
    }
    let index: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.actor.id.as_str(), i))
        .collect();
    let mut lanes: Vec<timeline::Lane> = nodes
        .iter()
        .map(|n| {
            let kind = kind_name(&n.actor.kind);
            let order = n
                .state
                .entered_at
                .as_deref()
                .unwrap_or(&n.actor.received_at);
            timeline::Lane {
                human: n.actor.kind == "human",
                order: millis(order).unwrap_or(i64::MAX),
                label_width: name_width(&n.actor.name)
                    + if kind.is_empty() {
                        0
                    } else {
                        6 + text_width(kind, SUB_PX)
                    },
                ..timeline::Lane::default()
            }
        })
        .collect();
    // 出来事の印（行ごとに、印の順の種類と時刻）と、model と effort の替わり目（行ごとに、元の行）。
    let mut marks: Vec<Vec<(&str, &str)>> = vec![Vec::new(); nodes.len()];
    // 推しの幕引き（行ごとに、受けた時刻と訳）。推しの切れの説明に使う。
    let mut lost: Vec<Vec<(i64, Option<&str>)>> = vec![Vec::new(); nodes.len()];
    for e in &data.events {
        let (Some(&i), Some(t)) = (index.get(e.actor_id.as_str()), millis(&e.received_at)) else {
            continue;
        };
        match e.kind.as_str() {
            "entered" | "reentered" | "forked" => lanes[i].presence.push((t, Presence::In)),
            "closed" if e.inferred => {
                lanes[i].presence.push((t, Presence::Lost));
                lost[i].push((t, e.reason.as_deref()));
            }
            "cleared" | "exited" | "closed" => lanes[i].presence.push((t, Presence::Out)),
            kind if MARK_EVENTS.contains(&kind) => {
                lanes[i].marks.push(t);
                marks[i].push((kind, &e.received_at));
            }
            _ => {}
        }
    }
    let mut changes: Vec<Vec<(&db::Setting, bool)>> = vec![Vec::new(); nodes.len()];
    let mut requested: BTreeMap<usize, (&str, &str)> = BTreeMap::new();
    for s in &data.settings {
        let (Some(&i), Some(t)) = (index.get(s.actor_id.as_str()), millis(&s.received_at)) else {
            continue;
        };
        // 見た行が、その前の最新の頼んだ行と違うか（プロフィールの「食い違い」と同じ決め方）。
        let pair = (s.model.as_str(), s.effort.as_str());
        let mismatch = s.source == "observed" && requested.get(&i).is_some_and(|r| *r != pair);
        if s.source == "requested" {
            requested.insert(i, pair);
        }
        lanes[i].changes.push(timeline::Change {
            at: t,
            label: format!("{} · {}", short_model(&s.model), s.effort),
        });
        changes[i].push((s, mismatch));
    }
    let links: Vec<timeline::Link> = data
        .graph
        .edges
        .iter()
        .filter_map(|e| {
            let kind = match e.kind.as_str() {
                "launched" => timeline::Kind::Launched,
                "continued" => timeline::Kind::Continued,
                _ => return None,
            };
            Some(timeline::Link {
                from: *index.get(e.from_actor.as_str())?,
                to: *index.get(e.to_actor.as_str())?,
                kind,
                at: millis(&e.received_at)?,
            })
        })
        .collect();
    let layout = timeline::layout(&lanes, &links, window);
    let clock = |t: i64| {
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(t) * 1_000_000)
            .ok()
            .and_then(|t| t.format(&Rfc3339).ok())
            .map(|iso| time(&iso).local)
            .unwrap_or_default()
    };
    let mut views: Vec<LaneView> = layout
        .order
        .iter()
        .map(|&i| {
            let n = &nodes[i];
            let y = layout.rows[i];
            LaneView {
                href: format!("/actors/{}", n.actor.id),
                name: n.actor.name.clone(),
                kind: kind_name(&n.actor.kind),
                human: lanes[i].human,
                y,
                text_y: y + ROW_H / 2 + 4,
                kind_x: MARGIN + name_width(&n.actor.name) + 6,
                segs: Vec::new(),
                marks: Vec::new(),
            }
        })
        .collect();
    let rank: BTreeMap<usize, usize> = layout
        .order
        .iter()
        .enumerate()
        .map(|(r, &i)| (i, r))
        .collect();
    for s in &layout.segments {
        let name = &nodes[s.lane].actor.name;
        let row = s
            .change
            .map(|c| (&lanes[s.lane].changes[c], changes[s.lane][c]));
        let (class, what) = match row {
            Some((change, (setting, mismatch))) => (
                format!("seg {}", tone(&setting.model)),
                format!(
                    "{}（{}{}）",
                    change.label,
                    source_name(&setting.source),
                    if mismatch { "・指定と違う" } else { "" }
                ),
            ),
            None => (
                "seg m-none".to_owned(),
                "model と effort の記録なし".to_owned(),
            ),
        };
        // 推しの切れの訳: その切れの終わりか後の、初めの推しの幕引き。
        let why = || {
            let reason = lost[s.lane]
                .iter()
                .find(|(t, _)| *t >= s.end)
                .and_then(|(_, r)| *r);
            lost_reason(reason)
        };
        let until = if s.inferred && s.open_end {
            format!("続く（表示する期間の後に推しの幕引き: {}）", why())
        } else if s.open_end {
            "続く".to_owned()
        } else if s.inferred {
            format!("{}（推し: {}）", clock(s.end), why())
        } else {
            clock(s.end)
        };
        let class = if s.inferred {
            format!("{class} inferred")
        } else {
            class
        };
        let mid = s.y + s.h / 2;
        views[rank[&s.lane]].segs.push(SegView {
            class,
            x: s.x,
            y: s.y,
            w: s.w,
            h: s.h,
            title: format!("{name} · {what} · {} から {until}", clock(s.start)),
            label: s
                .label_x
                .and_then(|x| row.map(|(c, _)| (x, mid + 4, c.label.clone()))),
            open_end: s.open_end.then(|| {
                let x = s.x + s.w;
                format!("M{x},{} L{},{mid} L{x},{}z", s.y, x + END_ROOM, s.y + s.h)
            }),
            open_start: s.open_start.then(|| {
                format!(
                    "M{},{} L{},{mid} L{},{}z",
                    s.x,
                    s.y,
                    s.x.saturating_sub(6),
                    s.x,
                    s.y + s.h
                )
            }),
        });
    }
    for m in &layout.marks {
        let (kind, at) = marks[m.lane][m.mark];
        views[rank[&m.lane]].marks.push(MarkView {
            kind: kind.to_owned(),
            x: m.x,
            y0: m.y0,
            y1: m.y1,
            title: format!("{} · {}", event_name(kind), time(at).local),
        });
    }
    let now = (layout.start..=layout.end)
        .contains(&window.now)
        .then(|| layout.x(window.now));
    let arrows = layout
        .arrows
        .into_iter()
        .map(|a| ArrowView {
            class: match a.kind {
                timeline::Kind::Launched => "link launched",
                timeline::Kind::Continued => "link continued",
            },
            d: a.d,
        })
        .collect();
    let grid_y0 = timeline::AXIS_H - 6;
    Some(TimelineSvg {
        width: layout.width,
        height: layout.height,
        tick_y: timeline::AXIS_H - 10,
        grid_y0,
        grid_y1: layout.height,
        row_h: ROW_H,
        now,
        ticks: layout.ticks,
        lanes: views,
        arrows,
    })
}

/// 行の名の字の幅（太字は 1 px 大きく見積もる）。
fn name_width(name: &str) -> u32 {
    text_width(name, NAME_PX + 1)
}

/// 起こした人の鎖: このアクターから、起こした・継がせた線を上り、人で止める。根から順に返す。
/// 両方の線がある時は、起こした（launched）方を辿る。
fn chain(
    id: &str,
    edges: &[db::Link],
    nodes: &BTreeMap<&str, &ActorItem>,
) -> Vec<(ActorLink, &'static str)> {
    let mut out = Vec::new();
    let mut seen: HashSet<&str> = HashSet::from([id]);
    let mut current = id;
    for _ in 0..MAX_DEPTH {
        let up = |kind: &str| {
            edges
                .iter()
                .find(|l| l.to_actor == current && l.kind == kind)
        };
        let Some(edge) = up("launched").or_else(|| up("continued")) else {
            break;
        };
        let from = edge.from_actor.as_str();
        if !seen.insert(from) {
            break;
        }
        let verb = if edge.kind == "launched" {
            "起動した"
        } else {
            "引き継がせた"
        };
        out.push((actor_link(from, nodes), verb));
        if nodes.get(from).is_some_and(|n| n.kind == "human") {
            break;
        }
        current = from;
    }
    out.reverse();
    out
}

/// 一覧の URL（`/?名=値&…`）。
fn href(pairs: &[(String, String)]) -> String {
    format!("/?{}", query(pairs))
}

/// URL の引数の字（`名=値&…`。`?` は付けない）。
fn query(pairs: &[(String, String)]) -> String {
    let query: Vec<String> = pairs
        .iter()
        .map(|(n, v)| format!("{}={}", encode(n), encode(v)))
        .collect();
    query.join("&")
}

/// URL の引数の値を % で包む（英数字と `-._~:` の外）。
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~:".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        GRAPH_PERIOD, LIST_PERIOD, encode, event_name, from_local, lost_reason, period,
        quick_links, quick_ranges, time, to_local,
    };
    /// 出来事の種類（定義の EventType の 10 種。v0.4.0 の tag の src/api.rs の EVENT_TYPES と同じ順）。
    const EVENT_TYPES: [&str; 10] = [
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
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    fn at(iso: &str) -> OffsetDateTime {
        OffsetDateTime::parse(iso, &Rfc3339).unwrap()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn 日時の欄の値は日本時間として_rfc_3339_にする() {
        assert_eq!(from_local("2026-11-22T11:00"), "2026-11-22T11:00:00+09:00");
        assert_eq!(
            from_local("2026-11-22T11:00:05"),
            "2026-11-22T11:00:05+09:00"
        );
        assert_eq!(
            from_local("2026-11-22T11:00:05.250"),
            "2026-11-22T11:00:05.250+09:00"
        );
    }

    #[test]
    fn rfc_3339_の値はそのまま() {
        for iso in [
            "2026-11-22T11:00:00+09:00",
            "2026-11-22T02:00:00Z",
            "2026-11-22T02:00:00.123Z",
            "2026-11-21T21:00:00-05:00",
        ] {
            assert_eq!(from_local(iso), iso);
        }
    }

    #[test]
    fn 読めない値はそのまま返し_引数を読む所が断る() {
        for bad in [
            "きのう",
            "",
            "2026-11-22",
            "2026-11-22T11",
            "2026-13-01T00:00",
            "2026-11-22T25:00",
            "2026-11-22 11:00",
            "2026-11-22T11:00Z",
            "2026-11-22T11:00+09:00",
        ] {
            assert_eq!(from_local(bad), bad, "{bad}");
        }
    }

    #[test]
    fn 期間の欄だけを_rfc_3339_にそろえる() {
        let mut list = pairs(&[
            ("label", "tpm.key:t-23zs"),
            ("active_from", "2026-11-22T11:00"),
            ("active_to", "2026-11-22T12:30:00+09:00"),
            ("at", "2026-11-22T11:00"),
        ]);
        assert!(period(&mut list, LIST_PERIOD).is_ok());
        assert_eq!(
            list,
            pairs(&[
                ("label", "tpm.key:t-23zs"),
                ("active_from", "2026-11-22T11:00:00+09:00"),
                ("active_to", "2026-11-22T12:30:00+09:00"),
                // 期間の欄でない時刻は、API と同じ（替えない）。
                ("at", "2026-11-22T11:00"),
            ])
        );
        let mut graph = pairs(&[("from", "2026-11-22T00:00"), ("to", "2026-11-23T00:00")]);
        assert!(period(&mut graph, GRAPH_PERIOD).is_ok());
        assert_eq!(
            graph,
            pairs(&[
                ("from", "2026-11-22T00:00:00+09:00"),
                ("to", "2026-11-23T00:00:00+09:00"),
            ])
        );
    }

    #[test]
    fn rfc_3339_の値を日時の欄の日本時間の字にする() {
        assert_eq!(to_local("2026-11-22T11:00:00+09:00"), "2026-11-22T11:00");
        assert_eq!(to_local("2026-11-22T02:00:00Z"), "2026-11-22T11:00");
        assert_eq!(to_local("2026-11-22T20:30:00Z"), "2026-11-23T05:30");
        assert_eq!(to_local("2026-11-22T02:00:07Z"), "2026-11-22T11:00:07");
        assert_eq!(
            to_local("2026-11-22T02:00:07.120Z"),
            "2026-11-22T11:00:07.120"
        );
        assert_eq!(to_local("きのう"), "きのう");
        // 欄の字を送り直すと、同じ時刻。
        for iso in ["2026-11-22T02:00:00Z", "2026-11-22T02:00:07.120Z"] {
            assert_eq!(at(&from_local(&to_local(iso))), at(iso), "{iso}");
        }
    }

    #[test]
    fn すぐ選ぶ期間は今から決め_今日と昨日は日本時間の_0_時から() {
        // 直近の期間の終わりは、今の秒を切り上げた時刻（今の秒の中に来た記録も入れる）。
        let ranges = quick_ranges(at("2026-11-23T01:30:15.250Z"));
        let want = [
            (
                "直近 1 時間",
                "2026-11-23T09:30:16+09:00",
                "2026-11-23T10:30:16+09:00",
            ),
            (
                "今日",
                "2026-11-23T00:00:00+09:00",
                "2026-11-24T00:00:00+09:00",
            ),
            (
                "昨日",
                "2026-11-22T00:00:00+09:00",
                "2026-11-23T00:00:00+09:00",
            ),
            (
                "直近 7 日",
                "2026-11-16T10:30:16+09:00",
                "2026-11-23T10:30:16+09:00",
            ),
        ];
        let got: Vec<(&str, &str, &str)> = ranges
            .iter()
            .map(|(n, f, t)| (*n, f.as_str(), t.as_str()))
            .collect();
        assert_eq!(got, want);
        // ちょうどの秒なら、切り上げない。
        let ranges = quick_ranges(at("2026-11-23T01:30:15Z"));
        assert_eq!(ranges[0].2, "2026-11-23T10:30:15+09:00");
        // UTC ではまだ前の日でも、日本時間の日で決める。
        let ranges = quick_ranges(at("2026-11-22T20:00:00Z"));
        assert_eq!(ranges[1].1, "2026-11-23T00:00:00+09:00");
        assert_eq!(ranges[2].1, "2026-11-22T00:00:00+09:00");
    }

    #[test]
    fn すぐ選ぶ_link_はほかの条件を残し_期間を替え_after_を落とす() {
        let now = at("2026-11-23T01:30:15Z");
        let list = pairs(&[
            ("label", "tpm.key:t-23zs"),
            ("active_from", "2026-11-01T00:00:00+09:00"),
            ("kind", "ai"),
            ("after", "abc"),
            ("label", "tpm.role:se"),
            ("limit", "10"),
        ]);
        let links = quick_links("/", &list, LIST_PERIOD, now);
        let names: Vec<&str> = links.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["直近 1 時間", "今日", "昨日", "直近 7 日"]);
        assert_eq!(
            links[1].1,
            "/?label=tpm.key:t-23zs&kind=ai&label=tpm.role:se&limit=10\
             &active_from=2026-11-23T00:00:00%2B09:00&active_to=2026-11-24T00:00:00%2B09:00"
        );
        let graph = pairs(&[("to", "2026-11-01T00:00:00+09:00"), ("label", "a:b")]);
        let links = quick_links("/graph/timeline", &graph, GRAPH_PERIOD, now);
        assert_eq!(
            links[0].1,
            "/graph/timeline?label=a:b&from=2026-11-23T09:30:15%2B09:00&to=2026-11-23T10:30:15%2B09:00"
        );
        // 条件が無くても、期間だけを付ける。
        let links = quick_links("/graph/network", &Vec::new(), GRAPH_PERIOD, now);
        assert_eq!(
            links[2].1,
            "/graph/network?from=2026-11-22T00:00:00%2B09:00&to=2026-11-23T00:00:00%2B09:00"
        );
    }

    #[test]
    fn 出来事の種類はみな日本語の名を持つ() {
        let names: Vec<&str> = EVENT_TYPES.iter().map(|t| event_name(t)).collect();
        assert_eq!(
            names,
            [
                "登場",
                "再登場",
                "分身",
                "要約",
                "交代",
                "退場",
                "幕引き",
                "名乗り",
                "model 不明",
                "取り消し"
            ]
        );
        // 知らない種類は、id のまま出す。
        assert_eq!(event_name("unknown"), "unknown");
    }

    #[test]
    fn 時刻は日本の時刻で出し_読めない字はそのまま() {
        let t = time("2026-11-22T04:00:01.123Z");
        assert_eq!(t.local, "2026-11-22 13:00:01");
        assert_eq!(t.iso, "2026-11-22T04:00:01.123Z");
        assert_eq!(time("きのう").local, "きのう");
    }

    #[test]
    fn url_の値を包む() {
        assert_eq!(encode("tpm.key:t-23zs"), "tpm.key:t-23zs");
        assert_eq!(encode("a b&c=d+"), "a%20b%26c%3Dd%2B");
        assert_eq!(encode("<"), "%3C");
    }

    #[test]
    fn 推しの訳が無い物と知らない訳は_roster_が推したと書く() {
        assert_eq!(lost_reason(Some("workspace_gone")), "居場所が消えた");
        assert_eq!(
            lost_reason(Some("superseded")),
            "居場所に別の session が入った"
        );
        assert_eq!(lost_reason(None), "roster が落ちたと推した");
        assert_eq!(lost_reason(Some("other")), "roster が落ちたと推した");
    }
}
