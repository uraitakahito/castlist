//! 図（タイムライン・相関図）の点と線の DB の所。重い読みの接続（[`crate::db::HEAVY_READS`] 本）の 1 つの transaction の中で読む。

/// 図の SQL（名と SQL）。試験が `explain query plan` で、大きな表を端から読まないかを見る。
pub const LOOKUPS: &[(&str, &str)] = &[];
