//! 並べて来る書き込みの試験: 同じ鍵は 1 つのアクターにまとめ、名の番号に穴も重なりも無い。
//! 同じ id・同じ model と effort・同じ知らない名は、並べて来ても 1 行にまとめる。
//! 書く鍵を握られている間も、読む操作は待たずに答える。
mod common;

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::sample::{self, KEY, SONNET, sample};
use common::{App, SE1, ai, at, event, get, post, put, setting};
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
