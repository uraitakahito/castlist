//! 画面: アクターの一覧（`/`）とプロフィール（`/actors/{id}`）と系図（`/graph/tree`）と、それが読む CSS（`/static/site.css`）。
//!
//! - サーバーが HTML を組んで返す（askama。`templates/`）。字はみな HTML として逃がす。script は使わない。
//! - CSP（[`crate::CSP`]）が inline の script と style を断るので、style は castlist が配る CSS の file だけ。
//! - 一覧の絞りは、監査の検索の API（`GET /v1/actors`）と同じ引数を URL に置く（[`search::actor_query`] で読む）。
//! - 時刻は、castlist が受けた時刻（UTC）を日本の時刻（[`OFFSET`]）で出し、ISO の字を `<time datetime>` と `title` に置く。

use crate::api::{ApiError, GRAPH_PARAMS, GraphQuery};
use crate::db::{self, ActorDetail, Db};
use crate::search::{self, ActorItem, ActorQuery, MAX_DEPTH, Pairs, Params};
use crate::tree::{self, text_width};
use askama::Template;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

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
pub fn event_name(kind: &str) -> &str {
    EVENT_NAMES
        .iter()
        .find(|(id, _)| *id == kind)
        .map_or(kind, |(_, name)| name)
}

// --- CSS ---

const SITE_CSS: &str = include_str!("../static/site.css");

/// CSS の path。中身の写しを `?v=` に付けるので、中身が替われば browser は読み直す（長く持たせてよい）。
static CSS_HREF: LazyLock<String> = LazyLock::new(|| {
    let digest = Sha256::digest(SITE_CSS.as_bytes());
    let v: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("/static/site.css?v={v}")
});

/// `GET /static/site.css`: 画面の CSS（binary に埋め込んだ物）。
pub async fn site_css() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        SITE_CSS,
    )
        .into_response()
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

fn kind_name(kind: &str) -> &'static str {
    if kind == "human" { "人" } else { "AI" }
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

/// 一覧の行と、プロフィールの「今の様子」に出す物。
struct Row {
    href: String,
    name: String,
    kind: &'static str,
    present: bool,
    /// 居る・居ないを替える出来事の最新（日本語の名・id）。
    presence: Option<(String, String)>,
    key: Option<String>,
    role: Option<String>,
    /// 札の全部（鍵・値）。
    labels: Vec<(String, String)>,
    /// 最新の model と effort。
    setting: Option<(String, String)>,
    first_entered: Option<Time>,
    last_exited: Option<Time>,
    skewed: bool,
}

fn row(item: &ActorItem) -> Row {
    Row {
        href: item.url.clone(),
        name: item.name.clone(),
        kind: kind_name(&item.kind),
        present: item.present,
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
                "active_from" => form.active_from = value,
                "active_to" => form.active_to = value,
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
    /// 見た行が、その前の最新の頼んだ物と違う。
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
        "頼んだ"
    } else {
        "見た"
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
            eprintln!("画面を組めない: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "画面を組めない").into_response()
        }
    }
}

/// 誤りの画面（API の誤りと同じ状態と訳）。
fn error_page(e: ApiError) -> Response {
    let ApiError(status, message) = e;
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

/// `GET /`: アクターの一覧。引数は `GET /v1/actors` と同じ。空の値は付けなかったのと同じ（form は空の欄も送る）。
/// 既定は新しい順（`order=desc`）。絞る条件が無ければ、この 7 日に居た者を出す。
pub async fn list(State(db): State<Db>, q: Result<Query<Pairs>, QueryRejection>) -> Response {
    match list_page(db, q).await {
        Ok(page) => render(StatusCode::OK, &page),
        Err(e) => error_page(e),
    }
}

async fn list_page(db: Db, q: Result<Query<Pairs>, QueryRejection>) -> Result<ListPage, ApiError> {
    let mut pairs = search::pairs(q)?;
    pairs.retain(|(_, v)| !v.is_empty());
    let form = Form::new(&pairs);
    let mut effective = pairs.clone();
    if !effective.iter().any(|(n, _)| n == "order") {
        effective.push(("order".to_owned(), "desc".to_owned()));
    }
    if !effective.iter().any(|(n, _)| n == "limit") {
        effective.push(("limit".to_owned(), PAGE_SIZE.to_owned()));
    }
    let p = Params::new(effective.clone(), search::ACTOR_PARAMS, &["label"])?;
    let as_of = db::now();
    let mut query: ActorQuery = search::actor_query(&p, &as_of)?;
    let defaulted = !query.narrowed();
    if defaulted {
        let from = db::stamp(OffsetDateTime::now_utc() - time::Duration::days(DEFAULT_DAYS));
        query.active_from = Some(from.clone());
        // 次の頁も、同じ窓で続ける。
        effective.push(("active_from".to_owned(), from));
    }
    let page = db.search_actors(&query).await?;
    let next = search::actor_next(&query, &page).map(|after| {
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
        next,
        as_of: time(&as_of),
    })
}

/// `GET /actors/{id}`: 1 つのアクターのプロフィール。無いアクターは 404 の画面。
pub async fn profile(State(db): State<Db>, Path(id): Path<String>) -> Response {
    match profile_page(db, &id).await {
        Ok(Some(page)) => render(StatusCode::OK, &page),
        Ok(None) => error_page(crate::api::not_found()),
        Err(e) => error_page(e),
    }
}

async fn profile_page(db: Db, id: &str) -> Result<Option<ProfilePage>, ApiError> {
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
            let mut detail = e.detail.clone();
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
                sent: time(&e.at),
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

/// `GET /graph/tree?label=…&from=…&to=…`: 系図（相関図 A）。引数は `GET /v1/graph` と同じ（空の値は付けなかったのと同じ）。
/// 並べる計算と HTML を組むのは、tokio の糸を塞がないよう spawn_blocking の糸で行う。
pub async fn tree(State(db): State<Db>, q: Result<Query<Pairs>, QueryRejection>) -> Response {
    match tree_page(db, q).await {
        Ok(response) => response,
        Err(e) => error_page(e),
    }
}

async fn tree_page(db: Db, q: Result<Query<Pairs>, QueryRejection>) -> Result<Response, ApiError> {
    let mut pairs = search::pairs(q)?;
    pairs.retain(|(_, v)| !v.is_empty());
    let form = TreeForm::new(&pairs);
    let p = Params::new(pairs, GRAPH_PARAMS, &[])?;
    let query = GraphQuery::new(&p)?;
    let as_of = db::now();
    let graph = query.run(&db).await?;
    tokio::task::spawn_blocking(move || {
        let page = TreePage {
            css: &CSS_HREF,
            count: graph.nodes.len(),
            svg: tree_svg(&graph),
            form,
            as_of: time(&as_of),
        };
        render(StatusCode::OK, &page)
    })
    .await
    .map_err(|e| {
        eprintln!("系図を組めない: {e}");
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "系図を組めない".to_owned(),
        )
    })
}

#[derive(Template)]
#[template(path = "tree.html")]
struct TreePage {
    css: &'static str,
    form: TreeForm,
    count: usize,
    /// 点が無ければ無い。
    svg: Option<TreeSvg>,
    as_of: Time,
}

/// 系図の絞りの form（今の値）。
#[derive(Default)]
struct TreeForm {
    label: String,
    from: String,
    to: String,
}

impl TreeForm {
    fn new(pairs: &Pairs) -> Self {
        let mut form = Self::default();
        for (name, value) in pairs {
            match name.as_str() {
                "label" => form.label.clone_from(value),
                "from" => form.from.clone_from(value),
                "to" => form.to.clone_from(value),
                _ => {}
            }
        }
        form
    }
}

struct TreeSvg {
    width: u32,
    height: u32,
    boxes: Vec<TreeBox>,
    lines: Vec<TreeLine>,
}

/// 系図の箱 1 つ（置き場と 3 行の字）。
struct TreeBox {
    href: String,
    name: String,
    human: bool,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    /// 字を置く横の真ん中と、3 行の字の下の線。
    cx: u32,
    y1: u32,
    y2: u32,
    y3: u32,
    /// 2 行目: 種類と、最新の model · effort（`claude-` を落とす）。
    detail: String,
    present: bool,
    /// 3 行目の、居るかの後の出来事の名（無ければ空）。
    presence: String,
}

struct TreeLine {
    class: &'static str,
    d: String,
}

/// 名の字の大きさ（太字）と、2・3 行目の字の大きさ（px。site.css の `.tree` と同じ）。
const NAME_PX: u32 = 13;
const SUB_PX: u32 = 11;
/// 箱の字の左右の余白と、箱の狭さの下限。
const BOX_PAD: u32 = 12;
const BOX_MIN: u32 = 96;

/// 系図の SVG に置く物。点が無ければ None。
fn tree_svg(graph: &db::Graph) -> Option<TreeSvg> {
    if graph.nodes.is_empty() {
        return None;
    }
    let index: BTreeMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.actor.id.as_str(), i))
        .collect();
    let texts: Vec<(String, String, String)> = graph
        .nodes
        .iter()
        .map(|n| {
            let kind = kind_name(&n.actor.kind);
            let detail = match &n.current_settings {
                Some(s) => format!("{kind} · {} · {}", short_model(&s.model), s.effort),
                None => kind.to_owned(),
            };
            let state = n.state.as_ref();
            let present = if state.is_some_and(|s| s.present) {
                "いる"
            } else {
                "いない"
            };
            let presence = state
                .and_then(|s| s.presence.as_deref())
                .map(|p| format!("{present} · {}", event_name(p)))
                .unwrap_or_else(|| present.to_owned());
            (n.actor.name.clone(), detail, presence)
        })
        .collect();
    let nodes: Vec<tree::Node> = graph
        .nodes
        .iter()
        .zip(&texts)
        .map(|(n, (name, detail, presence))| tree::Node {
            // 太字は 1 px 大きく見積もる。
            width: (text_width(name, NAME_PX + 1)
                .max(text_width(detail, SUB_PX))
                .max(text_width(presence, SUB_PX))
                + BOX_PAD * 2)
                .max(BOX_MIN),
            order: n
                .state
                .as_ref()
                .and_then(|s| s.entered_at.clone())
                .unwrap_or_else(|| n.actor.received_at.clone()),
        })
        .collect();
    let edges: Vec<tree::Edge> = graph
        .edges
        .iter()
        .filter_map(|e| {
            let kind = match e.kind.as_str() {
                "launched" => tree::Kind::Launched,
                "continued" => tree::Kind::Continued,
                _ => return None,
            };
            Some(tree::Edge {
                from: *index.get(e.from_actor.as_str())?,
                to: *index.get(e.to_actor.as_str())?,
                kind,
            })
        })
        .collect();
    let layout = tree::layout(&nodes, &edges);
    let boxes = graph
        .nodes
        .iter()
        .zip(texts)
        .zip(&layout.boxes)
        .map(|((n, (name, detail, _)), r)| {
            let state = n.state.as_ref();
            TreeBox {
                href: format!("/actors/{}", n.actor.id),
                name,
                human: n.actor.kind == "human",
                x: r.x,
                y: r.y,
                w: r.w,
                h: r.h,
                cx: r.x + r.w / 2,
                y1: r.y + 20,
                y2: r.y + 37,
                y3: r.y + 52,
                detail,
                present: state.is_some_and(|s| s.present),
                presence: state
                    .and_then(|s| s.presence.as_deref())
                    .map(|p| event_name(p).to_owned())
                    .unwrap_or_default(),
            }
        })
        .collect();
    let lines = layout
        .lines
        .into_iter()
        .map(|l| TreeLine {
            class: match l.kind {
                tree::Kind::Launched => "edge launched",
                tree::Kind::Continued => "edge continued",
            },
            d: l.d,
        })
        .collect();
    Some(TreeSvg {
        width: layout.width,
        height: layout.height,
        boxes,
        lines,
    })
}

/// model の名を短くする（`claude-opus-5-5` → `opus-5-5`）。
fn short_model(model: &str) -> &str {
    model.strip_prefix("claude-").unwrap_or(model)
}

/// `GET /graph/timeline?label=…&from=…&to=…`: 時間の帯（相関図 B）。
pub async fn timeline(State(_db): State<Db>, _q: Result<Query<Pairs>, QueryRejection>) -> Response {
    StatusCode::NOT_IMPLEMENTED.into_response()
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
            "起こした"
        } else {
            "継がせた"
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
    let query: Vec<String> = pairs
        .iter()
        .map(|(n, v)| format!("{}={}", encode(n), encode(v)))
        .collect();
    format!("/?{}", query.join("&"))
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
    use super::{encode, event_name, time};
    use crate::api::EVENT_TYPES;

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
}
