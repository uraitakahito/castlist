//! 守りの層。生成した道と手の道（backup・logs）の前に、外から順に掛ける（[`crate::app_with`]）:
//!
//! 1. [`stamp`]: どの答えにも型の推し量りを止める header などを付ける（断った答えにも付く）。
//! 2. 問い合わせごとの記録の 1 行（tower-http の TraceLayer。断った物も残る）。
//! 3. [`guard`]: `Host` と `Origin` で断る。`HEAD` は `GET` と同じに答え、本文を捨てる。
//! 4. [`timeout`]: 答えの頭までに時間がかかりすぎれば 503。本文を送る間（backup の file）は数えない（0.4.x の server と同じ）。
//! 5. [`limit`]: 本文を上限まで読む。超えれば 413（生成した骨は本文を自分で読むので、axum の上限が掛からない）。
//!
//! 断る答えは、生成した層の誤りと同じ restJson1 の形（[`reply`]）。型は定義（ai.common）の `Forbidden`・`PayloadTooLarge`・
//! `Unavailable` で、どの操作の誤りにも入っている（service の errors）。
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use http_body_util::{BodyExt, LengthLimitError, Limited};

use crate::BODY_LIMIT;
use crate::errors::reply;

/// host（castlist を動かす機械）の中からの要求だけを受ける（0.4.x の server の guard と同じ）。
/// - `Host` が 127.0.0.1・localhost でなければ断る（外の site が名前を付け替えて届ける要求を防ぐ）。
/// - `GET`・`HEAD` のほかの要求に `Origin` が付いていれば断る（browser の中のページからは書かせない。roster と `curl` は付けない）。
///
/// `HEAD` は `GET` にして通し、答えの本文を捨てる（状態と header は `GET` と同じ）。 0.4.x の server は axum の `get` の道が
/// `HEAD` にも答えていた。生成した骨の道は `GET` だけなので、ここで揃える。
pub async fn guard(mut request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.rsplit_once(':').map_or(v, |(name, _port)| name));
    if !matches!(host, Some("127.0.0.1" | "localhost")) {
        return reply(
            StatusCode::FORBIDDEN,
            "Forbidden",
            "Host は 127.0.0.1 か localhost だけを受ける",
        );
    }
    if request.method() != Method::GET
        && request.method() != Method::HEAD
        && request.headers().contains_key(header::ORIGIN)
    {
        return reply(
            StatusCode::FORBIDDEN,
            "Forbidden",
            "browser の中のページからは書けない",
        );
    }
    let head = request.method() == Method::HEAD;
    if head {
        *request.method_mut() = Method::GET;
    }
    let response = next.run(request).await;
    if head {
        // 本文を捨てる前に、本文の大きさが決まっていれば Content-Length を付ける（GET と同じ header にする。
        // 付けずに捨てると、外の層が空の本文の 0 を付ける）。流す本文（大きさが決まらない物）は付けない。
        let (mut parts, body) = response.into_parts();
        if !parts.headers.contains_key(header::CONTENT_LENGTH)
            && let Some(len) = http_body::Body::size_hint(&body).exact()
        {
            parts
                .headers
                .insert(header::CONTENT_LENGTH, HeaderValue::from(len));
        }
        Response::from_parts(parts, Body::empty())
    } else {
        response
    }
}

/// `limit` の内に答えの頭を返せなければ、503 で返す（待っている物は捨てる）。
pub async fn timeout(State(limit): State<Duration>, request: Request, next: Next) -> Response {
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "Unavailable",
            &format!(
                "時間がかかりすぎた（{}）。少し待ってから送り直す",
                length(limit)
            ),
        ),
    }
}

/// 長さの字（秒で割り切れれば秒、ほかは ms）。
fn length(d: Duration) -> String {
    if d.subsec_nanos() == 0 {
        format!("{} 秒", d.as_secs())
    } else {
        format!("{} ms", d.as_millis())
    }
}

/// 本文を [`BODY_LIMIT`] まで読んでから通す。超えれば 413 で断る（`Content-Length` の有る無しを問わない）。
/// castlist の本文は小さな JSON なので、先に読み切ってよい（生成した骨も、読み切ってから解く）。
pub async fn limit(request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    match Limited::new(body, BODY_LIMIT).collect().await {
        Ok(body) => {
            next.run(Request::from_parts(parts, Body::from(body.to_bytes())))
                .await
        }
        Err(e) if e.is::<LengthLimitError>() => reply(
            StatusCode::PAYLOAD_TOO_LARGE,
            "PayloadTooLarge",
            &format!("本文は {} MiB まで", BODY_LIMIT / (1024 * 1024)),
        ),
        // 送り手が途中で切った、など。生成した層が読めない本文に返すのと同じ型にする。
        Err(e) => {
            tracing::info!(error = %e, "本文を読めない");
            reply(
                StatusCode::BAD_REQUEST,
                "SerializationException",
                "本文を読めない",
            )
        }
    }
}

/// どの答えにも、型の推し量りを止める header と、referrer を送らせない header を付ける。
/// 画面（HTML）の答えは無いので、CSP は付けない（2.0.0 で画面を外した）。
pub async fn stamp(request: Request, next: Next) -> Response {
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
    response
}

#[cfg(test)]
mod tests {
    use super::length;
    use std::time::Duration;

    #[test]
    fn 長さの字() {
        assert_eq!(length(Duration::from_secs(5)), "5 秒");
        assert_eq!(length(Duration::from_millis(200)), "200 ms");
    }
}
