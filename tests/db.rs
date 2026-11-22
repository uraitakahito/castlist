//! DB の決まりの試験: 足すだけ・外のキー・接続の設定・索引。書く接続と読む接続を直に使う。

mod common;

use castlist::db::{Db, LOOKUPS};
use common::app::{App, ai, at, event, human, link, remove_db, setting, temp_db};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

const SE1: &str = "claude.session:s-1";
const PM: &str = "tpm.person:pm";

/// どの表にも 1 行以上ある DB を作る。
async fn filled() -> (App, String) {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["settings"] = serde_json::json!({ "model": "claude-opus-5-5", "effort": "high" });
    let se1 = app.register(&body).await;
    app.register(&human("pm")).await;
    app.put("/v1/links/l-1", &link("launched", PM, SE1)).await;
    app.put("/v1/events/e-1", &event(SE1, "entered", &at(0)))
        .await;
    app.put(
        "/v1/settings/t-1",
        &setting(SE1, "claude-opus-5-5", "max", &at(0)),
    )
    .await;
    let id = se1["id"].as_str().expect("id").to_owned();
    (app, id)
}

/// 表と、1 行を指す列と、書き替えを試す列。
const TABLES: &[(&str, &str, &str)] = &[
    ("actors", "seq", "at"),
    ("labels", "seq", "value"),
    ("bindings", "seq", "value"),
    ("links", "seq", "at"),
    ("events", "seq", "at"),
    ("settings", "seq", "at"),
    ("models", "id", "name"),
    ("efforts", "id", "name"),
];

#[tokio::test]
async fn どの表も_書き替えと消しを断る() {
    let (app, _) = filled().await;
    for (table, pk, column) in TABLES {
        let before = app.count(table).await;
        assert!(before > 0, "{table} に行が無い");
        let one = format!("{pk} = (select min({pk}) from {table})");
        let attempts = [
            format!("update {table} set {column} = {column} where {one}"),
            format!("delete from {table} where {one}"),
            format!("insert or replace into {table} select * from {table} where {one}"),
            format!("replace into {table} select * from {table} where {one}"),
            format!(
                "insert into {table} select * from {table} where {one} on conflict do update set {column} = excluded.{column}"
            ),
        ];
        for sql in attempts {
            let refused = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .execute(app.db.write())
                .await
                .expect_err(&format!("断らない: {sql}"));
            assert!(
                refused
                    .to_string()
                    .contains(&format!("append-only: {table}")),
                "{sql}: {refused}"
            );
        }
        assert_eq!(app.count(table).await, before, "{table}");
    }
}

#[tokio::test]
async fn 読む接続_4_本と書く接続のどれも_外のキーと再帰の_trigger_が効いている() {
    let app = App::start().await;
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    held.push(app.db.write().acquire().await.expect("書く接続"));
    for conn in &mut held {
        let fk: i64 = sqlx::query_scalar("pragma foreign_keys")
            .fetch_one(&mut **conn)
            .await
            .unwrap();
        let recursive: i64 = sqlx::query_scalar("pragma recursive_triggers")
            .fetch_one(&mut **conn)
            .await
            .unwrap();
        let journal: String = sqlx::query_scalar("pragma journal_mode")
            .fetch_one(&mut **conn)
            .await
            .unwrap();
        let busy: i64 = sqlx::query_scalar("pragma busy_timeout")
            .fetch_one(&mut **conn)
            .await
            .unwrap();
        assert_eq!((fk, recursive, journal.as_str(), busy), (1, 1, "wal", 5000));
    }
    // 読む接続は書けない。
    let refused = sqlx::query("insert into models(name, first_seen_at) values('x', 'y')")
        .execute(&mut *held[0])
        .await
        .expect_err("読む接続で書けた");
    assert!(refused.to_string().contains("readonly"), "{refused}");
}

#[tokio::test]
async fn 外のキーの列のどれも_無い親を指す行を断る() {
    let (app, id) = filled().await;
    let now = at(0);
    let cases = [
        (
            "labels.actor_id",
            "insert into labels(actor_id, key, value, received_at) values('missing', 'k', 'v', ?2)",
        ),
        (
            "bindings.actor_id",
            "insert into bindings(actor_id, issuer, type, value, is_key, at, received_at) values('missing', 'tpm', 't', 'v', 0, ?2, ?2)",
        ),
        (
            "links.from_actor",
            "insert into links(id, type, from_actor, to_actor, at, received_at, clock_skew, body_hash) values('fk-1', 'continued', 'missing', ?1, ?2, ?2, 0, 'x')",
        ),
        (
            "links.to_actor",
            "insert into links(id, type, from_actor, to_actor, at, received_at, clock_skew, body_hash) values('fk-2', 'continued', ?1, 'missing', ?2, ?2, 0, 'x')",
        ),
        (
            "events.actor_id",
            "insert into events(id, actor_id, type, at, received_at, clock_skew, body_hash) values('fk-3', 'missing', 'entered', ?2, ?2, 0, 'x')",
        ),
        (
            "settings.actor_id",
            "insert into settings(actor_id, model_id, effort_id, source, at, received_at, clock_skew) values('missing', (select min(id) from models), (select min(id) from efforts), 'observed', ?2, ?2, 0)",
        ),
        (
            "settings.model_id",
            "insert into settings(actor_id, model_id, effort_id, source, at, received_at, clock_skew) values(?1, 999999, (select min(id) from efforts), 'observed', ?2, ?2, 0)",
        ),
        (
            "settings.effort_id",
            "insert into settings(actor_id, model_id, effort_id, source, at, received_at, clock_skew) values(?1, (select min(id) from models), 999999, 'observed', ?2, ?2, 0)",
        ),
    ];
    for (column, sql) in cases {
        let refused = sqlx::query(sql)
            .bind(&id)
            .bind(&now)
            .execute(app.db.write())
            .await
            .expect_err(&format!("断らない: {column}"));
        assert!(
            refused.to_string().contains("FOREIGN KEY"),
            "{column}: {refused}"
        );
    }
    let broken: Vec<(String,)> = sqlx::query_as("pragma foreign_key_check")
        .fetch_all(app.db.read())
        .await
        .unwrap();
    assert!(broken.is_empty(), "{broken:?}");
}

#[tokio::test]
async fn 外のキーの崩れた_db_は開かない() {
    let path = temp_db("broken");
    let db = Db::open(&path).await.expect("Db::open");
    db.close().await;
    // 外のキーを切った接続で、無いアクターを指す行を入れる。
    let mut conn = SqliteConnectOptions::new()
        .filename(&path)
        .foreign_keys(false)
        .connect()
        .await
        .unwrap();
    sqlx::query(
        "insert into labels(actor_id, key, value, received_at) values('missing', 'k', 'v', '2026-01-01T00:00:00.000Z')",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();
    let refused = Db::open(&path).await.err().expect("崩れた DB を開いた");
    let message = refused.to_string();
    assert!(message.contains("labels"), "{message}");
    remove_db(&path);
}

#[tokio::test]
async fn 引き当てと見る_sql_は大きな表を端から読まない() {
    let (app, _) = filled().await;
    let names: Vec<&str> = LOOKUPS.iter().map(|(name, _)| *name).collect();
    for want in [
        "resolve",
        "actor",
        "labels",
        "bindings",
        "events",
        "settings",
        "links_in",
        "links_out",
        "state",
        "current_settings",
        "current_bindings",
        "binding_at",
        "label",
    ] {
        assert!(names.contains(&want), "{want} が LOOKUPS に無い: {names:?}");
    }
    for (name, sql) in LOOKUPS {
        let plan: Vec<(i64, i64, i64, String)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!("explain query plan {sql}")))
                .fetch_all(app.db.read())
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        let details: Vec<&str> = plan.iter().map(|row| row.3.as_str()).collect();
        // 計画の字は表の別名（a・e など）で出るので、端から読んでよい物の方を名指す:
        // 小さな master（models・efforts を別名なしで）と、JSON の配列（json_each）だけ。
        let scans: Vec<&&str> = details
            .iter()
            .filter(|d| {
                d.strip_prefix("SCAN ")
                    .and_then(|rest| rest.split_whitespace().next())
                    .is_some_and(|target| {
                        !["models", "efforts", "json_each", "CONSTANT"].contains(&target)
                    })
            })
            .collect();
        assert!(scans.is_empty(), "{name} が端から読む: {details:?}\n{sql}");
    }
}
