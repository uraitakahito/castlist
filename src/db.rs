//! 接続・移行・SQL。SQL は動く時に組む形（`query`・`query_as`）で書く。
//!
//! - 書く接続は 1 本だけ。番号（n）を数える所と、同じ物かを見る所が 1 列に並ぶ。
//! - 読む接続は 4 本で、読むだけ。WAL なので、書いている間も待たずに読める。
//! - どの表も足すだけ。この file の SQL は insert と select だけで、`ci/check-sql.sh` が見張る。
//!   「最新」は seq だけで決め、出し手の時刻（at）では決めない。

use serde::Serialize;
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{FromRow, Row, Sqlite, SqlitePool, Transaction};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// 書く接続を待つ長さ。過ぎれば「混んでいる」で返す。
pub const WRITE_WAIT: Duration = Duration::from_secs(2);
/// 重い読み（相関図・時間の帯）だけが使う、読むだけの接続の本数。軽い読みの 4 本とは別にして、重い読みが
/// 引き当てなどの軽い読みの接続を取らないようにする。
pub const HEAVY_READS: u32 = 2;
/// 出し手の時刻と castlist の時計が、これより離れていれば clock_skew を立てる。
pub const SKEW: time::Duration = time::Duration::minutes(5);

// --- 引き当てと見る SQL（試験が LOOKUPS で、索引を使うかを見る） ---

const RESOLVE: &str =
    "select a.id, a.name, a.kind from bindings b join actors a on a.id = b.actor_id
     where b.issuer = ?1 and b.type = ?2 and b.value = ?3 and b.is_key = 1";
const ACTOR: &str =
    "select id, name, kind, issuer, at, received_at, clock_skew from actors where id = ?1";
const LABELS: &str = "select key, value from labels where actor_id = ?1 order by key";
const BINDINGS: &str = "select issuer, type, value, is_key, at, received_at from bindings
     where actor_id = ?1 order by seq";
const EVENTS: &str = "select id, actor_id, type, detail, at, received_at, clock_skew from events
     where actor_id = ?1 order by seq";
const SETTINGS: &str = "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.at, s.received_at, s.clock_skew
     from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id
     where s.actor_id = ?1 order by s.seq";
const LINKS_IN: &str =
    "select seq, id, type, from_actor, to_actor, at, received_at, clock_skew from links
     where to_actor = ?1 order by seq";
const LINKS_OUT: &str =
    "select seq, id, type, from_actor, to_actor, at, received_at, clock_skew from links
     where from_actor = ?1 order by seq";
const STATE: &str = "select actor_id, presence, present, entered_at, exited_at from actor_state where actor_id = ?1";
const CURRENT_SETTINGS: &str = "select actor_id, model, effort, source, at, received_at
     from actor_current_settings where actor_id = ?1";
const CURRENT_BINDINGS: &str = "select issuer, type, value, is_key, at, received_at
     from actor_current_bindings where actor_id = ?1 order by type";
/// その時（received_at がそれより前）に、その付随する id を持っていたアクター。
const BINDING_AT: &str = "select a.id, a.name, a.kind from actors a where a.id = (
       select b.actor_id from bindings b
       where b.type = ?1 and b.value = ?2 and b.received_at <= ?3
       order by b.received_at desc, b.seq desc limit 1)";
const LABEL: &str = "select a.id, a.name, a.kind from labels l join actors a on a.id = l.actor_id
     where l.key = ?1 and l.value = ?2 order by a.seq";
/// 相関図: ラベルと時刻の窓で絞ったアクター。
const GRAPH_LABEL: &str = "select a.id from labels l join actors a on a.id = l.actor_id
     where l.key = ?1 and l.value = ?2 and a.received_at >= ?3 and a.received_at <= ?4 order by a.seq";
const GRAPH_WINDOW: &str =
    "select id from actors where received_at >= ?1 and received_at <= ?2 order by seq";
/// 相関図: アクターの組（JSON の配列）に触れる線と、その組の中身。
const GRAPH_LINKS: &str =
    "select seq, id, type, from_actor, to_actor, at, received_at, clock_skew from links
     where from_actor in (select value from json_each(?1))
     union
     select seq, id, type, from_actor, to_actor, at, received_at, clock_skew from links
     where to_actor in (select value from json_each(?1))
     order by seq";
const GRAPH_ACTORS: &str = "select id, name, kind, issuer, at, received_at, clock_skew from actors
     where id in (select value from json_each(?1)) order by seq";
const GRAPH_STATE: &str =
    "select actor_id, presence, present, entered_at, exited_at from actor_state
     where actor_id in (select value from json_each(?1))";
const GRAPH_SETTINGS: &str = "select actor_id, model, effort, source, at, received_at
     from actor_current_settings where actor_id in (select value from json_each(?1))";
/// 時間の帯: アクターの組の、出来事と model と effort の全部（seq の順）。
const TIMELINE_EVENTS: &str = "select actor_id, type, received_at from events
     where actor_id in (select value from json_each(?1)) order by seq";
const TIMELINE_SETTINGS: &str = "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.at, s.received_at, s.clock_skew
     from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id
     where s.actor_id in (select value from json_each(?1)) order by s.seq";

/// 引き当てと見る SQL（名と SQL）。試験が `explain query plan` で、大きな表を端から読まないかを見る。
pub const LOOKUPS: &[(&str, &str)] = &[
    ("resolve", RESOLVE),
    ("actor", ACTOR),
    ("labels", LABELS),
    ("bindings", BINDINGS),
    ("events", EVENTS),
    ("settings", SETTINGS),
    ("links_in", LINKS_IN),
    ("links_out", LINKS_OUT),
    ("state", STATE),
    ("current_settings", CURRENT_SETTINGS),
    ("current_bindings", CURRENT_BINDINGS),
    ("binding_at", BINDING_AT),
    ("label", LABEL),
    ("graph_label", GRAPH_LABEL),
    ("graph_links", GRAPH_LINKS),
    ("graph_actors", GRAPH_ACTORS),
    ("graph_state", GRAPH_STATE),
    ("graph_settings", GRAPH_SETTINGS),
    ("timeline_events", TIMELINE_EVENTS),
    ("timeline_settings", TIMELINE_SETTINGS),
];

/// どの表の最新の received_at より前にならないよう、表ごとの最新（seq の大きい方）を見る。
const CLOCK: &str = "select max(t) from (
       select (select received_at from actors order by seq desc limit 1) as t
       union all select (select received_at from labels order by seq desc limit 1)
       union all select (select received_at from bindings order by seq desc limit 1)
       union all select (select received_at from links order by seq desc limit 1)
       union all select (select received_at from events order by seq desc limit 1)
       union all select (select received_at from settings order by seq desc limit 1))";

// --- 型 ---

#[derive(Clone)]
pub struct Db {
    read: SqlitePool,
    /// 重い読みだけが使う、読むだけの接続（[`HEAVY_READS`] 本）。
    heavy: SqlitePool,
    write: SqlitePool,
    path: PathBuf,
}

/// アクターを指す時の短い形。
#[derive(Clone, Debug, Serialize, FromRow, PartialEq, Eq)]
pub struct ActorRef {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Actor {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub issuer: String,
    pub at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Binding {
    pub issuer: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub kind: String,
    pub value: String,
    pub is_key: bool,
    pub at: String,
    pub received_at: String,
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub id: String,
    pub actor_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub detail: Value,
    pub at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

impl FromRow<'_, SqliteRow> for Event {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let detail: String = row.try_get("detail")?;
        Ok(Self {
            id: row.try_get("id")?,
            actor_id: row.try_get("actor_id")?,
            kind: row.try_get("type")?,
            detail: serde_json::from_str(&detail).map_err(|e| sqlx::Error::Decode(e.into()))?,
            at: row.try_get("at")?,
            received_at: row.try_get("received_at")?,
            clock_skew: row.try_get("clock_skew")?,
        })
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct Setting {
    pub id: Option<String>,
    pub actor_id: String,
    pub model: String,
    pub effort: String,
    pub source: String,
    pub at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CurrentSettings {
    #[serde(skip)]
    pub actor_id: String,
    pub model: String,
    pub effort: String,
    pub source: String,
    pub at: String,
    pub received_at: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Link {
    #[serde(skip)]
    pub seq: i64,
    pub id: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub kind: String,
    #[serde(rename = "from")]
    pub from_actor: String,
    #[serde(rename = "to")]
    pub to_actor: String,
    pub at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

/// 居るか（view の actor_state）。
#[derive(Debug, Serialize, FromRow)]
pub struct State {
    #[serde(skip)]
    pub actor_id: String,
    /// 居る・居ないを替える出来事の最新の種類（まだ無ければ null）。
    pub presence: Option<String>,
    pub present: bool,
    /// 初めの登場の received_at。
    pub entered_at: Option<String>,
    /// 最後の退場（cleared・exited・closed）の received_at。
    pub exited_at: Option<String>,
}

/// 1 つのアクターの全部。
#[derive(Debug, Serialize)]
pub struct ActorDetail {
    #[serde(flatten)]
    pub actor: Actor,
    pub labels: std::collections::BTreeMap<String, String>,
    pub bindings: Vec<Binding>,
    pub current_bindings: Vec<Binding>,
    pub events: Vec<Event>,
    pub settings: Vec<Setting>,
    pub current_settings: Option<CurrentSettings>,
    pub links_in: Vec<Link>,
    pub links_out: Vec<Link>,
    pub state: State,
}

/// 相関図の点。
#[derive(Debug, Serialize)]
pub struct Node {
    #[serde(flatten)]
    pub actor: Actor,
    pub state: Option<State>,
    pub current_settings: Option<CurrentSettings>,
}

#[derive(Debug, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Link>,
}

/// 時間の帯の出来事（帯と印に要る物だけ）。
#[derive(Debug, FromRow)]
pub struct Moment {
    pub actor_id: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub received_at: String,
}

/// 時間の帯: 相関図の点と線と、点の出来事と model と effort の全部（どれも seq の順）。
#[derive(Debug)]
pub struct Timeline {
    pub graph: Graph,
    pub events: Vec<Moment>,
    pub settings: Vec<Setting>,
}

/// 付随する id 1 つ（`type:value`）。
#[derive(Clone, Debug)]
pub struct Ref {
    pub kind: String,
    pub value: String,
}

pub struct NewActor<'a> {
    pub issuer: &'a str,
    pub kind: &'a str,
    pub prefix: &'a str,
    pub labels: &'a [(String, String)],
    /// 初めの物が鍵。
    pub bindings: &'a [Ref],
    pub at: &'a str,
    pub at_time: OffsetDateTime,
    /// 頼んだ model と effort。
    pub settings: Option<(&'a str, &'a str)>,
}

pub struct NewEvent<'a> {
    pub issuer: &'a str,
    pub by: &'a Ref,
    pub kind: &'a str,
    pub detail: &'a str,
    pub at: &'a str,
    pub at_time: OffsetDateTime,
    pub body_hash: &'a str,
}

pub struct NewLink<'a> {
    pub issuer: &'a str,
    pub kind: &'a str,
    pub from: &'a Ref,
    pub to: &'a Ref,
    pub at: &'a str,
    pub at_time: OffsetDateTime,
    pub body_hash: &'a str,
}

pub struct NewSetting<'a> {
    pub issuer: &'a str,
    pub by: &'a Ref,
    pub model: &'a str,
    pub effort: &'a str,
    pub source: &'a str,
    pub at: &'a str,
    pub at_time: OffsetDateTime,
    pub body_hash: &'a str,
}

/// 書いた結果。
pub enum Put<T> {
    /// 新しく足した。
    Created(T),
    /// 同じ物が既にあった（足していない）。
    Same(T),
}

/// 書けない訳。
#[derive(Debug)]
pub enum WriteError {
    /// 書く接続が空かない（混んでいる）。
    Busy,
    /// 指すアクターが無いなど（422）。
    Invalid(String),
    /// 同じ id で違う中身など（409）。
    Conflict(String),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for WriteError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::PoolTimedOut => WriteError::Busy,
            e => WriteError::Db(e),
        }
    }
}

/// DB を開けない訳。
#[derive(Debug)]
pub enum OpenError {
    Db(sqlx::Error),
    Migrate(sqlx::migrate::MigrateError),
    /// 外のキーの崩れた行がある（表と rowid と、指す先の表）。
    ForeignKey(Vec<String>),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Db(e) => write!(f, "DB を開けない: {e}"),
            OpenError::Migrate(e) => write!(f, "DB の移行に落ちた: {e}"),
            OpenError::ForeignKey(rows) => write!(
                f,
                "外のキーの崩れた行があるので起きない: {}",
                rows.join("・")
            ),
        }
    }
}

impl std::error::Error for OpenError {}

// --- 時刻 ---

/// castlist の時計の字（UTC・ミリ秒まで・いつも 24 字なので、字の順が時刻の順になる）。
pub fn stamp(t: OffsetDateTime) -> String {
    let t = t.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        t.year().clamp(0, 9999),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        t.millisecond()
    )
}

/// 今の castlist の時刻。
pub fn now() -> String {
    stamp(OffsetDateTime::now_utc())
}

/// 書く transaction の中で、受けた時刻を決める: 今と、どの表の最新の received_at の、遅い方。
async fn received_at(tx: &mut Transaction<'_, Sqlite>) -> Result<String, sqlx::Error> {
    let latest: Option<String> = sqlx::query_scalar(CLOCK).fetch_one(&mut **tx).await?;
    let now = now();
    Ok(match latest {
        Some(latest) if latest > now => latest,
        _ => now,
    })
}

/// 出し手の時刻が、受けた時刻から 5 分より離れているか。
fn skewed(at: OffsetDateTime, received: &str) -> bool {
    OffsetDateTime::parse(received, &Rfc3339).is_ok_and(|r| (r - at).abs() > SKEW)
}

// --- 開く ---

impl Db {
    /// 開き、移行し、外のキーの崩れが無いかを見る。崩れがあれば開かない。
    pub async fn open(path: &Path) -> Result<Self, OpenError> {
        // どの接続にも: 外のキーを効かせ、置き換え（REPLACE）が行を消す時も trigger を動かす。
        // 一時の物は memory に置く（動かす image には /tmp が無い）。
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .pragma("recursive_triggers", "ON")
            .pragma("temp_store", "MEMORY");
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(WRITE_WAIT)
            .connect_with(options.clone())
            .await
            .map_err(OpenError::Db)?;
        sqlx::migrate!()
            .run(&write)
            .await
            .map_err(OpenError::Migrate)?;
        let broken: Vec<(String, Option<i64>, String)> =
            sqlx::query_as("select \"table\", rowid, parent from pragma_foreign_key_check")
                .fetch_all(&write)
                .await
                .map_err(OpenError::Db)?;
        if !broken.is_empty() {
            write.close().await;
            return Err(OpenError::ForeignKey(
                broken
                    .into_iter()
                    .map(|(table, rowid, parent)| {
                        let rowid = rowid.map_or("-".to_owned(), |r| r.to_string());
                        format!("{table} の rowid {rowid}（{parent} を指す）")
                    })
                    .collect(),
            ));
        }
        let heavy = SqlitePoolOptions::new()
            .max_connections(HEAVY_READS)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options.clone().read_only(true))
            .await
            .map_err(OpenError::Db)?;
        let read = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options.read_only(true))
            .await
            .map_err(OpenError::Db)?;
        Ok(Self {
            read,
            heavy,
            write,
            path: path.to_owned(),
        })
    }

    /// 重い読みだけが使う接続（[`HEAVY_READS`] 本・読むだけ）。
    pub fn heavy(&self) -> &SqlitePool {
        &self.heavy
    }

    /// 読む接続（4 本・読むだけ）。
    pub fn read(&self) -> &SqlitePool {
        &self.read
    }

    /// 書く接続（1 本だけ）。
    pub fn write(&self) -> &SqlitePool {
        &self.write
    }

    pub async fn close(&self) {
        self.read.close().await;
        self.heavy.close().await;
        self.write.close().await;
    }
}

// --- 書く ---

/// model・effort の名の番号。知らない名なら足す（同じ transaction の中で。2 度目は足さない）。
async fn master(
    tx: &mut Transaction<'_, Sqlite>,
    table: Master,
    name: &str,
    received: &str,
) -> Result<i64, sqlx::Error> {
    let (insert, select) = match table {
        Master::Models => (
            "insert into models(name, first_seen_at) values(?1, ?2) on conflict(name) do nothing",
            "select id from models where name = ?1",
        ),
        Master::Efforts => (
            "insert into efforts(name, first_seen_at) values(?1, ?2) on conflict(name) do nothing",
            "select id from efforts where name = ?1",
        ),
    };
    sqlx::query(insert)
        .bind(name)
        .bind(received)
        .execute(&mut **tx)
        .await?;
    sqlx::query_scalar(select)
        .bind(name)
        .fetch_one(&mut **tx)
        .await
}

#[derive(Clone, Copy)]
enum Master {
    Models,
    Efforts,
}

/// 鍵の付随する id から、アクターを引く。
async fn resolve_in<'e, E>(
    executor: E,
    issuer: &str,
    r: &Ref,
) -> Result<Option<ActorRef>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(RESOLVE)
        .bind(issuer)
        .bind(&r.kind)
        .bind(&r.value)
        .fetch_optional(executor)
        .await
}

async fn must_resolve(
    tx: &mut Transaction<'_, Sqlite>,
    issuer: &str,
    r: &Ref,
    role: &str,
) -> Result<ActorRef, WriteError> {
    resolve_in(&mut **tx, issuer, r).await?.ok_or_else(|| {
        WriteError::Invalid(format!(
            "{role} のアクター（{}:{}）を知らない",
            r.kind, r.value
        ))
    })
}

impl Db {
    /// アクターを登録する。鍵の付随する id が既にあれば、そのアクターを返す（created = false）。
    /// その時は、変わった鍵でない付随する id と、変わった頼んだ model と effort だけを足す。
    pub async fn register(&self, new: &NewActor<'_>) -> Result<(ActorRef, bool), WriteError> {
        let mut tx = self.write.begin().await?;
        let received = received_at(&mut tx).await?;
        let skew = i64::from(skewed(new.at_time, &received));
        let (key, rest) = new
            .bindings
            .split_first()
            .ok_or_else(|| WriteError::Invalid("bindings が空".to_owned()))?;
        let (actor, created) = match resolve_in(&mut *tx, new.issuer, key).await? {
            Some(actor) => (actor, false),
            None => {
                // n は、同じ出し手と接頭辞のアクターの数 ＋ 1（数を持つ行は作らない）。
                let n: i64 = sqlx::query_scalar(
                    "select count(*) + 1 from actors where issuer = ?1 and prefix = ?2",
                )
                .bind(new.issuer)
                .bind(new.prefix)
                .fetch_one(&mut *tx)
                .await?;
                let actor = ActorRef {
                    id: uuid::Uuid::now_v7().to_string(),
                    name: format!("{}.{n}", new.prefix),
                    kind: new.kind.to_owned(),
                };
                sqlx::query(
                    "insert into actors(id, issuer, kind, prefix, n, name, at, received_at, clock_skew)
                     values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )
                .bind(&actor.id)
                .bind(new.issuer)
                .bind(new.kind)
                .bind(new.prefix)
                .bind(n)
                .bind(&actor.name)
                .bind(new.at)
                .bind(&received)
                .bind(skew)
                .execute(&mut *tx)
                .await?;
                for (key, value) in new.labels {
                    sqlx::query(
                        "insert into labels(actor_id, key, value, received_at) values(?1, ?2, ?3, ?4)",
                    )
                    .bind(&actor.id)
                    .bind(key)
                    .bind(value)
                    .bind(&received)
                    .execute(&mut *tx)
                    .await?;
                }
                insert_binding(&mut tx, &actor.id, new.issuer, key, true, new.at, &received)
                    .await?;
                (actor, true)
            }
        };
        for binding in rest {
            let latest: Option<String> = sqlx::query_scalar(
                "select value from bindings where actor_id = ?1 and type = ?2 order by seq desc limit 1",
            )
            .bind(&actor.id)
            .bind(&binding.kind)
            .fetch_optional(&mut *tx)
            .await?;
            if latest.as_deref() != Some(binding.value.as_str()) {
                insert_binding(
                    &mut tx, &actor.id, new.issuer, binding, false, new.at, &received,
                )
                .await?;
            }
        }
        if let Some((model, effort)) = new.settings {
            let model_id = master(&mut tx, Master::Models, model, &received).await?;
            let effort_id = master(&mut tx, Master::Efforts, effort, &received).await?;
            if latest_setting(&mut tx, &actor.id).await? != Some((model_id, effort_id)) {
                sqlx::query(
                    "insert into settings(actor_id, model_id, effort_id, source, at, received_at, clock_skew)
                     values(?1, ?2, ?3, 'requested', ?4, ?5, ?6)",
                )
                .bind(&actor.id)
                .bind(model_id)
                .bind(effort_id)
                .bind(new.at)
                .bind(&received)
                .bind(skew)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok((actor, created))
    }

    /// 出来事を足す。同じ id で同じ中身なら Same、違えば Conflict。
    pub async fn put_event(&self, id: &str, new: &NewEvent<'_>) -> Result<Put<Event>, WriteError> {
        let mut tx = self.write.begin().await?;
        let select = "select id, actor_id, type, detail, at, received_at, clock_skew from events where id = ?1";
        if let Some(hash) =
            body_hash(&mut tx, "select body_hash from events where id = ?1", id).await?
        {
            return same_or_conflict(&mut tx, hash, new.body_hash, select, id, "出来事").await;
        }
        let actor = must_resolve(&mut tx, new.issuer, new.by, "by").await?;
        let received = received_at(&mut tx).await?;
        sqlx::query(
            "insert into events(id, actor_id, type, detail, at, received_at, clock_skew, body_hash)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(id)
        .bind(&actor.id)
        .bind(new.kind)
        .bind(new.detail)
        .bind(new.at)
        .bind(&received)
        .bind(skewed(new.at_time, &received))
        .bind(new.body_hash)
        .execute(&mut *tx)
        .await?;
        let event = sqlx::query_as(select).bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Put::Created(event))
    }

    /// 線を足す。起こされる（launched）のは、1 つのアクターに 1 度だけ。
    pub async fn put_link(&self, id: &str, new: &NewLink<'_>) -> Result<Put<Link>, WriteError> {
        let mut tx = self.write.begin().await?;
        let select = "select seq, id, type, from_actor, to_actor, at, received_at, clock_skew from links where id = ?1";
        if let Some(hash) =
            body_hash(&mut tx, "select body_hash from links where id = ?1", id).await?
        {
            return same_or_conflict(&mut tx, hash, new.body_hash, select, id, "線").await;
        }
        let from = must_resolve(&mut tx, new.issuer, new.from, "from").await?;
        let to = must_resolve(&mut tx, new.issuer, new.to, "to").await?;
        if from.id == to.id {
            return Err(WriteError::Invalid("from と to が同じアクター".to_owned()));
        }
        let received = received_at(&mut tx).await?;
        let inserted = sqlx::query(
            "insert into links(id, type, from_actor, to_actor, at, received_at, clock_skew, body_hash)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(id)
        .bind(new.kind)
        .bind(&from.id)
        .bind(&to.id)
        .bind(new.at)
        .bind(&received)
        .bind(skewed(new.at_time, &received))
        .bind(new.body_hash)
        .execute(&mut *tx)
        .await;
        match inserted {
            // id は上で見たので、重なるのは「起こされるのは 1 度だけ」の索引。
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                return Err(WriteError::Conflict(format!(
                    "{} は、もう別の線で起こされている",
                    to.name
                )));
            }
            other => other?,
        };
        let link = sqlx::query_as(select).bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Put::Created(link))
    }

    /// model と effort を足す。同じ id で同じ中身、またはそのアクターの最新と同じなら、足さずに Same。
    pub async fn put_setting(
        &self,
        id: &str,
        new: &NewSetting<'_>,
    ) -> Result<Put<Setting>, WriteError> {
        let mut tx = self.write.begin().await?;
        let select = "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.at, s.received_at, s.clock_skew
             from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id
             where s.id = ?1";
        let stored: Option<Option<String>> =
            sqlx::query_scalar("select body_hash from settings where id = ?1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(hash) = stored {
            return same_or_conflict(
                &mut tx,
                hash.unwrap_or_default(),
                new.body_hash,
                select,
                id,
                "model と effort",
            )
            .await;
        }
        let actor = must_resolve(&mut tx, new.issuer, new.by, "by").await?;
        let received = received_at(&mut tx).await?;
        let model_id = master(&mut tx, Master::Models, new.model, &received).await?;
        let effort_id = master(&mut tx, Master::Efforts, new.effort, &received).await?;
        if latest_setting(&mut tx, &actor.id).await? == Some((model_id, effort_id)) {
            let latest = sqlx::query_as(
                "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.at, s.received_at, s.clock_skew
                 from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id
                 where s.actor_id = ?1 order by s.seq desc limit 1",
            )
            .bind(&actor.id)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(Put::Same(latest));
        }
        sqlx::query(
            "insert into settings(id, actor_id, model_id, effort_id, source, at, received_at, clock_skew, body_hash)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(id)
        .bind(&actor.id)
        .bind(model_id)
        .bind(effort_id)
        .bind(new.source)
        .bind(new.at)
        .bind(&received)
        .bind(skewed(new.at_time, &received))
        .bind(new.body_hash)
        .execute(&mut *tx)
        .await?;
        let setting = sqlx::query_as(select).bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Put::Created(setting))
    }
}

async fn insert_binding(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
    issuer: &str,
    binding: &Ref,
    is_key: bool,
    at: &str,
    received: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "insert into bindings(actor_id, issuer, type, value, is_key, at, received_at)
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(actor_id)
    .bind(issuer)
    .bind(&binding.kind)
    .bind(&binding.value)
    .bind(is_key)
    .bind(at)
    .bind(received)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 相関図の組を引く（読みの transaction の中で）。点と線と、点の id の組（JSON の配列の字）を返す。
async fn graph_in(
    tx: &mut Transaction<'_, Sqlite>,
    label: Option<(&str, &str)>,
    from: &str,
    to: &str,
) -> Result<(Graph, String), sqlx::Error> {
    let ids: Vec<String> = match label {
        Some((key, value)) => {
            sqlx::query_scalar(GRAPH_LABEL)
                .bind(key)
                .bind(value)
                .bind(from)
                .bind(to)
                .fetch_all(&mut **tx)
                .await?
        }
        None => {
            sqlx::query_scalar(GRAPH_WINDOW)
                .bind(from)
                .bind(to)
                .fetch_all(&mut **tx)
                .await?
        }
    };
    let json = |ids: &[String]| Value::from(ids.to_vec()).to_string();
    let edges: Vec<Link> = sqlx::query_as(GRAPH_LINKS)
        .bind(json(&ids))
        .fetch_all(&mut **tx)
        .await?;
    let mut all = ids.clone();
    let mut seen: std::collections::HashSet<String> = ids.into_iter().collect();
    for edge in &edges {
        for end in [&edge.from_actor, &edge.to_actor] {
            if seen.insert(end.clone()) {
                all.push(end.clone());
            }
        }
    }
    let all = json(&all);
    let actors: Vec<Actor> = sqlx::query_as(GRAPH_ACTORS)
        .bind(&all)
        .fetch_all(&mut **tx)
        .await?;
    let mut states: std::collections::HashMap<String, State> =
        sqlx::query_as::<_, State>(GRAPH_STATE)
            .bind(&all)
            .fetch_all(&mut **tx)
            .await?
            .into_iter()
            .map(|s| (s.actor_id.clone(), s))
            .collect();
    let mut settings: std::collections::HashMap<String, CurrentSettings> =
        sqlx::query_as::<_, CurrentSettings>(GRAPH_SETTINGS)
            .bind(&all)
            .fetch_all(&mut **tx)
            .await?
            .into_iter()
            .map(|s| (s.actor_id.clone(), s))
            .collect();
    let nodes = actors
        .into_iter()
        .map(|actor| Node {
            state: states.remove(&actor.id),
            current_settings: settings.remove(&actor.id),
            actor,
        })
        .collect();
    Ok((Graph { nodes, edges }, all))
}

/// そのアクターの最新の model と effort の番号。
async fn latest_setting(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
) -> Result<Option<(i64, i64)>, sqlx::Error> {
    sqlx::query_as(
        "select model_id, effort_id from settings where actor_id = ?1 order by seq desc limit 1",
    )
    .bind(actor_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn body_hash(
    tx: &mut Transaction<'_, Sqlite>,
    sql: &'static str,
    id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
}

/// 同じ id の行が既にある時: 中身の写しが同じなら Same（その行を返す）、違えば Conflict（行はそのまま）。
async fn same_or_conflict<T>(
    tx: &mut Transaction<'_, Sqlite>,
    stored: String,
    sent: &str,
    select: &'static str,
    id: &str,
    what: &str,
) -> Result<Put<T>, WriteError>
where
    T: for<'r> FromRow<'r, SqliteRow> + Send + Unpin,
{
    if stored != sent {
        return Err(WriteError::Conflict(format!(
            "同じ id（{id}）の{what}が、違う中身で既にある"
        )));
    }
    let row = sqlx::query_as(select).bind(id).fetch_one(&mut **tx).await?;
    Ok(Put::Same(row))
}

// --- 読む ---

impl Db {
    /// 鍵の付随する id から、アクターを引く（無ければ None）。
    pub async fn resolve(
        &self,
        issuer: &str,
        refs: &[Ref],
    ) -> Result<Vec<Option<ActorRef>>, sqlx::Error> {
        let mut conn = self.read.acquire().await?;
        let mut found = Vec::with_capacity(refs.len());
        for r in refs {
            found.push(resolve_in(&mut *conn, issuer, r).await?);
        }
        Ok(found)
    }

    /// 1 つのアクターの全部（1 つの読む transaction の中で、揃った写しを読む）。
    pub async fn actor(&self, id: &str) -> Result<Option<ActorDetail>, sqlx::Error> {
        let mut tx = self.read.begin().await?;
        let Some(actor) = sqlx::query_as::<_, Actor>(ACTOR)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
        else {
            return Ok(None);
        };
        let labels: Vec<(String, String)> =
            sqlx::query_as(LABELS).bind(id).fetch_all(&mut *tx).await?;
        let detail = ActorDetail {
            labels: labels.into_iter().collect(),
            bindings: sqlx::query_as(BINDINGS)
                .bind(id)
                .fetch_all(&mut *tx)
                .await?,
            current_bindings: sqlx::query_as(CURRENT_BINDINGS)
                .bind(id)
                .fetch_all(&mut *tx)
                .await?,
            events: sqlx::query_as(EVENTS).bind(id).fetch_all(&mut *tx).await?,
            settings: sqlx::query_as(SETTINGS)
                .bind(id)
                .fetch_all(&mut *tx)
                .await?,
            current_settings: sqlx::query_as(CURRENT_SETTINGS)
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?,
            links_in: sqlx::query_as(LINKS_IN)
                .bind(id)
                .fetch_all(&mut *tx)
                .await?,
            links_out: sqlx::query_as(LINKS_OUT)
                .bind(id)
                .fetch_all(&mut *tx)
                .await?,
            state: sqlx::query_as(STATE).bind(id).fetch_one(&mut *tx).await?,
            actor,
        };
        tx.commit().await?;
        Ok(Some(detail))
    }

    /// その時（received_at が `at` 以前）に、その付随する id を持っていたアクター。
    pub async fn actor_at(&self, r: &Ref, at: &str) -> Result<Option<ActorRef>, sqlx::Error> {
        sqlx::query_as(BINDING_AT)
            .bind(&r.kind)
            .bind(&r.value)
            .bind(at)
            .fetch_optional(&self.read)
            .await
    }

    /// そのラベルを持つアクター（登録の順）。
    pub async fn actors_with_label(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<ActorRef>, sqlx::Error> {
        sqlx::query_as(LABEL)
            .bind(key)
            .bind(value)
            .fetch_all(&self.read)
            .await
    }

    /// 相関図。ラベル（無ければ全部）と、登録の時刻の窓で絞る。絞った組から出る線の先のアクターも、一歩だけ含める。
    pub async fn graph(
        &self,
        label: Option<(&str, &str)>,
        from: &str,
        to: &str,
    ) -> Result<Graph, sqlx::Error> {
        let mut tx = self.heavy.begin().await?;
        let (graph, _) = graph_in(&mut tx, label, from, to).await?;
        tx.commit().await?;
        Ok(graph)
    }

    /// 時間の帯: 相関図と同じ組（[`Db::graph`]）と、その点の出来事と model と effort の全部。1 つの読みの中で引く。
    pub async fn timeline(
        &self,
        label: Option<(&str, &str)>,
        from: &str,
        to: &str,
    ) -> Result<Timeline, sqlx::Error> {
        let mut tx = self.heavy.begin().await?;
        let (graph, all) = graph_in(&mut tx, label, from, to).await?;
        let events: Vec<Moment> = sqlx::query_as(TIMELINE_EVENTS)
            .bind(&all)
            .fetch_all(&mut *tx)
            .await?;
        let settings: Vec<Setting> = sqlx::query_as(TIMELINE_SETTINGS)
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

    /// 動かしたまま、DB の揃った写しを 1 つの file に作る（DB の隣に置く。呼んだ側が読み終えたら消す）。
    /// 読む接続で `vacuum into` する（書く接続を塞がない）。
    pub async fn backup(&self) -> Result<PathBuf, sqlx::Error> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let mut name = self.path.clone().into_os_string();
        name.push(format!(
            ".backup-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let out = PathBuf::from(name);
        let _ = tokio::fs::remove_file(&out).await;
        sqlx::query("vacuum into ?1")
            .bind(out.to_string_lossy().into_owned())
            .execute(&self.read)
            .await?;
        Ok(out)
    }
}
