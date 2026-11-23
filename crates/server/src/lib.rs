//! 新しい castlist の server（AI-36）。
//!
//! 定義（model/）から作った骨（crates/castlist-sdk/）に処理（handler）を入れ、守りの層を前に掛ける。
//! 手の道（画面・backup・logs）は、この後の PR で同じ Router に足す。
pub mod db;
mod generated;
mod register;

use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use castlist_sdk::output::GetHealthOutput;
use castlist_sdk::{Castlist, CastlistConfig};

use crate::db::Db;
use crate::generated::Generated;

/// 動かす Router。DB は、どの handler も同じ物を使う。
pub fn app(db: Arc<Db>) -> Router {
    let generated = Castlist::builder(CastlistConfig::builder().build())
        .get_health(get_health)
        .register_actor(move |input| register::register(db.clone(), input))
        .build()
        .expect("どの操作にも handler がある");
    Router::new()
        .fallback_service(Generated::new(generated))
        .layer(middleware::from_fn(guard))
}

async fn get_health(_input: castlist_sdk::input::GetHealthInput) -> GetHealthOutput {
    GetHealthOutput {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        now: aws_smithy_types::DateTime::from(std::time::SystemTime::now()),
    }
}

/// host（castlist を動かす機械）の中からの要求だけを受ける（今の castlist の Host と Origin の守り）。
/// 時間切れ・問い合わせの記録・断る時の誤りの形・HEAD の扱いは、守りの層の PR（AI-36 の段 2）で足す。
/// - `Host` が 127.0.0.1・localhost でなければ断る。
/// - `GET`・`HEAD` のほかの要求に `Origin` が付いていれば断る（browser の中のページからは書かせない）。
async fn guard(request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.rsplit_once(':').map_or(v, |(name, _port)| name));
    if !matches!(host, Some("127.0.0.1" | "localhost")) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request.method() != Method::GET
        && request.method() != Method::HEAD
        && request.headers().contains_key(header::ORIGIN)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}
