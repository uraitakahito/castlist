//! 接続・移行・SQL。SQL は動く時に組む形（`query`・`query_as`）で書く。
//!
//! - 書く接続は 1 本だけ。番号（n）を数える所と、同じ物かを見る所が 1 列に並ぶ。
//! - 読む接続は 4 本（軽い読み）と 2 本（重い読み）で、読むだけ。WAL なので、書いている間も待たずに読める。
//! - どの表も足すだけ。この file の SQL は insert と select だけで、`ci/check-sql.sh` が見張る。
//!   「最新」は seq だけで決め、送り手の時刻（sent_at）では決めない。
use std::fmt;
use std::path::Path;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

/// 書く接続を待つ長さ。過ぎれば「混んでいる」で返す。
pub const WRITE_WAIT: Duration = Duration::from_secs(2);
/// 軽い読みの接続の本数。
pub const READS: u32 = 4;
/// 重い読み（図の点と線・タイムライン）だけが使う、読むだけの接続の本数。
pub const HEAVY_READS: u32 = 2;

pub struct Db {
    read: SqlitePool,
    heavy: SqlitePool,
    write: SqlitePool,
}

/// 付随する id 1 つ。
#[derive(Clone, Debug)]
pub struct Ref {
    pub kind: String,
    pub value: String,
}

/// 登録の中身（形は handler が見てある）。
pub struct NewActor<'a> {
    pub issuer: &'a str,
    pub kind: &'a str,
    pub prefix: &'a str,
    pub labels: &'a [(String, String)],
    /// 初めの物が鍵。
    pub bindings: &'a [Ref],
    pub sent_at: time::OffsetDateTime,
    /// 頼んだ model と effort。
    pub settings: Option<(&'a str, &'a str)>,
}

/// アクターを指す時の短い形。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorRef {
    pub id: String,
    pub name: String,
    pub kind: String,
}

/// 書けない訳。
#[derive(Debug)]
pub enum WriteError {
    /// 書く接続が空かない（混んでいる）。
    Busy,
    /// 定義の形では言えない、中身の誤り。
    Invalid(String),
    Db(sqlx::Error),
}

/// DB を開けない訳。
#[derive(Debug)]
pub enum OpenError {
    Db(sqlx::Error),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Db(e) => write!(f, "DB を開けない: {e}"),
        }
    }
}

impl std::error::Error for OpenError {}

impl Db {
    /// 開く（まだ移行も検査もしない）。
    pub async fn open(path: &Path) -> Result<Self, OpenError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Delete);
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .map_err(OpenError::Db)?;
        let heavy = SqlitePoolOptions::new()
            .max_connections(HEAVY_READS)
            .connect_with(options.clone())
            .await
            .map_err(OpenError::Db)?;
        let read = SqlitePoolOptions::new()
            .max_connections(READS)
            .connect_with(options)
            .await
            .map_err(OpenError::Db)?;
        Ok(Self { read, heavy, write })
    }

    /// 軽い読みの接続（読むだけ）。
    pub fn read(&self) -> &SqlitePool {
        &self.read
    }

    /// 重い読みだけが使う接続（読むだけ）。
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

    /// アクターを登録する（まだ無い）。
    pub async fn register(&self, _new: &NewActor<'_>) -> Result<(ActorRef, bool), WriteError> {
        Err(WriteError::Invalid("まだ登録できない".to_owned()))
    }
}
