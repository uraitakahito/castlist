//! 図（タイムライン・相関図）の点と線の DB の所。重い読みの接続（[`crate::db::HEAVY_READS`] 本）の 1 つの transaction の中で読み、
//! 引き当てなどの軽い読みの接続を取らない。SQL は0.4.x の server（src/db.rs）と同じ決まり:
//!
//! - ラベル（無ければ全部）と、登録を受けた時刻の窓（両端を含む）でアクターの組を絞る。
//! - 組に触れる線（`from` か `to` が組の中）の全部と、その線の向こうの端のアクター（一歩だけ）を足す。
//! - 点も線も受けた順（seq）。点には今の状態と、最新の model と effort を付ける。
use std::collections::{HashMap, HashSet};

use serde_json::Value;
use sqlx::{FromRow, Sqlite, Transaction};

use crate::db::{Actor, Db, Link, Setting, State, inferred_sql};

/// ラベルと時刻の窓で絞ったアクター（受けた順）。
const GRAPH_LABEL: &str = "select a.id from labels l join actors a on a.id = l.actor_id
     where l.key = ?1 and l.value = ?2 and a.received_at >= ?3 and a.received_at <= ?4 order by a.seq";
/// 時刻の窓だけで絞ったアクター（受けた順。窓が無ければ全部なので、LOOKUPS には入れない）。
const GRAPH_WINDOW: &str =
    "select id from actors where received_at >= ?1 and received_at <= ?2 order by seq";
/// アクターの組（JSON の配列）に触れる線（受けた順）。
const GRAPH_LINKS: &str =
    "select seq, id, type, from_actor, to_actor, sent_at, received_at, clock_skew from links
     where from_actor in (select value from json_each(?1))
     union
     select seq, id, type, from_actor, to_actor, sent_at, received_at, clock_skew from links
     where to_actor in (select value from json_each(?1))
     order by seq";
/// 点のアクター（受けた順）と、今の状態と、model と effort の最新（seq で）。
const GRAPH_ACTORS: &str =
    "select id, name, kind, issuer, sent_at, received_at, clock_skew from actors
     where id in (select value from json_each(?1)) order by seq";
const GRAPH_STATE: &str = concat!(
    "select actor_id, presence, present, ",
    inferred_sql!("actor_state.actor_id"),
    " as inferred, entered_at, exited_at from actor_state
     where actor_id in (select value from json_each(?1))"
);
const GRAPH_SETTINGS: &str = "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.sent_at, s.received_at, s.clock_skew
     from json_each(?1)
     join settings s on s.seq = (select s2.seq from settings s2 where s2.actor_id = json_each.value order by s2.seq desc limit 1)
     join models m on m.id = s.model_id join efforts f on f.id = s.effort_id";
/// タイムライン: 点の出来事（帯と印に要る物だけ）と、model と effort の全部（どれも受けた順）。
const TIMELINE_EVENTS: &str = "select actor_id, type, received_at,
       coalesce(type = 'closed' and json_type(detail, '$.inferred') = 'true', 0) as inferred,
       case when json_type(detail, '$.reason') = 'text' then json_extract(detail, '$.reason') end as reason from events
     where actor_id in (select value from json_each(?1)) order by seq";
const TIMELINE_SETTINGS: &str = "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.sent_at, s.received_at, s.clock_skew
     from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id
     where s.actor_id in (select value from json_each(?1)) order by s.seq";

/// 図の SQL（名と SQL）。試験が `explain query plan` で、大きな表を端から読まないかを見る。
pub const LOOKUPS: &[(&str, &str)] = &[
    ("graph_label", GRAPH_LABEL),
    ("graph_links", GRAPH_LINKS),
    ("graph_actors", GRAPH_ACTORS),
    ("graph_state", GRAPH_STATE),
    ("graph_settings", GRAPH_SETTINGS),
    ("timeline_events", TIMELINE_EVENTS),
    ("timeline_settings", TIMELINE_SETTINGS),
];

/// 図の絞り。時刻は DB に残す字（UTC・ミリ秒まで・24 字）。窓が無ければ `""` から `"9999"` まで（全部）。
#[derive(Clone, Debug)]
pub struct GraphQuery {
    /// ラベル（鍵・値）。
    pub label: Option<(String, String)>,
    pub from: String,
    pub to: String,
}

/// 図の点: 登録の欄と、今の状態と、最新の model と effort。
#[derive(Clone, Debug)]
pub struct Node {
    pub actor: Actor,
    pub state: State,
    pub current_settings: Option<Setting>,
}

/// 図の点と線（どれも受けた順）。
#[derive(Clone, Debug)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Link>,
}

/// タイムラインの出来事（帯と印に要る物だけ）。
#[derive(Clone, Debug, FromRow)]
pub struct Moment {
    pub actor_id: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub received_at: String,
    /// 推しの幕引き（`closed` で `detail.inferred` が JSON の true）。
    pub inferred: bool,
    /// `detail.reason`（字の時だけ）。
    pub reason: Option<String>,
}

/// タイムライン: 図の点と線と、点の出来事と model と effort の全部（どれも受けた順）。
#[derive(Clone, Debug)]
pub struct Timeline {
    pub graph: Graph,
    pub events: Vec<Moment>,
    pub settings: Vec<Setting>,
}

/// 点の今の状態と、そのアクターの id。
#[derive(FromRow)]
struct NodeState {
    actor_id: String,
    #[sqlx(flatten)]
    state: State,
}

/// 図の組を引く（読む transaction の中で）。点と線と、点の id の組（JSON の配列の字）を返す。
async fn graph_in(
    tx: &mut Transaction<'_, Sqlite>,
    q: &GraphQuery,
) -> Result<(Graph, String), sqlx::Error> {
    let ids: Vec<String> = match &q.label {
        Some((key, value)) => {
            sqlx::query_scalar(GRAPH_LABEL)
                .bind(key)
                .bind(value)
                .bind(&q.from)
                .bind(&q.to)
                .fetch_all(&mut **tx)
                .await?
        }
        None => {
            sqlx::query_scalar(GRAPH_WINDOW)
                .bind(&q.from)
                .bind(&q.to)
                .fetch_all(&mut **tx)
                .await?
        }
    };
    let edges: Vec<Link> = sqlx::query_as(GRAPH_LINKS)
        .bind(Value::from(ids.clone()).to_string())
        .fetch_all(&mut **tx)
        .await?;
    // 組と、組に触れる線の向こうの端（一歩だけ）。
    let mut all = ids.clone();
    let mut seen: HashSet<String> = ids.into_iter().collect();
    for edge in &edges {
        for end in [&edge.from_actor, &edge.to_actor] {
            if seen.insert(end.clone()) {
                all.push(end.clone());
            }
        }
    }
    let all = Value::from(all).to_string();
    let actors: Vec<Actor> = sqlx::query_as(GRAPH_ACTORS)
        .bind(&all)
        .fetch_all(&mut **tx)
        .await?;
    let mut states: HashMap<String, State> = sqlx::query_as::<_, NodeState>(GRAPH_STATE)
        .bind(&all)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|s| (s.actor_id, s.state))
        .collect();
    let mut settings: HashMap<String, Setting> = sqlx::query_as::<_, Setting>(GRAPH_SETTINGS)
        .bind(&all)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|s| (s.actor_id.clone(), s))
        .collect();
    let nodes = actors
        .into_iter()
        .map(|actor| Node {
            // actor_state は actors のどの行にもある。
            state: states.remove(&actor.id).unwrap_or_default(),
            current_settings: settings.remove(&actor.id),
            actor,
        })
        .collect();
    Ok((Graph { nodes, edges }, all))
}

impl Db {
    /// 図の点と線（1 つの読む transaction の中で、揃った写しを読む）。
    pub async fn graph(&self, q: &GraphQuery) -> Result<Graph, sqlx::Error> {
        let mut tx = self.heavy().begin().await?;
        let (graph, _) = graph_in(&mut tx, q).await?;
        tx.commit().await?;
        Ok(graph)
    }

    /// タイムライン: 図の点と線と同じ組（[`Db::graph`]）と、その点の出来事と model と effort の全部。1 つの読む transaction の中で読む。
    pub async fn timeline(&self, q: &GraphQuery) -> Result<Timeline, sqlx::Error> {
        let mut tx = self.heavy().begin().await?;
        let (graph, all) = graph_in(&mut tx, q).await?;
        let events = sqlx::query_as(TIMELINE_EVENTS)
            .bind(&all)
            .fetch_all(&mut *tx)
            .await?;
        let settings = sqlx::query_as(TIMELINE_SETTINGS)
            .bind(&all)
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Timeline {
            graph,
            events,
            settings,
        })
    }
}
