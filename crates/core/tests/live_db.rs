//! Integration tests against real servers.
//!
//! They only run when told where a *disposable* server is:
//!
//! ```text
//! PURICAT_TEST_MYSQL=root@127.0.0.1:3306      [PURICAT_TEST_MYSQL_PASSWORD=...]
//! PURICAT_TEST_POSTGRES=me@127.0.0.1:5432     [PURICAT_TEST_POSTGRES_PASSWORD=...]
//! ```
//!
//! Each test creates its own database named `puricat_it_<random>` and drops it again.
//! Nothing else on the server is read or modified.

use puricat_core::db::{ObjectKind, PageRequest, Session, SortSpec, TableRef, ValueKind};
use puricat_core::model::{ConnectionProfile, Engine};
use puricat_core::Error;
use sqlx::{Connection, Executor};

struct Target {
    profile: ConnectionProfile,
    password: Option<String>,
}

/// Parse `user@host:port` from the environment; `None` skips the test.
fn target(engine: Engine, var: &str) -> Option<Target> {
    let spec = std::env::var(var).ok()?;
    let (user, rest) = spec.split_once('@')?;
    let (host, port) = rest.rsplit_once(':')?;
    let profile = serde_json::from_value(serde_json::json!({
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

fn page(offset: u64, limit: u32, sort: Option<(&str, bool)>) -> PageRequest {
    serde_json::from_value(serde_json::json!({
        "offset": offset, "limit": limit,
        "sort": sort.map(|(column, descending)| serde_json::json!({"column": column, "descending": descending})),
    }))
    .unwrap()
}

fn cell(page: &puricat_core::db::RowPage, row: usize, column: &str) -> Option<String> {
    let c = page
        .columns
        .iter()
        .position(|c| c.name == column)
        .expect("column exists");
    page.rows[row][c].clone()
}

/// The `name` cells of the rows a filter leaves, in primary-key order.
async fn names(
    session: &Session,
    table: &TableRef,
    filters: serde_json::Value,
) -> Result<Vec<String>, Error> {
    let req: PageRequest = serde_json::from_value(serde_json::json!({
        "offset": 0, "limit": 100, "filters": filters,
    }))
    .unwrap();
    let p = session.fetch_rows(table, &req).await?;
    Ok((0..p.rows.len())
        .map(|r| cell(&p, r, "name").unwrap())
        .collect())
}

/// Filters on the three `items` rows, which hold the same values on both engines.
async fn filter_checks(session: &Session, items: &TableRef) {
    use serde_json::json;
    let one =
        |column: &str, op: &str, value: &str| json!([{"column": column, "op": op, "value": value}]);
    let is = |column: &str, op: &str| json!([{"column": column, "op": op}]);
    macro_rules! leaves {
        ($filters:expr, $expected:expr) => {
            let filters = $filters;
            let expected: &[&str] = &$expected;
            assert_eq!(
                names(session, items, filters.clone()).await.unwrap(),
                expected,
                "{filters}"
            );
        };
    }

    // Typed comparisons: exact on 64-bit integers and decimals, whatever the spelling.
    leaves!(one("id", "equals", "2"), ["second"]);
    leaves!(one("big", "equals", "9007199254740993"), [THAI]);
    leaves!(one("big", "equals", "9007199254740992"), []);
    leaves!(one("price", "equals", "0.5"), ["third"]);
    leaves!(one("price", "equals", ".50"), ["third"]);
    leaves!(one("price", "greater", "1"), [THAI]);
    leaves!(one("price", "lessOrEqual", "0.5"), ["third"]);
    leaves!(one("created", "greaterOrEqual", "2026-01-01"), [THAI]);
    leaves!(one("name", "equals", "second"), ["second"]);
    // "Is not" keeps the rows that hold NULL.
    leaves!(one("price", "notEquals", "0.5"), [THAI, "second"]);
    leaves!(one("note", "notContains", "sho"), [THAI, "second"]);

    // Text search: case-insensitive, on the text the grid shows, wildcards taken literally.
    leaves!(one("name", "contains", "ECON"), ["second"]);
    leaves!(one("name", "contains", "'quote'"), [THAI]);
    leaves!(one("name", "contains", "โลก"), [THAI]);
    leaves!(one("name", "contains", "%"), []);
    leaves!(one("name", "contains", "_"), []);
    leaves!(one("name", "startsWith", "th"), ["third"]);
    leaves!(one("name", "endsWith", "d"), ["second", "third"]);
    leaves!(one("created", "contains", "2026-01"), [THAI]);
    leaves!(one("big", "startsWith", "-"), ["third"]);
    leaves!(one("doc", "contains", "\"a\""), [THAI]);
    leaves!(one("doc", "equals", "{\"a\": 1}"), [THAI]);

    // NULL tests, also on a binary column.
    leaves!(is("doc", "isNull"), ["second", "third"]);
    leaves!(is("payload", "isNotNull"), [THAI, "third"]);

    // Several conditions must all hold; paging and sorting apply to what is left.
    leaves!(
        json!([{"column": "price", "op": "isNotNull"}, {"column": "note", "op": "equals", "value": "short"}]),
        ["third"]
    );
    let req: PageRequest = serde_json::from_value(json!({
        "offset": 0, "limit": 1,
        "sort": {"column": "name", "descending": true},
        "filters": is("created", "isNull"),
    }))
    .unwrap();
    let p = session.fetch_rows(items, &req).await.unwrap();
    assert!(p.has_more && p.rows.len() == 1);
    assert_eq!(cell(&p, 0, "name").as_deref(), Some("third"));

    // Refused before anything is sent.
    for filters in [
        one("nope", "equals", "1"),
        one("payload", "equals", "00"),
        one("payload", "contains", "00"),
        one("doc", "greater", "1"),
        is("id", "equals"),
    ] {
        assert!(
            matches!(
                names(session, items, filters.clone()).await,
                Err(Error::Invalid(_))
            ),
            "{filters}"
        );
    }
    // A value the column's type cannot hold is an error, never a silent match on 0.
    assert!(names(session, items, one("id", "equals", "abc"))
        .await
        .is_err());
    assert!(names(session, items, one("created", "equals", "garbage"))
        .await
        .is_err());
}

/// One cell read in full, by the row's primary key: what a page shows shortened.
async fn full_cell_checks(session: &Session, items: &TableRef, keyless: &TableRef) {
    let key = |id: &str| -> Vec<puricat_core::db::write::CellValue> {
        serde_json::from_value(serde_json::json!([{"column": "id", "value": id}])).unwrap()
    };
    // The page cut this text at 1000 characters; here it is whole, multi-byte and all.
    let note = session.fetch_cell(items, &key("1"), "note").await.unwrap();
    assert_eq!(note.value.as_deref(), Some("ก".repeat(1500).as_str()));
    assert!(!note.cut);
    // Binary: every byte, as hex.
    let payload = session
        .fetch_cell(items, &key("1"), "payload")
        .await
        .unwrap();
    assert!(payload
        .value
        .unwrap()
        .eq_ignore_ascii_case(&format!("0x{}", "ab".repeat(30))));
    // NULL stays NULL; a short value is itself.
    let null = session.fetch_cell(items, &key("2"), "note").await.unwrap();
    assert_eq!((null.value, null.cut), (None, false));
    let short = session.fetch_cell(items, &key("3"), "note").await.unwrap();
    assert_eq!(short.value.as_deref(), Some("short"));

    // A row that is gone, an unknown column, an incomplete key, a table without a key.
    assert!(matches!(
        session.fetch_cell(items, &key("999"), "note").await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        session.fetch_cell(items, &key("1"), "nope").await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        session.fetch_cell(items, &[], "note").await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        session.fetch_cell(keyless, &[], "v").await,
        Err(Error::Unsupported(_))
    ));
}

const THAI: &str = "สวัสดี \"โลก\"\tแท็บ 'quote'";

// ---- MySQL ---------------------------------------------------------------------------

async fn mysql_admin(t: &Target) -> sqlx::MySqlConnection {
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    sqlx::MySqlConnection::connect_with(&opts)
        .await
        .expect("admin connection")
}

#[tokio::test]
async fn mysql_browse_and_page() {
    let Some(t) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let db = scratch_name();
    let mut admin = mysql_admin(&t).await;
    admin
        .execute(format!("CREATE DATABASE `{db}` CHARACTER SET utf8mb4").as_str())
        .await
        .unwrap();

    let outcome = tokio::spawn(mysql_body(t, db.clone())).await;

    admin
        .execute(format!("DROP DATABASE `{db}`").as_str())
        .await
        .unwrap();
    outcome.expect("test body");
}

async fn mysql_body(t: Target, db: String) {
    let mut admin = mysql_admin(&t).await;
    admin.execute(format!("USE `{db}`").as_str()).await.unwrap();
    admin
        .execute(
            "CREATE TABLE items (
                id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
                name VARCHAR(100) NOT NULL,
                price DECIMAL(24,6),
                big BIGINT,
                flag TINYINT(1) DEFAULT 0,
                created DATETIME(6),
                payload BLOB,
                doc JSON,
                note TEXT,
                bits BIT(8)
             )",
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO items (name, price, big, flag, created, payload, doc, note, bits) VALUES
         (?, 12345678901234.123456, 9007199254740993, 1, '2026-01-02 03:04:05.123456', ?, '{\"a\": 1}', ?, b'00000101'),
         ('second', NULL, NULL, 0, NULL, NULL, NULL, NULL, NULL),
         ('third', 0.5, -1, 0, NULL, x'00ff', NULL, 'short', NULL)",
    )
    .bind(THAI)
    .bind(vec![0xABu8; 30])
    .bind("ก".repeat(1500))
    .execute(&mut admin)
    .await
    .unwrap();
    admin
        .execute("CREATE VIEW v_items AS SELECT id, name FROM items")
        .await
        .unwrap();
    admin
        .execute("CREATE TABLE pairs (a INT, b VARCHAR(10), PRIMARY KEY (b, a))")
        .await
        .unwrap();
    admin
        .execute("CREATE TABLE `odd ``name` (v INT)")
        .await
        .unwrap();
    admin
        .execute("INSERT INTO `odd ``name` VALUES (7)")
        .await
        .unwrap();

    let session = Session::connect(&t.profile, t.password.as_deref())
        .await
        .unwrap();
    let info = session.server_info().await.unwrap();
    assert!(!info.version.is_empty());
    assert!(!info.capabilities.schemas);
    assert!(!info.read_only);

    let databases = session.list_databases().await.unwrap();
    assert!(databases.iter().any(|d| d.name == db && !d.system));
    assert!(databases
        .iter()
        .any(|d| d.name == "information_schema" && d.system));
    assert!(matches!(
        session.list_schemas(&db).await,
        Err(Error::Unsupported(_))
    ));

    let objects = session.list_objects(&db, None).await.unwrap();
    let kind = |name: &str| objects.iter().find(|o| o.name == name).map(|o| o.kind);
    assert_eq!(kind("items"), Some(ObjectKind::Table));
    assert_eq!(kind("v_items"), Some(ObjectKind::View));
    assert_eq!(kind("odd `name"), Some(ObjectKind::Table));

    let items = TableRef {
        database: db.clone(),
        schema: None,
        name: "items".into(),
    };
    let meta = session.describe_table(&items).await.unwrap();
    assert_eq!(meta.primary_key, vec!["id"]);
    let col = |name: &str| meta.columns.iter().find(|c| c.name == name).unwrap();
    assert!(col("id").primary_key && col("id").generated && !col("id").nullable);
    assert_eq!(col("id").data_type, "bigint unsigned");
    assert_eq!(col("price").kind, ValueKind::Number);
    assert_eq!(col("flag").kind, ValueKind::Bool);
    assert_eq!(col("created").kind, ValueKind::Temporal);
    assert_eq!(col("payload").kind, ValueKind::Binary);
    assert_eq!(col("doc").kind, ValueKind::Json);
    assert!(col("name").kind == ValueKind::Text && !col("name").nullable);

    // Values arrive exactly as the server renders them: no float rounding, no charset damage.
    let p = session
        .fetch_rows(&items, &page(0, 200, None))
        .await
        .unwrap();
    assert_eq!(p.rows.len(), 3);
    assert!(!p.has_more);
    assert_eq!(p.ordered_by, vec!["id"]);
    assert_eq!(cell(&p, 0, "name").as_deref(), Some(THAI));
    assert_eq!(
        cell(&p, 0, "price").as_deref(),
        Some("12345678901234.123456")
    );
    assert_eq!(cell(&p, 0, "big").as_deref(), Some("9007199254740993"));
    assert_eq!(
        cell(&p, 0, "created").as_deref(),
        Some("2026-01-02 03:04:05.123456")
    );
    assert_eq!(cell(&p, 0, "doc").as_deref(), Some("{\"a\": 1}"));
    assert_eq!(cell(&p, 0, "bits").as_deref(), Some("5"));
    assert_eq!(cell(&p, 1, "price"), None);
    assert_eq!(cell(&p, 2, "payload").as_deref(), Some("0x00FF"));
    // Long text and binary are cut to the preview length and flagged.
    assert_eq!(cell(&p, 0, "note").unwrap().chars().count(), 1000);
    assert_eq!(
        cell(&p, 0, "payload").unwrap(),
        format!("0x{}", "AB".repeat(24))
    );
    let flagged = |row: u32, name: &str| {
        let c = p.columns.iter().position(|c| c.name == name).unwrap() as u32;
        p.truncated.contains(&[row, c])
    };
    assert!(flagged(0, "note") && flagged(0, "payload"));
    assert!(!flagged(2, "note") && !flagged(2, "payload"));

    // Paging and sorting.
    let first = session.fetch_rows(&items, &page(0, 2, None)).await.unwrap();
    assert!(first.has_more && first.rows.len() == 2);
    let last = session.fetch_rows(&items, &page(2, 2, None)).await.unwrap();
    assert!(!last.has_more && last.rows.len() == 1 && last.offset == 2);
    let sorted = session
        .fetch_rows(&items, &page(0, 10, Some(("name", true))))
        .await
        .unwrap();
    assert_eq!(cell(&sorted, 0, "name").as_deref(), Some(THAI));
    assert_eq!(cell(&sorted, 2, "name").as_deref(), Some("second"));
    assert_eq!(sorted.ordered_by, vec!["name", "id"]);

    // Requests that must be refused.
    assert!(matches!(
        session
            .fetch_rows(
                &items,
                &page(0, 10, Some(("name; DROP TABLE items", false)))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        session.fetch_rows(&items, &page(0, 0, None)).await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        session.fetch_rows(&items, &page(0, 1001, None)).await,
        Err(Error::Invalid(_))
    ));
    let missing = TableRef {
        name: "nope".into(),
        ..items.clone()
    };
    assert!(matches!(
        session.fetch_rows(&missing, &page(0, 10, None)).await,
        Err(Error::NotFound(_))
    ));

    filter_checks(&session, &items).await;
    // MySQL only: the grid shows 1 and 0 for TINYINT(1) and a number for BIT.
    assert_eq!(
        names(
            &session,
            &items,
            serde_json::json!([{"column": "flag", "op": "equals", "value": "1"}])
        )
        .await
        .unwrap(),
        [THAI]
    );
    assert_eq!(
        names(
            &session,
            &items,
            serde_json::json!([{"column": "bits", "op": "equals", "value": "5"}])
        )
        .await
        .unwrap(),
        [THAI]
    );

    // The session keeps the server's time zone and SQL dialect; only strict mode is added.
    // (Read through PuriCat's own session, from MySQL's variable tables.)
    let mut variables = std::collections::HashMap::new();
    for scope in ["session_variables", "global_variables"] {
        let table = TableRef {
            database: "performance_schema".into(),
            schema: None,
            name: scope.into(),
        };
        let p = session
            .fetch_rows(&table, &page(0, 1000, None))
            .await
            .unwrap();
        for row in &p.rows {
            variables.insert(
                (scope, row[0].clone().unwrap_or_default()),
                row[1].clone().unwrap_or_default(),
            );
        }
    }
    let var = |scope: &'static str, name: &str| variables[&(scope, name.to_string())].clone();
    assert_eq!(
        var("session_variables", "time_zone"),
        var("global_variables", "time_zone")
    );
    let mode = var("session_variables", "sql_mode");
    assert!(mode.contains("STRICT_ALL_TABLES"), "{mode}");
    assert_eq!(
        mode.contains("PIPES_AS_CONCAT"),
        var("global_variables", "sql_mode").contains("PIPES_AS_CONCAT")
    );

    // Composite keys keep their key order; awkward names are quoted, not interpolated.
    let pairs = TableRef {
        name: "pairs".into(),
        ..items.clone()
    };
    assert_eq!(
        session.describe_table(&pairs).await.unwrap().primary_key,
        vec!["b", "a"]
    );
    // Views are readable through the same path.
    let view = TableRef {
        name: "v_items".into(),
        ..items.clone()
    };
    let v = session.fetch_rows(&view, &page(0, 10, None)).await.unwrap();
    assert_eq!(v.rows.len(), 3);
    assert!(v.primary_key.is_empty() && p.primary_key == vec!["id"]);
    // An expression default is not a server-generated column.
    assert!(!col("flag").generated && col("flag").default.as_deref() == Some("0"));
    let odd = TableRef {
        name: "odd `name".into(),
        ..items.clone()
    };
    full_cell_checks(&session, &items, &odd).await;
    let p = session.fetch_rows(&odd, &page(0, 10, None)).await.unwrap();
    assert_eq!(cell(&p, 0, "v").as_deref(), Some("7"));
    assert!(p.ordered_by.is_empty(), "keyless table has no stable order");
    session.close().await;

    // A read-only profile is confirmed by the server itself.
    let mut read_only = t.profile.clone();
    read_only.read_only = true;
    let session = Session::connect(&read_only, t.password.as_deref())
        .await
        .unwrap();
    assert!(session.server_info().await.unwrap().read_only);
    session.close().await;

    // Wrong credentials fail without leaking the password.
    let mut wrong = t.profile.clone();
    wrong.username = "puricat_no_such_user".into();
    match Session::connect(&wrong, Some("s3cret-value")).await {
        Err(Error::Database(message)) => assert!(!message.contains("s3cret-value")),
        other => panic!("expected a database error, got {:?}", other.err()),
    }
}

// ---- PostgreSQL ----------------------------------------------------------------------

async fn pg_admin(t: &Target, database: &str) -> sqlx::PgConnection {
    let mut opts = sqlx::postgres::PgConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username)
        .database(database);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    sqlx::PgConnection::connect_with(&opts)
        .await
        .expect("admin connection")
}

#[tokio::test]
async fn postgres_browse_and_page() {
    let Some(t) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let db = scratch_name();
    let mut admin = pg_admin(&t, "postgres").await;
    admin
        .execute(format!("CREATE DATABASE \"{db}\"").as_str())
        .await
        .unwrap();

    let outcome = tokio::spawn(postgres_body(t, db.clone())).await;

    admin
        .execute(format!("DROP DATABASE \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.expect("test body");
}

async fn postgres_body(t: Target, db: String) {
    let mut admin = pg_admin(&t, &db).await;
    admin
        .execute(
            "CREATE SCHEMA shop;
             CREATE TYPE shop.mood AS ENUM ('calm', 'busy');
             CREATE TABLE shop.items (
                id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                name text NOT NULL,
                price numeric(24,6),
                big bigint,
                flag boolean DEFAULT false,
                created timestamptz,
                payload bytea,
                doc jsonb,
                note text,
                tags text[],
                uid uuid,
                state shop.mood
             );
             CREATE VIEW shop.v_items AS SELECT id, name FROM shop.items;
             CREATE MATERIALIZED VIEW shop.m_items AS SELECT count(*) AS n FROM shop.items;
             CREATE TABLE shop.pairs (a int, b text, PRIMARY KEY (b, a));
             CREATE TABLE shop.\"Odd \"\"Name\" (v int);
             INSERT INTO shop.\"Odd \"\"Name\" VALUES (7);
             SET TIME ZONE 'UTC';",
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO shop.items (name, price, big, flag, created, payload, doc, note, tags, uid, state) VALUES
         ($1, 12345678901234.123456, 9007199254740993, true, '2026-01-02 03:04:05.123456+00', $2, '{\"a\": 1}', $3,
          ARRAY['x', 'y z'], '6f1c1c9e-0b0f-4c4e-9a52-0d6b7c0a1f11', 'calm'),
         ('second', NULL, NULL, false, NULL, NULL, NULL, NULL, NULL, NULL, NULL),
         ('third', 0.5, -1, false, NULL, '\\x00ff', NULL, 'short', '{}', NULL, 'busy')",
    )
    .bind(THAI)
    .bind(vec![0xABu8; 30])
    .bind("ก".repeat(1500))
    .execute(&mut admin)
    .await
    .unwrap();

    // No default database in the profile: the session starts on `postgres` and still
    // reaches the scratch database through its own pool.
    let session = Session::connect(&t.profile, t.password.as_deref())
        .await
        .unwrap();
    let info = session.server_info().await.unwrap();
    assert!(info.capabilities.schemas && info.capabilities.returning);
    assert!(!info.read_only);

    assert!(session
        .list_databases()
        .await
        .unwrap()
        .iter()
        .any(|d| d.name == db));
    let schemas = session.list_schemas(&db).await.unwrap();
    assert!(schemas.iter().any(|s| s.name == "shop" && !s.system));
    assert!(schemas.iter().any(|s| s.name == "pg_catalog" && s.system));
    assert!(schemas.iter().all(|s| !s.name.starts_with("pg_toast")));
    assert!(matches!(
        session.list_objects(&db, None).await,
        Err(Error::Invalid(_))
    ));

    let objects = session.list_objects(&db, Some("shop")).await.unwrap();
    let kind = |name: &str| objects.iter().find(|o| o.name == name).map(|o| o.kind);
    assert_eq!(kind("items"), Some(ObjectKind::Table));
    assert_eq!(kind("v_items"), Some(ObjectKind::View));
    assert_eq!(kind("m_items"), Some(ObjectKind::Matview));
    assert_eq!(kind("Odd \"Name"), Some(ObjectKind::Table));

    let items = TableRef {
        database: db.clone(),
        schema: Some("shop".into()),
        name: "items".into(),
    };
    let meta = session.describe_table(&items).await.unwrap();
    assert_eq!(meta.primary_key, vec!["id"]);
    let col = |name: &str| meta.columns.iter().find(|c| c.name == name).unwrap();
    assert!(col("id").primary_key && col("id").generated && !col("id").nullable);
    assert_eq!(col("price").data_type, "numeric(24,6)");
    assert_eq!(col("price").kind, ValueKind::Number);
    assert_eq!(col("flag").kind, ValueKind::Bool);
    assert_eq!(col("flag").default.as_deref(), Some("false"));
    assert_eq!(col("created").kind, ValueKind::Temporal);
    assert_eq!(col("payload").kind, ValueKind::Binary);
    assert_eq!(col("doc").kind, ValueKind::Json);
    assert_eq!(col("tags").kind, ValueKind::Other);
    assert_eq!(col("state").data_type, "shop.mood");

    let p = session
        .fetch_rows(&items, &page(0, 200, None))
        .await
        .unwrap();
    assert_eq!(p.rows.len(), 3);
    assert_eq!(p.ordered_by, vec!["id"]);
    assert_eq!(cell(&p, 0, "name").as_deref(), Some(THAI));
    assert_eq!(
        cell(&p, 0, "price").as_deref(),
        Some("12345678901234.123456")
    );
    assert_eq!(cell(&p, 0, "big").as_deref(), Some("9007199254740993"));
    assert_eq!(cell(&p, 0, "flag").as_deref(), Some("true"));
    assert!(cell(&p, 0, "created").unwrap().starts_with("2026-01-0"));
    assert_eq!(cell(&p, 0, "doc").as_deref(), Some("{\"a\": 1}"));
    assert_eq!(cell(&p, 0, "tags").as_deref(), Some("{x,\"y z\"}"));
    assert_eq!(
        cell(&p, 0, "uid").as_deref(),
        Some("6f1c1c9e-0b0f-4c4e-9a52-0d6b7c0a1f11")
    );
    assert_eq!(cell(&p, 0, "state").as_deref(), Some("calm"));
    assert_eq!(cell(&p, 1, "price"), None);
    assert_eq!(cell(&p, 2, "payload").as_deref(), Some("0x00ff"));
    assert_eq!(cell(&p, 0, "note").unwrap().chars().count(), 1000);
    assert_eq!(
        cell(&p, 0, "payload").unwrap(),
        format!("0x{}", "ab".repeat(24))
    );
    let flagged = |row: u32, name: &str| {
        let c = p.columns.iter().position(|c| c.name == name).unwrap() as u32;
        p.truncated.contains(&[row, c])
    };
    assert!(flagged(0, "note") && flagged(0, "payload") && !flagged(2, "payload"));

    let first = session.fetch_rows(&items, &page(0, 2, None)).await.unwrap();
    assert!(first.has_more && first.rows.len() == 2);
    let sorted = session
        .fetch_rows(&items, &page(0, 10, Some(("big", false))))
        .await
        .unwrap();
    assert_eq!(cell(&sorted, 0, "big").as_deref(), Some("-1"));
    assert!(matches!(
        session
            .fetch_rows(&items, &page(0, 10, Some(("nope", false))))
            .await,
        Err(Error::Invalid(_))
    ));

    filter_checks(&session, &items).await;
    // PostgreSQL only: equality on text is case-sensitive, and types of its own
    // (uuid, enum, array) are compared as the text the grid shows.
    for (column, op, value, expected) in [
        ("name", "equals", "Second", vec![]),
        ("flag", "equals", "true", vec![THAI]),
        (
            "uid",
            "equals",
            "6f1c1c9e-0b0f-4c4e-9a52-0d6b7c0a1f11",
            vec![THAI],
        ),
        ("state", "equals", "busy", vec!["third"]),
        ("state", "notEquals", "busy", vec![THAI, "second"]),
        ("tags", "contains", "y z", vec![THAI]),
    ] {
        assert_eq!(
            names(
                &session,
                &items,
                serde_json::json!([{"column": column, "op": op, "value": value}])
            )
            .await
            .unwrap(),
            expected,
            "{column} {op} {value}"
        );
    }
    assert!(matches!(
        names(
            &session,
            &items,
            serde_json::json!([{"column": "state", "op": "greater", "value": "a"}])
        )
        .await,
        Err(Error::Invalid(_))
    ));

    let pairs = TableRef {
        name: "pairs".into(),
        ..items.clone()
    };
    assert_eq!(
        session.describe_table(&pairs).await.unwrap().primary_key,
        vec!["b", "a"]
    );
    let odd = TableRef {
        name: "Odd \"Name".into(),
        ..items.clone()
    };
    full_cell_checks(&session, &items, &odd).await;
    let p = session.fetch_rows(&odd, &page(0, 10, None)).await.unwrap();
    assert_eq!(cell(&p, 0, "v").as_deref(), Some("7"));
    assert!(p.ordered_by.is_empty());
    // Views and materialized views are readable through the same path.
    let view = TableRef {
        name: "v_items".into(),
        ..items.clone()
    };
    assert_eq!(
        session
            .fetch_rows(&view, &page(0, 10, None))
            .await
            .unwrap()
            .rows
            .len(),
        3
    );
    session.close().await;

    let mut read_only = t.profile.clone();
    read_only.read_only = true;
    read_only.database = Some(db.clone());
    let session = Session::connect(&read_only, t.password.as_deref())
        .await
        .unwrap();
    assert!(session.server_info().await.unwrap().read_only);
    session.close().await;

    let mut wrong = t.profile.clone();
    wrong.database = Some("puricat_no_such_database".into());
    assert!(matches!(
        Session::connect(&wrong, t.password.as_deref()).await,
        Err(Error::Database(_))
    ));
}

/// Nothing listens on port 1: the user must get the real reason quickly, not a pool timeout.
#[tokio::test]
async fn unreachable_server_fails_fast_with_the_real_reason() {
    for engine in ["mysql", "postgres"] {
        let profile: ConnectionProfile = serde_json::from_value(serde_json::json!({
            "name": "nowhere", "engine": engine, "host": "127.0.0.1", "port": 1, "username": "nobody",
        }))
        .unwrap();
        let started = std::time::Instant::now();
        let error = Session::connect(&profile, None)
            .await
            .err()
            .expect("must fail");
        assert!(
            started.elapsed().as_secs() < 5,
            "{engine}: took {:?}",
            started.elapsed()
        );
        let message = error.to_string().to_lowercase();
        assert!(
            matches!(error, Error::Database(_)) && message.contains("refused"),
            "{engine}: {message}"
        );
    }
}

#[test]
fn sort_spec_wire_format() {
    let s: SortSpec = serde_json::from_str(r#"{"column":"a"}"#).unwrap();
    assert!(!s.descending);
}
