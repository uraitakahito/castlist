//! 接続・移行・SQL（作っている途中）。

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::fmt;
use std::path::{Path, PathBuf};

/// 引き当てと見る SQL（名と SQL）。試験が `explain query plan` で索引を使うかを見る。
pub const LOOKUPS: &[(&str, &str)] = &[];

#[derive(Clone)]
pub struct Db {
    read: SqlitePool,
    write: SqlitePool,
    #[allow(dead_code)]
    path: PathBuf,
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
    pub async fn open(path: &Path) -> Result<Self, OpenError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .map_err(OpenError::Db)?;
        let read = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.read_only(true))
            .await
            .map_err(OpenError::Db)?;
        Ok(Self {
            read,
            write,
            path: path.to_owned(),
        })
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
        self.write.close().await;
    }
}
