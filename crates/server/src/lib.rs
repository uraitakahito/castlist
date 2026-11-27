//! 新しい castlist の server（AI-36）。
//!
//! 定義（model/）から作った骨（crates/castlist-sdk/）に処理（handler）を入れ、守りの層（[`guard`]）を前に掛ける。
//! 手の道（Smithy の定義の外）の backup・logs（[`hand`]）は、同じ Router に並べる。画面は持たない（2.0.0 で外した。
//! 画面は、API を通して callboard が出す）。
//! 起こす binary（src/main.rs）は、 0.4.x の server と同じく記録の file（[`runlog`]）を DB の隣の `logs/` に残す。
mod actor;
mod actor_settings;
mod body;
pub mod db;
mod errors;
mod event;
mod generated;
mod get_graph;
mod get_timeline;
pub mod graph;
mod guard;
mod hand;
mod lineage;
mod link;
mod list_actors;
mod list_events;
mod list_settings;
mod list_usage;
mod register;
mod resolve;
mod rows;
pub mod runlog;
pub mod search;
mod setting;
mod token;
mod usage;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::middleware;
use castlist_sdk::error::GetHealthError;
use castlist_sdk::output::GetHealthOutput;
use castlist_sdk::{Castlist, CastlistConfig};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::db::Db;
use crate::generated::Generated;

/// 1 つの問い合わせにかけてよい長さ。過ぎれば 503 `Unavailable` で返す（0.4.x の server と同じ）。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// 本文の上限（axum の既定と同じ 2 MiB）。超えれば 413 `PayloadTooLarge` で返す。
pub const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// 守りの層の設定。
#[derive(Clone)]
pub struct Config {
    /// 1 つの問い合わせにかけてよい長さ（既定は [`REQUEST_TIMEOUT`]。試験は短くする）。
    pub timeout: Duration,
    /// 記録の置き場。あれば、残した記録を読む道 `GET /api/logs` が付く。
    pub runlog: Option<runlog::RunLog>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timeout: REQUEST_TIMEOUT,
            runlog: None,
        }
    }
}

/// 動かす Router（記録を読む道 `GET /api/logs` は無い。試験と、記録の置き場が無い時）。
pub fn app(db: Arc<Db>) -> Router {
    app_with(db, Config::default())
}

/// 動かす Router。DB は、どの handler も同じ物を使う。
pub fn app_with(db: Arc<Db>, config: Config) -> Router {
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
        .get_timeline({
            let db = db.clone();
            move |input| get_timeline::get_timeline(db.clone(), input)
        })
        .put_setting({
            let db = db.clone();
            move |input| setting::put_setting(db.clone(), input)
        })
        .put_usage({
            let db = db.clone();
            move |input| usage::put_usage(db.clone(), input)
        })
        .list_usage({
            let db = db.clone();
            move |input| list_usage::list_usage(db.clone(), input)
        })
        .build()
        .expect("どの操作にも handler がある");
    hand::routes(db, config.runlog)
        .fallback_service(Generated::new(generated))
        .layer(middleware::from_fn(guard::limit))
        .layer(middleware::from_fn_with_state(
            config.timeout,
            guard::timeout,
        ))
        .layer(middleware::from_fn(guard::guard))
        // 問い合わせごとの 1 行（記録の file にだけ書く。src/runlog.rs の init）。guard が断った物も残る。 0.4.x の server と同じ。
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        // 一番外に置く。guard が断った答えにも付く。
        .layer(middleware::from_fn(guard::stamp))
}

async fn get_health(
    _input: castlist_sdk::input::GetHealthInput,
) -> Result<GetHealthOutput, GetHealthError> {
    Ok(GetHealthOutput {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        now: aws_smithy_types::DateTime::from(std::time::SystemTime::now()),
    })
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
