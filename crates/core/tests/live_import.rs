//! A large import's batches against real servers: rows the analysis finds a problem with,
//! rows the server refuses (a CHECK constraint the analysis cannot know), skipping them or
//! stopping, and stopping by hand. Every database created here is named
//! `puricat_it_<random>` and dropped again.

use puricat_core::db::smart::{BatchOutcome, OnProblem, PasteRequest, RowStatus};
use puricat_core::db::{Session, TableRef};
use puricat_core::model::{ConnectionProfile, Engine};
use puricat_core::Error;
use serde_json::json;
use sqlx::{Connection, Executor};
use std::sync::atomic::AtomicBool;

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

fn upsert(rows: &[[&str; 3]]) -> PasteRequest {
    serde_json::from_value(json!({
        "mode": "upsert", "columns": ["id", "name", "n"], "key": ["id"], "update": ["name", "n"],
        "rows": rows,
    }))
    .unwrap()
}

/// (row, what the analysis said or `None` for the server, whether there is a message).
fn failed(out: &BatchOutcome) -> Vec<(usize, Option<RowStatus>, bool)> {
    out.failed
        .iter()
        .map(|f| (f.row, f.status, f.message.is_some()))
        .collect()
}

/// Reads the table's ids in order, on a connection of the test's own.
enum Ids {
    Mysql(sqlx::mysql::MySqlConnectOptions),
    Postgres(sqlx::postgres::PgConnectOptions),
}

impl Ids {
    async fn get(&self) -> Vec<i64> {
        match self {
            Ids::Mysql(o) => {
                let mut c = sqlx::MySqlConnection::connect_with(o).await.unwrap();
                sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM people ORDER BY id")
                    .fetch_all(&mut c)
                    .await
                    .unwrap()
            }
            Ids::Postgres(o) => {
                let mut c = sqlx::PgConnection::connect_with(o).await.unwrap();
                sqlx::query_scalar("SELECT id::bigint FROM people ORDER BY id")
                    .fetch_all(&mut c)
                    .await
                    .unwrap()
            }
        }
    }
}

/// The same steps on either engine.
async fn batches(session: &Session, table: &TableRef, ids: &Ids) {
    let go = AtomicBool::new(false);
    // Skip: a row too long for the column (the analysis), a row the CHECK refuses (the server).
    let first = session
        .import_batch(
            table,
            &upsert(&[
                ["1", "Ann", "1"],
                ["2", "Bob", "2"],
                ["3", "TooLongName", "3"],
                ["4", "Dee", "-1"],
                ["5", "Eve", "5"],
            ]),
            OnProblem::Skip,
            &go,
        )
        .await
        .unwrap();
    assert_eq!(
        (
            first.inserted,
            first.updated,
            first.stopped,
            first.cancelled
        ),
        (3, 0, false, false)
    );
    assert_eq!(
        failed(&first),
        [(2, Some(RowStatus::Invalid), true), (3, None, true)]
    );
    assert_eq!(ids.get().await, [1, 2, 5]);

    // Stop, with a row the server refuses: nothing of this batch is written.
    let refused = session
        .import_batch(
            table,
            &upsert(&[["6", "Fay", "6"], ["7", "Gus", "-7"]]),
            OnProblem::Stop,
            &go,
        )
        .await
        .unwrap_err();
    assert!(!matches!(refused, Error::Conflict(_)), "{refused}");
    assert_eq!(ids.get().await, [1, 2, 5]);

    // Stop, with a row the analysis finds: the batch is not run at all.
    let problem = session
        .import_batch(
            table,
            &upsert(&[["8", "Hal", "8"], ["9", "TooLongName", "9"]]),
            OnProblem::Stop,
            &go,
        )
        .await
        .unwrap();
    assert!(problem.stopped);
    assert_eq!(failed(&problem), [(1, Some(RowStatus::Invalid), true)]);
    assert_eq!(ids.get().await, [1, 2, 5]);

    // Rows that exist are updated or left as they are, as in a paste.
    let again = session
        .import_batch(
            table,
            &upsert(&[["1", "Ann2", "1"], ["2", "Bob", "2"]]),
            OnProblem::Skip,
            &go,
        )
        .await
        .unwrap();
    assert_eq!((again.inserted, again.updated, again.unchanged), (0, 1, 1));

    // Stopped before it runs: rolled back, and said so.
    let stopped = session
        .import_batch(
            table,
            &upsert(&[["10", "Ivy", "10"]]),
            OnProblem::Skip,
            &AtomicBool::new(true),
        )
        .await
        .unwrap_err();
    assert!(matches!(stopped, Error::Conflict(_)), "{stopped}");
    assert_eq!(ids.get().await, [1, 2, 5]);

    // Skipping a refused first row: it is reported, and the row after it is written.
    let late = AtomicBool::new(false);
    let refused_first = session
        .import_batch(
            table,
            &upsert(&[["11", "Jo", "-11"], ["12", "Kit", "12"]]),
            OnProblem::Skip,
            &late,
        )
        .await
        .unwrap();
    assert_eq!(failed(&refused_first), [(0, None, true)]);
    assert_eq!(refused_first.inserted, 1);
    assert_eq!(ids.get().await, [1, 2, 5, 12]);
}

#[tokio::test]
async fn mysql_import_batches() {
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
        let o = opts.database(&d);
        let mut conn = sqlx::MySqlConnection::connect_with(&o).await.unwrap();
        conn.execute("CREATE TABLE people (id INT NOT NULL PRIMARY KEY, name VARCHAR(5) NOT NULL, n INT, CHECK (n > 0))")
            .await
            .unwrap();
        let session = Session::connect(&profile, password.as_deref()).await.unwrap();
        let table = TableRef { database: d.clone(), schema: None, name: "people".into() };
        batches(&session, &table, &Ids::Mysql(o)).await;
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
async fn postgres_import_batches() {
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
        conn.execute("CREATE TABLE public.people (id integer PRIMARY KEY, name varchar(5) NOT NULL, n integer CHECK (n > 0))")
            .await
            .unwrap();
        let session = Session::connect(&p, pw.as_deref()).await.unwrap();
        let table = TableRef { database: d.clone(), schema: Some("public".into()), name: "people".into() };
        batches(&session, &table, &Ids::Postgres(o)).await;
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}
