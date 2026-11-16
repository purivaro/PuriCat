//! Integration tests for saving grid edits, against real servers.
//! Same opt-in environment variables and the same disposable-database rule as `live_db.rs`.

use puricat_core::db::write::ChangeSet;
use puricat_core::db::{PageRequest, RowPage, Session, TableRef};
use puricat_core::model::{ConnectionProfile, Engine};
use puricat_core::Error;
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

fn changes(value: serde_json::Value) -> ChangeSet {
    serde_json::from_value(value).expect("valid change set")
}

async fn rows(session: &Session, table: &TableRef) -> RowPage {
    let page: PageRequest = serde_json::from_value(json!({"offset": 0, "limit": 500})).unwrap();
    session.fetch_rows(table, &page).await.unwrap()
}

/// Cell of the row whose first column equals `id`.
fn value(page: &RowPage, id: &str, column: &str) -> Option<String> {
    let c = page
        .columns
        .iter()
        .position(|c| c.name == column)
        .expect("column");
    let row = page
        .rows
        .iter()
        .find(|r| r[0].as_deref() == Some(id))
        .unwrap_or_else(|| panic!("row {id} not found"));
    row[c].clone()
}

fn has_row(page: &RowPage, id: &str) -> bool {
    page.rows.iter().any(|r| r[0].as_deref() == Some(id))
}

const THAI: &str = "แก้ไข \"แล้ว\"\tแท็บ\nบรรทัดใหม่ 'q'";

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
async fn mysql_save_edits() {
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
                flag TINYINT(1) NOT NULL DEFAULT 0,
                created DATETIME(6),
                doc JSON,
                note TEXT,
                bits BIT(8),
                status ENUM('new', 'done') NOT NULL DEFAULT 'new',
                total DECIMAL(24,6) GENERATED ALWAYS AS (price * 2) STORED
             )",
        )
        .await
        .unwrap();
    admin
        .execute(
            "INSERT INTO items (id, name, price, note, bits) VALUES
             (1, 'first', 1.5, 'keep', b'00000101'), (2, 'second', NULL, NULL, NULL), (3, 'third', 3, 'x', NULL),
             (9007199254740992, 'big-a', 1, NULL, NULL), (9007199254740993, 'big-b', 1, NULL, NULL)",
        )
        .await
        .unwrap();
    admin
        .execute("CREATE TABLE pairs (a INT, b VARCHAR(10), v INT, PRIMARY KEY (b, a))")
        .await
        .unwrap();
    admin
        .execute("INSERT INTO pairs VALUES (1, 'x', 10), (2, 'x', 20)")
        .await
        .unwrap();
    admin
        .execute("CREATE TABLE log (message VARCHAR(50))")
        .await
        .unwrap();
    admin.execute("INSERT INTO log VALUES ('a')").await.unwrap();

    let session = Session::connect(&t.profile, t.password.as_deref())
        .await
        .unwrap();
    let items = TableRef {
        database: db.clone(),
        schema: None,
        name: "items".into(),
    };
    let key = |id: &str| json!([{"column": "id", "value": id}]);

    // Which columns the grid may write.
    let page = rows(&session, &items).await;
    let editable = |name: &str| {
        page.columns
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .editable
    };
    assert!(!editable("id") && !editable("total"));
    assert!(editable("name") && editable("flag") && editable("status") && editable("bits"));

    // The plan shown for review is parameterised: values never appear in the SQL text.
    let edit = changes(json!({
        "updates": [{"key": key("1"), "changes": [
            {"column": "name", "original": "first", "value": THAI},
            {"column": "price", "original": "1.500000", "value": "12345678901234.123456"},
            {"column": "big", "original": null, "value": "-9007199254740993"},
            {"column": "flag", "original": "0", "value": "1"},
            {"column": "created", "original": null, "value": "2027-12-31 23:59:59.000001"},
            {"column": "doc", "original": null, "value": "{\"b\": [1, 2]}"},
            {"column": "note", "original": "keep", "value": null},
            {"column": "bits", "original": "5", "value": "255"},
            {"column": "status", "original": "new", "value": "done"}
        ]}],
        "inserts": [{"values": [{"column": "name", "value": "fresh"}, {"column": "price", "value": null}]}],
        "deletes": [{"key": key("2")}]
    }));
    let plan = session.plan_changes(&items, &edit).await.unwrap();
    assert_eq!(plan.len(), 3);
    assert!(plan
        .iter()
        .all(|s| !s.sql.contains("fresh") && !s.sql.contains("แก้ไข")));

    let done = session.apply_changes(&items, &edit).await.unwrap();
    assert_eq!((done.updated, done.inserted, done.deleted), (1, 1, 1));
    let page = rows(&session, &items).await;
    assert_eq!(value(&page, "1", "name").as_deref(), Some(THAI));
    assert_eq!(
        value(&page, "1", "price").as_deref(),
        Some("12345678901234.123456")
    );
    assert_eq!(
        value(&page, "1", "big").as_deref(),
        Some("-9007199254740993")
    );
    assert_eq!(value(&page, "1", "flag").as_deref(), Some("1"));
    assert_eq!(
        value(&page, "1", "created").as_deref(),
        Some("2027-12-31 23:59:59.000001")
    );
    assert_eq!(value(&page, "1", "doc").as_deref(), Some("{\"b\": [1, 2]}"));
    assert_eq!(value(&page, "1", "note"), None);
    assert_eq!(value(&page, "1", "bits").as_deref(), Some("255"));
    assert_eq!(value(&page, "1", "status").as_deref(), Some("done"));
    assert_eq!(
        value(&page, "1", "total").as_deref(),
        Some("24691357802468.246912")
    );
    assert!(!has_row(&page, "2"));
    let fresh = page
        .rows
        .iter()
        .find(|r| r[1].as_deref() == Some("fresh"))
        .expect("inserted row");
    assert_eq!(
        fresh[4].as_deref(),
        Some("0"),
        "omitted columns take the server default"
    );

    // Saving a value identical to the stored one still counts as one matched row.
    let same = changes(json!({"updates": [{"key": key("3"), "changes": [
        {"column": "name", "original": "third", "value": "third"}]}]}));
    session.apply_changes(&items, &same).await.unwrap();

    // 64-bit keys beyond 2^53 address exactly one row.
    let big = changes(
        json!({"updates": [{"key": key("9007199254740993"), "changes": [
        {"column": "name", "original": "big-b", "value": "touched"}]}]}),
    );
    session.apply_changes(&items, &big).await.unwrap();
    let page = rows(&session, &items).await;
    assert_eq!(
        value(&page, "9007199254740993", "name").as_deref(),
        Some("touched")
    );
    assert_eq!(
        value(&page, "9007199254740992", "name").as_deref(),
        Some("big-a")
    );

    // Someone else changed the row after we loaded it: detected, and the whole save —
    // including the valid first statement — is rolled back.
    admin
        .execute("UPDATE items SET name = 'changed elsewhere' WHERE id = 3")
        .await
        .unwrap();
    let stale = changes(json!({"updates": [
        {"key": key("1"), "changes": [{"column": "flag", "original": "1", "value": "0"}]},
        {"key": key("3"), "changes": [{"column": "name", "original": "third", "value": "mine"}]}
    ]}));
    match session.apply_changes(&items, &stale).await {
        Err(Error::Conflict(message)) => assert!(message.contains("id=3"), "{message}"),
        other => panic!("expected a conflict, got {other:?}"),
    }
    let page = rows(&session, &items).await;
    assert_eq!(
        value(&page, "3", "name").as_deref(),
        Some("changed elsewhere")
    );
    assert_eq!(
        value(&page, "1", "flag").as_deref(),
        Some("1"),
        "rolled back"
    );

    // A NULL the user saw that has since been filled in is a conflict too.
    admin
        .execute("UPDATE items SET note = 'now set' WHERE id = 3")
        .await
        .unwrap();
    let stale_null = changes(json!({"updates": [{"key": key("3"), "changes": [
        {"column": "note", "original": null, "value": "mine"}]}]}));
    assert!(matches!(
        session.apply_changes(&items, &stale_null).await,
        Err(Error::Conflict(_))
    ));

    // Deleting a row that is already gone.
    let gone = changes(json!({"deletes": [{"key": key("2")}]}));
    assert!(matches!(
        session.apply_changes(&items, &gone).await,
        Err(Error::Conflict(_))
    ));

    // Invalid input is rejected by the server (strict mode), not silently coerced.
    for (column, original, bad) in [
        ("price", "12345678901234.123456", "abc"),
        ("big", "-9007199254740993", "12x"),
        ("status", "done", "nope"),
    ] {
        let invalid = changes(json!({"updates": [{"key": key("1"), "changes": [
            {"column": column, "original": original, "value": bad}]}]}));
        match session.apply_changes(&items, &invalid).await {
            Err(Error::Database(message)) => {
                assert!(message.contains("Nothing was saved"), "{message}")
            }
            other => panic!("{column}: expected a database error, got {other:?}"),
        }
    }
    let null_name = changes(json!({"updates": [{"key": key("1"), "changes": [
        {"column": "name", "original": THAI, "value": null}]}]}));
    assert!(matches!(
        session.apply_changes(&items, &null_name).await,
        Err(Error::Database(_))
    ));
    let page = rows(&session, &items).await;
    assert_eq!(
        value(&page, "1", "price").as_deref(),
        Some("12345678901234.123456")
    );

    // Generated columns and unknown columns are refused before anything is sent.
    for column in ["id", "total", "nope"] {
        let bad = changes(json!({"updates": [{"key": key("1"), "changes": [
            {"column": column, "original": "1", "value": "2"}]}]}));
        assert!(
            matches!(
                session.apply_changes(&items, &bad).await,
                Err(Error::Invalid(_))
            ),
            "{column}"
        );
    }

    // Composite keys.
    let pairs = TableRef {
        name: "pairs".into(),
        ..items.clone()
    };
    let composite = changes(json!({"updates": [{
        "key": [{"column": "a", "value": "2"}, {"column": "b", "value": "x"}],
        "changes": [{"column": "v", "original": "20", "value": "21"}]}]}));
    session.apply_changes(&pairs, &composite).await.unwrap();
    let page = rows(&session, &pairs).await;
    let v: Vec<_> = page.rows.iter().map(|r| r[2].clone().unwrap()).collect();
    assert_eq!(v, vec!["10", "21"]);
    let partial = changes(json!({"deletes": [{"key": [{"column": "b", "value": "x"}]}]}));
    assert!(matches!(
        session.apply_changes(&pairs, &partial).await,
        Err(Error::Invalid(_))
    ));

    // Keyless tables: existing rows are off limits, appending is fine.
    let log = TableRef {
        name: "log".into(),
        ..items.clone()
    };
    let touch = changes(json!({"deletes": [{"key": []}]}));
    assert!(matches!(
        session.apply_changes(&log, &touch).await,
        Err(Error::Unsupported(_))
    ));
    let append = changes(json!({"inserts": [{"values": [{"column": "message", "value": "b"}]}]}));
    session.apply_changes(&log, &append).await.unwrap();
    assert_eq!(rows(&session, &log).await.rows.len(), 2);
    session.close().await;

    // A read-only session: the server itself refuses the write.
    let mut read_only = t.profile.clone();
    read_only.read_only = true;
    let session = Session::connect(&read_only, t.password.as_deref())
        .await
        .unwrap();
    assert!(matches!(
        session.apply_changes(&log, &append).await,
        Err(Error::Database(_))
    ));
    assert_eq!(rows(&session, &log).await.rows.len(), 2);
    session.close().await;
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
async fn postgres_save_edits() {
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
                flag boolean NOT NULL DEFAULT false,
                created timestamptz,
                doc jsonb,
                tags text[],
                uid uuid,
                state shop.mood,
                day date,
                short varchar(5),
                total numeric GENERATED ALWAYS AS (price * 2) STORED
             );
             INSERT INTO shop.items (name, price, state) VALUES ('first', 1.5, 'calm'), ('second', NULL, NULL), ('third', 3, 'busy');
             CREATE TABLE shop.\"Pairs\" (a int, b text, v int, PRIMARY KEY (b, a));
             INSERT INTO shop.\"Pairs\" VALUES (1, 'x', 10), (2, 'x', 20);
             CREATE TABLE shop.log (message text);
             INSERT INTO shop.log VALUES ('a');",
        )
        .await
        .unwrap();

    let mut profile = t.profile.clone();
    profile.database = Some(db.clone());
    let session = Session::connect(&profile, t.password.as_deref())
        .await
        .unwrap();
    let items = TableRef {
        database: db.clone(),
        schema: Some("shop".into()),
        name: "items".into(),
    };
    let key = |id: &str| json!([{"column": "id", "value": id}]);

    let page = rows(&session, &items).await;
    let editable = |name: &str| {
        page.columns
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .editable
    };
    assert!(!editable("id") && !editable("total"));
    assert!(editable("name") && editable("tags") && editable("state"));

    let edit = changes(json!({
        "updates": [{"key": key("1"), "changes": [
            {"column": "name", "original": "first", "value": THAI},
            {"column": "price", "original": "1.500000", "value": "12345678901234.123456"},
            {"column": "flag", "original": "false", "value": "true"},
            {"column": "created", "original": null, "value": "2027-12-31 23:59:59.000001+00"},
            {"column": "doc", "original": null, "value": "{\"b\": [1, 2]}"},
            {"column": "tags", "original": null, "value": "{a,\"b c\"}"},
            {"column": "uid", "original": null, "value": "6f1c1c9e-0b0f-4c4e-9a52-0d6b7c0a1f11"},
            {"column": "state", "original": "calm", "value": null},
            {"column": "day", "original": null, "value": "2026-02-28"},
            {"column": "short", "original": null, "value": "ไทย"}
        ]}],
        "inserts": [
            {"values": [{"column": "name", "value": "fresh"}, {"column": "state", "value": "busy"}]}
        ],
        "deletes": [{"key": key("2")}]
    }));
    let done = session.apply_changes(&items, &edit).await.unwrap();
    assert_eq!((done.updated, done.inserted, done.deleted), (1, 1, 1));
    let page = rows(&session, &items).await;
    assert_eq!(value(&page, "1", "name").as_deref(), Some(THAI));
    assert_eq!(
        value(&page, "1", "price").as_deref(),
        Some("12345678901234.123456")
    );
    assert_eq!(value(&page, "1", "flag").as_deref(), Some("true"));
    assert!(value(&page, "1", "created").unwrap().contains("59.000001"));
    assert_eq!(value(&page, "1", "doc").as_deref(), Some("{\"b\": [1, 2]}"));
    assert_eq!(value(&page, "1", "tags").as_deref(), Some("{a,\"b c\"}"));
    assert_eq!(
        value(&page, "1", "uid").as_deref(),
        Some("6f1c1c9e-0b0f-4c4e-9a52-0d6b7c0a1f11")
    );
    assert_eq!(value(&page, "1", "state"), None);
    assert_eq!(value(&page, "1", "day").as_deref(), Some("2026-02-28"));
    assert_eq!(value(&page, "1", "short").as_deref(), Some("ไทย"));
    assert!(!has_row(&page, "2"));
    let fresh = page
        .rows
        .iter()
        .find(|r| r[1].as_deref() == Some("fresh"))
        .expect("inserted row");
    assert_eq!(
        fresh[3].as_deref(),
        Some("false"),
        "omitted columns take the server default"
    );

    // Conflict with rollback of the whole save.
    admin
        .execute("UPDATE shop.items SET name = 'changed elsewhere' WHERE id = 3")
        .await
        .unwrap();
    let stale = changes(json!({"updates": [
        {"key": key("1"), "changes": [{"column": "flag", "original": "true", "value": "false"}]},
        {"key": key("3"), "changes": [{"column": "name", "original": "third", "value": "mine"}]}
    ]}));
    match session.apply_changes(&items, &stale).await {
        Err(Error::Conflict(message)) => assert!(message.contains("id=3"), "{message}"),
        other => panic!("expected a conflict, got {other:?}"),
    }
    let page = rows(&session, &items).await;
    assert_eq!(
        value(&page, "3", "name").as_deref(),
        Some("changed elsewhere")
    );
    assert_eq!(
        value(&page, "1", "flag").as_deref(),
        Some("true"),
        "rolled back"
    );

    // Invalid input: the server's own message, and nothing kept.
    for (column, original, bad) in [
        ("price", json!("12345678901234.123456"), "abc"),
        ("state", json!(null), "nope"),
        ("short", json!("ไทย"), "too long for five"),
        ("day", json!("2026-02-28"), "2026-02-30"),
        ("doc", json!("{\"b\": [1, 2]}"), "{not json"),
    ] {
        let invalid = changes(json!({"updates": [{"key": key("1"), "changes": [
            {"column": column, "original": original, "value": bad}]}]}));
        match session.apply_changes(&items, &invalid).await {
            Err(Error::Database(message)) => {
                assert!(message.contains("Nothing was saved"), "{message}")
            }
            other => panic!("{column}: expected a database error, got {other:?}"),
        }
    }
    for column in ["id", "total", "nope"] {
        let bad = changes(json!({"updates": [{"key": key("1"), "changes": [
            {"column": column, "original": "1", "value": "2"}]}]}));
        assert!(
            matches!(
                session.apply_changes(&items, &bad).await,
                Err(Error::Invalid(_))
            ),
            "{column}"
        );
    }

    // Composite key and a quoted table name.
    let pairs = TableRef {
        name: "Pairs".into(),
        ..items.clone()
    };
    let composite = changes(json!({"updates": [{
        "key": [{"column": "a", "value": "2"}, {"column": "b", "value": "x"}],
        "changes": [{"column": "v", "original": "20", "value": "21"}]}]}));
    session.apply_changes(&pairs, &composite).await.unwrap();
    let v: Vec<_> = rows(&session, &pairs)
        .await
        .rows
        .iter()
        .map(|r| r[2].clone().unwrap())
        .collect();
    assert_eq!(v, vec!["10", "21"]);

    // Keyless: append only.
    let log = TableRef {
        name: "log".into(),
        ..items.clone()
    };
    assert!(matches!(
        session
            .apply_changes(&log, &changes(json!({"deletes": [{"key": []}]})))
            .await,
        Err(Error::Unsupported(_))
    ));
    let append = changes(json!({"inserts": [{"values": [{"column": "message", "value": "b"}]}]}));
    session.apply_changes(&log, &append).await.unwrap();
    assert_eq!(rows(&session, &log).await.rows.len(), 2);
    session.close().await;

    // Read-only session: refused by the server.
    profile.read_only = true;
    let session = Session::connect(&profile, t.password.as_deref())
        .await
        .unwrap();
    assert!(matches!(
        session.apply_changes(&log, &append).await,
        Err(Error::Database(_))
    ));
    assert_eq!(rows(&session, &log).await.rows.len(), 2);
    session.close().await;
}
