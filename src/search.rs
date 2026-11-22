//! 監査の検索（まだ無い）。

use crate::db::Db;
use crate::error;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

/// 検索の SQL（名と SQL）。試験が `explain query plan` で、大きな表を端から読まないかを見る。
pub fn plans() -> Vec<(&'static str, String)> {
    Vec::new()
}

/// まだ無い。
pub async fn todo(State(_db): State<Db>) -> Response {
    error(StatusCode::NOT_IMPLEMENTED, "まだ無い")
}
