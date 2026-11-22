//! 監査の検索: アクター・起こした線の上下・model と effort・出来事を、条件で探す（読む接続だけ）。
//!
//! - 時刻の条件は、どれも castlist が受けた時刻（`received_at`）で比べる。送り手の時刻（`at`）では比べない。
//!   「最新」は seq の大きい方。答えの行には `at`・`received_at`・`clock_skew` を付ける。
//! - 頁は keyset で分ける（`after` は前の答えの `next`）。並べる値と seq を後ろから続けるので、
//!   辿る間に行が足されても、元の組に穴も重なりも出ない。
//! - 知らない引数・読めない値は 400 で断る（黙って捨てない）。訳には引数の名を入れる。
//! - SQL は決まった欠片を繋ぎ、値は bind するだけ（[`plans`] の試験が、索引で引くことを見る）。

use crate::api::{Answer, ApiError, bad_request, is_name, is_value, not_found};
use crate::db::{self, Db, Link};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::sqlite::SqliteRow;
use sqlx::{FromRow, QueryBuilder, Row, Sqlite};
use std::collections::{BTreeMap, HashSet};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// 1 頁の既定と上限。
const LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 1000;
/// 線を辿る段の上限。
pub(crate) const MAX_DEPTH: u32 = 20;

// --- SQL ---

/// 一覧の 1 行（アクター）。初めの登場・最後の退場・居るか・最新の model と effort は、
/// アクター 1 つごとの「order by seq limit 1」で引く（events(actor_id, type, seq)・settings(actor_id, seq)）。
/// 時計のずれた行を持つか（skewed）も、アクター 1 つごとに引く（画面の印。API の答えには出さない）。
const ACTOR_ITEM: &str = concat!(
    "select a.seq, a.id, a.name, a.kind, a.issuer, a.sent_at, a.received_at, a.clock_skew,
       (select e.received_at from events e
        where e.actor_id = a.id and e.type in ('entered', 'reentered', 'forked')
        order by e.seq limit 1) as first_entered_at,
       (select e.received_at from events e
        where e.actor_id = a.id and e.type in ('cleared', 'exited', 'closed')
        order by e.seq desc limit 1) as last_exited_at,
       (select e.type from events e
        where e.actor_id = a.id and e.type in ('cleared', 'exited', 'closed')
        order by e.seq desc limit 1) as last_exit_type,
       coalesce((select e.type in ('entered', 'reentered', 'forked') from events e
                 where e.actor_id = a.id
                   and e.type in ('entered', 'reentered', 'forked', 'cleared', 'exited', 'closed')
                 order by e.seq desc limit 1), 0) as present,
       (select e.type from events e
        where e.actor_id = a.id
          and e.type in ('entered', 'reentered', 'forked', 'cleared', 'exited', 'closed')
        order by e.seq desc limit 1) as presence,
       ",
    crate::inferred_sql!("a.id"),
    " as inferred,
       (a.clock_skew
        or exists(select 1 from events e where e.actor_id = a.id and e.clock_skew = 1)
        or exists(select 1 from settings s where s.actor_id = a.id and s.clock_skew = 1)) as skewed,
       (select m.name from settings s join models m on m.id = s.model_id
        where s.actor_id = a.id order by s.seq desc limit 1) as model,
       (select f.name from settings s join efforts f on f.id = s.effort_id
        where s.actor_id = a.id order by s.seq desc limit 1) as effort
     from actors a where 1"
);
/// 一覧の頁のアクターのラベル（JSON の配列の id で）。
const ACTOR_LABELS: &str = "select actor_id, key, value from labels
     where actor_id in (select value from json_each(?1)) order by key";
/// アクターの種類（無ければ行が無い）。線を上る時に、人で止める。
const ACTOR_KIND: &str = "select kind from actors where id = ?1";
/// 上る線（そのアクターを起こした・継がせた線）と、下る線。
const LINEAGE_UP: &str =
    "select seq, id, type, from_actor, to_actor, sent_at, received_at, clock_skew from links
     where to_actor = ?1 and (?2 is null or type = ?2) order by seq";
const LINEAGE_DOWN: &str =
    "select seq, id, type, from_actor, to_actor, sent_at, received_at, clock_skew from links
     where from_actor = ?1 and (?2 is null or type = ?2) order by seq";

macro_rules! setting_row {
    () => {
        "select s.seq, s.id, s.actor_id, a.name as actor_name, m.name as model, s.model_id,
                f.name as effort, s.effort_id, s.source, s.sent_at, s.received_at, s.clock_skew
         from settings s join actors a on a.id = s.actor_id
         join models m on m.id = s.model_id join efforts f on f.id = s.effort_id"
    };
}
/// あるアクターの model と effort の全部（受けた順）と、ある時に効いていた物（受けた時刻がそれ以前の、seq の最大）。
const ACTOR_SETTINGS: &str = concat!(setting_row!(), " where s.actor_id = ?1 order by s.seq");
const ACTOR_SETTINGS_AT: &str = concat!(
    setting_row!(),
    " where s.actor_id = ?1 and s.received_at <= ?2 order by s.seq desc limit 1"
);
/// 見た行が、同じアクターのその前の最新の頼んだ行と、model か effort で違う。
const MISMATCH: &str = " and s.source = 'observed'
     and coalesce((select r.model_id != s.model_id or r.effort_id != s.effort_id from settings r
                   where r.actor_id = s.actor_id and r.source = 'requested' and r.seq < s.seq
                   order by r.seq desc limit 1), 0)";
const EVENT_ROW: &str = "select e.seq, e.id, e.actor_id, a.name as actor_name, e.type, e.detail, e.sent_at, e.received_at, e.clock_skew
     from events e join actors a on a.id = e.actor_id";

// --- 型 ---

/// アクターの一覧の並べ方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// 初めの登場（entered・reentered・forked）の受けた時刻。
    #[default]
    Entered,
    /// 最後の退場（cleared・exited・closed）の受けた時刻。
    Exited,
}

impl Sort {
    fn name(self) -> &'static str {
        match self {
            Sort::Entered => "entered",
            Sort::Exited => "exited",
        }
    }

    /// 並べる値（まだ無ければ ''。昇順で先、降順で後）。
    fn key(self) -> &'static str {
        match self {
            Sort::Entered => "coalesce(first_entered_at, '')",
            Sort::Exited => "coalesce(last_exited_at, '')",
        }
    }
}

/// アクターを探す条件（どれも AND）。
#[derive(Clone, Debug, Default)]
pub struct ActorQuery {
    /// ラベル（鍵・値）。
    pub labels: Vec<(String, String)>,
    /// 付随する id（種類・値）と時刻: その時に、その値を最後に受けたアクター。
    pub binding: Option<(String, String, String)>,
    /// 居た間（初めの登場〜最後の退場。退いていなければ今まで）が、この窓に掛かる。
    pub active_from: Option<String>,
    pub active_to: Option<String>,
    pub kind: Option<String>,
    /// settings のどれかの行が、この名を持つ。
    pub model: Option<String>,
    pub effort: Option<String>,
    /// 今居るか（actor_state と同じ決め方）。
    pub active: Option<bool>,
    /// このアクターが起こした（launched）。
    pub launched_by: Option<String>,
    /// clock_skew = 1 の行（アクター・出来事・model と effort）を持つ。
    pub skewed: bool,
    /// id の JSON の配列（線の上下の点を引く時）。
    pub ids: Option<String>,
    pub sort: Sort,
    pub desc: bool,
    /// 前の頁の最後（並べる値・seq）。
    pub after: Option<(String, i64)>,
    /// 無ければ全部。
    pub limit: Option<i64>,
}

/// 一覧の 1 行。
#[derive(Debug, Serialize, FromRow)]
pub struct ActorItem {
    #[serde(skip)]
    pub seq: i64,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub issuer: String,
    pub sent_at: String,
    pub received_at: String,
    pub clock_skew: bool,
    #[sqlx(skip)]
    pub labels: BTreeMap<String, String>,
    pub first_entered_at: Option<String>,
    pub last_exited_at: Option<String>,
    pub last_exit_type: Option<String>,
    pub present: bool,
    /// 今の状態を決めた出来事が、推しの幕引き（`closed` で `detail.inferred` が JSON の true）。
    pub inferred: bool,
    /// 居る・居ないを替える出来事の最新の種類（画面に出す。API の答えには出さない）。
    #[serde(skip)]
    pub presence: Option<String>,
    /// アクター・出来事・model と effort のどれかに、時計のずれた行がある（画面の印）。
    #[serde(skip)]
    pub skewed: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// プロフィールの頁の path。
    #[sqlx(skip)]
    pub url: String,
    #[serde(skip)]
    pub sort_key: String,
}

/// model と effort の 1 行（名と番号の両方）。
#[derive(Debug, Serialize, FromRow)]
pub struct SettingRow {
    #[serde(skip)]
    pub seq: i64,
    pub id: Option<String>,
    pub actor_id: String,
    pub actor_name: String,
    pub model: String,
    pub model_id: i64,
    pub effort: String,
    pub effort_id: i64,
    pub source: String,
    pub sent_at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

/// 出来事の 1 行（アクターの名も）。
#[derive(Debug, Serialize)]
pub struct EventRow {
    #[serde(skip)]
    pub seq: i64,
    pub id: String,
    pub actor_id: String,
    pub actor_name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub detail: Value,
    pub sent_at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

impl FromRow<'_, SqliteRow> for EventRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let detail: String = row.try_get("detail")?;
        Ok(Self {
            seq: row.try_get("seq")?,
            id: row.try_get("id")?,
            actor_id: row.try_get("actor_id")?,
            actor_name: row.try_get("actor_name")?,
            kind: row.try_get("type")?,
            detail: serde_json::from_str(&detail).map_err(|e| sqlx::Error::Decode(e.into()))?,
            sent_at: row.try_get("sent_at")?,
            received_at: row.try_get("received_at")?,
            clock_skew: row.try_get("clock_skew")?,
        })
    }
}

/// model と effort の行を探す条件。
#[derive(Clone, Debug, Default)]
pub struct SettingQuery {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub source: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub mismatch: bool,
    pub skewed: bool,
    /// 前の頁の最後の seq（無ければ 0）。
    pub after: i64,
    pub limit: i64,
}

/// 出来事を探す条件。
#[derive(Clone, Debug, Default)]
pub struct EventQuery {
    /// 種類の JSON の配列。
    pub types: Option<String>,
    pub actor: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// detail の決まった欄（reason・source）。
    pub reason: Option<String>,
    pub source: Option<String>,
    pub skewed: bool,
    pub after: i64,
    pub limit: i64,
}

/// 線の上下（点と線）。
#[derive(Debug, Serialize)]
pub struct Lineage {
    pub nodes: Vec<ActorItem>,
    pub edges: Vec<Link>,
}

// --- SQL を組む ---

impl ActorQuery {
    /// 索引で引ける条件（ラベル・付随する id・窓・model・effort・起こした者・時計のずれ・今居る・id の組）があるか。
    /// これが無いと、アクターの表を端から読むことになる。
    pub fn narrowed(&self) -> bool {
        !self.labels.is_empty()
            || self.binding.is_some()
            || self.windowed()
            || self.model.is_some()
            || self.effort.is_some()
            || self.launched_by.is_some()
            || self.skewed
            || self.ids.is_some()
    }

    fn windowed(&self) -> bool {
        self.active_from.is_some() || self.active_to.is_some() || self.active == Some(true)
    }

    fn builder(&self) -> QueryBuilder<Sqlite> {
        let key = self.sort.key();
        let mut q = QueryBuilder::new(format!("select *, {key} as sort_key from ("));
        q.push(ACTOR_ITEM);
        for (k, v) in &self.labels {
            q.push(" and a.id in (select l.actor_id from labels l where l.key = ")
                .push_bind(k.clone())
                .push(" and l.value = ")
                .push_bind(v.clone())
                .push(")");
        }
        if let Some((kind, value, at)) = &self.binding {
            q.push(" and a.id = (select b.actor_id from bindings b where b.type = ")
                .push_bind(kind.clone())
                .push(" and b.value = ")
                .push_bind(value.clone())
                .push(" and b.received_at <= ")
                .push_bind(at.clone())
                .push(" order by b.received_at desc, b.seq desc limit 1)");
        }
        if self.windowed() {
            // 窓に掛かるなら、窓の終わりまでに登場している（events(type, received_at) で引く）。
            q.push(
                " and a.id in (select e.actor_id from events e
                  where e.type in ('entered', 'reentered', 'forked') and e.received_at <= ",
            )
            .push_bind(self.active_to.clone().unwrap_or_else(|| "9999".to_owned()))
            .push(")");
        }
        if let Some(kind) = &self.kind {
            q.push(" and a.kind = ").push_bind(kind.clone());
        }
        if let Some(model) = &self.model {
            q.push(" and a.id in (select s.actor_id from settings s where s.model_id = (select id from models where name = ")
                .push_bind(model.clone())
                .push("))");
        }
        if let Some(effort) = &self.effort {
            q.push(" and a.id in (select s.actor_id from settings s where s.effort_id = (select id from efforts where name = ")
                .push_bind(effort.clone())
                .push("))");
        }
        if let Some(by) = &self.launched_by {
            q.push(" and a.id in (select k.to_actor from links k where k.from_actor = ")
                .push_bind(by.clone())
                .push(" and k.type = 'launched')");
        }
        if self.skewed {
            q.push(
                " and a.id in (select id from actors where clock_skew = 1
                  union all select actor_id from events where clock_skew = 1
                  union all select actor_id from settings where clock_skew = 1)",
            );
        }
        if let Some(ids) = &self.ids {
            q.push(" and a.id in (select value from json_each(")
                .push_bind(ids.clone())
                .push("))");
        }
        q.push(") where 1");
        if self.windowed() {
            q.push(" and first_entered_at is not null");
        }
        if let Some(to) = &self.active_to {
            q.push(" and first_entered_at <= ").push_bind(to.clone());
        }
        if let Some(from) = &self.active_from {
            q.push(" and (present or last_exited_at >= ")
                .push_bind(from.clone())
                .push(")");
        }
        if let Some(active) = self.active {
            q.push(" and present = ").push_bind(active);
        }
        let (cmp, dir) = if self.desc {
            ("<", "desc")
        } else {
            (">", "asc")
        };
        if let Some((value, seq)) = &self.after {
            q.push(format!(" and ({key} {cmp} "))
                .push_bind(value.clone())
                .push(format!(" or ({key} = "))
                .push_bind(value.clone())
                .push(format!(" and seq {cmp} "))
                .push_bind(*seq)
                .push("))");
        }
        q.push(format!(" order by {key} {dir}, seq {dir}"));
        if let Some(limit) = self.limit {
            // 次の頁があるかを見るため、1 つ多く読む。
            q.push(" limit ").push_bind(limit + 1);
        }
        q
    }
}

impl SettingQuery {
    fn builder(&self) -> QueryBuilder<Sqlite> {
        let mut q = QueryBuilder::new(concat!(setting_row!(), " where s.seq > "));
        q.push_bind(self.after);
        if let Some(model) = &self.model {
            q.push(" and s.model_id = (select id from models where name = ")
                .push_bind(model.clone())
                .push(")");
        }
        if let Some(effort) = &self.effort {
            q.push(" and s.effort_id = (select id from efforts where name = ")
                .push_bind(effort.clone())
                .push(")");
        }
        if let Some(source) = &self.source {
            q.push(" and s.source = ").push_bind(source.clone());
        }
        if let Some(from) = &self.from {
            q.push(" and s.received_at >= ").push_bind(from.clone());
        }
        if let Some(to) = &self.to {
            q.push(" and s.received_at <= ").push_bind(to.clone());
        }
        if self.mismatch {
            q.push(MISMATCH);
        }
        if self.skewed {
            q.push(" and s.clock_skew = 1");
        }
        q.push(" order by s.seq limit ").push_bind(self.limit + 1);
        q
    }
}

impl EventQuery {
    fn builder(&self) -> QueryBuilder<Sqlite> {
        let mut q = QueryBuilder::new(format!("{EVENT_ROW} where e.seq > "));
        q.push_bind(self.after);
        if let Some(types) = &self.types {
            q.push(" and e.type in (select value from json_each(")
                .push_bind(types.clone())
                .push("))");
        }
        if let Some(actor) = &self.actor {
            q.push(" and e.actor_id = ").push_bind(actor.clone());
        }
        if let Some(from) = &self.from {
            q.push(" and e.received_at >= ").push_bind(from.clone());
        }
        if let Some(to) = &self.to {
            q.push(" and e.received_at <= ").push_bind(to.clone());
        }
        if let Some(reason) = &self.reason {
            q.push(" and json_extract(e.detail, '$.reason') = ")
                .push_bind(reason.clone());
        }
        if let Some(source) = &self.source {
            q.push(" and json_extract(e.detail, '$.source') = ")
                .push_bind(source.clone());
        }
        if self.skewed {
            q.push(" and e.clock_skew = 1");
        }
        q.push(" order by e.seq limit ").push_bind(self.limit + 1);
        q
    }
}

/// 検索の SQL（名と SQL）。試験が `explain query plan` で、大きな表を端から読まないかを見る。
/// 条件を 1 つずつと、全部を重ねた物。値は bind するので、ここでは仮の値を入れる。
pub fn plans() -> Vec<(&'static str, String)> {
    fn x() -> Option<String> {
        Some("x".to_owned())
    }
    fn label(q: &mut ActorQuery) {
        q.labels = vec![("k".into(), "v".into())];
    }
    let actors = |f: fn(&mut ActorQuery)| {
        let mut q = ActorQuery {
            limit: Some(LIMIT),
            ..ActorQuery::default()
        };
        f(&mut q);
        q.builder().into_string()
    };
    let settings = |f: fn(&mut SettingQuery)| {
        let mut q = SettingQuery {
            limit: LIMIT,
            ..SettingQuery::default()
        };
        f(&mut q);
        q.builder().into_string()
    };
    let events = |f: fn(&mut EventQuery)| {
        let mut q = EventQuery {
            limit: LIMIT,
            ..EventQuery::default()
        };
        f(&mut q);
        q.builder().into_string()
    };
    vec![
        ("actors_label", actors(label)),
        (
            "actors_labels",
            actors(|q| q.labels = vec![("k".into(), "v".into()); 2]),
        ),
        (
            "actors_binding",
            actors(|q| q.binding = Some(("t".into(), "v".into(), "at".into()))),
        ),
        (
            "actors_window",
            actors(|q| (q.active_from, q.active_to) = (x(), x())),
        ),
        ("actors_window_from", actors(|q| q.active_from = x())),
        ("actors_model", actors(|q| q.model = x())),
        ("actors_effort", actors(|q| q.effort = x())),
        ("actors_skewed", actors(|q| q.skewed = true)),
        ("actors_launched_by", actors(|q| q.launched_by = x())),
        ("actors_active", actors(|q| q.active = Some(true))),
        (
            "actors_kind",
            actors(|q| {
                label(q);
                (q.kind, q.active) = (x(), Some(false));
            }),
        ),
        (
            "actors_after",
            actors(|q| {
                label(q);
                q.after = Some(("x".into(), 1));
            }),
        ),
        (
            "actors_exited_desc",
            actors(|q| {
                label(q);
                (q.sort, q.desc, q.after) = (Sort::Exited, true, Some(("x".into(), 1)));
            }),
        ),
        (
            "actors_all",
            actors(|q| {
                *q = ActorQuery {
                    labels: vec![("k".into(), "v".into()); 2],
                    binding: Some(("t".into(), "v".into(), "at".into())),
                    active_from: x(),
                    active_to: x(),
                    kind: x(),
                    model: x(),
                    effort: x(),
                    active: Some(true),
                    launched_by: x(),
                    skewed: true,
                    ids: x(),
                    sort: Sort::Exited,
                    desc: true,
                    after: Some(("x".into(), 1)),
                    limit: Some(LIMIT),
                }
            }),
        ),
        ("actor_items", actors(|q| (q.ids, q.limit) = (x(), None))),
        ("actor_labels", ACTOR_LABELS.to_owned()),
        ("lineage_kind", ACTOR_KIND.to_owned()),
        ("lineage_up", LINEAGE_UP.to_owned()),
        ("lineage_down", LINEAGE_DOWN.to_owned()),
        ("actor_settings_at", ACTOR_SETTINGS_AT.to_owned()),
        ("actor_settings", ACTOR_SETTINGS.to_owned()),
        ("settings", settings(|_| ())),
        ("settings_model", settings(|q| q.model = x())),
        ("settings_effort", settings(|q| q.effort = x())),
        ("settings_window", settings(|q| (q.from, q.to) = (x(), x()))),
        ("settings_mismatch", settings(|q| q.mismatch = true)),
        ("settings_skewed", settings(|q| q.skewed = true)),
        (
            "settings_all",
            settings(|q| {
                *q = SettingQuery {
                    model: x(),
                    effort: x(),
                    source: x(),
                    from: x(),
                    to: x(),
                    mismatch: true,
                    skewed: true,
                    after: 1,
                    limit: LIMIT,
                }
            }),
        ),
        ("events", events(|_| ())),
        ("events_types", events(|q| q.types = x())),
        ("events_actor", events(|q| q.actor = x())),
        ("events_window", events(|q| (q.from, q.to) = (x(), x()))),
        (
            "events_detail",
            events(|q| (q.reason, q.source) = (x(), x())),
        ),
        ("events_skewed", events(|q| q.skewed = true)),
        (
            "events_all",
            events(|q| {
                *q = EventQuery {
                    types: x(),
                    actor: x(),
                    from: x(),
                    to: x(),
                    reason: x(),
                    source: x(),
                    skewed: true,
                    after: 1,
                    limit: LIMIT,
                }
            }),
        ),
    ]
}

// --- 読む ---

/// 1 頁と、次の頁の印の元（並べる値・seq。無ければ終わり）。
pub struct Page<T> {
    pub rows: Vec<T>,
    pub more: bool,
}

fn page<T>(mut rows: Vec<T>, limit: Option<i64>) -> Page<T> {
    let limit = limit.and_then(|l| usize::try_from(l).ok());
    let more = limit.is_some_and(|l| rows.len() > l);
    if let Some(l) = limit {
        rows.truncate(l);
    }
    Page { rows, more }
}

impl Db {
    /// アクターを探す（1 つの読む transaction の中で、揃った写しを読む）。
    pub async fn search_actors(&self, q: &ActorQuery) -> Result<Page<ActorItem>, sqlx::Error> {
        let mut tx = self.read().begin().await?;
        let mut builder = q.builder();
        let rows: Vec<ActorItem> = builder.build_query_as().fetch_all(&mut *tx).await?;
        let mut page = page(rows, q.limit);
        let ids =
            Value::from(page.rows.iter().map(|a| a.id.clone()).collect::<Vec<_>>()).to_string();
        let labels: Vec<(String, String, String)> = sqlx::query_as(ACTOR_LABELS)
            .bind(ids)
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        let mut by_actor: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (actor, key, value) in labels {
            by_actor.entry(actor).or_default().insert(key, value);
        }
        for item in &mut page.rows {
            item.labels = by_actor.remove(&item.id).unwrap_or_default();
            item.url = format!("/actors/{}", item.id);
        }
        Ok(page)
    }

    /// 線を上り（人で止める）、下る。アクターが無ければ None。
    pub async fn lineage(
        &self,
        id: &str,
        up: u32,
        down: u32,
        kind: Option<&str>,
    ) -> Result<Option<Lineage>, sqlx::Error> {
        let mut tx = self.read().begin().await?;
        let found: Option<String> = sqlx::query_scalar(ACTOR_KIND)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        if found.is_none() {
            return Ok(None);
        }
        let mut seen: HashSet<String> = HashSet::from([id.to_owned()]);
        let mut order = vec![id.to_owned()];
        let mut edges: BTreeMap<i64, Link> = BTreeMap::new();
        for (sql, depth, upward) in [(LINEAGE_UP, up, true), (LINEAGE_DOWN, down, false)] {
            let mut frontier = vec![id.to_owned()];
            for _ in 0..depth {
                let mut next = Vec::new();
                for node in &frontier {
                    if upward {
                        let node_kind: Option<String> = sqlx::query_scalar(ACTOR_KIND)
                            .bind(node)
                            .fetch_optional(&mut *tx)
                            .await?;
                        if node_kind.as_deref() == Some("human") {
                            continue;
                        }
                    }
                    let links: Vec<Link> = sqlx::query_as(sql)
                        .bind(node)
                        .bind(kind)
                        .fetch_all(&mut *tx)
                        .await?;
                    for link in links {
                        let end = if upward {
                            &link.from_actor
                        } else {
                            &link.to_actor
                        };
                        if seen.insert(end.clone()) {
                            order.push(end.clone());
                            next.push(end.clone());
                        }
                        edges.insert(link.seq, link);
                    }
                }
                if next.is_empty() {
                    break;
                }
                frontier = next;
            }
        }
        tx.commit().await?;
        let nodes = self
            .search_actors(&ActorQuery {
                ids: Some(Value::from(order).to_string()),
                ..ActorQuery::default()
            })
            .await?
            .rows;
        Ok(Some(Lineage {
            nodes,
            edges: edges.into_values().collect(),
        }))
    }

    /// あるアクターの model と effort。`at` があれば、その時に効いていた 1 行（無ければ None）。
    /// アクターが無ければ Err ではなく、外の None。
    pub async fn actor_settings(
        &self,
        id: &str,
        at: Option<&str>,
    ) -> Result<Option<Vec<SettingRow>>, sqlx::Error> {
        let mut tx = self.read().begin().await?;
        let found: Option<String> = sqlx::query_scalar(ACTOR_KIND)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        if found.is_none() {
            return Ok(None);
        }
        let rows = match at {
            Some(at) => {
                sqlx::query_as(ACTOR_SETTINGS_AT)
                    .bind(id)
                    .bind(at)
                    .fetch_all(&mut *tx)
                    .await?
            }
            None => {
                sqlx::query_as(ACTOR_SETTINGS)
                    .bind(id)
                    .fetch_all(&mut *tx)
                    .await?
            }
        };
        tx.commit().await?;
        Ok(Some(rows))
    }

    pub async fn search_settings(&self, q: &SettingQuery) -> Result<Page<SettingRow>, sqlx::Error> {
        let mut builder = q.builder();
        let rows = builder.build_query_as().fetch_all(self.read()).await?;
        Ok(page(rows, Some(q.limit)))
    }

    pub async fn search_events(&self, q: &EventQuery) -> Result<Page<EventRow>, sqlx::Error> {
        let mut builder = q.builder();
        let rows = builder.build_query_as().fetch_all(self.read()).await?;
        Ok(page(rows, Some(q.limit)))
    }
}

// --- 引数 ---

pub(crate) type Pairs = Vec<(String, String)>;

/// 問い合わせの引数（名と値の並び）。知らない名と、1 つだけの名の重なりは、読む時に断る。
pub(crate) struct Params(Pairs);

/// 問い合わせの引数を、名と値の並びで読む（読めなければ 400）。
pub(crate) fn pairs(q: Result<Query<Pairs>, QueryRejection>) -> Result<Pairs, ApiError> {
    q.map(|Query(pairs)| pairs)
        .map_err(|e| bad_request(format!("問い合わせの引数が読めない: {e}")))
}

impl Params {
    pub(crate) fn parse(
        q: Result<Query<Pairs>, QueryRejection>,
        known: &[&str],
        repeatable: &[&str],
    ) -> Result<Self, ApiError> {
        Self::new(pairs(q)?, known, repeatable)
    }

    pub(crate) fn new(pairs: Pairs, known: &[&str], repeatable: &[&str]) -> Result<Self, ApiError> {
        for (i, (name, _)) in pairs.iter().enumerate() {
            if !known.contains(&name.as_str()) {
                return Err(bad_request(format!(
                    "知らない引数: {name}（使えるのは {}）",
                    known.join("・")
                )));
            }
            if !repeatable.contains(&name.as_str()) && pairs[..i].iter().any(|(n, _)| n == name) {
                return Err(bad_request(format!("{name} は 1 つだけ付ける")));
            }
        }
        Ok(Self(pairs))
    }

    pub(crate) fn one(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.0
            .iter()
            .filter(move |(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// 時刻（RFC 3339）を、castlist の時計の字（UTC・ミリ秒まで）にする。
    pub(crate) fn time(&self, name: &str) -> Result<Option<String>, ApiError> {
        self.one(name)
            .map(|v| {
                OffsetDateTime::parse(v, &Rfc3339)
                    .map(db::stamp)
                    .map_err(|_| {
                        bad_request(format!(
                            "{name} は RFC 3339 の時刻（例 2026-11-22T13:00:00+09:00）"
                        ))
                    })
            })
            .transpose()
    }

    fn flag(&self, name: &str) -> Result<Option<bool>, ApiError> {
        match self.one(name) {
            None => Ok(None),
            Some("true") => Ok(Some(true)),
            Some("false") => Ok(Some(false)),
            Some(_) => Err(bad_request(format!("{name} は true か false"))),
        }
    }

    /// `true` だけを受ける絞り（付けなければ絞らない）。
    fn only_true(&self, name: &str) -> Result<bool, ApiError> {
        match self.one(name) {
            None => Ok(false),
            Some("true") => Ok(true),
            Some(_) => Err(bad_request(format!(
                "{name} は true だけ（絞らないなら付けない）"
            ))),
        }
    }

    fn limit(&self) -> Result<i64, ApiError> {
        match self.one("limit") {
            None => Ok(LIMIT),
            Some(v) => v
                .parse()
                .ok()
                .filter(|n| (1..=MAX_LIMIT).contains(n))
                .ok_or_else(|| bad_request(format!("limit は 1 から {MAX_LIMIT} までの数"))),
        }
    }

    /// 名の形の値（model・effort・kind など）。
    fn name(&self, name: &str) -> Result<Option<String>, ApiError> {
        self.one(name)
            .map(|v| {
                if is_name(v) {
                    Ok(v.to_owned())
                } else {
                    Err(bad_request(format!(
                        "{name} は英小文字・数字・- . で 64 字まで"
                    )))
                }
            })
            .transpose()
    }

    /// 空でない 256 字までの値（id・detail の欄）。
    fn value(&self, name: &str) -> Result<Option<String>, ApiError> {
        self.one(name)
            .map(|v| {
                if is_value(v) {
                    Ok(v.to_owned())
                } else {
                    Err(bad_request(format!("{name} は空でない 256 字まで")))
                }
            })
            .transpose()
    }

    /// 決まった物のどれか。
    fn choice(
        &self,
        name: &str,
        choices: &[&'static str],
    ) -> Result<Option<&'static str>, ApiError> {
        self.one(name)
            .map(|v| {
                choices.iter().find(|c| **c == v).copied().ok_or_else(|| {
                    bad_request(format!("{name} は {} のどれか", choices.join("・")))
                })
            })
            .transpose()
    }
}

/// `鍵:値`・`種類:値` を初めの `:` で分ける。
pub(crate) fn pair(name: &str, v: &str, shape: &str) -> Result<(String, String), ApiError> {
    match v.split_once(':') {
        Some((key, value)) if is_name(key) && is_value(value) => {
            Ok((key.to_owned(), value.to_owned()))
        }
        _ => Err(bad_request(format!("{name} は「{shape}」の形（{v}）"))),
    }
}

/// 頁の印: 中身を 16 進にした字（中を読んで使わない。前の答えの next をそのまま渡す）。
fn cursor(parts: &[&str]) -> String {
    parts
        .join("\n")
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn uncursor(v: &str, what: &str, n: usize) -> Result<Vec<String>, ApiError> {
    let bad = || bad_request("after が読めない（前の答えの next を、そのまま渡す）");
    if !v.len().is_multiple_of(2) || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let bytes = (0..v.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&v[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| bad())?;
    let text = String::from_utf8(bytes).map_err(|_| bad())?;
    let parts: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if parts.len() != n || parts[0] != what {
        return Err(bad_request(format!(
            "after が読めない（{what} の検索の next ではない）"
        )));
    }
    Ok(parts)
}

fn seq_after(p: &Params, what: &str) -> Result<i64, ApiError> {
    match p.one("after") {
        None => Ok(0),
        Some(v) => uncursor(v, what, 2)?[1]
            .parse()
            .map_err(|_| bad_request("after が読めない（前の答えの next を、そのまま渡す）")),
    }
}

// --- 道 ---

pub(crate) const ACTOR_PARAMS: &[&str] = &[
    "label",
    "binding",
    "at",
    "active_from",
    "active_to",
    "kind",
    "model",
    "effort",
    "active",
    "launched_by",
    "skewed",
    "sort",
    "order",
    "limit",
    "after",
];

/// `GET /v1/actors`: アクターを条件で探す。
pub async fn actors(State(db): State<Db>, q: Result<Query<Pairs>, QueryRejection>) -> Answer {
    let p = Params::parse(q, ACTOR_PARAMS, &["label"])?;
    let as_of = db::now();
    let query = actor_query(&p, &as_of)?;
    if !query.narrowed() {
        return Err(bad_request(
            "絞る条件を 1 つ以上付ける（label・binding・active_from・active_to・active=true・model・effort・launched_by・skewed のどれか）",
        ));
    }
    let page = db.search_actors(&query).await?;
    let next = actor_next(&query, &page);
    Ok(Json(json!({ "as_of": as_of, "actors": page.rows, "next": next })).into_response())
}

/// `GET /v1/actors` の引数（[`ACTOR_PARAMS`]）を、探す条件にする。`as_of` は `at` の無い `binding` の時刻。
/// 一覧の画面も、同じ引数をこれで読む。
pub(crate) fn actor_query(p: &Params, as_of: &str) -> Result<ActorQuery, ApiError> {
    let labels = p
        .all("label")
        .map(|v| pair("label", v, "鍵:値"))
        .collect::<Result<Vec<_>, _>>()?;
    let at = p.time("at")?;
    let binding = match p.one("binding") {
        Some(v) => {
            let (kind, value) = pair("binding", v, "種類:値")?;
            Some((kind, value, at.unwrap_or_else(|| as_of.to_owned())))
        }
        None if at.is_some() => {
            return Err(bad_request(
                "at は binding と一緒に付ける（居た間で探すなら active_from・active_to）",
            ));
        }
        None => None,
    };
    let sort = match p.choice("sort", &["entered", "exited"])? {
        Some("exited") => Sort::Exited,
        _ => Sort::Entered,
    };
    let desc = p.choice("order", &["asc", "desc"])? == Some("desc");
    let order = if desc { "desc" } else { "asc" };
    let after = match p.one("after") {
        None => None,
        Some(v) => {
            let parts = uncursor(v, "actors", 5)?;
            if parts[1] != sort.name() || parts[2] != order {
                return Err(bad_request(
                    "after は、同じ sort と order の答えの next を渡す",
                ));
            }
            let seq = parts[4]
                .parse()
                .map_err(|_| bad_request("after が読めない（前の答えの next を、そのまま渡す）"))?;
            Some((parts[3].clone(), seq))
        }
    };
    Ok(ActorQuery {
        labels,
        binding,
        active_from: p.time("active_from")?,
        active_to: p.time("active_to")?,
        kind: p.choice("kind", &["ai", "human"])?.map(str::to_owned),
        model: p.name("model")?,
        effort: p.name("effort")?,
        active: p.flag("active")?,
        launched_by: p.value("launched_by")?,
        skewed: p.only_true("skewed")?,
        ids: None,
        sort,
        desc,
        after,
        limit: Some(p.limit()?),
    })
}

/// アクターの一覧の次の頁の印（終わりなら None）。
pub(crate) fn actor_next(query: &ActorQuery, page: &Page<ActorItem>) -> Option<String> {
    let order = if query.desc { "desc" } else { "asc" };
    page.more.then(|| page.rows.last()).flatten().map(|last| {
        cursor(&[
            "actors",
            query.sort.name(),
            order,
            &last.sort_key,
            &last.seq.to_string(),
        ])
    })
}

/// `GET /v1/actors/{id}/lineage?direction=up|down|both&depth=N&type=launched|continued`。
pub async fn lineage(
    State(db): State<Db>,
    Path(id): Path<String>,
    q: Result<Query<Pairs>, QueryRejection>,
) -> Answer {
    let p = Params::parse(q, &["direction", "depth", "type"], &[])?;
    let direction = p
        .choice("direction", &["up", "down", "both"])?
        .unwrap_or("both");
    let depth = match p.one("depth") {
        None => None,
        Some(v) => Some(
            v.parse::<u32>()
                .ok()
                .filter(|n| (1..=MAX_DEPTH).contains(n))
                .ok_or_else(|| bad_request(format!("depth は 1 から {MAX_DEPTH} までの数")))?,
        ),
    };
    let kind = p.choice("type", &["launched", "continued"])?;
    // 上は既定で根まで（20 段まで）、下は既定で 1 段。
    let up = if direction == "down" {
        0
    } else {
        depth.unwrap_or(MAX_DEPTH)
    };
    let down = if direction == "up" {
        0
    } else {
        depth.unwrap_or(1)
    };
    let as_of = db::now();
    let lineage = db
        .lineage(&id, up, down, kind)
        .await?
        .ok_or_else(not_found)?;
    Ok(Json(json!({
        "as_of": as_of,
        "actor_id": id,
        "nodes": lineage.nodes,
        "edges": lineage.edges,
    }))
    .into_response())
}

/// `GET /v1/actors/{id}/settings?at=…`: その時に効いていた model と effort（無ければ全部）。
pub async fn actor_settings(
    State(db): State<Db>,
    Path(id): Path<String>,
    q: Result<Query<Pairs>, QueryRejection>,
) -> Answer {
    let p = Params::parse(q, &["at"], &[])?;
    let at = p.time("at")?;
    let as_of = db::now();
    let mut rows = db
        .actor_settings(&id, at.as_deref())
        .await?
        .ok_or_else(not_found)?;
    let body = match at {
        Some(at) => json!({ "as_of": as_of, "actor_id": id, "at": at, "setting": rows.pop() }),
        None => json!({ "as_of": as_of, "actor_id": id, "settings": rows }),
    };
    Ok(Json(body).into_response())
}

/// `GET /v1/settings`: model と effort の行を、アクターをまたいで探す。
pub async fn settings(State(db): State<Db>, q: Result<Query<Pairs>, QueryRejection>) -> Answer {
    let p = Params::parse(
        q,
        &[
            "model", "effort", "source", "from", "to", "mismatch", "skewed", "limit", "after",
        ],
        &[],
    )?;
    let query = SettingQuery {
        model: p.name("model")?,
        effort: p.name("effort")?,
        source: p
            .choice("source", &["requested", "observed"])?
            .map(str::to_owned),
        from: p.time("from")?,
        to: p.time("to")?,
        mismatch: p.only_true("mismatch")?,
        skewed: p.only_true("skewed")?,
        after: seq_after(&p, "settings")?,
        limit: p.limit()?,
    };
    let as_of = db::now();
    let page = db.search_settings(&query).await?;
    let next = next_seq(page.more, page.rows.last().map(|r| r.seq), "settings");
    Ok(Json(json!({ "as_of": as_of, "settings": page.rows, "next": next })).into_response())
}

/// `GET /v1/events`: 出来事を、アクターをまたいで探す。
pub async fn events(State(db): State<Db>, q: Result<Query<Pairs>, QueryRejection>) -> Answer {
    let p = Params::parse(
        q,
        &[
            "type", "actor", "from", "to", "reason", "source", "skewed", "limit", "after",
        ],
        &["type"],
    )?;
    let types = p
        .all("type")
        .map(|t| {
            crate::api::EVENT_TYPES
                .contains(&t)
                .then(|| t.to_owned())
                .ok_or_else(|| {
                    bad_request(format!(
                        "type は {} のどれか（{t}）",
                        crate::api::EVENT_TYPES.join("・")
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let query = EventQuery {
        types: (!types.is_empty()).then(|| Value::from(types).to_string()),
        actor: p.value("actor")?,
        from: p.time("from")?,
        to: p.time("to")?,
        reason: p.value("reason")?,
        source: p.value("source")?,
        skewed: p.only_true("skewed")?,
        after: seq_after(&p, "events")?,
        limit: p.limit()?,
    };
    let as_of = db::now();
    let page = db.search_events(&query).await?;
    let next = next_seq(page.more, page.rows.last().map(|r| r.seq), "events");
    Ok(Json(json!({ "as_of": as_of, "events": page.rows, "next": next })).into_response())
}

fn next_seq(more: bool, last: Option<i64>, what: &str) -> Option<String> {
    last.filter(|_| more)
        .map(|seq| cursor(&[what, &seq.to_string()]))
}

#[cfg(test)]
mod tests {
    use super::{cursor, uncursor};

    #[test]
    fn 頁の印は戻せて_崩れた物と別の検索の物は断る() {
        let c = cursor(&["actors", "entered", "asc", "2026-11-22T00:00:00.000Z", "7"]);
        let parts = uncursor(&c, "actors", 5).ok().unwrap();
        assert_eq!(parts[3], "2026-11-22T00:00:00.000Z");
        assert!(uncursor(&c, "events", 2).is_err());
        assert!(uncursor("zz", "events", 2).is_err());
        assert!(uncursor("abc", "events", 2).is_err());
        assert!(uncursor("ff", "events", 2).is_err());
    }
}
