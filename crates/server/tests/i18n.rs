//! 外に出す文の言語（AI-48）。castlist は、頼みの `Accept-Language` を見て、誤りの文を日本語か英語で返す。
//! - 並びを前から見て、初めに当たった `ja`・`en`。header が無い・`*`・どちらでもない言語は、英語。
//! - 守りの層（Host・Origin・本文の上限・時間切れ）が断る文も、同じ決まりで返す。
//! - 誤りの種類（`X-Amzn-Errortype`）と状態の数は、言語で変わらない。
//!
//! ほかの試験は、共通の送り方（tests/common の `App::send`）が日本語を頼む。ここは `App::send_raw` で、何も足さずに送る。
mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::{App, SE1, ai, error_type, event, get, put};
use serde_json::Value;

/// 断られる要求（Host が違う）。言語は `language` のとおりに付ける（None なら付けない）。
fn foreign_host(language: Option<&str>) -> Request<Body> {
    let mut request = Request::get("/healthz").header(header::HOST, "evil.example");
    if let Some(language) = language {
        request = request.header(header::ACCEPT_LANGUAGE, language);
    }
    request.body(Body::empty()).unwrap()
}

/// 何も足さずに送り、状態・誤りの種類・`message` を返す。
async fn refused(app: &App, request: Request<Body>) -> (StatusCode, String, String) {
    let (status, headers, bytes): (StatusCode, HeaderMap, Vec<u8>) = app.send_raw(request).await;
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let json: Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"));
    let message = json["message"].as_str().unwrap_or_else(|| panic!("{body}"));
    (status, error_type(&headers).to_owned(), message.to_owned())
}

const HOST_JA: &str = "Host は 127.0.0.1 か localhost だけを受ける";
const HOST_EN: &str = "Only Host 127.0.0.1 or localhost is accepted";

#[tokio::test]
async fn 言語を付けない頼みには_英語で返す() {
    let app = App::start().await;
    let (status, kind, message) = refused(&app, foreign_host(None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(kind, "Forbidden");
    assert_eq!(message, HOST_EN);
}

#[tokio::test]
async fn 頼みの言語の並びを前から見て_初めに当たった言語で返す() {
    let app = App::start().await;
    for (language, want) in [
        ("ja", HOST_JA),
        ("ja-JP", HOST_JA),
        ("ja,en;q=0.9", HOST_JA),
        ("en", HOST_EN),
        ("en-US,ja;q=0.8", HOST_EN),
        ("fr-CH, fr;q=0.9, ja;q=0.8", HOST_JA),
        ("fr", HOST_EN),
        ("*", HOST_EN),
    ] {
        let (status, kind, message) = refused(&app, foreign_host(Some(language))).await;
        // 状態と種類は、言語で変わらない。
        assert_eq!(status, StatusCode::FORBIDDEN, "{language}");
        assert_eq!(kind, "Forbidden", "{language}");
        assert_eq!(message, want, "{language}");
    }
}

fn write_with_origin(language: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/api/actors")
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::ORIGIN, "http://127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(language) = language {
        request = request.header(header::ACCEPT_LANGUAGE, language);
    }
    request
        .body(Body::from(ai("se", "s-1").to_string()))
        .unwrap()
}

#[tokio::test]
async fn origin_の付いた書く要求を断る文も_頼みの言語で返す() {
    let app = App::start().await;
    let (_, kind, message) = refused(&app, write_with_origin(None)).await;
    assert_eq!(kind, "Forbidden");
    assert_eq!(message, "Pages in the browser cannot write");
    let (_, _, message) = refused(&app, write_with_origin(Some("ja"))).await;
    assert_eq!(message, "browser の中のページからは書けない");
}

fn too_large(language: Option<&str>) -> Request<Body> {
    let mut body = ai("se", "s-1").to_string().into_bytes();
    body.resize(castlist::BODY_LIMIT + 1, b' ');
    let mut request = Request::post("/api/actors")
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(language) = language {
        request = request.header(header::ACCEPT_LANGUAGE, language);
    }
    request.body(Body::from(body)).unwrap()
}

#[tokio::test]
async fn 本文の上限を超えた時の文も_頼みの言語で返す() {
    let app = App::start().await;
    let (status, kind, message) = refused(&app, too_large(None)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(kind, "PayloadTooLarge");
    assert_eq!(message, "The body is limited to 2 MiB");
    let (_, _, message) = refused(&app, too_large(Some("ja"))).await;
    assert_eq!(message, "本文は 2 MiB まで");
}

fn register(language: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/api/actors")
        .header(header::HOST, "127.0.0.1:8802")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(language) = language {
        request = request.header(header::ACCEPT_LANGUAGE, language);
    }
    request
        .body(Body::from(ai("se", "s-1").to_string()))
        .unwrap()
}

#[tokio::test]
async fn 時間切れの文も_頼みの言語で返す() {
    // 時間切れを短くし、1 本だけの書く接続を握って書く操作を待たせる（tests/guard.rs の時間切れの試験と同じ）。
    let app = App::start_with(castlist::Config {
        timeout: Duration::from_millis(200),
        ..castlist::Config::default()
    })
    .await;
    let held = app.db.write().acquire().await.unwrap();
    let (status, kind, message) = refused(&app, register(None)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(kind, "Unavailable");
    assert_eq!(
        message,
        "Took too long (200 ms). Wait a moment and send again"
    );
    let (_, _, message) = refused(&app, register(Some("ja"))).await;
    assert_eq!(
        message,
        "時間がかかりすぎた（200 ms）。少し待ってから送り直す"
    );
    drop(held);
}

#[tokio::test]
async fn 混んでいる時の文も_頼みの言語で返す() {
    // 書く接続を握ったまま、時間切れ（既定）より先に、書く接続の待ち（2 秒）が切れる。生成した handler の返す誤りの道。
    let app = App::start().await;
    let held = app.db.write().acquire().await.unwrap();
    let (status, kind, message) = refused(&app, register(Some("en"))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(kind, "Unavailable");
    assert_eq!(message, "Busy. Wait a moment and send again");
    let (_, _, message) = refused(&app, register(Some("ja"))).await;
    assert_eq!(message, "混んでいる。少し待ってから送り直す");
    drop(held);
}

#[tokio::test]
async fn 共通の送り方は_日本語を頼む() {
    // ほかの試験が使う送り方（App::send）は、言語を付けていない要求に日本語を頼む（roster と同じ）。
    let app = App::start().await;
    let (_, _, body) = app.send(foreign_host(None)).await;
    assert!(body.contains(HOST_JA), "{body}");
    // 付けてあれば、そのまま。
    let (_, _, body) = app.send(foreign_host(Some("en"))).await;
    assert!(body.contains(HOST_EN), "{body}");
}

/// 要求に言語を付ける。
fn in_language(mut request: Request<Body>, language: &'static str) -> Request<Body> {
    request
        .headers_mut()
        .insert(header::ACCEPT_LANGUAGE, language.parse().unwrap());
    request
}

const NO_ACTOR: &str = "019abcde-0000-7000-8000-000000000001";

#[tokio::test]
async fn 無いアクターの文も_頼みの言語で返す() {
    let app = App::start().await;
    let path = format!("/api/actors/{NO_ACTOR}");
    let (status, kind, message) = refused(&app, get(&path)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(kind, "NotFound");
    assert_eq!(message, format!("Actor {NO_ACTOR} does not exist"));
    let (_, _, message) = refused(&app, in_language(get(&path), "ja")).await;
    assert_eq!(message, format!("アクター {NO_ACTOR} は無い"));
}

#[tokio::test]
async fn 引数を断る文も_頼みの言語で返す() {
    let app = App::start().await;
    let (status, kind, message) = refused(&app, get("/api/actors")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(kind, "ValidationException");
    assert_eq!(
        message,
        "Give at least one filter (one of label, binding, activeFrom, activeTo, active=true, model, effort, launchedBy, skewed)"
    );
    let (_, _, message) = refused(&app, in_language(get("/api/actors"), "ja")).await;
    assert_eq!(
        message,
        "絞る条件を 1 つ以上付ける（label・binding・activeFrom・activeTo・active=true・model・effort・launchedBy・skewed のどれか）"
    );
}

#[tokio::test]
async fn 頁の印を断る文は_検索ごとの文を_頼みの言語で返す() {
    let app = App::start().await;
    // 崩れた印（16 進だが、字に戻せない）。
    let broken = "/api/actors?label=project:castlist&nextToken=ff";
    let (_, kind, message) = refused(&app, get(broken)).await;
    assert_eq!(kind, "ValidationException");
    assert_eq!(
        message,
        "Cannot read nextToken (pass the nextToken from the previous answer unchanged)"
    );
    let (_, _, message) = refused(&app, in_language(get(broken), "ja")).await;
    assert_eq!(
        message,
        "nextToken が読めない（前の答えの nextToken を、そのまま渡す）"
    );
    // ほかの検索の印（出来事の検索の「events\n7」を、アクターの検索に渡す）。
    let other = "/api/actors?label=project:castlist&nextToken=6576656e74730a37";
    let (_, _, message) = refused(&app, get(other)).await;
    assert_eq!(
        message,
        "Cannot read nextToken (it is not a nextToken of the actor search)"
    );
    let (_, _, message) = refused(&app, in_language(get(other), "ja")).await;
    assert_eq!(
        message,
        "nextToken が読めない（アクターの検索の nextToken ではない）"
    );
}

#[tokio::test]
async fn 書く操作を断る文も_頼みの言語で返す() {
    // DB の層が作る文（引けない by）。書く取引の中で作る文にも、頼みの言語が届く。
    let app = App::start().await;
    let body = event(SE1, "entered");
    let (status, kind, message) = refused(&app, put("/api/events/e-1", &body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(kind, "ValidationException");
    assert_eq!(
        message,
        format!("The actor in by ({SE1}) is not among the key bindings of issuer tpm")
    );
    let (_, _, message) = refused(&app, in_language(put("/api/events/e-1", &body), "ja")).await;
    assert_eq!(
        message,
        format!("by のアクター（{SE1}）が、出し手 tpm の鍵の付随する id に無い")
    );
}

/// 引数を付けて起こした `castlist-server` の、使い方の行（stderr）と終了コード。環境変数は、名指した物だけを渡す。
fn usage(env: &[(&str, &str)]) -> (Option<i32>, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_castlist-server"))
        .arg("--help")
        .env_clear()
        .envs(env.iter().copied())
        .output()
        .expect("castlist-server を起こす");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim_end().to_owned(),
    )
}

const USAGE_JA: &str = "使い方: castlist-server（環境変数 CASTLIST_DB・CASTLIST_LISTEN で起きる）";
const USAGE_EN: &str = "Usage: castlist-server (starts from the environment variables CASTLIST_DB and CASTLIST_LISTEN)";

#[test]
fn 使い方の行は_環境の言語で出し_無ければ英語() {
    assert_eq!(usage(&[]), (Some(2), USAGE_EN.to_owned()));
    assert_eq!(
        usage(&[("LANG", "ja_JP.UTF-8")]),
        (Some(2), USAGE_JA.to_owned())
    );
    assert_eq!(
        usage(&[("LANG", "en_US.UTF-8")]),
        (Some(2), USAGE_EN.to_owned())
    );
    // LC_ALL → LC_MESSAGES → LANG の順に見て、初めの空でない値で決める（空でない LC_ALL は、ほかを上書きする）。
    // C・POSIX は「訳さない」の意味なので、英語。
    assert_eq!(
        usage(&[("LC_ALL", "ja_JP.UTF-8"), ("LANG", "en_US.UTF-8")]),
        (Some(2), USAGE_JA.to_owned())
    );
    assert_eq!(
        usage(&[("LC_MESSAGES", "en_US.UTF-8"), ("LANG", "ja_JP.UTF-8")]),
        (Some(2), USAGE_EN.to_owned())
    );
    assert_eq!(
        usage(&[("LC_ALL", "C"), ("LANG", "ja_JP.UTF-8")]),
        (Some(2), USAGE_EN.to_owned())
    );
    assert_eq!(
        usage(&[("LC_ALL", "POSIX"), ("LANG", "ja_JP.UTF-8")]),
        (Some(2), USAGE_EN.to_owned())
    );
    assert_eq!(
        usage(&[("LC_ALL", ""), ("LANG", "ja_JP.UTF-8")]),
        (Some(2), USAGE_JA.to_owned())
    );
    assert_eq!(
        usage(&[("LC_ALL", ""), ("LANG", "C.UTF-8")]),
        (Some(2), USAGE_EN.to_owned())
    );
}
