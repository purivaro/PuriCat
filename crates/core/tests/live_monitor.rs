//! The sessions of a server, and ending one, against real servers. Same opt-in environment
//! variables as `live_db.rs`. Nothing is created: a second connection of the test sleeps,
//! and the session it runs in is the one that is stopped and then ended.

use std::time::{Duration, Instant};

use puricat_core::db::monitor::End;
use puricat_core::db::Session;
use puricat_core::model::{ConnectionProfile, Engine};
use serde_json::json;
use sqlx::{Connection, Row};

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

/// Wait until the listing shows a session running `marker`, and return its id.
async fn find(session: &Session, marker: &str) -> u64 {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let list = session.processes().await.unwrap();
        if let Some(p) = list
            .iter()
            .find(|p| p.query.as_deref().is_some_and(|q| q.contains(marker)) && !p.own)
        {
            assert!(!p.idle, "a running statement listed as idle: {p:?}");
            return p.id;
        }
        assert!(
            Instant::now() < end,
            "the sleeping session never showed: {list:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn mysql_sessions_are_listed_stopped_and_ended() {
    let Some((profile, password)) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let session = Session::connect(&profile, password.as_deref())
        .await
        .unwrap();
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&profile.host)
        .port(profile.port)
        .username(&profile.username);
    if let Some(pw) = &password {
        opts = opts.password(pw);
    }
    let mut sleeper = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();

    // The listing has this session itself, marked, with its own query.
    let list = session.processes().await.unwrap();
    let own: Vec<_> = list.iter().filter(|p| p.own).collect();
    assert_eq!(own.len(), 1, "{list:?}");
    assert!(
        own[0].query.as_deref().unwrap().contains("processlist"),
        "{:?}",
        own[0]
    );

    // Stop the query: the statement ends early and the connection stays usable.
    let marker = "puricat_monitor_stop";
    let started = Instant::now();
    let sql = format!("SELECT SLEEP(30) AS {marker}");
    let running = tokio::spawn(async move {
        let r = sqlx::query(&sql)
            .fetch_one(&mut sleeper)
            .await
            .map(|r| r.try_get::<i64, _>(0).ok());
        (r, sleeper)
    });
    let id = find(&session, marker).await;
    assert_eq!(
        session.end_process(id, End::Query).await.unwrap(),
        format!("KILL QUERY {id}")
    );
    let (outcome, mut sleeper) = running.await.unwrap();
    // A stopped SLEEP returns 1 rather than an error.
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the query was not stopped: {outcome:?}"
    );
    let still: i64 = sqlx::query("SELECT 5")
        .fetch_one(&mut sleeper)
        .await
        .unwrap()
        .get(0);
    assert_eq!(still, 5, "the session should have survived KILL QUERY");

    // End the connection: the next statement on it fails.
    assert_eq!(
        session.end_process(id, End::Connection).await.unwrap(),
        format!("KILL CONNECTION {id}")
    );
    assert!(sqlx::query("SELECT 6")
        .fetch_one(&mut sleeper)
        .await
        .is_err());
    // A session that is gone: the server says so.
    let gone = session.end_process(id, End::Connection).await.unwrap_err();
    assert!(
        gone.to_string().contains(&id.to_string())
            || gone.to_string().to_lowercase().contains("unknown thread"),
        "{gone}"
    );
    session.close().await;
}

#[tokio::test]
async fn postgres_sessions_are_listed_stopped_and_ended() {
    let Some((profile, password)) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let session = Session::connect(&profile, password.as_deref())
        .await
        .unwrap();
    let mut opts = sqlx::postgres::PgConnectOptions::new()
        .host(&profile.host)
        .port(profile.port)
        .username(&profile.username)
        .database("postgres")
        .application_name("puricat-monitor-test");
    if let Some(pw) = &password {
        opts = opts.password(pw);
    }
    let mut sleeper = sqlx::PgConnection::connect_with(&opts).await.unwrap();

    let list = session.processes().await.unwrap();
    assert_eq!(list.iter().filter(|p| p.own).count(), 1, "{list:?}");
    // The test's own idle connection is listed with its application name.
    assert!(
        list.iter()
            .any(|p| p.client.as_deref() == Some("puricat-monitor-test") && p.idle),
        "{list:?}"
    );

    let marker = "puricat_monitor_stop";
    let sql = format!("SELECT pg_sleep(30) AS {marker}");
    let running = tokio::spawn(async move {
        let r = sqlx::query(&sql).execute(&mut sleeper).await.map(|_| ());
        (r, sleeper)
    });
    let id = find(&session, marker).await;
    assert_eq!(
        session.end_process(id, End::Query).await.unwrap(),
        format!("SELECT pg_cancel_backend({id})")
    );
    let (outcome, mut sleeper) = running.await.unwrap();
    assert!(outcome
        .unwrap_err()
        .to_string()
        .contains("canceling statement"));
    let still: i32 = sqlx::query("SELECT 5")
        .fetch_one(&mut sleeper)
        .await
        .unwrap()
        .get(0);
    assert_eq!(still, 5);

    assert_eq!(
        session.end_process(id, End::Connection).await.unwrap(),
        format!("SELECT pg_terminate_backend({id})")
    );
    assert!(sqlx::query("SELECT 6")
        .fetch_one(&mut sleeper)
        .await
        .is_err());
    let gone = session.end_process(id, End::Connection).await.unwrap_err();
    assert!(gone.to_string().contains("did not signal"), "{gone}");
    session.close().await;
}
