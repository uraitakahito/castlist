//! 今の server の DB（`GET /v1/backup` の写し）を、新しい server の DB へ移す（`castlist-next migrate --from <写し> --to <新しい DB>`）。
//!
//! - 移す元は、読むだけ（`mode=ro&immutable=1`）で開き、書かない。隣に WAL の残り（`-wal`）があれば、写しでないので断る。
//!   時刻の列は `sent_at`（0.3 から）か `at`（0.1.x）かを、表ごとに列を見て決める。
//! - 移す先は、新しい migrations を当ててから、台帳の行が 1 つも無いことを見る（あれば断る）。
//! - 1 つの transaction の中で、親の表から順に insert だけで写す（外のキーと足すだけの trigger は効いたまま）。
//!   seq・送り手の id・時刻の字・clock_skew・body_hash・detail は字のまま。model と effort は名で結び直す
//!   （新しい DB の migrations が effort の 5 つを先に置くので、番号は替わりうる）。
//! - 写した後、同じ transaction の中で、表ごとの行の数と中身の digest・seq の続き（`sqlite_sequence`）・外のキーを
//!   突き合わせ、1 つでも合わなければ何も足さずに戻す。
//!
//! この file の SQL は insert と select だけ（`ci/check-sql.sh` が見張る）。
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{AssertSqlSafe, Connection};

use crate::db::{Db, OpenError};

/// 移す表（親が先）と、その列（新しい DB の名）。
const TABLES: &[(&str, &[&str])] = &[
    (
        "actors",
        &[
            "seq",
            "id",
            "issuer",
            "kind",
            "prefix",
            "n",
            "name",
            "sent_at",
            "received_at",
            "clock_skew",
        ],
    ),
    (
        "labels",
        &["seq", "actor_id", "key", "value", "received_at"],
    ),
    (
        "bindings",
        &[
            "seq",
            "actor_id",
            "issuer",
            "type",
            "value",
            "is_key",
            "sent_at",
            "received_at",
        ],
    ),
    (
        "links",
        &[
            "seq",
            "id",
            "type",
            "from_actor",
            "to_actor",
            "sent_at",
            "received_at",
            "clock_skew",
            "body_hash",
        ],
    ),
    (
        "events",
        &[
            "seq",
            "id",
            "actor_id",
            "type",
            "detail",
            "sent_at",
            "received_at",
            "clock_skew",
            "body_hash",
        ],
    ),
    ("models", &["id", "name", "first_seen_at"]),
    ("efforts", &["id", "name", "first_seen_at"]),
    (
        "settings",
        &[
            "seq",
            "id",
            "actor_id",
            "model_id",
            "effort_id",
            "source",
            "sent_at",
            "received_at",
            "clock_skew",
            "body_hash",
        ],
    ),
];

/// seq を持つ表（`sqlite_sequence` の続きを見る）。
const SEQ_TABLES: &[&str] = &[
    "actors", "labels", "bindings", "links", "events", "settings",
];

/// 新しい DB の migrations（0001）が先に置く effort。この 5 つの first_seen_at は新しい DB の物になるので、digest に入れない。
pub const PRESET_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// 移した表の数と digest の突き合わせ（1 つの表）。
#[derive(Clone, Debug)]
pub struct TableCheck {
    pub table: &'static str,
    pub old_rows: i64,
    pub new_rows: i64,
    pub old_digest: String,
    pub new_digest: String,
}

impl TableCheck {
    pub fn ok(&self) -> bool {
        self.old_rows == self.new_rows && self.old_digest == self.new_digest
    }
}

/// 行の番号（seq）の続き（`sqlite_sequence` の 1 つの表。行がまだ無ければ 0）。
#[derive(Clone, Debug)]
pub struct SeqCheck {
    pub table: &'static str,
    pub old: i64,
    pub new: i64,
}

impl SeqCheck {
    pub fn ok(&self) -> bool {
        self.old == self.new
    }
}

/// 突き合わせの全部。
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub tables: Vec<TableCheck>,
    pub sequences: Vec<SeqCheck>,
    /// 新しい DB の `pragma foreign_key_check` に出た行（空なら崩れは無い）。
    pub broken: Vec<String>,
    /// 新しい API の形（ActorId・SenderId）に合わない id（`<表>: <id>`。空なら無い）。
    pub bad_ids: Vec<String>,
}

impl Summary {
    /// どの表も数と digest が合い、seq が続き、外のキーが崩れていないか。
    pub fn ok(&self) -> bool {
        self.tables.iter().all(TableCheck::ok)
            && self.sequences.iter().all(SeqCheck::ok)
            && self.broken.is_empty()
            && self.bad_ids.is_empty()
    }

    /// 名で表の突き合わせを引く。
    pub fn table(&self, name: &str) -> Option<&TableCheck> {
        self.tables.iter().find(|t| t.table == name)
    }
}

/// 1 行に 1 つ: `<表> <今の行の数> → <新しい行の数> digest <今の物> → <新しい物> ok|NG`（digest は頭の 16 字）。
impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = |ok: bool| if ok { "ok" } else { "NG" };
        for t in &self.tables {
            writeln!(
                f,
                "{} {} → {} digest {} → {} {}",
                t.table,
                t.old_rows,
                t.new_rows,
                &t.old_digest[..16],
                &t.new_digest[..16],
                mark(t.ok())
            )?;
        }
        for s in &self.sequences {
            writeln!(f, "seq {} {} → {} {}", s.table, s.old, s.new, mark(s.ok()))?;
        }
        if self.broken.is_empty() {
            writeln!(f, "foreign_key_check ok")
        } else {
            writeln!(f, "foreign_key_check NG: {}", self.broken.join("・"))
        }
    }
}

/// 移せなかった訳。
#[derive(Debug)]
pub enum MigrateError {
    /// 移す元が読めない・今の server の DB でない。
    Source(String),
    /// 移す先が空でない・元と同じ file。
    Target(String),
    /// 移す先を開けない（移行に落ちた・外のキーの崩れた行がある）。
    Open(OpenError),
    Db(sqlx::Error),
    /// 写した後の突き合わせが合わない（移す先には何も足していない）。
    Mismatch(Summary),
}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(why) => write!(f, "移す元: {why}"),
            Self::Target(why) => write!(f, "移す先: {why}"),
            Self::Open(e) => write!(f, "移す先を開けない: {e}"),
            Self::Db(e) => write!(f, "DB: {e}"),
            Self::Mismatch(summary) => {
                write!(f, "突き合わせが合わないので、何も足さずに戻した\n{summary}")
            }
        }
    }
}

impl std::error::Error for MigrateError {}

impl From<sqlx::Error> for MigrateError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

/// 表ごとの、送り手の時刻の列の名（`sent_at` か、0.1.x の `at`）。時刻の列の無い表は持たない。
type TimeColumns = BTreeMap<&'static str, &'static str>;

/// 今の server の DB の写し `from` を、新しい DB `to`（無ければ作る）へ移し、突き合わせを返す。
pub async fn migrate(from: &Path, to: &Path) -> Result<Summary, MigrateError> {
    let old_time = inspect_source(from).await?;
    if same_file(from, to).await {
        return Err(MigrateError::Target(format!(
            "移す元と同じ file: {}",
            to.display()
        )));
    }
    let db = Db::open(to).await.map_err(MigrateError::Open)?;
    let result = copy(&db, from, &old_time).await;
    db.close().await;
    result
}

/// 今の server の DB の写し `from` と、新しい DB `to` の中身を突き合わせる（どちらにも書かない）。
pub async fn verify(from: &Path, to: &Path) -> Result<Summary, MigrateError> {
    let old_time = inspect_source(from).await?;
    let mut conn = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(to)
            .read_only(true)
            .create_if_missing(false),
    )
    .await?;
    let new_time = time_columns(&mut conn, "main")
        .await?
        .map_err(MigrateError::Target)?;
    attach(&mut conn, from).await?;
    let summary = summarize(&mut conn, &old_time, &new_time).await;
    sqlx::query("detach database old")
        .execute(&mut conn)
        .await?;
    conn.close().await?;
    summary
}

/// 移す元が、読める今の server の DB の写しかを見て、時刻の列の名を返す（移す先は、まだ作らない）。
async fn inspect_source(from: &Path) -> Result<TimeColumns, MigrateError> {
    match tokio::fs::metadata(from).await {
        Ok(m) if m.is_file() => {}
        Ok(_) => return Err(source(from, "file でない")),
        Err(e) => return Err(source(from, &format!("読めない（{e}）"))),
    }
    let wal = from.with_file_name(format!(
        "{}-wal",
        from.file_name().unwrap_or_default().to_string_lossy()
    ));
    if tokio::fs::metadata(&wal).await.is_ok_and(|m| m.len() > 0) {
        return Err(source(
            from,
            "隣に WAL の残り（-wal）がある。動いている DB でなく、GET /v1/backup の写しを渡す",
        ));
    }
    let mut conn = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(from)
            .read_only(true)
            .immutable(true)
            .create_if_missing(false),
    )
    .await
    .map_err(|e| source(from, &format!("開けない（{e}）")))?;
    let found = time_columns(&mut conn, "main").await;
    let _ = conn.close().await;
    match found {
        Ok(Ok(columns)) => Ok(columns),
        Ok(Err(why)) => Err(source(from, &why)),
        Err(e) => Err(source(from, &format!("SQLite の DB として読めない（{e}）"))),
    }
}

fn source(from: &Path, why: &str) -> MigrateError {
    MigrateError::Source(format!("{}: {why}", from.display()))
}

/// 2 つの path が同じ file を指すか（無い path は、どれとも同じでない）。
async fn same_file(a: &Path, b: &Path) -> bool {
    match (
        tokio::fs::canonicalize(a).await,
        tokio::fs::canonicalize(b).await,
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `schema` の表がみな在るかを見て、表ごとの送り手の時刻の列の名を返す（無い表・列は、外の Err の中の訳）。
async fn time_columns(
    conn: &mut SqliteConnection,
    schema: &str,
) -> Result<Result<TimeColumns, String>, sqlx::Error> {
    let mut found = TimeColumns::new();
    for (table, columns) in TABLES {
        let have: Vec<String> = sqlx::query_scalar("select name from pragma_table_info(?1, ?2)")
            .bind(table)
            .bind(schema)
            .fetch_all(&mut *conn)
            .await?;
        if have.is_empty() {
            return Ok(Err(format!("castlist の DB でない（表 {table} が無い）")));
        }
        if !columns.contains(&"sent_at") {
            continue;
        }
        let name = if have.iter().any(|c| c == "sent_at") {
            "sent_at"
        } else if have.iter().any(|c| c == "at") {
            "at"
        } else {
            return Ok(Err(format!(
                "表 {table} に送り手の時刻の列（sent_at か at）が無い"
            )));
        };
        found.insert(table, name);
    }
    Ok(Ok(found))
}

/// 移す元を、読むだけの `old` として付ける（URI の `mode=ro&immutable=1`）。
async fn attach(conn: &mut SqliteConnection, from: &Path) -> Result<(), MigrateError> {
    let path = from
        .to_str()
        .ok_or_else(|| source(from, "path が UTF-8 でない"))?;
    let encoded: String = path
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    sqlx::query("attach database ?1 as old")
        .bind(format!("file:{encoded}?mode=ro&immutable=1"))
        .execute(&mut *conn)
        .await
        .map_err(|e| source(from, &format!("付けられない（{e}）")))?;
    Ok(())
}

/// 空の移す先へ、1 つの transaction で写し、突き合わせが合えば commit する。
async fn copy(db: &Db, from: &Path, old_time: &TimeColumns) -> Result<Summary, MigrateError> {
    let mut conn = db.write().acquire().await?;
    let rows: i64 = sqlx::query_scalar(
        "select (select count(*) from actors) + (select count(*) from labels)
              + (select count(*) from bindings) + (select count(*) from links)
              + (select count(*) from events) + (select count(*) from settings)
              + (select count(*) from models)",
    )
    .fetch_one(&mut *conn)
    .await?;
    if rows > 0 {
        return Err(MigrateError::Target(format!(
            "台帳の行が {rows} ある。空の DB（アクターの無い物）にだけ移す"
        )));
    }
    attach(&mut conn, from).await?;
    let result = async {
        let mut tx = conn.begin().await?;
        for (table, _) in TABLES {
            sqlx::query(AssertSqlSafe(copy_sql(table, old_time)))
                .execute(&mut *tx)
                .await?;
        }
        let new_time: TimeColumns = old_time.keys().map(|t| (*t, "sent_at")).collect();
        let summary = summarize(&mut tx, old_time, &new_time).await?;
        if summary.ok() {
            tx.commit().await?;
            Ok(summary)
        } else {
            tx.rollback().await?;
            Err(MigrateError::Mismatch(summary))
        }
    }
    .await;
    sqlx::query("detach database old")
        .execute(&mut *conn)
        .await?;
    result
}

/// 列の式（送り手の時刻の列は、その DB の名に読み替える）。
fn column(table: &str, column: &str, time: &TimeColumns) -> String {
    match (column, time.get(table)) {
        ("sent_at", Some(name)) => format!("t.{name}"),
        _ => format!("t.{column}"),
    }
}

/// 1 つの表を、移す元（old）から移す先（main）へ写す insert。
fn copy_sql(table: &str, old_time: &TimeColumns) -> String {
    let (_, columns) = TABLES.iter().find(|(t, _)| *t == table).expect("移す表");
    match table {
        // model と effort は名と first_seen_at だけを写す（番号は移す先が振る）。effort は、先に在る物を足さない。
        "models" => "insert into main.models(name, first_seen_at)
             select t.name, t.first_seen_at from old.models t order by t.id"
            .to_owned(),
        "efforts" => "insert into main.efforts(name, first_seen_at)
             select t.name, t.first_seen_at from old.efforts t
             where t.name not in (select name from main.efforts) order by t.id"
            .to_owned(),
        _ => {
            let select: Vec<String> = columns
                .iter()
                .map(|c| match *c {
                    "model_id" => {
                        "(select n.id from main.models n join old.models o on o.name = n.name
                       where o.id = t.model_id)"
                            .to_owned()
                    }
                    "effort_id" => {
                        "(select n.id from main.efforts n join old.efforts o on o.name = n.name
                       where o.id = t.effort_id)"
                            .to_owned()
                    }
                    c => column(table, c, old_time),
                })
                .collect();
            format!(
                "insert into main.{table}({}) select {} from old.{table} t order by t.seq",
                columns.join(", "),
                select.join(", ")
            )
        }
    }
}

/// 1 つの表の、意味のある列を 1 行 1 つの JSON の配列にして受けた順に並べる select。
/// 中の番号（model と effort の id）は名に替え、先に置かれた effort の first_seen_at は外す。
fn digest_sql(table: &str, schema: &str, time: &TimeColumns) -> String {
    let (_, columns) = TABLES.iter().find(|(t, _)| *t == table).expect("移す表");
    match table {
        "models" => format!(
            "select json_array(t.name, t.first_seen_at) from {schema}.models t order by t.name"
        ),
        "efforts" => {
            let preset: Vec<String> = PRESET_EFFORTS.iter().map(|e| format!("'{e}'")).collect();
            format!(
                "select json_array(t.name, case when t.name in ({}) then null else t.first_seen_at end)
                 from {schema}.efforts t order by t.name",
                preset.join(", ")
            )
        }
        _ => {
            let select: Vec<String> = columns
                .iter()
                .map(|c| match *c {
                    "model_id" => {
                        format!("(select name from {schema}.models where id = t.model_id)")
                    }
                    "effort_id" => {
                        format!("(select name from {schema}.efforts where id = t.effort_id)")
                    }
                    c => column(table, c, time),
                })
                .collect();
            format!(
                "select json_array({}) from {schema}.{table} t order by t.seq",
                select.join(", ")
            )
        }
    }
}

/// 移す元（old）と移す先（main）の、表ごとの行の数と digest・seq の続き・外のキーを突き合わせる。
async fn summarize(
    conn: &mut SqliteConnection,
    old_time: &TimeColumns,
    new_time: &TimeColumns,
) -> Result<Summary, MigrateError> {
    let mut summary = Summary::default();
    for (table, _) in TABLES {
        let (old_rows, old_digest) = digest(conn, table, "old", old_time).await?;
        let (new_rows, new_digest) = digest(conn, table, "main", new_time).await?;
        summary.tables.push(TableCheck {
            table,
            old_rows,
            new_rows,
            old_digest,
            new_digest,
        });
    }
    for table in SEQ_TABLES {
        // 行の無い表は、sqlite_sequence に行が無いこと（今の DB）も、0 の行があること（insert … select で 0 行を
        // 写した新しい DB）もある。どちらも 0 と読む。
        let mut seq = [0, 0];
        for (slot, schema) in seq.iter_mut().zip(["old", "main"]) {
            *slot = sqlx::query_scalar(AssertSqlSafe(format!(
                "select coalesce((select seq from {schema}.sqlite_sequence where name = ?1), 0)"
            )))
            .bind(table)
            .fetch_one(&mut *conn)
            .await?;
        }
        summary.sequences.push(SeqCheck {
            table,
            old: seq[0],
            new: seq[1],
        });
    }
    // 新しい server の Db::open と同じ形（表・rowid・指す先）。
    let broken: Vec<(String, Option<i64>, String)> = sqlx::query_as(
        "select \"table\", rowid, parent from pragma_foreign_key_check(null, 'main')",
    )
    .fetch_all(&mut *conn)
    .await?;
    summary.broken = broken
        .into_iter()
        .map(|(table, rowid, parent)| {
            let rowid = rowid.map_or("-".to_owned(), |r| r.to_string());
            format!("{table} の rowid {rowid}（{parent} を指す）")
        })
        .collect();
    Ok(summary)
}

/// 1 つの表の行の数と、行ごとの JSON の字（改行で区切る）の SHA-256（小文字の 16 進）。
async fn digest(
    conn: &mut SqliteConnection,
    table: &str,
    schema: &str,
    time: &TimeColumns,
) -> Result<(i64, String), sqlx::Error> {
    let count: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
        "select count(*) from {schema}.{table}"
    )))
    .fetch_one(&mut *conn)
    .await?;
    let rows: Vec<String> = sqlx::query_scalar(AssertSqlSafe(digest_sql(table, schema, time)))
        .fetch_all(&mut *conn)
        .await?;
    let mut hash = Sha256::new();
    for row in &rows {
        hash.update(row.as_bytes());
        hash.update(b"\n");
    }
    let hex = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((count, hex))
}
