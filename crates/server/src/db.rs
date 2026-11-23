//! 接続・移行・SQL。SQL は動く時に組む形（`query`・`query_as`）で書く。
//!
//! - 書く接続は 1 本だけ。番号（n）を数える所と、同じ物かを見る所が 1 列に並ぶ。
//! - 読む接続は 4 本（軽い読み）と 2 本（重い読み）で、読むだけ。WAL なので、書いている間も待たずに読める。
//! - どの表も足すだけ。この file の SQL は insert と select だけで、`ci/check-sql.sh` が見張る。
//!   「最新」は seq だけで決め、送り手の時刻（sent_at）では決めない。
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::body;

/// 書く接続を待つ長さ。過ぎれば「混んでいる」で返す。
pub const WRITE_WAIT: Duration = Duration::from_secs(2);
/// 読む接続（軽い読み・重い読み）を待つ長さ。過ぎれば「混んでいる」で返す。問い合わせの時間切れ（5 秒）より短くし、
/// 混んでいる時は「混んでいる」の 503 が、時間切れの 503 より先に答える。
pub const READ_WAIT: Duration = Duration::from_secs(3);
/// 軽い読みの接続の本数。
pub const READS: u32 = 4;
/// 重い読み（図の点と線・タイムライン）だけが使う、読むだけの接続の本数。軽い読みの接続とは別にして、
/// 重い読みが引き当てなどの軽い読みの接続を取らないようにする。
pub const HEAVY_READS: u32 = 2;
/// 送り手の時刻と castlist の時計が、これより離れていれば clock_skew を立てる（ちょうどなら立てない）。
pub const SKEW: time::Duration = time::Duration::minutes(5);

// --- 引き当てと見る SQL（試験が LOOKUPS で、索引を使うかを見る） ---

/// 鍵の付随する id から、アクターを引く。
const RESOLVE: &str =
    "select a.id, a.name, a.kind from bindings b join actors a on a.id = b.actor_id
     where b.issuer = ?1 and b.type = ?2 and b.value = ?3 and b.is_key = 1";
const ACTOR: &str =
    "select id, name, kind, issuer, sent_at, received_at, clock_skew from actors where id = ?1";
const LABELS: &str = "select key, value from labels where actor_id = ?1 order by key";
const BINDINGS: &str = "select issuer, type, value, is_key, sent_at, received_at from bindings
     where actor_id = ?1 order by seq";
const CURRENT_BINDINGS: &str = "select issuer, type, value, is_key, sent_at, received_at
     from actor_current_bindings where actor_id = ?1 order by type";
const EVENTS: &str =
    "select id, actor_id, type, detail, sent_at, received_at, clock_skew from events
     where actor_id = ?1 order by seq";
const SETTINGS: &str = "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.sent_at, s.received_at, s.clock_skew
     from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id
     where s.actor_id = ?1 order by s.seq";
/// model と effort の最新（seq で）。
const CURRENT_SETTINGS: &str = "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.sent_at, s.received_at, s.clock_skew
     from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id
     where s.actor_id = ?1 order by s.seq desc limit 1";
const LINKS_IN: &str =
    "select id, type, from_actor, to_actor, sent_at, received_at, clock_skew from links
     where to_actor = ?1 order by seq";
const LINKS_OUT: &str =
    "select id, type, from_actor, to_actor, sent_at, received_at, clock_skew from links
     where from_actor = ?1 order by seq";
/// 今の停止を決めた出来事（最後の居る出来事より後の、初めの退場。停止中の後の退場は数えない）が、
/// 推しの幕引き（`closed` で `detail.inferred` が JSON の true）か。居るままなら 0。
/// `$actor` はアクターの id の列の式（一覧（search.rs）と図でも同じ決まりを使う）。
macro_rules! inferred_sql {
    ($actor:literal) => {
        concat!(
            "coalesce((select e.type = 'closed' and coalesce(json_type(e.detail, '$.inferred') = 'true', 0) from events e
             where e.actor_id = ",
            $actor,
            " and e.type in ('cleared', 'exited', 'closed')
               and e.seq > coalesce((select i.seq from events i where i.actor_id = ",
            $actor,
            " and i.type in ('entered', 'reentered', 'forked') order by i.seq desc limit 1), 0)
             order by e.seq limit 1), 0)"
        )
    };
}
pub(crate) use inferred_sql;
const STATE: &str = concat!(
    "select presence, present, ",
    inferred_sql!("actor_state.actor_id"),
    " as inferred, entered_at, exited_at from actor_state where actor_id = ?1"
);

/// 送り手の id で、足した行を引く（答えの形）と、その本文の写しを引く。
const EVENT: &str =
    "select id, actor_id, type, detail, sent_at, received_at, clock_skew from events where id = ?1";
const EVENT_HASH: &str = "select body_hash from events where id = ?1";
const LINK: &str = "select id, type, from_actor, to_actor, sent_at, received_at, clock_skew from links where id = ?1";
const LINK_HASH: &str = "select body_hash from links where id = ?1";
const SETTING: &str = "select s.id, s.actor_id, m.name as model, f.name as effort, s.source, s.sent_at, s.received_at, s.clock_skew
     from settings s join models m on m.id = s.model_id join efforts f on f.id = s.effort_id
     where s.id = ?1";
const SETTING_HASH: &str = "select body_hash from settings where id = ?1";
/// そのアクターの最新（seq で）の model と effort の番号。
const LATEST_SETTING: &str =
    "select model_id, effort_id from settings where actor_id = ?1 order by seq desc limit 1";

/// 引き当てと見る SQL（名と SQL）。試験が `explain query plan` で、大きな表を端から読まないかを見る。
pub const LOOKUPS: &[(&str, &str)] = &[
    ("resolve", RESOLVE),
    ("actor", ACTOR),
    ("labels", LABELS),
    ("bindings", BINDINGS),
    ("current_bindings", CURRENT_BINDINGS),
    ("events", EVENTS),
    ("settings", SETTINGS),
    ("current_settings", CURRENT_SETTINGS),
    ("links_in", LINKS_IN),
    ("links_out", LINKS_OUT),
    ("state", STATE),
    ("event", EVENT),
    ("event_hash", EVENT_HASH),
    ("link", LINK),
    ("link_hash", LINK_HASH),
    ("setting", SETTING),
    ("setting_hash", SETTING_HASH),
    ("latest_setting", LATEST_SETTING),
];

/// どの表の最新の received_at より前にならないよう、表ごとの最新（seq の大きい方）を見る。
const CLOCK: &str = "select max(t) from (
       select (select received_at from actors order by seq desc limit 1) as t
       union all select (select received_at from labels order by seq desc limit 1)
       union all select (select received_at from bindings order by seq desc limit 1)
       union all select (select received_at from links order by seq desc limit 1)
       union all select (select received_at from events order by seq desc limit 1)
       union all select (select received_at from settings order by seq desc limit 1))";

pub struct Db {
    /// DB の file（バックアップの一時の file を、この隣に作る）。
    path: PathBuf,
    read: SqlitePool,
    /// 重い読みだけが使う、読むだけの接続（[`HEAVY_READS`] 本）。
    heavy: SqlitePool,
    write: SqlitePool,
}

/// 付随する id 1 つ。
#[derive(Clone, Debug)]
pub struct Ref {
    pub kind: String,
    pub value: String,
}

impl Ref {
    /// 参照の字（`種類:値`）を、初めの `:` で分ける（種類は `:` を持たない。形は定義が見てある）。
    pub fn parse(s: &str) -> Self {
        let (kind, value) = s.split_once(':').unwrap_or_default();
        Ref {
            kind: kind.to_owned(),
            value: value.to_owned(),
        }
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.value)
    }
}

/// 登録の中身（形は定義が見てある）。
pub struct NewActor<'a> {
    pub issuer: &'a str,
    pub kind: &'a str,
    pub prefix: &'a str,
    pub labels: &'a [(String, String)],
    /// 初めの物が鍵。
    pub bindings: &'a [Ref],
    pub sent_at: OffsetDateTime,
    /// 頼んだ model と effort。
    pub settings: Option<(&'a str, &'a str)>,
}

/// 出来事の中身（形は定義が見てある）。
pub struct NewEvent<'a> {
    pub issuer: &'a str,
    pub by: &'a Ref,
    pub kind: &'a str,
    /// JSON の物。
    pub detail: &'a serde_json::Value,
    pub sent_at: OffsetDateTime,
}

/// 線の中身（形は定義が見てある）。
pub struct NewLink<'a> {
    pub issuer: &'a str,
    pub kind: &'a str,
    pub from: &'a Ref,
    pub to: &'a Ref,
    pub sent_at: OffsetDateTime,
}

/// model と effort の中身（形は定義が見てある）。
pub struct NewSetting<'a> {
    pub issuer: &'a str,
    pub by: &'a Ref,
    pub model: &'a str,
    pub effort: &'a str,
    pub source: &'a str,
    pub sent_at: OffsetDateTime,
}

/// 送り手の id で足した結果。
#[derive(Debug)]
pub enum Put<T> {
    /// 新しく足した。
    Created(T),
    /// 同じ物が既にあった（足していない）。
    Same(T),
}

/// アクターを指す時の短い形。
#[derive(Clone, Debug, FromRow, PartialEq, Eq)]
pub struct ActorRef {
    pub id: String,
    pub name: String,
    pub kind: String,
}

/// アクターの行。
#[derive(Clone, Debug, FromRow)]
pub struct Actor {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub issuer: String,
    pub sent_at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

/// 付随する id の行。
#[derive(Clone, Debug, FromRow)]
pub struct Binding {
    pub issuer: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub value: String,
    pub is_key: bool,
    pub sent_at: String,
    pub received_at: String,
}

/// 出来事の行。detail は JSON の物の字。
#[derive(Clone, Debug, FromRow)]
pub struct Event {
    pub id: String,
    pub actor_id: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub detail: String,
    pub sent_at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

/// model と effort の行（名に直した物）。id は送り手の物で、登録の時に頼んだ物には無い。
#[derive(Clone, Debug, FromRow)]
pub struct Setting {
    pub id: Option<String>,
    pub actor_id: String,
    pub model: String,
    pub effort: String,
    pub source: String,
    pub sent_at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

/// 線の行。
#[derive(Clone, Debug, FromRow)]
pub struct Link {
    pub id: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub from_actor: String,
    pub to_actor: String,
    pub sent_at: String,
    pub received_at: String,
    pub clock_skew: bool,
}

/// 今の状態（view の actor_state と、推しの停止か）。
#[derive(Clone, Debug, Default, FromRow)]
pub struct State {
    pub presence: Option<String>,
    pub present: bool,
    pub inferred: bool,
    pub entered_at: Option<String>,
    pub exited_at: Option<String>,
}

/// 1 つのアクターの全部。
#[derive(Clone, Debug)]
pub struct ActorDetail {
    pub actor: Actor,
    /// 鍵の順。
    pub labels: Vec<(String, String)>,
    pub bindings: Vec<Binding>,
    pub current_bindings: Vec<Binding>,
    pub events: Vec<Event>,
    pub settings: Vec<Setting>,
    pub current_settings: Option<Setting>,
    pub links_in: Vec<Link>,
    pub links_out: Vec<Link>,
    pub state: State,
}

/// 書けない訳。
#[derive(Debug)]
pub enum WriteError {
    /// 書く接続が空かない（混んでいる）。
    Busy,
    /// 定義の形では言えない、中身の誤り（指すアクターが無いなど）。
    Invalid(String),
    /// 同じ id で違う中身・2 度目の起こされるなど。
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
                "外のキーの崩れた行があるので開かない: {}",
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

/// 送り手の時刻が、受けた時刻から 5 分より離れているか。
fn skewed(sent: OffsetDateTime, received: &str) -> bool {
    OffsetDateTime::parse(received, &Rfc3339).is_ok_and(|r| (r - sent).abs() > SKEW)
}

/// 送り手の時刻の字（RFC 3339・UTC。秒の下は要る桁だけ）。年が 4 桁に収まらなければ読めない時刻。
fn sent_text(t: OffsetDateTime) -> Result<String, WriteError> {
    t.to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|_| WriteError::Invalid("sentAt は 0000 年から 9999 年までの時刻".to_owned()))
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
        sqlx::migrate!("./migrations")
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
            .acquire_timeout(READ_WAIT)
            .connect_with(options.clone().read_only(true))
            .await
            .map_err(OpenError::Db)?;
        let read = SqlitePoolOptions::new()
            .max_connections(READS)
            .acquire_timeout(READ_WAIT)
            .connect_with(options.read_only(true))
            .await
            .map_err(OpenError::Db)?;
        Ok(Self {
            path: path.to_owned(),
            read,
            heavy,
            write,
        })
    }

    /// 軽い読みの接続（[`READS`] 本・読むだけ）。
    pub fn read(&self) -> &SqlitePool {
        &self.read
    }

    /// 重い読みだけが使う接続（[`HEAVY_READS`] 本・読むだけ）。
    pub fn heavy(&self) -> &SqlitePool {
        &self.heavy
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

    /// 揃ったバックアップを、DB の隣の一時の file（`<DB>.backup-<pid>-<番号>`）に `VACUUM INTO` で作り、その path を返す
    /// （今の server と同じ）。DB 全体を読むので、重い読みの接続で作る（軽い読みの接続を取らない）。消すのは呼んだ側。
    pub async fn backup(&self) -> Result<PathBuf, sqlx::Error> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let mut name = self.path.clone().into_os_string();
        name.push(format!(
            ".backup-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let out = PathBuf::from(name);
        // VACUUM INTO は、在る file には書かない。
        let _ = tokio::fs::remove_file(&out).await;
        sqlx::query("vacuum into ?1")
            .bind(out.to_string_lossy().into_owned())
            .execute(&self.heavy)
            .await?;
        Ok(out)
    }
}

// --- 書く ---

#[derive(Clone, Copy)]
enum Master {
    Models,
    Efforts,
}

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

async fn insert_binding(
    tx: &mut Transaction<'_, Sqlite>,
    actor_id: &str,
    issuer: &str,
    binding: &Ref,
    is_key: bool,
    sent_at: &str,
    received: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "insert into bindings(actor_id, issuer, type, value, is_key, sent_at, received_at)
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(actor_id)
    .bind(issuer)
    .bind(&binding.kind)
    .bind(&binding.value)
    .bind(is_key)
    .bind(sent_at)
    .bind(received)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

impl Db {
    /// アクターを登録する。鍵の付随する id が出し手の中に既にあれば、そのアクターを返す（created = false）。
    /// その時は、変わった鍵でない付随する id と、変わった頼んだ model と effort だけを足す。
    pub async fn register(&self, new: &NewActor<'_>) -> Result<(ActorRef, bool), WriteError> {
        let (key, rest) = new.bindings.split_first().ok_or_else(|| {
            WriteError::Invalid("bindings に 1 つ以上（初めの物が鍵）".to_owned())
        })?;
        let mut tx = self.write.begin().await?;
        let received = received_at(&mut tx).await?;
        let skew = i64::from(skewed(new.sent_at, &received));
        let sent_at = sent_text(new.sent_at)?;
        let found: Option<ActorRef> = sqlx::query_as(RESOLVE)
            .bind(new.issuer)
            .bind(&key.kind)
            .bind(&key.value)
            .fetch_optional(&mut *tx)
            .await?;
        let (actor, created) = match found {
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
                    "insert into actors(id, issuer, kind, prefix, n, name, sent_at, received_at, clock_skew)
                     values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )
                .bind(&actor.id)
                .bind(new.issuer)
                .bind(new.kind)
                .bind(new.prefix)
                .bind(n)
                .bind(&actor.name)
                .bind(&sent_at)
                .bind(&received)
                .bind(skew)
                .execute(&mut *tx)
                .await?;
                // 札は新しく作る時だけ書く。
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
                insert_binding(
                    &mut tx, &actor.id, new.issuer, key, true, &sent_at, &received,
                )
                .await?;
                (actor, true)
            }
        };
        // 鍵でない付随する id は、そのアクターの同じ種類の最新と違う時だけ足す（新しく作った時も）。
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
                    &mut tx, &actor.id, new.issuer, binding, false, &sent_at, &received,
                )
                .await?;
            }
        }
        // 頼んだ model と effort は、最新（source を問わない）と違う時だけ足す。
        if let Some((model, effort)) = new.settings {
            let model_id = master(&mut tx, Master::Models, model, &received).await?;
            let effort_id = master(&mut tx, Master::Efforts, effort, &received).await?;
            let latest: Option<(i64, i64)> = sqlx::query_as(LATEST_SETTING)
                .bind(&actor.id)
                .fetch_optional(&mut *tx)
                .await?;
            if latest != Some((model_id, effort_id)) {
                sqlx::query(
                    "insert into settings(actor_id, model_id, effort_id, source, sent_at, received_at, clock_skew)
                     values(?1, ?2, ?3, 'requested', ?4, ?5, ?6)",
                )
                .bind(&actor.id)
                .bind(model_id)
                .bind(effort_id)
                .bind(&sent_at)
                .bind(&received)
                .bind(skew)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok((actor, created))
    }
}

// --- 送り手の id で足す ---

/// 同じ id の行の、本文の写し（行が無ければ None。写しの無い行は Some(None)）。
async fn stored_hash(
    tx: &mut Transaction<'_, Sqlite>,
    sql: &'static str,
    id: &str,
) -> Result<Option<Option<String>>, sqlx::Error> {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
}

/// 同じ id の行が既にある時: 本文の写しが同じなら Same（その行を返す）、違えば Conflict（行はそのまま）。
async fn same_or_conflict<T>(
    mut tx: Transaction<'_, Sqlite>,
    stored: Option<String>,
    sent: &str,
    select: &'static str,
    id: &str,
    what: &str,
) -> Result<Put<T>, WriteError>
where
    T: for<'r> FromRow<'r, SqliteRow> + Send + Unpin,
{
    if stored.as_deref() != Some(sent) {
        return Err(WriteError::Conflict(format!(
            "同じ id（{id}）の{what}が、違う中身で既にある"
        )));
    }
    let row = sqlx::query_as(select).bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Put::Same(row))
}

/// 出し手の中の鍵の付随する id で、アクターを引く。引けなければ中身の誤り（`field` は本文の欄の名）。
async fn must_resolve(
    tx: &mut Transaction<'_, Sqlite>,
    issuer: &str,
    r: &Ref,
    field: &str,
) -> Result<ActorRef, WriteError> {
    sqlx::query_as(RESOLVE)
        .bind(issuer)
        .bind(&r.kind)
        .bind(&r.value)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| {
            WriteError::Invalid(format!(
                "{field} のアクター（{r}）が、出し手 {issuer} の鍵の付随する id に無い"
            ))
        })
}

impl Db {
    /// 出来事を足す。同じ id で同じ本文なら Same、違えば Conflict（by を引くより先に見る）。
    pub async fn put_event(&self, id: &str, new: &NewEvent<'_>) -> Result<Put<Event>, WriteError> {
        let sent_at = sent_text(new.sent_at)?;
        let detail = body::canonical(new.detail);
        let hash = body::hash(&json!({
            "issuer": new.issuer,
            "by": new.by.to_string(),
            "type": new.kind,
            "detail": new.detail,
            "sentAt": sent_at,
        }));
        let mut tx = self.write.begin().await?;
        if let Some(stored) = stored_hash(&mut tx, EVENT_HASH, id).await? {
            return same_or_conflict(tx, stored, &hash, EVENT, id, "出来事").await;
        }
        let actor = must_resolve(&mut tx, new.issuer, new.by, "by").await?;
        let received = received_at(&mut tx).await?;
        sqlx::query(
            "insert into events(id, actor_id, type, detail, sent_at, received_at, clock_skew, body_hash)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(id)
        .bind(&actor.id)
        .bind(new.kind)
        .bind(&detail)
        .bind(&sent_at)
        .bind(&received)
        .bind(skewed(new.sent_at, &received))
        .bind(&hash)
        .execute(&mut *tx)
        .await?;
        let event = sqlx::query_as(EVENT).bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Put::Created(event))
    }

    /// 線を足す。同じ id の扱いは出来事と同じ。from と to が同じアクターなら中身の誤り（同じ id かを見た後）。
    /// 起こされる（launched）のは、1 つのアクターに 1 度だけ（2 度目は Conflict）。
    pub async fn put_link(&self, id: &str, new: &NewLink<'_>) -> Result<Put<Link>, WriteError> {
        let sent_at = sent_text(new.sent_at)?;
        let hash = body::hash(&json!({
            "issuer": new.issuer,
            "type": new.kind,
            "from": new.from.to_string(),
            "to": new.to.to_string(),
            "sentAt": sent_at,
        }));
        let mut tx = self.write.begin().await?;
        if let Some(stored) = stored_hash(&mut tx, LINK_HASH, id).await? {
            return same_or_conflict(tx, stored, &hash, LINK, id, "線").await;
        }
        let from = must_resolve(&mut tx, new.issuer, new.from, "from").await?;
        let to = must_resolve(&mut tx, new.issuer, new.to, "to").await?;
        if from.id == to.id {
            return Err(WriteError::Invalid(format!(
                "from と to が同じアクター（{}）",
                from.name
            )));
        }
        let received = received_at(&mut tx).await?;
        let inserted = sqlx::query(
            "insert into links(id, type, from_actor, to_actor, sent_at, received_at, clock_skew, body_hash)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(id)
        .bind(new.kind)
        .bind(&from.id)
        .bind(&to.id)
        .bind(&sent_at)
        .bind(&received)
        .bind(skewed(new.sent_at, &received))
        .bind(&hash)
        .execute(&mut *tx)
        .await;
        match inserted {
            // id は上で見たので、重なるのは「起こされるのは 1 度だけ」の索引（links_launched）。
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                return Err(WriteError::Conflict(format!(
                    "{} は、もう別の線で起こされている",
                    to.name
                )));
            }
            other => other?,
        };
        let link = sqlx::query_as(LINK).bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Put::Created(link))
    }

    /// model と effort を足す。同じ id の扱いは出来事と同じ。新しい id でも、そのアクターの最新（seq で。source は比べない）と
    /// model と effort が同じなら、足さずに最新の行を Same で返す（その id は残らない）。知らない名は一覧に足す。
    pub async fn put_setting(
        &self,
        id: &str,
        new: &NewSetting<'_>,
    ) -> Result<Put<Setting>, WriteError> {
        let sent_at = sent_text(new.sent_at)?;
        let hash = body::hash(&json!({
            "issuer": new.issuer,
            "by": new.by.to_string(),
            "model": new.model,
            "effort": new.effort,
            "source": new.source,
            "sentAt": sent_at,
        }));
        let mut tx = self.write.begin().await?;
        if let Some(stored) = stored_hash(&mut tx, SETTING_HASH, id).await? {
            return same_or_conflict(tx, stored, &hash, SETTING, id, "model と effort").await;
        }
        let actor = must_resolve(&mut tx, new.issuer, new.by, "by").await?;
        let received = received_at(&mut tx).await?;
        let model_id = master(&mut tx, Master::Models, new.model, &received).await?;
        let effort_id = master(&mut tx, Master::Efforts, new.effort, &received).await?;
        let latest: Option<(i64, i64)> = sqlx::query_as(LATEST_SETTING)
            .bind(&actor.id)
            .fetch_optional(&mut *tx)
            .await?;
        if latest == Some((model_id, effort_id)) {
            let latest = sqlx::query_as(CURRENT_SETTINGS)
                .bind(&actor.id)
                .fetch_one(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(Put::Same(latest));
        }
        sqlx::query(
            "insert into settings(id, actor_id, model_id, effort_id, source, sent_at, received_at, clock_skew, body_hash)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(id)
        .bind(&actor.id)
        .bind(model_id)
        .bind(effort_id)
        .bind(new.source)
        .bind(&sent_at)
        .bind(&received)
        .bind(skewed(new.sent_at, &received))
        .bind(&hash)
        .execute(&mut *tx)
        .await?;
        let setting = sqlx::query_as(SETTING).bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Put::Created(setting))
    }
}

// --- 読む ---

impl Db {
    /// 鍵の付随する id から、出し手の中のアクターを引く（引けなければ None）。答えは問いと同じ順。
    pub async fn resolve(
        &self,
        issuer: &str,
        refs: &[Ref],
    ) -> Result<Vec<Option<ActorRef>>, sqlx::Error> {
        let mut tx = self.read.begin().await?;
        let mut found = Vec::with_capacity(refs.len());
        for r in refs {
            found.push(
                sqlx::query_as(RESOLVE)
                    .bind(issuer)
                    .bind(&r.kind)
                    .bind(&r.value)
                    .fetch_optional(&mut *tx)
                    .await?,
            );
        }
        tx.commit().await?;
        Ok(found)
    }

    /// 1 つのアクターの全部（無ければ None）。1 つの読む transaction の中で、揃った写しを読む。
    pub async fn actor(&self, id: &str) -> Result<Option<ActorDetail>, sqlx::Error> {
        let mut tx = self.read.begin().await?;
        let Some(actor) = sqlx::query_as::<_, Actor>(ACTOR)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
        else {
            return Ok(None);
        };
        let detail = ActorDetail {
            labels: sqlx::query_as(LABELS).bind(id).fetch_all(&mut *tx).await?,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> OffsetDateTime {
        OffsetDateTime::parse(s, &Rfc3339).unwrap()
    }

    #[test]
    fn stamp_is_utc_with_milliseconds_and_24_chars() {
        assert_eq!(
            stamp(t("2026-01-02T03:04:05.678901+09:00")),
            "2026-01-01T18:04:05.678Z"
        );
        assert_eq!(stamp(t("2026-01-02T03:04:05Z")), "2026-01-02T03:04:05.000Z");
    }

    #[test]
    fn sent_text_is_utc_with_only_the_needed_fraction() {
        assert_eq!(
            sent_text(t("2099-01-01T00:00:00Z")).unwrap(),
            "2099-01-01T00:00:00Z"
        );
        assert_eq!(
            sent_text(t("2099-01-01T08:59:59.999+09:00")).unwrap(),
            "2098-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn skew_is_more_than_5_minutes() {
        let received = "2099-01-01T00:05:00.000Z";
        assert!(!skewed(t("2099-01-01T00:00:00Z"), received));
        assert!(!skewed(t("2099-01-01T00:10:00Z"), received));
        assert!(skewed(t("2098-12-31T23:59:59.999Z"), received));
        assert!(skewed(t("2099-01-01T00:10:00.001Z"), received));
    }
}
