//! 新しい castlist の server（AI-36）。
//!
//! 定義（model/）から作った骨（crates/castlist-sdk/）に処理（handler）を入れ、守りの層を前に掛ける。
//! 手の道（画面・backup・logs）は、この後の PR で同じ Router に足す。
mod actor;
mod body;
pub mod db;
mod errors;
mod event;
mod generated;
mod lineage;
mod link;
mod list_actors;
mod register;
mod resolve;
mod rows;
pub mod search;
mod setting;

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
        .register_actor({
            let db = db.clone();
            move |input| register::register(db.clone(), input)
        })
        .get_actor({
            let db = db.clone();
            move |input| actor::get_actor(db.clone(), input)
        })
        .list_actors({
            let db = db.clone();
            move |input| list_actors::list_actors(db.clone(), input)
        })
        .get_lineage({
            let db = db.clone();
            move |input| lineage::get_lineage(db.clone(), input)
        })
        .resolve_actors({
            let db = db.clone();
            move |input| resolve::resolve(db.clone(), input)
        })
        .put_event({
            let db = db.clone();
            move |input| event::put_event(db.clone(), input)
        })
        .put_link({
            let db = db.clone();
            move |input| link::put_link(db.clone(), input)
        })
        .put_setting(move |input| setting::put_setting(db.clone(), input))
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

/// DB の字の時刻（RFC 3339）を、答えの時刻にする。読めなければ、その訳。
fn datetime(s: &str) -> Result<aws_smithy_types::DateTime, String> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .ok()
        .and_then(|t| aws_smithy_types::DateTime::from_nanos(t.unix_timestamp_nanos()).ok())
        .ok_or_else(|| format!("DB の時刻が読めない: {s}"))
}

/// 問いの時刻を、DB に残す字（UTC・ミリ秒まで・24 字。ミリ秒より下は切る）にする。字の順で比べられる。
fn stored_time(name: &str, t: &aws_smithy_types::DateTime) -> Result<String, String> {
    time::OffsetDateTime::from_unix_timestamp_nanos(t.as_nanos())
        .ok()
        .filter(|t| (0..=9999).contains(&t.year()))
        .map(db::stamp)
        .ok_or_else(|| format!("{name} は 0000 年から 9999 年までの時刻"))
}

/// 送り手の時刻を、time の時刻にする。扱えなければ、その訳。
fn sent_time(t: &aws_smithy_types::DateTime) -> Result<time::OffsetDateTime, String> {
    time::OffsetDateTime::from_unix_timestamp_nanos(t.as_nanos())
        .map_err(|_| "sentAt が扱える時刻の外".to_owned())
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
