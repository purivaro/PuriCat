//! Copying rows between tables on real servers, across engines both ways: pages in key order,
//! values kept whole (NULL, dates, decimals, Thai text), a row the target cannot hold left out
//! and reported with its source values, a second copy finding the rows already there.
//! Every database created here is named `puricat_it_<random>` and dropped again.

use puricat_core::db::copy;
use puricat_core::db::smart::{OnProblem, PasteRequest};
use puricat_core::db::{Session, TableRef};
use puricat_core::model::{ConnectionProfile, Engine};
use serde_json::json;
use sqlx::{Connection, Executor, Row};
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

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn upsert(columns: &[&str]) -> PasteRequest {
    serde_json::from_value(json!({
        "mode": "upsert", "columns": columns, "key": ["id"],
        "update": columns.iter().filter(|c| **c != "id").collect::<Vec<_>>(), "rows": [],
    }))
    .unwrap()
}

#[tokio::test]
async fn copy_between_mysql_and_postgres() {
    let (Some((my, my_pw)), Some((pg, pg_pw))) = (
        target(Engine::Mysql, "PURICAT_TEST_MYSQL"),
        target(Engine::Postgres, "PURICAT_TEST_POSTGRES"),
    ) else {
        eprintln!("skipped: PURICAT_TEST_MYSQL and PURICAT_TEST_POSTGRES are both needed");
        return;
    };
    let mut my_opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&my.host)
        .port(my.port)
        .username(&my.username);
    if let Some(pw) = &my_pw {
        my_opts = my_opts.password(pw);
    }
    let pg_opts = |database: &str| {
        let mut o = sqlx::postgres::PgConnectOptions::new()
            .host(&pg.host)
            .port(pg.port)
            .username(&pg.username)
            .database(database);
        if let Some(pw) = &pg_pw {
            o = o.password(pw);
        }
        o
    };
    let db = scratch_name();
    let mut my_admin = sqlx::MySqlConnection::connect_with(&my_opts).await.unwrap();
    my_admin
        .execute(format!("CREATE DATABASE `{db}` CHARACTER SET utf8mb4").as_str())
        .await
        .unwrap();
    let mut pg_admin = sqlx::PgConnection::connect_with(&pg_opts("postgres"))
        .await
        .unwrap();
    pg_admin
        .execute(format!("CREATE DATABASE \"{db}\"").as_str())
        .await
        .unwrap();
    let (d, mo, po) = (db.clone(), my_opts.clone().database(&db), pg_opts(&db));
    let (myp, mypw, pgp, pgpw) = (my.clone(), my_pw.clone(), pg.clone(), pg_pw.clone());

    let outcome = tokio::spawn(async move {
        let mut m = sqlx::MySqlConnection::connect_with(&mo).await.unwrap();
        for sql in [
            "SET SESSION cte_max_recursion_depth = 5000",
            "CREATE TABLE src (id INT NOT NULL PRIMARY KEY, name VARCHAR(40) NOT NULL, born DATE, amount DECIMAL(10,2), photo BLOB, note TEXT)",
            "INSERT INTO src (id, name, born, amount, note) \
               WITH RECURSIVE n (i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2503) \
               SELECT i, CONCAT('คน ', i), DATE_ADD('2026-01-01', INTERVAL i DAY), i / 4, IF(i % 7 = 0, NULL, 'ok') FROM n",
            "UPDATE src SET name = 'A name too long for ten' WHERE id = 1500",
            "CREATE TABLE back (id INT NOT NULL PRIMARY KEY, name VARCHAR(40) NOT NULL, born DATE)",
        ] {
            m.execute(sql).await.unwrap();
        }
        let mut p = sqlx::PgConnection::connect_with(&po).await.unwrap();
        p.execute("CREATE TABLE people (id integer PRIMARY KEY, name varchar(10) NOT NULL, born date, amount numeric(10,2), note text, extra text DEFAULT 'x')")
            .await
            .unwrap();
        let mysql = Session::connect(&myp, mypw.as_deref()).await.unwrap();
        let postgres = Session::connect(&pgp, pgpw.as_deref()).await.unwrap();
        let src = TableRef { database: d.clone(), schema: None, name: "src".into() };
        let people = TableRef { database: d.clone(), schema: Some("public".into()), name: "people".into() };
        let cols = ["id", "name", "born", "amount", "note"];
        let request = upsert(&cols);
        let go = AtomicBool::new(false);

        // Binary data is not copied.
        assert!(copy::preview(&mysql, &src, &names(&["id", "photo"]), &postgres, &people, &upsert(&["id", "photo"]), 10).await.is_err());
        // The first page, analysed against the target.
        let preview = copy::preview(&mysql, &src, &names(&cols), &postgres, &people, &request, 1000).await.unwrap();
        assert_eq!((preview.rows.len(), preview.analysis.counts.insert), (1000, 1000));

        // Three pages of 1,000: the long name is left out and comes back with its source values.
        let mut read = Vec::new();
        let mut inserted = 0;
        let mut failed = Vec::new();
        for page in 0..4u64 {
            let b = copy::batch(&mysql, &src, &names(&cols), &postgres, &people, &request, OnProblem::Skip, page * 1000, 1000, &go)
                .await
                .unwrap();
            read.push(b.read);
            inserted += b.outcome.inserted;
            failed.extend(b.failed_rows);
        }
        assert_eq!(read, [1000, 1000, 503, 0]);
        assert_eq!(inserted, 2502);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0][0].as_deref(), Some("1500"));
        let row = sqlx::query("SELECT count(*), sum(id)::bigint, count(note), max(born)::text, sum(amount)::text, min(name), count(*) FILTER (WHERE extra = 'x') FROM people")
            .fetch_one(&mut p)
            .await
            .unwrap();
        assert_eq!(row.get::<i64, _>(0), 2502);
        assert_eq!(row.get::<i64, _>(1), (1..=2503).sum::<i64>() - 1500);
        // Every seventh note was NULL and stays NULL; the rest are there.
        assert_eq!(row.get::<i64, _>(2), 2502 - (2503 / 7));
        assert_eq!(row.get::<String, _>(3), "2032-11-08");
        assert_eq!(row.get::<String, _>(4), format!("{:.2}", ((1..=2503).sum::<i64>() - 1500) as f64 / 4.0));
        assert_eq!(row.get::<String, _>(5), "คน 1");
        assert_eq!(row.get::<i64, _>(6), 2502);

        // Again: the rows are there and the same.
        let again = copy::batch(&mysql, &src, &names(&cols), &postgres, &people, &request, OnProblem::Skip, 0, 1000, &go).await.unwrap();
        assert_eq!((again.outcome.inserted, again.outcome.updated, again.outcome.unchanged), (0, 0, 1000));

        // And back, from PostgreSQL into MySQL, with fewer columns.
        let back = TableRef { database: d.clone(), schema: None, name: "back".into() };
        let three = ["id", "name", "born"];
        let mut total = 0;
        for page in 0..3u64 {
            total += copy::batch(&postgres, &people, &names(&three), &mysql, &back, &upsert(&three), OnProblem::Stop, page * 1000, 1000, &go)
                .await
                .unwrap()
                .outcome
                .inserted;
        }
        assert_eq!(total, 2502);
        let (count, born): (i64, String) = sqlx::query_as("SELECT COUNT(*), CAST(MAX(born) AS CHAR) FROM back").fetch_one(&mut m).await.unwrap();
        assert_eq!((count, born.as_str()), (2502, "2032-11-08"));
        mysql.close().await;
        postgres.close().await;
    })
    .await;
    my_admin
        .execute(format!("DROP DATABASE IF EXISTS `{db}`").as_str())
        .await
        .unwrap();
    pg_admin
        .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}
