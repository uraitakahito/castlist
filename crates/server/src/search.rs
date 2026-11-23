//! 監査の検索（ListActors と GetLineage）の DB の所。読む接続の 1 つの transaction の中で、揃った写しを読む。
//!
//! - 時刻の条件は、どれも castlist が受けた時刻（`received_at`）で比べる。問いの時刻は DB に残す字（[`crate::db::stamp`]）に直して渡す。
//!   「最新」は seq の大きい方。
//! - 頁は keyset で分ける（並べる値と seq を後ろから続ける）。辿る間に足されたアクターでは、穴も重なりも出ない。
//!   ただし並べる値（初めの登場・最後の退場）は出来事で動くので、もう出したアクターが辿る間に退場すると、次の頁で出直すことがある。
//! - SQL は決まった欠片を繋ぎ、値は bind するだけ（[`plans`] の試験が、索引で引くことを見る）。今の server（src/search.rs）の SQL と同じ。
use std::collections::{BTreeMap, HashSet};

use serde_json::Value;
use sqlx::{FromRow, QueryBuilder, Sqlite, Transaction};

use crate::db::{Db, Link, Ref, inferred_sql};

/// 1 頁の既定。
pub const LIMIT: i64 = 100;
/// 線を辿る段の上限（上りの既定）。
pub const MAX_DEPTH: u32 = 20;

/// 一覧の 1 行（アクター）。初めの登場・最後の退場・稼働中か・最新の model と effort は、
/// アクター 1 つごとの「order by seq limit 1」で引く（events(actor_id, type, seq)・settings(actor_id, seq)）。
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
       ",
    inferred_sql!("a.id"),
    " as inferred,
       (select m.name from settings s join models m on m.id = s.model_id
        where s.actor_id = a.id order by s.seq desc limit 1) as model,
       (select f.name from settings s join efforts f on f.id = s.effort_id
        where s.actor_id = a.id order by s.seq desc limit 1) as effort
     from actors a where 1"
);
/// 頁のアクターのラベル（JSON の配列の id で）。
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
    /// 並べる値（まだ無ければ ''。昇順で先、降順で後）。
    fn key(self) -> &'static str {
        match self {
            Sort::Entered => "coalesce(first_entered_at, '')",
            Sort::Exited => "coalesce(last_exited_at, '')",
        }
    }
}

/// アクターを探す条件（どれも AND）。時刻は DB に残す字（UTC・ミリ秒まで・24 字）。
#[derive(Clone, Debug, Default)]
pub struct ActorQuery {
    /// ラベル（鍵・値）。どれも持つ。
    pub labels: Vec<(String, String)>,
    /// 付随する id と時刻: その時までに、その値を最後に受けたアクター（出し手と鍵かを問わない）。
    pub binding: Option<(Ref, String)>,
    /// 稼働した期間（初めの登場〜最後の退場。退いていなければ今まで）が、この窓に掛かる（両端を含む）。
    pub active_from: Option<String>,
    pub active_to: Option<String>,
    pub kind: Option<String>,
    /// settings のどれかの行が、この名を持つ。
    pub model: Option<String>,
    pub effort: Option<String>,
    /// 今稼働中か（GetActor の state.present と同じ決め方）。
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

/// 一覧の 1 行。時刻は DB の字。ラベルは別の SQL で引いて入れる。
#[derive(Clone, Debug, FromRow)]
pub struct ActorItem {
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
    /// 今の停止が推しの幕引き（`closed` で `detail.inferred` が JSON の true）か。
    pub inferred: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// 並べる値（無ければ ''）。次の頁の印に入れる。
    pub sort_key: String,
}

/// 1 頁と、次の頁があるか。
pub struct Page {
    pub rows: Vec<ActorItem>,
    pub more: bool,
}

/// 線の上下（点と線）。
pub struct Lineage {
    pub nodes: Vec<ActorItem>,
    pub edges: Vec<Link>,
}

/// 線の行と、受けた順（seq。線を重ねずに受けた順に並べるため）。
#[derive(FromRow)]
struct SeqLink {
    seq: i64,
    #[sqlx(flatten)]
    link: Link,
}

impl ActorQuery {
    /// 索引で引ける条件（ラベル・付随する id・窓・model・effort・起こした者・時刻ずれ・今稼働中・id の組）があるか。
    /// これが無いと、アクターの表を端から読むことになる（kind と active=false は数えない）。
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
        if let Some((binding, at)) = &self.binding {
            q.push(" and a.id = (select b.actor_id from bindings b where b.type = ")
                .push_bind(binding.kind.clone())
                .push(" and b.value = ")
                .push_bind(binding.value.clone())
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

/// 検索の SQL（名と SQL）。試験が `explain query plan` で、大きな表を端から読まないかを見る。
/// 条件を 1 つずつと、全部を重ねた物。値は bind するので、ここでは仮の値を入れる。
pub fn plans() -> Vec<(&'static str, String)> {
    fn x() -> Option<String> {
        Some("x".to_owned())
    }
    fn label(q: &mut ActorQuery) {
        q.labels = vec![("k".into(), "v".into())];
    }
    fn binding() -> Option<(Ref, String)> {
        Some((Ref::parse("t:v"), "at".into()))
    }
    let actors = |f: fn(&mut ActorQuery)| {
        let mut q = ActorQuery {
            limit: Some(LIMIT),
            ..ActorQuery::default()
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
        ("actors_binding", actors(|q| q.binding = binding())),
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
                    binding: binding(),
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
    ]
}

/// 条件に合うアクターの 1 頁を、同じ transaction の中で読み、ラベルを付ける。
async fn actor_page(tx: &mut Transaction<'_, Sqlite>, q: &ActorQuery) -> Result<Page, sqlx::Error> {
    let mut builder = q.builder();
    let mut rows: Vec<ActorItem> = builder.build_query_as().fetch_all(&mut **tx).await?;
    let limit = q.limit.and_then(|l| usize::try_from(l).ok());
    let more = limit.is_some_and(|l| rows.len() > l);
    if let Some(l) = limit {
        rows.truncate(l);
    }
    let ids = Value::from(rows.iter().map(|a| a.id.clone()).collect::<Vec<_>>()).to_string();
    let labels: Vec<(String, String, String)> = sqlx::query_as(ACTOR_LABELS)
        .bind(ids)
        .fetch_all(&mut **tx)
        .await?;
    let mut by_actor: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (actor, key, value) in labels {
        by_actor.entry(actor).or_default().insert(key, value);
    }
    for item in &mut rows {
        item.labels = by_actor.remove(&item.id).unwrap_or_default();
    }
    Ok(Page { rows, more })
}

impl Db {
    /// アクターを探す（1 つの読む transaction の中で、揃った写しを読む）。
    pub async fn search_actors(&self, q: &ActorQuery) -> Result<Page, sqlx::Error> {
        let mut tx = self.read().begin().await?;
        let page = actor_page(&mut tx, q).await?;
        tx.commit().await?;
        Ok(page)
    }

    /// 線を上り（人で止める。人は含む）、下る（幅から先に、重ねずに）。アクターが無ければ None。
    /// 点は一覧の形を初めの登場の順（同じなら受けた順）に、線は受けた順に返す。1 つの読む transaction の中で読む。
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
                    let links: Vec<SeqLink> = sqlx::query_as(sql)
                        .bind(node)
                        .bind(kind)
                        .fetch_all(&mut *tx)
                        .await?;
                    for SeqLink { seq, link } in links {
                        let end = if upward {
                            &link.from_actor
                        } else {
                            &link.to_actor
                        };
                        if seen.insert(end.clone()) {
                            order.push(end.clone());
                            next.push(end.clone());
                        }
                        edges.insert(seq, link);
                    }
                }
                if next.is_empty() {
                    break;
                }
                frontier = next;
            }
        }
        let nodes = actor_page(
            &mut tx,
            &ActorQuery {
                ids: Some(Value::from(order).to_string()),
                ..ActorQuery::default()
            },
        )
        .await?
        .rows;
        tx.commit().await?;
        Ok(Some(Lineage {
            nodes,
            edges: edges.into_values().collect(),
        }))
    }
}
