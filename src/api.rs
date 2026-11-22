//! JSON の API（作っている途中）。

use crate::db::Db;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

pub async fn not_yet(State(_db): State<Db>) -> Response {
    crate::error(StatusCode::NOT_IMPLEMENTED, "まだ無い")
}
