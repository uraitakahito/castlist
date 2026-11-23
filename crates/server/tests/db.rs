//! DB の決まりの試験: 移行・足すだけ・外のキー・接続の設定。書く接続と読む接続を直に使う。
mod common;

use castlist_next::db::Db;
use common::{App, ai, at, remove_db, temp_db};
use serde_json::json;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

/// どの表にも 1 行以上ある DB を作る（線と出来事の道はまだ無いので、その 2 つは直に置く）。
async fn filled() -> (App, String) {
    let app = App::start().await;
    let mut body = ai("se", "s-1");
    body["settings"] = json!({ "model": "claude-opus-5-5", "effort": "high" });
    let se1 = app.register(&body).await;
    let pm = app.register(&common::human("pm")).await;
    let id = se1["actor"]["id"].as_str().expect("id").to_owned();
    let now = at(0);
    app.execute(&format!(
        "insert into links(id, type, from_actor, to_actor, sent_at, received_at, clock_skew, body_hash)
         values('l-1', 'launched', '{}', '{id}', '{now}', '{now}', 0, 'x')",
        pm["actor"]["id"].as_str().unwrap()
    ))
    .await;
    app.execute(&format!(
        "insert into events(id, actor_id, type, sent_at, received_at, clock_skew, body_hash)
         values('e-1', '{id}', 'entered', '{now}', '{now}', 0, 'x')"
    ))
    .await;
    (app, id)
}

/// 表と、1 行を指す列と、書き替えを試す列。
const TABLES: &[(&str, &str, &str)] = &[
    ("actors", "seq", "sent_at"),
    ("labels", "seq", "value"),
    ("bindings", "seq", "value"),
    ("links", "seq", "sent_at"),
    ("events", "seq", "sent_at"),
    ("settings", "seq", "sent_at"),
    ("models", "id", "name"),
    ("efforts", "id", "name"),
];

#[tokio::test]
async fn the_migration_makes_the_tables_and_views_of_the_ledger() {
    let app = App::start().await;
    let objects: Vec<String> = app
        .column(
            "select type || ' ' || name from sqlite_schema
             where type in ('table', 'view') and name not like 'sqlite_%' and name != '_sqlx_migrations'
             order by type, name",
        )
        .await;
    assert_eq!(
        objects,
        vec![
            "table actors",
            "table bindings",
            "table efforts",
            "table events",
            "table labels",
            "table links",
            "table models",
            "table settings",
            "view actor_current_bindings",
            "view actor_current_settings",
            "view actor_state",
        ]
    );
    // 送り手の時刻の列は sent_at（今の server の 0003 の後の形）。at の列はどこにも無い。
    for table in ["actors", "bindings", "links", "events", "settings"] {
        let columns: Vec<String> = app
            .column(&format!("select name from pragma_table_info('{table}')"))
            .await;
        assert!(
            columns.contains(&"sent_at".to_owned()),
            "{table}: {columns:?}"
        );
        assert!(!columns.contains(&"at".to_owned()), "{table}: {columns:?}");
    }
    // 足すだけの trigger は、表ごとに書き替えと消しの 2 つ。
    let triggers: Vec<String> = app
        .column("select name from sqlite_schema where type = 'trigger' order by name")
        .await;
    let mut want: Vec<String> = TABLES
        .iter()
        .flat_map(|(t, _, _)| [format!("{t}_no_delete"), format!("{t}_no_update")])
        .collect();
    want.sort();
    assert_eq!(triggers, want);
}

#[tokio::test]
async fn every_table_refuses_update_and_delete() {
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
async fn every_connection_has_foreign_keys_wal_busy_timeout_and_recursive_triggers() {
    let app = App::start().await;
    let mut held = Vec::new();
    for _ in 0..castlist_next::db::READS {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    for _ in 0..castlist_next::db::HEAVY_READS {
        held.push(app.db.heavy().acquire().await.expect("重い読みの接続"));
    }
    held.push(app.db.write().acquire().await.expect("書く接続"));
    for conn in &mut held {
        let mut got = Vec::new();
        for pragma in [
            "foreign_keys",
            "recursive_triggers",
            "busy_timeout",
            "temp_store",
        ] {
            let value: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("pragma {pragma}")))
                .fetch_one(&mut **conn)
                .await
                .unwrap();
            got.push(value);
        }
        let journal: String = sqlx::query_scalar("pragma journal_mode")
            .fetch_one(&mut **conn)
            .await
            .unwrap();
        // temp_store の 2 は MEMORY。
        assert_eq!((got, journal.as_str()), (vec![1, 1, 5000, 2], "wal"));
    }
    // 読む接続（軽い・重い）は書けない。
    for i in [0, castlist_next::db::READS as usize] {
        let refused = sqlx::query("insert into models(name, first_seen_at) values('x', 'y')")
            .execute(&mut *held[i])
            .await
            .expect_err("読む接続で書けた");
        assert!(refused.to_string().contains("readonly"), "{refused}");
    }
}

#[tokio::test]
async fn every_foreign_key_refuses_a_missing_parent() {
    let (app, id) = filled().await;
    let now = at(0);
    let cases = [
        (
            "labels.actor_id",
            "insert into labels(actor_id, key, value, received_at) values('missing', 'k', 'v', ?2)",
        ),
        (
            "bindings.actor_id",
            "insert into bindings(actor_id, issuer, type, value, is_key, sent_at, received_at) values('missing', 'tpm', 't', 'v', 0, ?2, ?2)",
        ),
        (
            "links.from_actor",
            "insert into links(id, type, from_actor, to_actor, sent_at, received_at, clock_skew, body_hash) values('fk-1', 'continued', 'missing', ?1, ?2, ?2, 0, 'x')",
        ),
        (
            "links.to_actor",
            "insert into links(id, type, from_actor, to_actor, sent_at, received_at, clock_skew, body_hash) values('fk-2', 'continued', ?1, 'missing', ?2, ?2, 0, 'x')",
        ),
        (
            "events.actor_id",
            "insert into events(id, actor_id, type, sent_at, received_at, clock_skew, body_hash) values('fk-3', 'missing', 'entered', ?2, ?2, 0, 'x')",
        ),
        (
            "settings.actor_id",
            "insert into settings(actor_id, model_id, effort_id, source, sent_at, received_at, clock_skew) values('missing', (select min(id) from models), (select min(id) from efforts), 'observed', ?2, ?2, 0)",
        ),
        (
            "settings.model_id",
            "insert into settings(actor_id, model_id, effort_id, source, sent_at, received_at, clock_skew) values(?1, 999999, (select min(id) from efforts), 'observed', ?2, ?2, 0)",
        ),
        (
            "settings.effort_id",
            "insert into settings(actor_id, model_id, effort_id, source, sent_at, received_at, clock_skew) values(?1, (select min(id) from models), 999999, 'observed', ?2, ?2, 0)",
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
    let broken: Vec<(String,)> = sqlx::query_as("select \"table\" from pragma_foreign_key_check")
        .fetch_all(app.db.read())
        .await
        .unwrap();
    assert!(broken.is_empty(), "{broken:?}");
}

#[tokio::test]
async fn a_db_with_broken_foreign_keys_is_not_opened() {
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
async fn opening_again_keeps_the_rows_and_does_not_migrate_twice() {
    let path = temp_db("reopen");
    let db = Db::open(&path).await.expect("Db::open");
    let app = castlist_next::app(std::sync::Arc::new(db));
    let request = common::post("/api/actors", &ai("se", "s-1"));
    let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let db = Db::open(&path).await.expect("2 度目の Db::open");
    let actors: i64 = sqlx::query_scalar("select count(*) from actors")
        .fetch_one(db.read())
        .await
        .unwrap();
    let migrations: i64 = sqlx::query_scalar("select count(*) from _sqlx_migrations")
        .fetch_one(db.read())
        .await
        .unwrap();
    assert_eq!((actors, migrations), (1, 1));
    db.close().await;
    remove_db(&path);
}
