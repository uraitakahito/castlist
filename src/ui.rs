//! 画面: アクターの一覧（`/`）とプロフィール（`/actors/{id}`）と、それが読む CSS（`/static/site.css`）。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// `GET /`: アクターの一覧（まだ作っていない）。
pub async fn list() -> Response {
    (StatusCode::NOT_IMPLEMENTED, "まだ作っていない").into_response()
}

/// `GET /actors/{id}`: プロフィール（まだ作っていない）。
pub async fn profile() -> Response {
    (StatusCode::NOT_IMPLEMENTED, "まだ作っていない").into_response()
}

/// `GET /static/site.css`（まだ作っていない）。
pub async fn site_css() -> Response {
    (StatusCode::NOT_IMPLEMENTED, "まだ作っていない").into_response()
}

/// 出来事の種類の、画面に出す日本語の名（まだ作っていない）。
pub fn event_name(kind: &str) -> &str {
    kind
}

#[cfg(test)]
mod tests {
    use super::event_name;
    use crate::api::EVENT_TYPES;

    #[test]
    fn 出来事の種類はみな日本語の名を持つ() {
        let names: Vec<&str> = EVENT_TYPES.iter().map(|t| event_name(t)).collect();
        assert_eq!(
            names,
            [
                "登場",
                "再登場",
                "分身",
                "要約",
                "交代",
                "退場",
                "幕引き",
                "名乗り",
                "model 不明",
                "取り消し"
            ]
        );
        // 知らない種類は、id のまま出す。
        assert_eq!(event_name("unknown"), "unknown");
    }
}
