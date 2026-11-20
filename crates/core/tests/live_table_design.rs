//! New tables from the table form, against real servers: what the statement makes is what
//! the server then describes. Every database created here is named `puricat_it_<random>` and
//! dropped again.

use puricat_core::db::table_design::NewTable;
use puricat_core::db::{Session, TableRef};
use puricat_core::model::{ConnectionProfile, Engine};
use serde_json::json;
use sqlx::{Connection, Executor, Row};

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

fn spec(
    database: &str,
    schema: Option<&str>,
    name: &str,
    columns: serde_json::Value,
    comment: Option<&str>,
) -> NewTable {
    serde_json::from_value(json!({
        "table": { "database": database, "schema": schema, "name": name },
        "columns": columns, "comment": comment,
    }))
    .unwrap()
}

fn columns() -> serde_json::Value {
    json!([
        { "name": "id", "type": "INT", "unsigned": true, "auto": true, "key": true },
        { "name": "ชื่อ", "type": "VARCHAR", "length": 100, "comment": "ชื่อลูกค้า" },
        { "name": "total", "type": "DECIMAL", "length": 12, "scale": 2, "default": { "kind": "value", "value": "0" } },
        { "name": "note", "type": "TEXT", "nullable": true },
        { "name": "placed", "type": "DATETIME", "default": { "kind": "currentTimestamp" } },
        { "name": "status", "type": "VARCHAR", "length": 20, "default": { "kind": "value", "value": "it's new" } }
    ])
}

#[tokio::test]
async fn mysql_new_table() {
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
        let session = Session::connect(&profile, password.as_deref()).await.unwrap();
        let ran = session.create_table(&spec(&d, None, "ลูกค้า orders", columns(), Some("ทดสอบ"))).await.unwrap();
        assert_eq!(ran.len(), 1);
        let meta = session
            .describe_table(&TableRef { database: d.clone(), schema: None, name: "ลูกค้า orders".into() })
            .await
            .unwrap();
        let names: Vec<_> = meta.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "ชื่อ", "total", "note", "placed", "status"]);
        assert_eq!(meta.primary_key, ["id"]);
        // A row with only the required value takes every default.
        let mut conn = sqlx::MySqlConnection::connect_with(&opts.database(&d)).await.unwrap();
        conn.execute("INSERT INTO `ลูกค้า orders` (`ชื่อ`) VALUES ('สมชาย')").await.unwrap();
        let row = sqlx::query("SELECT id, CAST(total AS CHAR), note IS NULL, placed IS NOT NULL, status, (SELECT CONVERT(TABLE_COMMENT USING utf8mb4) FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'ลูกค้า orders'), (SELECT CONVERT(COLUMN_COMMENT USING utf8mb4) FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND COLUMN_NAME = 'ชื่อ') FROM `ลูกค้า orders`")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(row.get::<u32, _>(0), 1);
        assert_eq!(row.get::<String, _>(1), "0.00");
        assert!(row.get::<bool, _>(2) && row.get::<bool, _>(3));
        assert_eq!(row.get::<String, _>(4), "it's new");
        assert_eq!((row.get::<String, _>(5), row.get::<String, _>(6)), ("ทดสอบ".into(), "ชื่อลูกค้า".into()));
        // The same name again: the server's refusal, in its words.
        let again = session.create_table(&spec(&d, None, "ลูกค้า orders", columns(), None)).await.unwrap_err();
        assert!(again.to_string().contains("already exists"), "{again}");
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
async fn postgres_new_table() {
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
        let session = Session::connect(&p, pw.as_deref()).await.unwrap();
        let cols = json!([
            { "name": "id", "type": "bigint", "auto": true, "key": true },
            { "name": "Order No", "type": "varchar", "length": 20, "comment": "เลขที่" },
            { "name": "amount", "type": "numeric", "length": 12, "scale": 2, "default": { "kind": "value", "value": "1.5" } },
            { "name": "at", "type": "timestamptz", "default": { "kind": "currentTimestamp" } },
            { "name": "flag", "type": "boolean", "nullable": true }
        ]);
        let ran = session.create_table(&spec(&d, Some("public"), "orders", cols.clone(), Some("ใบสั่ง"))).await.unwrap();
        assert_eq!(ran.len(), 3, "{ran:?}");
        let mut conn = sqlx::PgConnection::connect_with(&o).await.unwrap();
        conn.execute("INSERT INTO orders (\"Order No\") VALUES ('A-1')").await.unwrap();
        let row = sqlx::query("SELECT id, amount::text, at IS NOT NULL, flag IS NULL, obj_description('public.orders'::regclass), col_description('public.orders'::regclass, 2) FROM orders")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(row.get::<i64, _>(0), 1);
        assert_eq!(row.get::<String, _>(1), "1.50");
        assert!(row.get::<bool, _>(2) && row.get::<bool, _>(3));
        assert_eq!((row.get::<String, _>(4), row.get::<String, _>(5)), ("ใบสั่ง".into(), "เลขที่".into()));
        // A schema that does not exist: the server's refusal.
        let bad = spec(&d, Some("no_such_schema"), "t2", json!([{ "name": "a", "type": "text" }]), None);
        assert!(session.create_table(&bad).await.unwrap_err().to_string().contains("no_such_schema"));
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}
