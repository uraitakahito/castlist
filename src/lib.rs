//! アクターの台帳のサーバー。Claude Code の session 1 つ（起動 1 回）ごとに、本当の id と名を出し、
//! 登場・退場と、誰が誰を起こしたかを足すだけの記録で持つ（作っている途中）。

use axum::Router;

/// 問い合わせの道。
pub fn router() -> Router {
    Router::new()
}
