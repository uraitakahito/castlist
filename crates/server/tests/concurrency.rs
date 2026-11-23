//! 並べて来る書き込みの試験: 同じ鍵は 1 つのアクターにまとめ、名の番号に穴も重なりも無い。
//! 同じ id・同じ model と effort・同じ知らない名は、並べて来ても 1 行にまとめる。
//! 書く鍵を握られている間も、読む操作は待たずに答える。
//! 重い読み（図の点と線・タイムライン）は重い読みの接続だけを使い、引き当てを待たせない。
mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::sample::{self, KEY, SONNET, sample};
use common::{App, PM, SE1, ai, at, event, get, human, link, post, put, setting};
use serde_json::{Value, json};
use tower::ServiceExt;

/// 要求を別の task で送り、状態と JSON を返す。
fn spawn(app: &App, request: Request<Body>) -> tokio::task::JoinHandle<(StatusCode, Value)> {
    let router = app.app.clone();
    tokio::spawn(async move {
        let response = router.oneshot(request).await.unwrap();
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

/// 外のキーの崩れが無いこと。
async fn assert_foreign_keys(app: &App) {
    let broken: Vec<String> = app
        .column("select \"table\" from pragma_foreign_key_check")
        .await;
    assert!(broken.is_empty(), "{broken:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_registrations_with_the_same_key_make_one_actor() {
    let app = App::start().await;
    let tasks: Vec<_> = (0..50)
        .map(|_| spawn(&app, post("/api/actors", &ai("se", "same"))))
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
        seen.insert(answer["actor"].to_string());
    }
    assert_eq!(created, 1);
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(app.count("actors").await, 1);
    assert_eq!(app.count("bindings").await, 1);
    assert_eq!(app.count("labels").await, 1);
    assert_foreign_keys(&app).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_registrations_with_different_keys_are_named_1_to_50_without_gaps_or_duplicates() {
    let app = App::start().await;
    let tasks: Vec<_> = (0..50)
        .map(|i| spawn(&app, post("/api/actors", &ai("se", &format!("s-{i}")))))
        .collect();
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        names.insert(answer["actor"]["name"].as_str().unwrap().to_owned());
        ids.insert(answer["actor"]["id"].as_str().unwrap().to_owned());
    }
    let want: BTreeSet<String> = (1..=50).map(|n| format!("se.{n}")).collect();
    assert_eq!(names, want);
    assert_eq!(ids.len(), 50);
    assert_eq!(app.count("actors").await, 50);
    assert_foreign_keys(&app).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn twenty_settings_with_the_same_model_and_effort_for_one_actor_make_one_row() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let body = setting(SE1, "claude-opus-5-5", "high");
    let tasks: Vec<_> = (0..20)
        .map(|i| spawn(&app, put(&format!("/api/settings/same-{i}"), &body)))
        .collect();
    let mut created = 0;
    let mut rows = BTreeSet::new();
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "{status} {answer}"
        );
        if status == StatusCode::CREATED {
            created += 1;
        }
        // 足さなかった物も、足した 1 行を返す。
        rows.insert(answer.to_string());
    }
    assert_eq!(created, 1);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(app.count("settings").await, 1);
    assert_foreign_keys(&app).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_unknown_model_from_fifty_actors_at_once_is_one_row_in_the_list() {
    let app = App::start().await;
    for i in 0..50 {
        app.register(&ai("se", &format!("s-{i}"))).await;
    }
    let tasks: Vec<_> = (0..50)
        .map(|i| {
            let body = setting(&format!("claude.session:s-{i}"), "claude-brand-new", "high");
            spawn(&app, put(&format!("/api/settings/new-{i}"), &body))
        })
        .collect();
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    let rows: Vec<i64> = app
        .column("select count(*) from models where name = 'claude-brand-new'")
        .await;
    assert_eq!(rows, vec![1]);
    assert_eq!(app.count("settings").await, 50);
    assert_foreign_keys(&app).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn twenty_puts_of_the_same_event_at_once_add_one_row_and_answer_the_same() {
    let app = App::start().await;
    app.register(&ai("se", "s-1")).await;
    let body = event(SE1, "entered");
    let tasks: Vec<_> = (0..20)
        .map(|_| spawn(&app, put("/api/events/e-1", &body)))
        .collect();
    let mut created = 0;
    let mut answers = BTreeSet::new();
    for task in tasks {
        let (status, answer) = task.await.unwrap();
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "{status} {answer}"
        );
        if status == StatusCode::CREATED {
            created += 1;
        }
        answers.insert(answer.to_string());
    }
    assert_eq!(created, 1);
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(app.count("events").await, 1);
    assert_foreign_keys(&app).await;
}

/// 今の server の tests/search.rs と同じ: 書く transaction を開いて書き、2 秒握っている間に、読む操作を 10 回ずつ並べて送る。
/// どれも 200 で、100 ms のうちに答える（WAL なので、読む接続は書く鍵を待たない）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reads_answer_within_100_ms_while_the_write_lock_is_held_for_2_seconds() {
    let s = sample().await;
    let app = &s.app;
    let se2 = s.id("se.2").to_owned();
    let tpm = s.id("tpm.1").to_owned();
    let reads = |round: usize| -> Vec<(String, Request<Body>)> {
        let resolve = json!({ "issuer": "tpm", "refs": [sample::SE2, sample::TPM] });
        vec![
            (
                "ListActors".to_owned(),
                get(&format!("/api/actors?label=tpm.key:{KEY}")),
            ),
            (
                "GetLineage".to_owned(),
                get(&format!("/api/actors/{se2}/lineage")),
            ),
            (
                "GetActorSettingAt".to_owned(),
                get(&format!(
                    "/api/actors/{se2}/settings/at?time={}",
                    s.model_changed
                )),
            ),
            (
                "ListSettings mismatch".to_owned(),
                get("/api/settings?mismatch=true"),
            ),
            (
                "ListSettings model".to_owned(),
                get(&format!("/api/settings?model={SONNET}")),
            ),
            (
                "ListEvents type".to_owned(),
                get("/api/events?type=model_unreadable"),
            ),
            (
                "ListEvents actor".to_owned(),
                get(&format!("/api/events?actor={tpm}")),
            ),
            ("GetActor".to_owned(), get(&format!("/api/actors/{tpm}"))),
            ("ResolveActors".to_owned(), post("/api/resolve", &resolve)),
        ]
        .into_iter()
        .map(|(name, request)| (format!("{name}（{round} 回目）"), request))
        .collect()
    };
    let mut tx = app.db.write().begin().await.unwrap();
    sqlx::query("insert into models(name, first_seen_at) values('lock-holder', ?1)")
        .bind(at(0))
        .execute(&mut *tx)
        .await
        .unwrap();
    let held = Instant::now();
    let mut slowest = Duration::ZERO;
    for round in 1..=10 {
        let tasks: Vec<_> = reads(round)
            .into_iter()
            .map(|(name, request)| {
                let router = app.app.clone();
                tokio::spawn(async move {
                    let started = Instant::now();
                    let response = router.oneshot(request).await.unwrap();
                    (name, response.status(), started.elapsed())
                })
            })
            .collect();
        for task in tasks {
            let (name, status, took) = task.await.unwrap();
            assert_eq!(status, StatusCode::OK, "{name}");
            assert!(took < Duration::from_millis(100), "{name}: {took:?}");
            slowest = slowest.max(took);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    // 読む操作を送り終えるまで、書く鍵を握ったままだった。
    assert!(held.elapsed() < Duration::from_secs(2) + Duration::from_millis(500));
    tokio::time::sleep(Duration::from_secs(2).saturating_sub(held.elapsed())).await;
    tx.rollback().await.unwrap();
    println!("一番遅い読み: {slowest:?}");
}

/// タイムラインを組む重い計算は、tokio の糸の外（spawn_blocking）で走り、DB は重い読みの接続だけで読む。
/// 500 のアクター（登場と model の替わり目つき）のタイムラインを並べて組ませている間も、引き当て（ResolveActors）は 100 ms で答える。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resolving_answers_in_100_ms_while_timelines_of_500_actors_are_built() {
    let app = App::start().await;
    app.register(&human("pm")).await;
    for i in 0..500 {
        app.register(&ai("se", &format!("s-{i}"))).await;
        let me = format!("claude.session:s-{i}");
        let from = if i == 0 {
            PM.to_owned()
        } else {
            format!("claude.session:s-{}", (i - 1) / 3)
        };
        for (uri, body) in [
            (format!("/api/links/l-{i}"), link("launched", &from, &me)),
            (format!("/api/events/e-{i}"), event(&me, "entered")),
            (
                format!("/api/settings/t-{i}"),
                setting(&me, "claude-sonnet-5", "high"),
            ),
        ] {
            let (status, _, answer) = app.put(&uri, &body).await;
            assert_eq!(status, StatusCode::CREATED, "{uri}: {answer}");
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let renders: Vec<_> = (0..4)
        .map(|_| {
            let router = app.app.clone();
            let stop = stop.clone();
            tokio::spawn(async move {
                let mut done = 0;
                while !stop.load(Ordering::Relaxed) {
                    let response = router
                        .clone()
                        .oneshot(get("/graph/timeline"))
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
        let (status, _, answer) = app
            .post(
                "/api/resolve",
                &json!({ "issuer": "tpm", "refs": ["claude.session:s-1"] }),
            )
            .await;
        let took = started.elapsed();
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert!(took < Duration::from_millis(100), "{took:?}");
        slowest = slowest.max(took);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.store(true, Ordering::Relaxed);
    let mut rendered = 0;
    for task in renders {
        rendered += task.await.unwrap();
    }
    assert!(rendered > 0, "タイムラインを 1 度も組めていない");
    println!("一番遅い引き当て: {slowest:?}・組んだタイムライン: {rendered}");
}

/// 重い読み（図の点と線・タイムライン）は、重い読みの接続（HEAVY_READS 本）だけを使い、軽い読みの接続（READS 本）には触れない。
/// 軽い読みの接続は、いつも引き当てなどに空いている（今の server で、重い読みが読む接続を占めて引き当てが 100 ms を超えた。2026-11-23 の CI）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn heavy_reads_use_only_the_heavy_read_connections() {
    let app = App::start().await;
    app.register(&human("pm")).await;
    for i in 0..300 {
        app.register(&ai("se", &format!("s-{i}"))).await;
        let me = format!("claude.session:s-{i}");
        for (uri, body) in [
            (format!("/api/links/l-{i}"), link("launched", PM, &me)),
            (format!("/api/events/e-{i}"), event(&me, "entered")),
        ] {
            let (status, _, answer) = app.put(&uri, &body).await;
            assert_eq!(status, StatusCode::CREATED, "{uri}: {answer}");
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let renders: Vec<_> = (0..8)
        .map(|i| {
            let router = app.app.clone();
            let stop = stop.clone();
            let path = if i % 2 == 0 {
                "/graph/timeline"
            } else {
                "/api/graph"
            };
            tokio::spawn(async move {
                let mut done = 0;
                while !stop.load(Ordering::Relaxed) {
                    let response = router.clone().oneshot(get(path)).await.unwrap();
                    assert_eq!(response.status(), StatusCode::OK, "{path}");
                    let _ = http_body_util::BodyExt::collect(response.into_body())
                        .await
                        .unwrap();
                    done += 1;
                }
                done
            })
        })
        .collect();
    let (mut light, mut heavy) = (0, 0);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        let in_use = |pool: &sqlx::SqlitePool| pool.size() as usize - pool.num_idle();
        light = light.max(in_use(app.db.read()));
        heavy = heavy.max(in_use(app.db.heavy()));
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    stop.store(true, Ordering::Relaxed);
    let mut rendered = 0;
    for task in renders {
        rendered += task.await.unwrap();
    }
    assert!(rendered > 0, "重い読みが 1 度も答えていない");
    assert_eq!(light, 0, "重い読みが軽い読みの接続を使った");
    assert!(
        (1..=castlist_next::db::HEAVY_READS as usize).contains(&heavy),
        "重い読みが同時に使った重い読みの接続: {heavy} 本"
    );
}
