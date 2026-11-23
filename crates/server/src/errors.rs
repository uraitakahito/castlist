//! 操作に共通の誤りの作り方。
use castlist_sdk::error::InternalError;

/// server の中の失敗（DB の誤りなど）。訳は記録に書き、答えは決まった字にする（中の誤りの字を外に出さない）。
pub fn internal(operation: &str, error: &dyn std::fmt::Display) -> InternalError {
    tracing::error!(operation, %error, "server の中で失敗した");
    InternalError {
        message: "castlist の中で失敗した（記録を見る）".to_owned(),
    }
}
