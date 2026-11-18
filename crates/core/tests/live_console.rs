//! Integration tests for the SQL console, against real servers.
//! Same opt-in environment variables and disposable-database rule as `live_db.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use puricat_core::db::console::{Canceller, Console, RunOptions, RunOutcome, Status};
use puricat_core::db::export::{self, Format};
use puricat_core::db::script::Kind;
use puricat_core::db::Session;
use puricat_core::model::{ConnectionProfile, Engine};
use serde_json::json;
use sqlx::{Connection, Executor};

struct Target {
    profile: ConnectionProfile,
    password: Option<String>,
}

fn target(engine: Engine, var: &str) -> Option<Target> {
    let spec = std::env::var(var).ok()?;
    let (user, rest) = spec.split_once('@')?;
    let (host, port) = rest.rsplit_once(':')?;
    let profile = serde_json::from_value(json!({
        "name": "integration", "engine": engine, "host": host,
        "port": port.parse::<u16>().ok()?, "username": user,
    }))
    .ok()?;
    Some(Target {
        profile,
        password: std::env::var(format!("{var}_PASSWORD")).ok(),
    })
}

fn scratch_name() -> String {
    format!(
        "puricat_it_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

fn options(max_rows: u32, change_limit: Option<u64>) -> RunOptions {
    serde_json::from_value(json!({ "maxRows": max_rows, "changeLimit": change_limit })).unwrap()
}

async fn run(console: &mut Console, sql: &str) -> RunOutcome {
    console
        .run(sql, &options(1000, Some(1000)), &AtomicBool::new(false))
        .await
        .unwrap()
}

/// First result of the last statement, as rows of cells.
fn rows(outcome: &RunOutcome) -> Vec<Vec<Option<String>>> {
    outcome.statements.last().unwrap().results[0].rows.clone()
}

fn one(outcome: &RunOutcome) -> Option<String> {
    rows(outcome)[0][0].clone()
}

/// The behaviour both engines share. `sleep` is a statement that takes about 30 seconds;
/// `series` returns 5,000 rows; `temp` creates session-local state read back by `probe`.
async fn scenario(
    session: &Session,
    database: &str,
    sleep: &str,
    series: &str,
    temp: &str,
    probe: &str,
) {
    let mut console = session.open_console(Some(database)).await.unwrap();
    let limit = |max, change| options(max, change);
    let idle = AtomicBool::new(false);

    // A script: every statement reports its own kind, outcome and timing.
    let script = "SELECT id, name, price, big, note FROM items ORDER BY id;\n\
                  -- a comment; with a semicolon\n\
                  UPDATE items SET note = 'seen' WHERE id = 1;\n\
                  SELECT id, name FROM items WHERE 1 = 0;";
    let outcome = run(&mut console, script).await;
    assert_eq!(outcome.statements.len(), 3);
    assert_eq!(outcome.skipped, 0);
    let kinds: Vec<_> = outcome.statements.iter().map(|s| s.kind).collect();
    assert_eq!(kinds, vec![Kind::Read, Kind::Dml, Kind::Read]);
    assert!(outcome.statements.iter().all(|s| s.status == Status::Ok));

    let select = &outcome.statements[0].results[0];
    let names: Vec<_> = select.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["id", "name", "price", "big", "note"]);
    assert_eq!(select.rows.len(), 3);
    assert_eq!(select.rows[0][1].as_deref(), Some("สวัสดี \"โลก\""));
    assert_eq!(select.rows[0][2].as_deref(), Some("12345678901234.123456"));
    assert_eq!(select.rows[0][3].as_deref(), Some("9007199254740993"));
    assert_eq!(select.rows[1][2], None);
    assert_eq!(
        outcome.statements[0].rows_affected, None,
        "a query reports rows, not rows affected"
    );
    assert_eq!(outcome.statements[1].rows_affected, Some(1));
    assert!(outcome.statements[1].results.is_empty());
    // An empty result still has its column names.
    let empty = &outcome.statements[2].results[0];
    assert!(empty.rows.is_empty());
    assert_eq!(
        empty
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["id", "name"]
    );

    // The first error stops the script; later statements are not run.
    let outcome = run(
        &mut console,
        "SELECT 1; SELECT no_such_function_xyz(); UPDATE items SET note = 'never'",
    )
    .await;
    assert_eq!(outcome.statements.len(), 2);
    assert_eq!(outcome.skipped, 1);
    assert_eq!(outcome.statements[1].status, Status::Error);
    assert!(outcome.statements[1]
        .message
        .as_deref()
        .unwrap()
        .to_lowercase()
        .contains("no_such_function_xyz"));
    assert_eq!(
        one(&run(&mut console, "SELECT note FROM items WHERE id = 1").await).as_deref(),
        Some("seen")
    );

    // Impact guard: a change that touches more rows than allowed is rolled back, whatever
    // the statement looked like.
    let wide = "UPDATE items SET note = 'everything' WHERE id > 0";
    let outcome = console
        .run(wide, &limit(1000, Some(2)), &idle)
        .await
        .unwrap();
    assert_eq!(outcome.statements[0].status, Status::Blocked);
    assert_eq!(outcome.statements[0].rows_affected, Some(3));
    assert_eq!(outcome.resume.as_deref(), Some(wide));
    // Only the part that has not happened is offered for a second run.
    let two = format!("UPDATE items SET note = 'ก่อน' WHERE id = 1; {wide}; SELECT 1");
    let outcome = console
        .run(&two, &limit(1000, Some(2)), &idle)
        .await
        .unwrap();
    assert_eq!(outcome.resume, Some(format!("{wide}; SELECT 1")));
    assert_eq!(outcome.skipped, 1);
    assert_eq!(
        one(&run(
            &mut console,
            "SELECT count(*) FROM items WHERE note = 'everything'"
        )
        .await)
        .as_deref(),
        Some("0")
    );
    // Within the limit, or with the limit lifted, it commits.
    let outcome = console.run(wide, &limit(1000, None), &idle).await.unwrap();
    assert_eq!(outcome.statements[0].status, Status::Ok);
    assert_eq!(
        one(&run(
            &mut console,
            "SELECT count(*) FROM items WHERE note = 'everything'"
        )
        .await)
        .as_deref(),
        Some("3")
    );

    // A transaction the user opened is theirs: no guard, and it spans runs.
    let outcome = console
        .run(
            "BEGIN; UPDATE items SET note = 'mine'",
            &limit(1000, Some(1)),
            &idle,
        )
        .await
        .unwrap();
    assert!(outcome.statements.iter().all(|s| s.status == Status::Ok));
    assert!(outcome.in_transaction);
    assert_eq!(
        one(&run(
            &mut console,
            "SELECT count(*) FROM items WHERE note = 'mine'"
        )
        .await)
        .as_deref(),
        Some("3")
    );
    let outcome = run(&mut console, "ROLLBACK").await;
    assert!(!outcome.in_transaction);
    assert_eq!(
        one(&run(
            &mut console,
            "SELECT count(*) FROM items WHERE note = 'mine'"
        )
        .await)
        .as_deref(),
        Some("0")
    );

    // A comment in front of BEGIN does not hide it: the guard still leaves the user's
    // transaction alone (it would otherwise commit it, or block the statement).
    let outcome = console
        .run(
            "-- mine\nBEGIN; /* all of them */ UPDATE items SET note = 'commented'",
            &limit(1000, Some(1)),
            &idle,
        )
        .await
        .unwrap();
    assert!(outcome.statements.iter().all(|s| s.status == Status::Ok));
    assert!(outcome.in_transaction);
    assert!(!run(&mut console, "ROLLBACK").await.in_transaction);
    assert_eq!(
        one(&run(
            &mut console,
            "SELECT count(*) FROM items WHERE note = 'commented'"
        )
        .await)
        .as_deref(),
        Some("0")
    );

    // Session state lives as long as the tab's connection.
    run(&mut console, temp).await;
    assert_eq!(one(&run(&mut console, probe).await).as_deref(), Some("5"));

    // A large result is cut at the cap, the query is stopped, and the console stays usable.
    let outcome = console.run(series, &limit(100, None), &idle).await.unwrap();
    let capped = &outcome.statements.last().unwrap().results[0];
    assert_eq!(capped.rows.len(), 100);
    assert!(capped.capped);
    assert_eq!(
        one(&run(&mut console, "SELECT 41 + 1").await).as_deref(),
        Some("42")
    );

    // Long cells are cut for display only; the export has them in full, exactly.
    let outcome = run(&mut console, "SELECT id, name, wide FROM items ORDER BY id").await;
    let shown = &outcome.statements[0].results[0];
    assert_eq!(shown.rows[0][2].as_ref().unwrap().chars().count(), 1000);
    assert_eq!(shown.truncated, vec![[0, 2]]);
    let path = std::env::temp_dir().join(format!("{}.csv", scratch_name()));
    assert_eq!(
        export::write(&path, Format::Csv, console.result(shown.id).unwrap()).unwrap(),
        3
    );
    let csv = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(csv.contains(&"ก".repeat(1500)) && csv.contains("\"สวัสดี \"\"โลก\"\"\""));
    assert!(console.result(99).is_err());

    // Cancelling a running statement returns promptly and stops the script.
    let stop = Arc::new(AtomicBool::new(false));
    let canceller = console.canceller();
    let flag = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        flag.store(true, Ordering::Relaxed);
        canceller.cancel().await.unwrap();
    });
    let started = Instant::now();
    let outcome = console
        .run(&format!("{sleep}; SELECT 2"), &limit(1000, None), &stop)
        .await
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(outcome.statements[0].status, Status::Cancelled);
    assert_eq!(outcome.statements.len(), 1);
    assert_eq!(
        one(&run(&mut console, "SELECT 7").await).as_deref(),
        Some("7")
    );

    assert!(console
        .run("  -- nothing here\n", &limit(10, None), &idle)
        .await
        .is_err());
    assert!(console
        .run("SELECT 1", &limit(0, None), &idle)
        .await
        .is_err());

    // The server ends the tab's connection (a restart, an idle timeout, an administrator):
    // the run says so instead of failing the same way forever.
    match console.canceller() {
        Canceller::MySql {
            pool,
            connection_id,
        } => {
            pool.execute(format!("KILL {connection_id}").as_str())
                .await
                .unwrap();
        }
        Canceller::Postgres { pool, pid } => {
            sqlx::query("SELECT pg_terminate_backend($1)")
                .bind(pid)
                .execute(&pool)
                .await
                .unwrap();
        }
        // This scenario is about servers; a file has no connection to lose.
        Canceller::Sqlite { .. } => unreachable!("this scenario runs against a server"),
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let outcome = run(&mut console, "SELECT 1; SELECT 2").await;
    assert!(outcome.connection_lost);
    assert!(!outcome.in_transaction);
    assert_eq!(outcome.statements.len(), 1);
    assert_eq!(outcome.statements[0].status, Status::Error);
    assert!(outcome.statements[0]
        .message
        .as_deref()
        .unwrap()
        .contains("connection was lost"));
    assert!(console
        .run("SELECT 1", &limit(10, None), &idle)
        .await
        .is_err());
    // A new console on the same session works.
    let mut fresh = session.open_console(Some(database)).await.unwrap();
    assert_eq!(
        one(&run(&mut fresh, "SELECT 1").await).as_deref(),
        Some("1")
    );
}

const ITEMS: &str = "INSERT INTO items (name, price, big, wide) VALUES ('สวัสดี \"โลก\"', 12345678901234.123456, 9007199254740993, repeat('ก', 1500)), ('second', NULL, NULL, NULL), ('third', 0.5, -1, 'x')";

#[tokio::test]
async fn mysql_console() {
    let Some(t) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let db = scratch_name();
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
    admin
        .execute(format!("CREATE DATABASE `{db}` CHARACTER SET utf8mb4").as_str())
        .await
        .unwrap();
    admin.execute(format!("USE `{db}`").as_str()).await.unwrap();
    admin
        .execute("CREATE TABLE items (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(100), price DECIMAL(24,6), big BIGINT, note TEXT, wide TEXT, raw VARBINARY(8), flags BIT(8))")
        .await
        .unwrap();
    admin.execute(ITEMS).await.unwrap();
    admin
        .execute("UPDATE items SET raw = x'00ff10', flags = b'00000101' WHERE id = 1")
        .await
        .unwrap();

    let name = db.clone();
    let outcome = tokio::spawn(async move {
        let session = Session::connect(&t.profile, t.password.as_deref()).await.unwrap();
        scenario(
            &session,
            &name,
            "SELECT SLEEP(30)",
            "SET SESSION cte_max_recursion_depth = 6000; WITH RECURSIVE s (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < 5000) SELECT n FROM s",
            "SET @kept = 5",
            "SELECT @kept",
        )
        .await;

        // MySQL specifics: binary and BIT columns, USE switching the tab's database.
        let mut console = session.open_console(Some(&name)).await.unwrap();
        let outcome = run(&mut console, "SELECT raw, flags FROM items WHERE id = 1").await;
        assert_eq!(rows(&outcome)[0], vec![Some("0x00ff10".to_string()), Some("5".to_string())]);
        let outcome = run(&mut console, "USE information_schema; SELECT DATABASE()").await;
        assert_eq!(one(&outcome).as_deref(), Some("information_schema"));
        assert_eq!(outcome.statements[0].kind, Kind::Other);

        // With autocommit off every statement is already inside the user's transaction:
        // the guard must not commit it (START TRANSACTION would) nor block the statement.
        let outcome = console
            .run(
                "USE `{db}`; SET autocommit = 0; UPDATE items SET note = 'manual'".replace("{db}", &name).as_str(),
                &options(1000, Some(1)),
                &AtomicBool::new(false),
            )
            .await
            .unwrap();
        assert!(outcome.statements.iter().all(|s| s.status == Status::Ok), "{outcome:?}");
        assert!(outcome.in_transaction);
        let outcome = run(&mut console, "ROLLBACK; SELECT count(*) FROM items WHERE note = 'manual'").await;
        assert_eq!(one(&outcome).as_deref(), Some("0"));
        assert!(outcome.in_transaction, "autocommit is still off");
        assert!(!run(&mut console, "SET autocommit = 1").await.in_transaction);
        let outcome = console
            .run("UPDATE items SET note = 'guarded again'", &options(1000, Some(1)), &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(outcome.statements[0].status, Status::Blocked);

        // Many open consoles do not starve the session's own pool.
        let mut held = Vec::new();
        for _ in 0..6 {
            held.push(session.open_console(Some(&name)).await.unwrap());
        }
        assert!(!session.list_databases().await.unwrap().is_empty());
        drop(held);

        // A read-only session: the server refuses the write.
        let mut read_only = t.profile.clone();
        read_only.read_only = true;
        let guarded = Session::connect(&read_only, t.password.as_deref()).await.unwrap();
        let mut console = guarded.open_console(Some(&name)).await.unwrap();
        let outcome = run(&mut console, "DELETE FROM items WHERE id = 3").await;
        assert_eq!(outcome.statements[0].status, Status::Error);
        assert_eq!(one(&run(&mut console, "SELECT count(*) FROM items").await).as_deref(), Some("3"));
        guarded.close().await;
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE `{db}`").as_str())
        .await
        .unwrap();
    outcome.expect("test body");
}

#[tokio::test]
async fn postgres_console() {
    let Some(t) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let db = scratch_name();
    let connect = |database: &str| {
        let mut opts = sqlx::postgres::PgConnectOptions::new()
            .host(&t.profile.host)
            .port(t.profile.port)
            .username(&t.profile.username)
            .database(database);
        if let Some(pw) = &t.password {
            opts = opts.password(pw);
        }
        opts
    };
    let mut admin = sqlx::PgConnection::connect_with(&connect("postgres"))
        .await
        .unwrap();
    admin
        .execute(format!("CREATE DATABASE \"{db}\"").as_str())
        .await
        .unwrap();
    let mut setup = sqlx::PgConnection::connect_with(&connect(&db))
        .await
        .unwrap();
    setup
        .execute(
            "CREATE TYPE mood AS ENUM ('calm', 'busy');
             CREATE TABLE items (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, name text, price numeric(24,6), big bigint, note text, wide text, raw bytea, feeling mood, tags text[], seen timestamptz);",
        )
        .await
        .unwrap();
    setup.execute(ITEMS).await.unwrap();
    setup
        .execute("UPDATE items SET raw = '\\x00ff10', feeling = 'calm', tags = '{a,\"b c\"}', seen = '2026-01-02 03:04:05.123456+00' WHERE id = 1")
        .await
        .unwrap();
    drop(setup);

    let name = db.clone();
    let outcome = tokio::spawn(async move {
        let session = Session::connect(&t.profile, t.password.as_deref()).await.unwrap();
        scenario(
            &session,
            &name,
            "SELECT pg_sleep(30)",
            "SELECT n FROM generate_series(1, 5000) AS n",
            "CREATE TEMP TABLE kept AS SELECT 5 AS v",
            "SELECT v FROM kept",
        )
        .await;

        // PostgreSQL specifics: bytea, a user-defined enum, arrays, timestamptz, RETURNING,
        // dollar-quoted bodies, and an error inside the user's own transaction.
        let mut console = session.open_console(Some(&name)).await.unwrap();
        let outcome = run(&mut console, "SELECT raw, feeling, tags, seen FROM items WHERE id = 1").await;
        let row = &rows(&outcome)[0];
        assert_eq!(row[0].as_deref(), Some("0x00ff10"));
        assert_eq!(row[1].as_deref(), Some("calm"));
        assert_eq!(row[2].as_deref(), Some("{a,\"b c\"}"));
        assert!(row[3].as_deref().unwrap().starts_with("2026-01-02 03:04:05.123456"));

        let outcome = run(&mut console, "INSERT INTO items (name) VALUES ('ret') RETURNING id, name").await;
        assert_eq!(outcome.statements[0].kind, Kind::Dml);
        assert_eq!(rows(&outcome)[0][1].as_deref(), Some("ret"));
        assert_eq!(outcome.statements[0].rows_affected, Some(1));

        let outcome = run(&mut console, "DO $$ BEGIN UPDATE items SET note = 'do; block' WHERE id = 2; END $$; SELECT note FROM items WHERE id = 2").await;
        assert_eq!(outcome.statements.len(), 2);
        assert_eq!(one(&outcome).as_deref(), Some("do; block"));

        // A statement that writes is never cut short at the row cap: cancelling it would
        // undo it. The rows shown are capped; the change is complete and committed.
        let outcome = console
            .run("UPDATE items SET note = 'returned' RETURNING id", &options(2, None), &AtomicBool::new(false))
            .await
            .unwrap();
        let returned = &outcome.statements[0];
        assert_eq!(returned.status, Status::Ok);
        assert_eq!((returned.results[0].rows.len(), returned.results[0].capped), (2, true));
        assert_eq!(returned.rows_affected, Some(4));
        assert_eq!(one(&run(&mut console, "SELECT count(*) FROM items WHERE note = 'returned'").await).as_deref(), Some("4"));

        // Inside the user's transaction a large result is not cancelled either: that would
        // abort the transaction and lose what they did in it.
        let outcome = console
            .run(
                "BEGIN; UPDATE items SET note = 'kept' WHERE id = 1; SELECT n FROM generate_series(1, 5000) AS n",
                &options(100, Some(1000)),
                &AtomicBool::new(false),
            )
            .await
            .unwrap();
        assert!(outcome.statements.iter().all(|s| s.status == Status::Ok), "{outcome:?}");
        let big = &outcome.statements[2].results[0];
        assert_eq!((big.rows.len(), big.capped), (100, true));
        assert_eq!(one(&run(&mut console, "SELECT note FROM items WHERE id = 1").await).as_deref(), Some("kept"));
        assert!(!run(&mut console, "ROLLBACK").await.in_transaction);

        let outcome = run(&mut console, "BEGIN; SELECT 1/0").await;
        assert_eq!(outcome.statements[1].status, Status::Error);
        assert!(outcome.statements[1].message.as_deref().unwrap().contains("run ROLLBACK"));
        assert!(outcome.in_transaction);
        assert!(!run(&mut console, "ROLLBACK").await.in_transaction);
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.expect("test body");
}
