//! 操作に共通の誤りの作り方。
use castlist_sdk::error::{
    Conflict, GetActorError, GetLineageError, InternalError, ListActorsError, PutEventError,
    PutLinkError, PutSettingError, ResolveActorsError, Unavailable, ValidationException,
};

use crate::db::WriteError;

/// server の中の失敗（DB の誤りなど）。訳は記録に書き、答えは決まった字にする（中の誤りの字を外に出さない）。
pub fn internal(operation: &str, error: &dyn std::fmt::Display) -> InternalError {
    tracing::error!(operation, %error, "server の中で失敗した");
    InternalError {
        message: "castlist の中で失敗した（記録を見る）".to_owned(),
    }
}

/// 混んでいる（503。送り直せば通る見込み）。
pub fn busy() -> Unavailable {
    Unavailable {
        message: "混んでいる。少し待ってから送り直す".to_owned(),
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
                    Failure::Busy => busy().into(),
                    Failure::Internal(e) => e.into(),
                }
            }
        }
    )*};
}

from_failure!(
    GetActorError,
    ResolveActorsError,
    ListActorsError,
    GetLineageError
);

/// 送り手の id で足す操作（Put…）の失敗: 中身の誤り（400）・同じ id で違う中身など（409）・混んでいる（503）・中の失敗（500）。
pub enum PutFailure {
    Invalid(String),
    Conflict(String),
    Busy,
    Internal(InternalError),
}

impl PutFailure {
    /// 書けない訳を、答えの誤りにする。
    pub fn write(operation: &str, error: WriteError) -> Self {
        match error {
            WriteError::Busy => PutFailure::Busy,
            WriteError::Invalid(message) => PutFailure::Invalid(message),
            WriteError::Conflict(message) => PutFailure::Conflict(message),
            WriteError::Db(e) => PutFailure::Internal(internal(operation, &e)),
        }
    }
}

impl From<Failure> for PutFailure {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Busy => PutFailure::Busy,
            Failure::Internal(e) => PutFailure::Internal(e),
        }
    }
}

macro_rules! from_put_failure {
    ($($error:ty),*) => {$(
        impl From<PutFailure> for $error {
            fn from(failure: PutFailure) -> Self {
                match failure {
                    PutFailure::Invalid(message) => ValidationException {
                        message,
                        field_list: None,
                    }
                    .into(),
                    PutFailure::Conflict(message) => Conflict { message }.into(),
                    PutFailure::Busy => busy().into(),
                    PutFailure::Internal(e) => e.into(),
                }
            }
        }
    )*};
}

from_put_failure!(PutEventError, PutLinkError, PutSettingError);
