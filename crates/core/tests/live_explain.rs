//! EXPLAIN and named parameters against real servers: a plan that reads the whole table and
//! one that does not, a measured run of a query, the refusals, and parameter values (a quote,
//! a backslash, Thai text, a negative number, NULL) that come back from the server exactly as
//! given. Every database created here is named `puricat_it_<random>` and dropped again.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use puricat_core::db::console::RunOptions;
use puricat_core::db::script::{self, ParamValue};
use puricat_core::db::Session;
use puricat_core::model::{ConnectionProfile, Engine};
use serde_json::json;
use sqlx::{Connection, Executor};

fn target(engine: Engine, var: &str) -> Option<(ConnectionProfile, Option<String>)> {
    let spec = std::env::var(var).ok()?;
    let (user, rest) = spec.split_once('@')?;
    let (host, port) = rest.rsplit_once(':')?;
    let profile = serde_json::from_value(json!({
        "name": "integration", "engine": engine, "host": host,
        "port": port.parse::<u16>().ok()?, "username": user,
    }))
    .ok()?;
    Some((profile, std::env::var(format!("{var}_PASSWORD")).ok()))
}

fn scratch_name() -> String {
    format!(
        "puricat_it_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

fn values() -> HashMap<String, ParamValue> {
    [
        (
            "text".to_string(),
            ParamValue::Text("O'Neil \\ ทดสอบ".into()),
        ),
        ("n".to_string(), ParamValue::Number("-5".into())),
        ("none".to_string(), ParamValue::Null),
    ]
    .into()
}

/// The steps both engines share. `scan`: how the engine names a whole-table read.
async fn check(session: &Session, db: &str, scan: &str, engine: Engine, rows: &str) {
    let go = AtomicBool::new(false);
    let full = session
        .explain(Some(db), "SELECT * FROM t WHERE v = 3", false, &go)
        .await
        .unwrap();
    assert!(full.text.contains(scan), "{}", full.text);
    assert_eq!(full.scans.len(), 1, "{:?}", full.scans);
    assert!(!full.analyzed);
    let by_key = session
        .explain(Some(db), "SELECT * FROM t WHERE id = 3;", false, &go)
        .await
        .unwrap();
    assert!(by_key.scans.is_empty(), "{}", by_key.text);
    assert_eq!(by_key.statement, "SELECT * FROM t WHERE id = 3");
    let measured = session
        .explain(Some(db), "SELECT count(*) FROM t", true, &go)
        .await
        .unwrap();
    assert!(
        measured.analyzed && measured.text.contains("actual"),
        "{}",
        measured.text
    );
    // The statement is only explained, never run, without `analyze`.
    let delete = session
        .explain(Some(db), "DELETE FROM t", false, &go)
        .await
        .unwrap();
    assert!(!delete.text.is_empty());
    assert!(session
        .explain(Some(db), "DELETE FROM t", true, &go)
        .await
        .is_err());
    assert!(session
        .explain(Some(db), "SELECT 1; SELECT 2", false, &go)
        .await
        .is_err());

    // Parameters come back from the server as they were given.
    let sql = script::bind(
        engine,
        "SELECT :text AS a, :n + 1 AS b, :none AS c, ':text' AS d",
        &values(),
    )
    .unwrap();
    let mut console = session.open_console(Some(db)).await.unwrap();
    let outcome = console
        .run(
            &sql,
            &RunOptions {
                max_rows: 10,
                change_limit: None,
            },
            &go,
        )
        .await
        .unwrap();
    let row = outcome.statements[0].results[0].rows[0].clone();
    assert_eq!(row[0].as_deref(), Some("O'Neil \\ ทดสอบ"), "{sql}");
    assert_eq!(row[1].as_deref(), Some("-4"));
    assert_eq!(row[2], None);
    assert_eq!(row[3].as_deref(), Some(":text"));
    // And the rows still there: nothing was deleted.
    let count = console
        .run(
            "SELECT count(*) FROM t",
            &RunOptions {
                max_rows: 10,
                change_limit: None,
            },
            &go,
        )
        .await
        .unwrap();
    assert_eq!(
        count.statements[0].results[0].rows[0][0].as_deref(),
        Some(rows)
    );
}

#[tokio::test]
async fn mysql_explain_and_parameters() {
    let Some((profile, password)) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&profile.host)
        .port(profile.port)
        .username(&profile.username);
    if let Some(pw) = &password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
    let db = scratch_name();
    admin
        .execute(format!("CREATE DATABASE `{db}` CHARACTER SET utf8mb4").as_str())
        .await
        .unwrap();
    let d = db.clone();
    let outcome = tokio::spawn(async move {
        let mut conn = sqlx::MySqlConnection::connect_with(&opts.database(&d)).await.unwrap();
        for sql in [
            "CREATE TABLE t (id INT PRIMARY KEY, v INT)",
            "INSERT INTO t WITH RECURSIVE n (i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 200) SELECT i, i % 10 FROM n",
            "ANALYZE TABLE t",
        ] {
            conn.execute(sql).await.unwrap();
        }
        let session = Session::connect(&profile, password.as_deref()).await.unwrap();
        check(&session, &d, "Table scan on t", Engine::Mysql, "200").await;
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE IF EXISTS `{db}`").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}

#[tokio::test]
async fn postgres_explain_and_parameters() {
    let Some((profile, password)) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let opts = |database: &str| {
        let mut o = sqlx::postgres::PgConnectOptions::new()
            .host(&profile.host)
            .port(profile.port)
            .username(&profile.username)
            .database(database);
        if let Some(pw) = &password {
            o = o.password(pw);
        }
        o
    };
    let mut admin = sqlx::PgConnection::connect_with(&opts("postgres"))
        .await
        .unwrap();
    let db = scratch_name();
    admin
        .execute(format!("CREATE DATABASE \"{db}\"").as_str())
        .await
        .unwrap();
    let (d, o) = (db.clone(), opts(&db));
    let (p, pw) = (profile.clone(), password.clone());
    let outcome = tokio::spawn(async move {
        let mut conn = sqlx::PgConnection::connect_with(&o).await.unwrap();
        // Enough rows that the planner prefers the key to reading the table.
        conn.execute("CREATE TABLE t (id integer PRIMARY KEY, v integer); INSERT INTO t SELECT i, i % 10 FROM generate_series(1, 20000) i; ANALYZE t;")
            .await
            .unwrap();
        let session = Session::connect(&p, pw.as_deref()).await.unwrap();
        check(&session, &d, "Seq Scan on t", Engine::Postgres, "20000").await;
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}
