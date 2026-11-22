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

/// docs/src/db.md の「列」の章の表（`### \`<表>\`` ごとに「列・型・NULL・既定・…」の行）。
fn documented_columns() -> std::collections::BTreeMap<String, Vec<[String; 4]>> {
    let doc = include_str!("../docs/src/db.md");
    let mut tables = std::collections::BTreeMap::new();
    let mut in_section = false;
    let mut table: Option<String> = None;
    for line in doc.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            in_section = h.trim() == "列";
            table = None;
        } else if !in_section {
            continue;
        } else if let Some(h) = line.strip_prefix("### ") {
            table = Some(h.trim().trim_matches('`').to_owned());
            tables.insert(table.clone().unwrap(), Vec::new());
        } else if let (Some(t), true) = (&table, line.starts_with("| `")) {
            let cells: Vec<String> = line
                .split('|')
                .skip(1)
                .take(4)
                .map(|c| c.trim().trim_matches('`').to_owned())
                .collect();
            tables
                .get_mut(t)
                .unwrap()
                .push(cells.try_into().expect("列・型・NULL・既定の 4 つの欄"));
        }
    }
    tables
}

#[tokio::test]
async fn docs_の列の表は_移行した_db_の列と合う() {
    let path = temp_db("columns");
    let db = Db::open(&path).await.unwrap();
    let documented = documented_columns();
    let names: Vec<String> = sqlx::query_scalar(
        "select name from sqlite_schema where type = 'table'
         and name not like 'sqlite_%' and name != '_sqlx_migrations' order by name",
    )
    .fetch_all(db.read())
    .await
    .unwrap();
    assert_eq!(
        documented.keys().cloned().collect::<Vec<_>>(),
        names,
        "db.md の「列」の章の表と、DB の表が合わない"
    );
    for name in &names {
        let rows: Vec<(String, String, i64, Option<String>, i64)> = sqlx::query_as(
            "select name, type, \"notnull\", dflt_value, pk from pragma_table_xinfo(?1) order by cid",
        )
        .bind(name)
        .fetch_all(db.read())
        .await
        .unwrap();
        // NULL の欄: not null か主キーなら「不可」。型は大文字で書く。既定が無ければ空。
        let actual: Vec<[String; 4]> = rows
            .into_iter()
            .map(|(column, ty, notnull, default, pk)| {
                let null = if notnull == 1 || pk > 0 {
                    "不可"
                } else {
                    "可"
                };
                [
                    column,
                    ty.to_uppercase(),
                    null.to_owned(),
                    default.unwrap_or_default(),
                ]
            })
            .collect();
        assert_eq!(
            documented[name], actual,
            "db.md の {name} の列（列・型・NULL・既定）が、移行した DB と合わない"
        );
    }
    db.close().await;
    remove_db(&path);
}

#[tokio::test]
async fn 前の版の_db_を開くと_送り手の時刻の列は_sent_at_になり_値はそのまま() {
    let path = temp_db("v01");
    // 前の版（移行 0002 まで）の DB に、送り手の時刻（at）の付いた行を入れる。
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    sqlx::migrate!().run_to(2, &pool).await.unwrap();
    let sent = "2026-11-22T13:00:00.123+09:00";
    let got = "2026-11-22T04:00:00.000Z";
    for sql in [
        "insert into actors(id, issuer, kind, prefix, n, name, at, received_at, clock_skew)
         values('a1', 'tpm', 'ai', 'se', 1, 'se.1', ?1, ?2, 0)",
        "insert into actors(id, issuer, kind, prefix, n, name, at, received_at, clock_skew)
         values('a2', 'tpm', 'ai', 'se', 2, 'se.2', ?1, ?2, 0)",
        "insert into bindings(actor_id, issuer, type, value, is_key, at, received_at)
         values('a1', 'tpm', 'claude.session', 's-1', 1, ?1, ?2)",
        "insert into links(id, type, from_actor, to_actor, at, received_at, clock_skew, body_hash)
         values('l1', 'launched', 'a1', 'a2', ?1, ?2, 0, 'h')",
        "insert into events(id, actor_id, type, at, received_at, clock_skew, body_hash)
         values('e1', 'a1', 'entered', ?1, ?2, 0, 'h')",
        "insert into models(name, first_seen_at) values('claude-opus-5-5', ?2)",
        "insert into settings(actor_id, model_id, effort_id, source, at, received_at, clock_skew)
         values('a1', 1, 1, 'requested', ?1, ?2, 0)",
    ] {
        sqlx::query(sql)
            .bind(sent)
            .bind(got)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    pool.close().await;

    // 新しい版で開く（移行 0003 が当たる）。
    let db = Db::open(&path).await.unwrap();
    for table in ["actors", "bindings", "links", "events", "settings"] {
        let values: Vec<String> =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("select sent_at from {table}")))
                .fetch_all(db.read())
                .await
                .unwrap_or_else(|e| panic!("{table}: {e}"));
        assert!(
            !values.is_empty() && values.iter().all(|v| v == sent),
            "{table}: {values:?}"
        );
    }
    // どの表と view にも、もう at の列は無い。view の今の model と effort・付随する id も sent_at で出す。
    let at: Vec<String> = sqlx::query_scalar(
        "select s.name from sqlite_schema s join pragma_table_xinfo(s.name) c
         where s.type in ('table', 'view') and c.name = 'at'",
    )
    .fetch_all(db.read())
    .await
    .unwrap();
    assert!(at.is_empty(), "at の列が残る: {at:?}");
    let current: String =
        sqlx::query_scalar("select sent_at from actor_current_settings where actor_id = 'a1'")
            .fetch_one(db.read())
            .await
            .unwrap();
    assert_eq!(current, sent);
    let current: String =
        sqlx::query_scalar("select sent_at from actor_current_bindings where actor_id = 'a1'")
            .fetch_one(db.read())
            .await
            .unwrap();
    assert_eq!(current, sent);
    db.close().await;
    remove_db(&path);
}
