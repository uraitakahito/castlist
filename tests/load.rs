//! 負荷の試験（ふだんは流さない）: 本物の binary を一時の DB で起こし、50 の客が 10 秒叩く。
//! 7 割が読む物（resolve・graph・監査の検索を含む。検索は読む物の 2 割）、3 割が書く物。誤りと 5xx が 0 で、p99 が 1 秒未満であること。
//! 基準は CI の機械の揺れに合わせた（同じ code で p99 が 67〜324 ms に揺れた。2026-11-22）。待たせないことそのものは
//! tests/concurrency.rs（書く鍵を握った間も読む物が 100 ms の内に答える、など）が見る。ここは、詰まって誤りや
//! 秒の単位の待ちが出ないかを見る。
//! `cargo test --locked --release --test load -- --ignored --nocapture`
// 起こす道具（tests/common）は同期で書く。
#![allow(clippy::disallowed_methods)]

mod common;

use common::Server;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const CLIENTS: usize = 50;
const SECONDS: u64 = 10;
const ACTORS: usize = 200;
const GROUPS: usize = 20;

/// 接続を使い回す、小さな HTTP/1.1 の客。
struct Client {
    stream: BufReader<TcpStream>,
}

impl Client {
    async fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        stream.set_nodelay(true).expect("nodelay");
        Self {
            stream: BufReader::new(stream),
        }
    }

    async fn send(&mut self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        let body = body.map(Value::to_string).unwrap_or_default();
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let stream = self.stream.get_mut();
        stream.write_all(head.as_bytes()).await.expect("write");
        stream.write_all(body.as_bytes()).await.expect("write");
        let mut line = String::new();
        self.stream.read_line(&mut line).await.expect("read");
        let status: u16 = line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let mut length = 0;
        loop {
            line.clear();
            self.stream.read_line(&mut line).await.expect("read");
            let header = line.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().expect("content-length");
            }
        }
        let mut bytes = vec![0; length];
        self.stream.read_exact(&mut bytes).await.expect("read");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

/// 種から作る、小さな擬似乱数。
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("時刻")
}

fn register(session: &str, group: &str, workspace: usize) -> Value {
    json!({
        "issuer": "tpm",
        "kind": "ai",
        "name_prefix": "se",
        "labels": { "group": group },
        "bindings": [
            { "type": "claude.session", "value": session },
            { "type": "cmux.workspace.ref", "value": format!("workspace:{workspace}") },
        ],
        "settings": { "model": "claude-opus-5-5", "effort": "high" },
        "at": now(),
    })
}

#[ignore = "負荷の試験。CI の job load と、手元で --release で流す"]
#[tokio::test(flavor = "multi_thread")]
async fn 五十の客が_10_秒叩いても_誤りが無く_p99_が_1_秒未満() {
    let server = Server::start();
    let port = server.port;

    // 下ごしらえ: 200 のアクターを 20 の組に分けて登録し、登場させ、起こした線を張る。
    let mut setup = Client::connect(port).await;
    let mut ids = Vec::new();
    for i in 0..ACTORS {
        let (status, answer) = setup
            .send(
                "POST",
                "/v1/actors",
                Some(&register(
                    &format!("s-{i}"),
                    &format!("g-{}", i % GROUPS),
                    i,
                )),
            )
            .await;
        assert_eq!(status, 201, "{answer}");
        ids.push(answer["id"].as_str().expect("id").to_owned());
        let event = json!({ "issuer": "tpm", "by": format!("claude.session:s-{i}"), "type": "entered", "at": now() });
        let (status, _) = setup
            .send("PUT", &format!("/v1/events/setup-{i}"), Some(&event))
            .await;
        assert_eq!(status, 201);
        if i >= GROUPS {
            let link = json!({ "issuer": "tpm", "type": "launched", "from": format!("claude.session:s-{}", i - GROUPS), "to": format!("claude.session:s-{i}"), "at": now() });
            let (status, _) = setup
                .send("PUT", &format!("/v1/links/setup-{i}"), Some(&link))
                .await;
            assert_eq!(status, 201);
        }
    }
    let ids = Arc::new(ids);

    let deadline = Instant::now() + Duration::from_secs(SECONDS);
    let tasks: Vec<_> = (0..CLIENTS)
        .map(|c| {
            let ids = Arc::clone(&ids);
            tokio::spawn(async move {
                let mut client = Client::connect(port).await;
                let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ ((c as u64 + 1) * 0x1000_0001));
                let mut took = Vec::new();
                let mut errors = Vec::new();
                let mut n = 0;
                while Instant::now() < deadline {
                    n += 1;
                    let actor = rng.below(ACTORS);
                    let by = format!("claude.session:s-{actor}");
                    let (method, path, body) = match rng.below(100) {
                        0..16 => (
                            "POST",
                            "/v1/resolve".to_owned(),
                            Some(json!({ "issuer": "tpm", "refs": [by, format!("claude.session:s-{}", rng.below(ACTORS)), "claude.session:none"] })),
                        ),
                        16..30 => ("GET", format!("/v1/graph?label=group:g-{}", rng.below(GROUPS)), None),
                        30..42 => ("GET", format!("/v1/actors/{}", ids[actor]), None),
                        42..48 => ("GET", format!("/v1/actors?binding=cmux.workspace.ref:workspace:{actor}"), None),
                        48..56 => ("GET", "/healthz".to_owned(), None),
                        // 監査の検索（読む物の 2 割）。
                        56..70 => {
                            let path = match rng.below(6) {
                                0 => format!("/v1/actors?label=group:g-{}&limit=20", rng.below(GROUPS)),
                                1 => format!("/v1/actors?label=group:g-{}&active_from={}", rng.below(GROUPS), now()),
                                2 => format!("/v1/actors/{}/lineage", ids[actor]),
                                3 => format!("/v1/actors/{}/settings?at={}", ids[actor], now()),
                                4 => format!("/v1/events?actor={}&type=reported_in&limit=20", ids[actor]),
                                _ => "/v1/settings?mismatch=true&limit=20".to_owned(),
                            };
                            ("GET", path, None)
                        }
                        70..80 => (
                            "PUT",
                            format!("/v1/events/load-{c}-{n}"),
                            Some(json!({ "issuer": "tpm", "by": by, "type": "reported_in", "detail": { "n": n }, "at": now() })),
                        ),
                        80..90 => {
                            let effort = ["low", "medium", "high", "xhigh", "max"][rng.below(5)];
                            (
                                "PUT",
                                format!("/v1/settings/load-{c}-{n}"),
                                Some(json!({ "issuer": "tpm", "by": by, "model": "claude-opus-5-5", "effort": effort, "source": "observed", "at": now() })),
                            )
                        }
                        _ => (
                            "POST",
                            "/v1/actors".to_owned(),
                            Some(register(&format!("load-{c}-{n}"), "new", actor)),
                        ),
                    };
                    let started = Instant::now();
                    let (status, answer) = client.send(method, &path, body.as_ref()).await;
                    took.push(started.elapsed());
                    if !(200..300).contains(&status) {
                        errors.push(format!("{method} {path}: {status} {answer}"));
                    }
                }
                (took, errors)
            })
        })
        .collect();
    let mut took = Vec::new();
    let mut errors = Vec::new();
    for task in tasks {
        let (t, e) = task.await.expect("客");
        took.extend(t);
        errors.extend(e);
    }
    took.sort();
    let at = |p: f64| took[((took.len() as f64 * p) as usize).min(took.len() - 1)];
    let (p50, p99) = (at(0.50), at(0.99));
    println!(
        "負荷: {} 件 / {SECONDS} 秒（{} 件/秒）・誤り {} 件・p50 {p50:?}・p99 {p99:?}・最大 {:?}",
        took.len(),
        took.len() as u64 / SECONDS,
        errors.len(),
        took.last().copied().unwrap_or_default()
    );
    assert!(
        errors.is_empty(),
        "{} 件の誤り。初めの 5 件: {:?}",
        errors.len(),
        &errors[..errors.len().min(5)]
    );
    assert!(p99 < Duration::from_secs(1), "p99 {p99:?}");
    drop(server);
}
