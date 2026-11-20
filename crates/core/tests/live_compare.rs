//! Structure compare against real servers: two MySQL databases, two PostgreSQL schemas, made
//! with known differences. Every database created here is named `puricat_it_<random>` and
//! dropped again.

use puricat_core::db::compare::{compare, Comparison};
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

/// What both engines must find: `orders` differs in two columns' type and NULL, one column
/// each side has alone, and the foreign key's action; the renamed index is the same index.
fn check(c: &Comparison) {
    assert_eq!(c.only_a, ["only_a"]);
    assert_eq!(c.only_b, ["only_b"]);
    assert_eq!(c.same, ["customers"]);
    assert_eq!(c.views_only_a, ["v_a"]);
    assert!(c.views_only_b.is_empty());
    let [orders] = c.changed.as_slice() else {
        panic!("{c:#?}")
    };
    assert_eq!(orders.name, "orders");
    let columns: Vec<(&str, bool, bool, Vec<&str>)> = orders
        .columns
        .iter()
        .map(|x| {
            (
                x.name.as_str(),
                x.a.is_some(),
                x.b.is_some(),
                x.fields.clone(),
            )
        })
        .collect();
    assert_eq!(
        columns,
        [
            ("total", true, true, vec!["type", "nullable"]),
            ("note", true, false, vec![]),
            ("extra_col", false, true, vec![])
        ]
    );
    assert!(orders.indexes.is_empty(), "{:#?}", orders.indexes);
    let actions: Vec<(bool, &str)> = orders
        .foreign_keys
        .iter()
        .map(|k| {
            (
                k.a.is_some(),
                k.a.as_ref().or(k.b.as_ref()).unwrap().on_delete.as_str(),
            )
        })
        .collect();
    assert_eq!(actions, [(true, "CASCADE"), (false, "SET NULL")]);
}

#[tokio::test]
async fn mysql_compare_two_databases() {
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
    let (a, b) = (scratch_name(), scratch_name());
    for db in [&a, &b] {
        admin
            .execute(format!("CREATE DATABASE `{db}` CHARACTER SET utf8mb4").as_str())
            .await
            .unwrap();
    }
    let (da, db) = (a.clone(), b.clone());
    let outcome = tokio::spawn(async move {
        let mut ca = sqlx::MySqlConnection::connect_with(&opts.clone().database(&da)).await.unwrap();
        for sql in [
            "CREATE TABLE customers (id INT PRIMARY KEY, name VARCHAR(80) NOT NULL)",
            "CREATE TABLE orders (id INT AUTO_INCREMENT PRIMARY KEY, customer_id INT, total DECIMAL(12,2) NOT NULL, \
               status VARCHAR(20) DEFAULT 'new', note TEXT COMMENT 'ห้ามว่าง', INDEX by_status (status), \
               CONSTRAINT fk_c FOREIGN KEY (customer_id) REFERENCES customers (id) ON DELETE CASCADE)",
            "CREATE TABLE only_a (x INT)",
            "CREATE VIEW v_a AS SELECT 1 AS one",
        ] {
            ca.execute(sql).await.unwrap();
        }
        let mut cb = sqlx::MySqlConnection::connect_with(&opts.database(&db)).await.unwrap();
        for sql in [
            "CREATE TABLE customers (id INT PRIMARY KEY, name VARCHAR(80) NOT NULL)",
            "CREATE TABLE orders (id INT AUTO_INCREMENT PRIMARY KEY, customer_id INT, total DECIMAL(14,2) NULL, \
               status VARCHAR(20) DEFAULT 'new', extra_col INT, INDEX status_idx (status), \
               CONSTRAINT other_name FOREIGN KEY (customer_id) REFERENCES customers (id) ON DELETE SET NULL)",
            "CREATE TABLE only_b (y INT)",
        ] {
            cb.execute(sql).await.unwrap();
        }
        let session = Session::connect(&profile, password.as_deref()).await.unwrap();
        let left = session.structure(&da, None).await.unwrap();
        let right = session.structure(&db, None).await.unwrap();
        // The comment is read whole, Thai and all.
        let note = left.tables.iter().find(|t| t.name == "orders").unwrap().columns.iter().find(|c| c.name == "note").unwrap();
        assert_eq!(note.comment.as_deref(), Some("ห้ามว่าง"));
        check(&compare(&left, &right));
        assert_eq!(compare(&left, &left), Comparison { same: vec!["customers".into(), "only_a".into(), "orders".into()], ..Default::default() });
        session.close().await;
    })
    .await;
    for db in [&a, &b] {
        admin
            .execute(format!("DROP DATABASE IF EXISTS `{db}`").as_str())
            .await
            .unwrap();
    }
    outcome.unwrap();
}

#[tokio::test]
async fn postgres_compare_two_schemas() {
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
            "CREATE SCHEMA a; CREATE SCHEMA b; \
             CREATE TABLE a.customers (id integer PRIMARY KEY, name varchar(80) NOT NULL); \
             CREATE TABLE a.orders (id integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, customer_id integer \
               CONSTRAINT fk_c REFERENCES a.customers (id) ON DELETE CASCADE, total numeric(12,2) NOT NULL, \
               status varchar(20) DEFAULT 'new', note text); \
             COMMENT ON COLUMN a.orders.note IS 'ห้ามว่าง'; \
             CREATE INDEX by_status ON a.orders (status); CREATE INDEX recent ON a.orders (id) WHERE total > 0; \
             CREATE TABLE a.only_a (x integer); CREATE VIEW a.v_a AS SELECT 1 AS one; \
             CREATE TABLE b.customers (id integer PRIMARY KEY, name varchar(80) NOT NULL); \
             CREATE TABLE b.orders (id integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, customer_id integer \
               CONSTRAINT other_name REFERENCES b.customers (id) ON DELETE SET NULL, total numeric(14,2), \
               status varchar(20) DEFAULT 'new', extra_col integer); \
             CREATE INDEX status_idx ON b.orders (status); CREATE INDEX recent_too ON b.orders (id) WHERE total > 0; \
             CREATE TABLE b.only_b (y integer);",
        )
        .await
        .unwrap();
        let session = Session::connect(&p, pw.as_deref()).await.unwrap();
        let left = session.structure(&d, Some("a")).await.unwrap();
        let right = session.structure(&d, Some("b")).await.unwrap();
        let orders = left.tables.iter().find(|t| t.name == "orders").unwrap();
        assert_eq!(orders.columns[0].extra, "identity by default");
        assert!(orders.indexes.iter().any(|i| i.predicate.as_deref() == Some("(total > (0)::numeric)")), "{:?}", orders.indexes);
        check(&compare(&left, &right));
        assert!(session.structure(&d, None).await.is_err());
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}
