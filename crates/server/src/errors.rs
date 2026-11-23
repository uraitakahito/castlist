//! 操作に共通の誤りの作り方。
use castlist_sdk::error::{GetActorError, InternalError, ResolveActorsError, Unavailable};

/// server の中の失敗（DB の誤りなど）。訳は記録に書き、答えは決まった字にする（中の誤りの字を外に出さない）。
pub fn internal(operation: &str, error: &dyn std::fmt::Display) -> InternalError {
    tracing::error!(operation, %error, "server の中で失敗した");
    InternalError {
        message: "castlist の中で失敗した（記録を見る）".to_owned(),
    }
}

/// 読む操作の失敗: 混んでいる（503。送り直せば通る見込み）か、server の中の失敗（500）。
pub enum Failure {
    Busy,
    Internal(InternalError),
}

impl Failure {
    /// DB の誤り。接続を待ちきれなければ混んでいる、ほかは中の失敗。
    pub fn db(operation: &str, error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::PoolTimedOut => Failure::Busy,
            error => Failure::Internal(internal(operation, &error)),
        }
    }

    /// 残っている行を答えの形に直せない（中の失敗）。
    pub fn data(operation: &str, error: &dyn std::fmt::Display) -> Self {
        Failure::Internal(internal(operation, error))
    }
}

macro_rules! from_failure {
    ($($error:ty),*) => {$(
        impl From<Failure> for $error {
            fn from(failure: Failure) -> Self {
                match failure {
                    Failure::Busy => Unavailable {
                        message: "混んでいる。少し待ってから送り直す".to_owned(),
                    }
                    .into(),
                    Failure::Internal(e) => e.into(),
                }
            }
        }
    )*};
}

from_failure!(GetActorError, ResolveActorsError);
