//! 監査の検索（ListActors と GetLineage）の DB の所。読む接続の 1 つの transaction の中で、揃った写しを読む。
//!
//! - 時刻の条件は、どれも castlist が受けた時刻（`received_at`）で比べる。問いの時刻は DB に残す字（[`crate::db::stamp`]）に直して渡す。
//! - 頁は keyset で分ける（並べる値と seq を後ろから続ける）。
//! - SQL は決まった欠片を繋ぎ、値は bind するだけ（[`plans`] の試験が、索引で引くことを見る）。
use std::collections::BTreeMap;

use crate::db::{Db, Link, Ref};

/// アクターの一覧の並べ方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// 初めの登場（entered・reentered・forked）の受けた時刻。
    #[default]
    Entered,
    /// 最後の退場（cleared・exited・closed）の受けた時刻。
    Exited,
}

/// アクターを探す条件（どれも AND）。時刻は DB に残す字（UTC・ミリ秒まで・24 字）。
#[derive(Clone, Debug, Default)]
pub struct ActorQuery {
    /// ラベル（鍵・値）。どれも持つ。
    pub labels: Vec<(String, String)>,
    /// 付随する id と時刻: その時までに、その値を最後に受けたアクター。
    pub binding: Option<(Ref, String)>,
    pub active_from: Option<String>,
    pub active_to: Option<String>,
    pub kind: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub active: Option<bool>,
    pub launched_by: Option<String>,
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

impl ActorQuery {
    /// 索引で引ける条件があるか。
    pub fn narrowed(&self) -> bool {
        false
    }
}

/// 一覧の 1 行。時刻は DB の字。
#[derive(Clone, Debug)]
pub struct ActorItem {
    pub seq: i64,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub issuer: String,
    pub sent_at: String,
    pub received_at: String,
    pub clock_skew: bool,
    pub labels: BTreeMap<String, String>,
    pub first_entered_at: Option<String>,
    pub last_exited_at: Option<String>,
    pub last_exit_type: Option<String>,
    pub present: bool,
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

/// 検索の SQL（名と SQL）。試験が `explain query plan` で、大きな表を端から読まないかを見る。
pub fn plans() -> Vec<(&'static str, String)> {
    Vec::new()
}

impl Db {
    /// アクターを探す。
    pub async fn search_actors(&self, _q: &ActorQuery) -> Result<Page, sqlx::Error> {
        Err(sqlx::Error::Protocol("まだ無い".to_owned()))
    }

    /// 線を上り（人で止める）、下る。アクターが無ければ None。
    pub async fn lineage(
        &self,
        _id: &str,
        _up: u32,
        _down: u32,
        _kind: Option<&str>,
    ) -> Result<Option<Lineage>, sqlx::Error> {
        Err(sqlx::Error::Protocol("まだ無い".to_owned()))
    }
}
