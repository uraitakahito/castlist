//! 並べて来る要求の試験: 同じ物を 1 つにまとめ、番号に穴も重なりも無く、書く鍵を待つ間も読む物と /healthz は待たない。

mod common;

use axum::http::StatusCode;
use common::app::{App, ai, at, request, setting};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use tower::ServiceExt;

/// 外のキーの崩れが無いこと。
async fn assert_foreign_keys(app: &App) {
    let broken: Vec<(String,)> = sqlx::query_as("pragma foreign_key_check")
        .fetch_all(app.db.read())
        .await
        .unwrap();
    assert!(broken.is_empty(), "{broken:?}");
}

/// 要求を別の task で送り、状態と JSON を返す。
fn spawn(
    app: &App,
    method: &'static str,
    uri: String,
    body: Option<Value>,
) -> tokio::task::JoinHandle<(StatusCode, Value)> {
    let router = app.app.clone();
    tokio::spawn(async move {
        let response = router
            .oneshot(request(method, &uri, body.as_ref()))
            .await
            .unwrap();
        let status = response.status();
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 同じ鍵の_50_の登録は_1_つのアクターになる() {
    let app = App::start().await;
    let tasks: Vec<_> = (0..50)
        .map(|_| spawn(&app, "POST", "/v1/actors".into(), Some(ai("se", "same"))))
        .collect();
    let mut created = 0;
    let mut seen = BTreeSet::new();
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "{status} {answer}"
        );
        if status == StatusCode::CREATED {
            created += 1;
        }
        seen.insert((answer["id"].to_string(), answer["name"].to_string()));
    }
    assert_eq!(created, 1);
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(app.count("actors").await, 1);
    assert_foreign_keys(&app).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 違う鍵の_50_の登録は_穴も重なりも無く_1_から_50_の名になる() {
    let app = App::start().await;
    let tasks: Vec<_> = (0..50)
        .map(|i| {
            spawn(
                &app,
                "POST",
                "/v1/actors".into(),
                Some(ai("se", &format!("s-{i}"))),
            )
        })
        .collect();
    let mut names = BTreeSet::new();
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        names.insert(answer["name"].as_str().unwrap().to_owned());
    }
    let want: BTreeSet<String> = (1..=50).map(|n| format!("se.{n}")).collect();
    assert_eq!(names, want);
    assert_eq!(app.count("actors").await, 50);
    assert_foreign_keys(&app).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 同じ_model_と_effort_の_20_は_1_行になる() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let tasks: Vec<_> = (0..20)
        .map(|i| {
            spawn(
                &app,
                "PUT",
                format!("/v1/settings/same-{i}"),
                Some(setting(
                    "claude.session:s-1",
                    "claude-opus-5-5",
                    "high",
                    &at(0),
                )),
            )
        })
        .collect();
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "{status} {answer}"
        );
    }
    assert_eq!(app.count("settings").await, 1);
    assert_foreign_keys(&app).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 同じ知らない_model_の名を_50_のアクターが同時に送っても_1_行になる() {
    let app = App::start().await;
    for i in 0..50 {
        app.register(&ai("se", &format!("s-{i}"))).await;
    }
    let tasks: Vec<_> = (0..50)
        .map(|i| {
            spawn(
                &app,
                "PUT",
                format!("/v1/settings/new-{i}"),
                Some(setting(
                    &format!("claude.session:s-{i}"),
                    "claude-brand-new",
                    "high",
                    &at(0),
                )),
            )
        })
        .collect();
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    let rows: i64 = app
        .scalar("select count(*) from models where name = 'claude-brand-new'")
        .await;
    assert_eq!(rows, 1);
    assert_eq!(app.count("settings").await, 50);
    assert_foreign_keys(&app).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 書く鍵を_2_秒握っている間も_読む物は_100_ms_で答える() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let mut tx = app.db.write().begin().await.unwrap();
    sqlx::query("insert into models(name, first_seen_at) values('lock-holder', ?1)")
        .bind(at(0))
        .execute(&mut *tx)
        .await
        .unwrap();
    let held = Instant::now();
    let reads: [(&str, &str, Option<Value>); 3] = [
        ("GET", "/healthz", None),
        (
            "POST",
            "/v1/resolve",
            Some(json!({ "issuer": "tpm", "refs": ["claude.session:s-1"] })),
        ),
        ("GET", "/v1/graph?label=project:castlist", None),
    ];
    let mut slowest = Duration::ZERO;
    for round in 0..10 {
        let tasks: Vec<_> = (0..10)
            .map(|i| {
                let (method, uri, body) = reads[(round + i) % reads.len()].clone();
                let router = app.app.clone();
                tokio::spawn(async move {
                    let started = Instant::now();
                    let response = router
                        .oneshot(request(method, uri, body.as_ref()))
                        .await
                        .unwrap();
                    (uri, response.status(), started.elapsed())
                })
            })
            .collect();
        for task in tasks {
            let (uri, status, took) = task.await.unwrap();
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert!(took < Duration::from_millis(100), "{uri}: {took:?}");
            slowest = slowest.max(took);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    assert!(held.elapsed() < Duration::from_secs(2) + Duration::from_millis(500));
    tokio::time::sleep(Duration::from_secs(2).saturating_sub(held.elapsed())).await;
    tx.rollback().await.unwrap();
    println!("一番遅い読み: {slowest:?}");
    assert_foreign_keys(&app).await;
}

/// 糸が 1 本の runtime でも、書く鍵を待つ 20 の要求が /healthz を待たせない。
/// 2 秒より長く鍵を握れば、待った要求は「混んでいる」（503）で返る。
#[tokio::test(flavor = "current_thread")]
async fn 書く鍵を待つ_20_の要求があっても_healthz_は_100_ms_で答える() {
    let app = App::start().await;
    let conn = app.db.write().acquire().await.unwrap();
    let held = Instant::now();
    let tasks: Vec<_> = (0..20)
        .map(|i| {
            spawn(
                &app,
                "POST",
                "/v1/actors".into(),
                Some(ai("se", &format!("w-{i}"))),
            )
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let started = Instant::now();
    let response = app
        .app
        .clone()
        .oneshot(request("GET", "/healthz", None))
        .await
        .unwrap();
    let took = started.elapsed();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(took < Duration::from_millis(100), "{took:?}");
    tokio::time::sleep(Duration::from_millis(2300).saturating_sub(held.elapsed())).await;
    drop(conn);
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
        assert!(
            answer["error"]
                .as_str()
                .is_some_and(|e| e.contains("混んでいる")),
            "{answer}"
        );
    }
    assert_eq!(app.count("actors").await, 0);
    assert_foreign_keys(&app).await;
}

/// 系図を組む重い計算は、tokio の糸の外（spawn_blocking）で走る。500 のアクターの系図を並べて組ませている間も、
/// 引き当て（/v1/resolve）は 100 ms で答える。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn アクター_500_の系図を組む間も_引き当ては_100_ms_で答える() {
    let app = App::start().await;
    let root = app.register(&common::app::human("pm")).await;
    let root_ref = "tpm.person:pm".to_owned();
    assert!(root["id"].is_string());
    for i in 0..500 {
        app.register(&ai("se", &format!("s-{i}"))).await;
        // 3 つずつ枝分かれする木（0 は人の根から）。
        let from = if i == 0 {
            root_ref.clone()
        } else {
            format!("claude.session:s-{}", (i - 1) / 3)
        };
        let (status, answer) = app
            .put(
                &format!("/v1/links/l-{i}"),
                &common::app::link("launched", &from, &format!("claude.session:s-{i}")),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let renders: Vec<_> = (0..4)
        .map(|_| {
            let router = app.app.clone();
            let stop = stop.clone();
            tokio::spawn(async move {
                let mut done = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let response = router
                        .clone()
                        .oneshot(request("GET", "/graph/tree", None))
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::OK);
                    let bytes = http_body_util::BodyExt::collect(response.into_body())
                        .await
                        .unwrap()
                        .to_bytes();
                    let body = String::from_utf8(bytes.to_vec()).unwrap();
                    assert_eq!(body.matches("href=\"/actors/").count(), 501);
                    done += 1;
                }
                done
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut slowest = Duration::ZERO;
    for _ in 0..20 {
        let started = Instant::now();
        let (status, answer) = app
            .post(
                "/v1/resolve",
                &json!({ "issuer": "tpm", "refs": ["claude.session:s-1"] }),
            )
            .await;
        let took = started.elapsed();
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert!(took < Duration::from_millis(100), "{took:?}");
        slowest = slowest.max(took);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut rendered = 0;
    for task in renders {
        rendered += task.await.unwrap();
    }
    assert!(rendered > 0, "系図を 1 度も組めていない");
    println!("一番遅い引き当て: {slowest:?}・組んだ系図: {rendered}");
}

/// タイムラインを組む重い計算も、tokio の糸の外（spawn_blocking）で走る。500 のアクター（登場と model の替わり目つき）の
/// タイムラインを並べて組ませている間も、引き当て（/v1/resolve）は 100 ms で答える。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn アクター_500_のタイムラインを組む間も_引き当ては_100_ms_で答える() {
    let app = App::start().await;
    app.register(&common::app::human("pm")).await;
    let root_ref = "tpm.person:pm".to_owned();
    for i in 0..500 {
        app.register(&ai("se", &format!("s-{i}"))).await;
        let me = format!("claude.session:s-{i}");
        let from = if i == 0 {
            root_ref.clone()
        } else {
            format!("claude.session:s-{}", (i - 1) / 3)
        };
        let (status, answer) = app
            .put(
                &format!("/v1/links/l-{i}"),
                &common::app::link("launched", &from, &me),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        let (status, answer) = app
            .put(
                &format!("/v1/events/e-{i}"),
                &common::app::event(&me, "entered", &at(0)),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        let (status, answer) = app
            .put(
                &format!("/v1/settings/t-{i}"),
                &setting(&me, "claude-opus-5-5", "high", &at(0)),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let renders: Vec<_> = (0..4)
        .map(|_| {
            let router = app.app.clone();
            let stop = stop.clone();
            tokio::spawn(async move {
                let mut done = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let response = router
                        .clone()
                        .oneshot(request("GET", "/graph/timeline", None))
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::OK);
                    let bytes = http_body_util::BodyExt::collect(response.into_body())
                        .await
                        .unwrap()
                        .to_bytes();
                    let body = String::from_utf8(bytes.to_vec()).unwrap();
                    assert_eq!(body.matches("href=\"/actors/").count(), 501);
                    done += 1;
                }
                done
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut slowest = Duration::ZERO;
    for _ in 0..20 {
        let started = Instant::now();
        let (status, answer) = app
            .post(
                "/v1/resolve",
                &json!({ "issuer": "tpm", "refs": ["claude.session:s-1"] }),
            )
            .await;
        let took = started.elapsed();
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert!(took < Duration::from_millis(100), "{took:?}");
        slowest = slowest.max(took);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut rendered = 0;
    for task in renders {
        rendered += task.await.unwrap();
    }
    assert!(rendered > 0, "タイムラインを 1 度も組めていない");
    println!("一番遅い引き当て: {slowest:?}・組んだタイムライン: {rendered}");
}

/// 重い読み（図の点と線・タイムライン）は、読む接続 4 本のうち 2 本までしか同時に使わない。残りの 2 本は、いつも引き当てなどの
/// 軽い読みに空いている（4 本を重い読みが占めると、引き当てが空きを待ち、100 ms を超えることがあった。2026-11-23 の CI）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 重い読みは読む接続を_2_本までしか使わない() {
    let app = App::start().await;
    app.register(&common::app::human("pm")).await;
    for i in 0..300 {
        app.register(&ai("se", &format!("s-{i}"))).await;
        let me = format!("claude.session:s-{i}");
        let (status, answer) = app
            .put(
                &format!("/v1/links/l-{i}"),
                &common::app::link("launched", "tpm.person:pm", &me),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        let (status, answer) = app
            .put(
                &format!("/v1/events/e-{i}"),
                &common::app::event(&me, "entered", &at(0)),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let renders: Vec<_> = (0..8)
        .map(|i| {
            let router = app.app.clone();
            let stop = stop.clone();
            let path = if i % 2 == 0 {
                "/graph/timeline"
            } else {
                "/graph/tree"
            };
            tokio::spawn(async move {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let response = router
                        .clone()
                        .oneshot(request("GET", path, None))
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::OK);
                    let _ = http_body_util::BodyExt::collect(response.into_body())
                        .await
                        .unwrap();
                }
            })
        })
        .collect();
    let mut most = 0;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        let pool = app.db.read();
        let in_use = pool.size() as usize - pool.num_idle();
        most = most.max(in_use);
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for task in renders {
        task.await.unwrap();
    }
    assert!(most <= 2, "重い読みが同時に使った読む接続: {most} 本");
}
