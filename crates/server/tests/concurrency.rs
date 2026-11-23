//! 並べて来る登録の試験: 同じ鍵は 1 つのアクターにまとめ、名の番号に穴も重なりも無い。
mod common;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use common::{App, ai, post};
use serde_json::Value;
use tower::ServiceExt;

/// 登録を別の task で送り、状態と JSON を返す。
fn spawn(app: &App, body: Value) -> tokio::task::JoinHandle<(StatusCode, Value)> {
    let router = app.app.clone();
    tokio::spawn(async move {
        let response = router.oneshot(post("/api/actors", &body)).await.unwrap();
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
    let tasks: Vec<_> = (0..50).map(|_| spawn(&app, ai("se", "same"))).collect();
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
        .map(|i| spawn(&app, ai("se", &format!("s-{i}"))))
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
