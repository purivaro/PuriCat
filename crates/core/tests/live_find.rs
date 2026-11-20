//! Find in database against real servers: the counts per column, the rows a table filter then
//! shows, the columns an exact value is not looked in, and names. Every database created here
//! is named `puricat_it_<random>` and dropped again.

use puricat_core::db::filter::{Filter, FilterOp};
use puricat_core::db::find::{FindMode, TableFind};
use puricat_core::db::{Session, TableRef};
use puricat_core::model::{ConnectionProfile, Engine};
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

fn hits(found: &TableFind) -> Vec<(&str, u64)> {
    found
        .hits
        .iter()
        .map(|h| (h.column.as_str(), h.rows))
        .collect()
}

/// The rows a table tab opened from this result shows: its columns, any one of them.
async fn rows_shown(
    session: &Session,
    table: &TableRef,
    found: &TableFind,
    text: &str,
    op: FilterOp,
) -> u64 {
    let filters: Vec<Filter> = found
        .hits
        .iter()
        .map(|h| Filter {
            column: h.column.clone(),
            op,
            value: Some(text.into()),
            values: Vec::new(),
        })
        .collect();
    session
        .count_rows(table, &filters, true, &AtomicBool::new(false))
        .await
        .unwrap()
        .count
}

#[tokio::test]
async fn mysql_find_in_database() {
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
            "CREATE TABLE customers (id INT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY, name VARCHAR(80) NOT NULL, \
               email VARCHAR(80), born DATE, small TINYINT, photo BLOB)",
            "INSERT INTO customers (name, email, born, small) VALUES \
               ('Ann 50% off', 'ann@example.com', '1990-05-01', 7), ('Bob', 'bob_ANN@example.com', NULL, 100), \
               ('สมชาย ใจดี', NULL, NULL, -1)",
            "CREATE TABLE orders (id BIGINT NOT NULL PRIMARY KEY, note TEXT, total DECIMAL(10,2))",
            "INSERT INTO orders VALUES (1, 'for ann', 12.50), (100, 'x', 100.00)",
            "CREATE TABLE files (b BLOB)",
            "INSERT INTO files VALUES ('ann')",
        ] {
            conn.execute(sql).await.unwrap();
        }
        let session = Session::connect(&profile, password.as_deref()).await.unwrap();
        let stop = AtomicBool::new(false);
        let t = |name: &str| TableRef { database: d.clone(), schema: None, name: name.into() };

        // Contains, whatever the case (the collation's): per column, then the rows.
        let found = session.find_in_table(&t("customers"), "ANN", FindMode::Contains, &stop).await.unwrap();
        assert_eq!(found.searched, ["id", "name", "email", "born", "small"]);
        assert_eq!(hits(&found), [("name", 1), ("email", 2)]);
        assert_eq!(found.rows, 2);
        assert_eq!(rows_shown(&session, &t("customers"), &found, "ANN", FilterOp::Contains).await, 2);
        // Wildcards are literal; Thai text is found.
        let pct = session.find_in_table(&t("customers"), "50%", FindMode::Contains, &stop).await.unwrap();
        assert_eq!(hits(&pct), [("name", 1)]);
        let thai = session.find_in_table(&t("customers"), "ใจดี", FindMode::Contains, &stop).await.unwrap();
        assert_eq!((hits(&thai), thai.rows), (vec![("name", 1)], 1));
        let starts = session.find_in_table(&t("customers"), "bob", FindMode::StartsWith, &stop).await.unwrap();
        assert_eq!(hits(&starts), [("name", 1), ("email", 1)]);

        // Exactly: the number in the columns that can hold it, as their own type.
        let hundred = session.find_in_table(&t("customers"), "100", FindMode::Equals, &stop).await.unwrap();
        assert_eq!(hundred.searched, ["id", "name", "email", "small"]);
        assert_eq!(hits(&hundred), [("small", 1)]);
        let orders = session.find_in_table(&t("orders"), "100", FindMode::Equals, &stop).await.unwrap();
        assert_eq!(hits(&orders), [("id", 1), ("total", 1)]);
        assert_eq!(orders.rows, 1);
        assert_eq!(rows_shown(&session, &t("orders"), &orders, "100", FilterOp::Equals).await, 1);
        // 12.5 is not looked for in an integer column (MySQL would read it as 12).
        let decimal = session.find_in_table(&t("orders"), "12.5", FindMode::Equals, &stop).await.unwrap();
        assert_eq!(decimal.searched, ["note", "total"]);
        assert_eq!(hits(&decimal), [("total", 1)]);
        // Negative: not in the unsigned id; too big for TINYINT: not in small.
        let minus = session.find_in_table(&t("customers"), "-1", FindMode::Equals, &stop).await.unwrap();
        assert_eq!((minus.searched.contains(&"id".to_string()), hits(&minus)), (false, vec![("small", 1)]));
        let big = session.find_in_table(&t("customers"), "300", FindMode::Equals, &stop).await.unwrap();
        assert!(!big.searched.contains(&"small".to_string()));
        // A table of binary data only is not read.
        let files = session.find_in_table(&t("files"), "ann", FindMode::Contains, &stop).await.unwrap();
        assert_eq!((files.searched.len(), files.rows), (0, 0));
        // A table that is not there: the server's error.
        assert!(session.find_in_table(&t("nope"), "x", FindMode::Contains, &stop).await.is_err());

        // Names, whatever the case.
        let names = session.find_names(&d, None, "MAIL").await.unwrap();
        assert_eq!(
            names.iter().map(|n| (n.table.as_str(), n.column.as_deref(), n.data_type.as_deref())).collect::<Vec<_>>(),
            [("customers", Some("email"), Some("varchar(80)"))]
        );
        let names = session.find_names(&d, None, "order").await.unwrap();
        assert_eq!(names.iter().map(|n| (n.table.as_str(), n.column.as_deref())).collect::<Vec<_>>(), [("orders", None)]);
        assert!(session.find_names(&d, None, "_").await.unwrap().is_empty());
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
async fn postgres_find_in_database() {
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
        conn.execute(
            "CREATE TABLE public.people (id integer PRIMARY KEY, name text, tags text[], at timestamptz, ok boolean, \
               u uuid, amount numeric(10,2)); \
             INSERT INTO public.people VALUES \
               (1, 'Ann Lee', '{red,blue}', now(), true, 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', 1.50), \
               (40000, 'bob', NULL, NULL, false, NULL, 40000); \
             CREATE SCHEMA other; \
             CREATE TABLE other.items (id smallint PRIMARY KEY, code varchar(10)); \
             INSERT INTO other.items VALUES (1, 'ANN-1'), (2, '40000');",
        )
        .await
        .unwrap();
        let session = Session::connect(&p, pw.as_deref()).await.unwrap();
        let stop = AtomicBool::new(false);
        let people = TableRef { database: d.clone(), schema: Some("public".into()), name: "people".into() };
        let items = TableRef { database: d.clone(), schema: Some("other".into()), name: "items".into() };

        // Contains is case-insensitive here too (ILIKE), and looks at what the grid shows.
        let ann = session.find_in_table(&people, "ann", FindMode::Contains, &stop).await.unwrap();
        assert_eq!(hits(&ann), [("name", 1)]);
        let red = session.find_in_table(&people, "red", FindMode::Contains, &stop).await.unwrap();
        assert_eq!(hits(&red), [("tags", 1)]);
        assert_eq!(rows_shown(&session, &people, &red, "red", FilterOp::Contains).await, 1);

        // Exactly: text never meets a date, a boolean or an integer cast that would fail.
        let text = session.find_in_table(&people, "abc", FindMode::Equals, &stop).await.unwrap();
        assert_eq!(text.searched, ["name", "tags", "u"]);
        assert_eq!(text.rows, 0);
        let number = session.find_in_table(&people, "40000", FindMode::Equals, &stop).await.unwrap();
        assert_eq!(hits(&number), [("id", 1), ("amount", 1)]);
        assert_eq!(rows_shown(&session, &people, &number, "40000", FilterOp::Equals).await, 1);
        let money = session.find_in_table(&people, "1.5", FindMode::Equals, &stop).await.unwrap();
        assert_eq!((money.searched.contains(&"id".to_string()), hits(&money)), (false, vec![("amount", 1)]));
        let uuid = session
            .find_in_table(&people, "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11", FindMode::Equals, &stop)
            .await
            .unwrap();
        assert_eq!(hits(&uuid), [("u", 1)]);
        // 40000 does not fit a smallint: that column is not looked in, the table still is.
        let item = session.find_in_table(&items, "40000", FindMode::Equals, &stop).await.unwrap();
        assert_eq!((item.searched.clone(), hits(&item)), (vec!["code".to_string()], vec![("code", 1)]));

        // Names: one schema, or every schema that is not the server's own.
        let everywhere = session.find_names(&d, None, "CODE").await.unwrap();
        assert_eq!(
            everywhere.iter().map(|n| (n.schema.as_deref(), n.table.as_str(), n.column.as_deref())).collect::<Vec<_>>(),
            [(Some("other"), "items", Some("code"))]
        );
        assert!(session.find_names(&d, Some("public"), "code").await.unwrap().is_empty());
        let tables = session.find_names(&d, Some("public"), "peop").await.unwrap();
        assert_eq!(tables.iter().map(|n| (n.table.as_str(), n.column.as_deref())).collect::<Vec<_>>(), [("people", None)]);
        // The server's own catalogue is not searched.
        assert!(session.find_names(&d, None, "pg_class").await.unwrap().is_empty());
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}
