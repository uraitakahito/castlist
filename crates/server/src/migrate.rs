//! 今の server の DB（`GET /v1/backup` の写し）を、新しい server の DB へ移す（`castlist-next migrate --from <写し> --to <新しい DB>`）。
use std::fmt;
use std::path::Path;

use crate::db::OpenError;

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
}

impl Summary {
    /// どの表も数と digest が合い、seq が続き、外のキーが崩れていないか。
    pub fn ok(&self) -> bool {
        self.tables.iter().all(TableCheck::ok)
            && self.sequences.iter().all(SeqCheck::ok)
            && self.broken.is_empty()
    }

    /// 名で表の突き合わせを引く。
    pub fn table(&self, name: &str) -> Option<&TableCheck> {
        self.tables.iter().find(|t| t.table == name)
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for t in &self.tables {
            writeln!(f, "{} {}", t.table, if t.ok() { "ok" } else { "NG" })?;
        }
        Ok(())
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
            Self::Mismatch(summary) => write!(f, "突き合わせが合わない\n{summary}"),
        }
    }
}

impl std::error::Error for MigrateError {}

/// 今の server の DB の写し `from` を、新しい DB `to` へ移す。
pub async fn migrate(_from: &Path, _to: &Path) -> Result<Summary, MigrateError> {
    Err(MigrateError::Db(sqlx::Error::Protocol("まだ無い".into())))
}

/// 今の server の DB の写し `from` と、新しい DB `to` の中身を突き合わせる（どちらにも書かない）。
pub async fn verify(_from: &Path, _to: &Path) -> Result<Summary, MigrateError> {
    Err(MigrateError::Db(sqlx::Error::Protocol("まだ無い".into())))
}
