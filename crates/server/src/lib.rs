//! 新しい castlist の server（AI-36）。
//!
//! 定義（model/）から作った骨（crates/castlist-sdk/）に処理（handler）を入れ、守りの層を前に掛ける。
//! 手の道のうち画面（[`ui`]）は、同じ Router に並べる（Smithy の定義の外）。backup・logs は、この後の PR で足す。
//! どの答えにも型の推し量りを止める header を付け、画面（HTML）の答えには CSP を付ける（[`stamp`]。今の server と同じ）。
mod actor;
mod actor_settings;
mod body;
pub mod db;
mod errors;
mod event;
mod generated;
mod get_graph;
pub mod graph;
mod lineage;
mod link;
mod list_actors;
mod list_events;
mod list_settings;
mod register;
mod resolve;
mod rows;
pub mod search;
mod setting;
mod timeline;
mod token;
mod ui;

use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use castlist_sdk::output::GetHealthOutput;
use castlist_sdk::{Castlist, CastlistConfig};

use crate::db::Db;
use crate::generated::Generated;

/// 画面の応答に付ける Content-Security-Policy。script と style は、castlist 自身が配る file だけを許す。
/// form は castlist 自身へだけ送れる（一覧の絞りの form は GET で `/` に送る）。今の server（src/lib.rs）と同じ。
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

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
        .list_actor_settings({
            let db = db.clone();
            move |input| actor_settings::list_actor_settings(db.clone(), input)
        })
        .get_actor_setting_at({
            let db = db.clone();
            move |input| actor_settings::get_actor_setting_at(db.clone(), input)
        })
        .list_settings({
            let db = db.clone();
            move |input| list_settings::list_settings(db.clone(), input)
        })
        .list_events({
            let db = db.clone();
            move |input| list_events::list_events(db.clone(), input)
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
        .get_graph({
            let db = db.clone();
            move |input| get_graph::get_graph(db.clone(), input)
        })
        .put_setting({
            let db = db.clone();
            move |input| setting::put_setting(db.clone(), input)
        })
        .build()
        .expect("どの操作にも handler がある");
    ui::routes(db)
        .fallback_service(Generated::new(generated))
        .layer(middleware::from_fn(guard))
        // 一番外に置く。guard が断った答えにも付く。
        .layer(middleware::from_fn(stamp))
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

/// true だけを受ける絞り（付けなければ絞らない）。false は、絞らないのか逆に絞るのかが読めないので断る（その訳）。
fn only_true(name: &str, value: Option<bool>) -> Result<bool, String> {
    match value {
        None => Ok(false),
        Some(true) => Ok(true),
        Some(false) => Err(format!("{name} は true だけ（絞らないなら付けない）")),
    }
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

/// どの答えにも、型の推し量りを止める header と、referrer を送らせない header を付け、画面（HTML）の答えには CSP を付ける
/// （今の server の src/lib.rs の stamp と同じ）。
async fn stamp(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    let html = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"));
    if html {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        );
    }
    response
}
