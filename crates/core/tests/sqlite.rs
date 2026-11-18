//! SQLite, against real database files. No server is needed, so these always run.
//! Every test makes its own file in the system's temporary folder and removes it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use puricat_core::db::console::{Console, RunOptions, RunOutcome, Status};
use puricat_core::db::script::Kind;
use puricat_core::db::smart::RowStatus;
use puricat_core::db::{ObjectKind, Session, TableRef, ValueKind};
use puricat_core::model::{ConnectionProfile, Engine};
use puricat_core::Error;
use serde_json::{json, Value};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection, Executor};

const FIXTURE: &str = r#"
CREATE TABLE items (
  id     INTEGER PRIMARY KEY,
  name   TEXT NOT NULL,
  price  DECIMAL(10,2),
  qty    INT NOT NULL DEFAULT 0,
  big    BIGINT,
  ratio  REAL,
  active BOOLEAN DEFAULT 1,
  made   DATETIME,
  data   JSON,
  photo  BLOB,
  loose,
  code   VARCHAR(8) UNIQUE
);
INSERT INTO items VALUES
  (1, 'กาแฟ', 12.5, 3, 9223372036854775807, 0.1, 1, '2026-01-31 13:45:00', '{"a": 1.10}', x'00FF10', 42, 'A-1'),
  (2, 'Tea', 7, 0, -9007199254740993, 1e20, 0, '2026-02-01', NULL, NULL, 'text', 'B-2'),
  (3, '50% off_', NULL, 10, NULL, NULL, NULL, NULL, NULL, x'', NULL, NULL);
CREATE UNIQUE INDEX items_name_lower ON items (lower(name));
CREATE UNIQUE INDEX items_partial ON items (qty) WHERE qty > 100;

CREATE TABLE pairs (a TEXT NOT NULL, b INTEGER NOT NULL, note TEXT, PRIMARY KEY (b, a)) WITHOUT ROWID;
INSERT INTO pairs VALUES ('x', 1, 'one'), ('y', 1, 'two');
CREATE UNIQUE INDEX pairs_note ON pairs (note, a);

CREATE TABLE codes (id INTEGER PRIMARY KEY, label TEXT) WITHOUT ROWID;
CREATE TABLE strict_t (id INTEGER PRIMARY KEY, n INTEGER NOT NULL, t TEXT, v ANY) STRICT;
CREATE TABLE gen (
  id INTEGER PRIMARY KEY, w INT, h INT,
  area INT GENERATED ALWAYS AS (w * h) VIRTUAL,
  kept INT AS (w + h) STORED
);
INSERT INTO gen (w, h) VALUES (2, 3);
CREATE TABLE nokey (label TEXT, n INT);
INSERT INTO nokey VALUES ('a', 1);
CREATE TABLE counted (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT);
INSERT INTO counted (v) VALUES ('first');
CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES parent (id));
INSERT INTO parent VALUES (1, 'p');
CREATE VIEW cheap AS SELECT id, name FROM items WHERE price < 10;
"#;

/// A database file that is removed, with SQLite's side files, when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    async fn new(sql: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "puricat_it_{}.sqlite",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        ));
        let mut conn = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .connect()
            .await
            .unwrap();
        conn.execute(sql).await.unwrap();
        conn.close().await.unwrap();
        Scratch(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

fn profile(path: &str, read_only: bool) -> ConnectionProfile {
    serde_json::from_value::<ConnectionProfile>(json!({
        "name": "integration", "engine": "sqlite", "file": path,
        "readOnly": read_only, "connectTimeoutSecs": 1,
    }))
    .unwrap()
    .normalized()
    .unwrap()
}

async fn open(scratch: &Scratch) -> Session {
    Session::connect(&profile(scratch.path(), false), None)
        .await
        .unwrap()
}

fn table(name: &str) -> TableRef {
    TableRef {
        database: "main".into(),
        schema: None,
        name: name.into(),
    }
}

fn some(text: &str) -> Option<String> {
    Some(text.to_string())
}

/// The ids of `items` that pass the filters, in id order.
async fn ids(session: &Session, filters: Value) -> Result<Vec<String>, Error> {
    let request =
        serde_json::from_value(json!({ "offset": 0, "limit": 100, "filters": filters })).unwrap();
    let page = session.fetch_rows(&table("items"), &request).await?;
    Ok(page
        .rows
        .into_iter()
        .map(|r| r[0].clone().unwrap())
        .collect())
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

/// One value, asked through a console of its own.
async fn scalar(session: &Session, sql: &str) -> Option<String> {
    let mut console = session.open_console(None).await.unwrap();
    rows(&run(&mut console, sql).await)[0][0].clone()
}

const ENDLESS: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c)";

#[tokio::test]
async fn opens_an_existing_database_file_and_nothing_else() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let info = session.server_info().await.unwrap();
    assert_eq!(info.engine, Engine::Sqlite);
    assert!(info.version.starts_with("3."), "{}", info.version);
    assert_eq!(info.tls, None);
    assert!(!info.read_only);
    assert!(!info.capabilities.schemas);
    session.close().await;

    // A path that does not exist is an error, and no empty database is left behind.
    let missing = format!("{}.missing", scratch.path());
    let error = Session::connect(&profile(&missing, false), None)
        .await
        .err();
    assert!(matches!(error, Some(Error::NotFound(_))), "{error:?}");
    assert!(!Path::new(&missing).exists());

    // A folder, and a file that is something else.
    let folder = std::env::temp_dir();
    let error = Session::connect(&profile(folder.to_str().unwrap(), false), None)
        .await
        .err();
    assert!(matches!(error, Some(Error::Invalid(_))), "{error:?}");

    let other = Scratch(std::env::temp_dir().join(format!(
        "puricat_it_{}.txt",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )));
    std::fs::write(
        &other.0,
        "id,name\n1,not a database, just some text in a file\n",
    )
    .unwrap();
    let before = std::fs::read(&other.0).unwrap();
    for read_only in [true, false] {
        let error = Session::connect(&profile(other.path(), read_only), None)
            .await
            .err();
        match error {
            Some(Error::Database(m)) => assert!(m.contains("not a SQLite database"), "{m}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert_eq!(
        std::fs::read(&other.0).unwrap(),
        before,
        "the file was touched"
    );
}

#[tokio::test]
async fn the_catalogue_is_read_from_the_file() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;

    let databases = session.list_databases().await.unwrap();
    assert_eq!(databases.len(), 1);
    assert_eq!(
        (databases[0].name.as_str(), databases[0].system),
        ("main", false)
    );
    assert!(session.list_schemas("main").await.is_err());
    assert!(matches!(
        session.list_objects("other", None).await,
        Err(Error::NotFound(_))
    ));

    // SQLite's own tables (sqlite_sequence came with AUTOINCREMENT) are not listed.
    let objects = session.list_objects("main", None).await.unwrap();
    let names: Vec<&str> = objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "cheap", "child", "codes", "counted", "gen", "items", "nokey", "pairs", "parent",
            "strict_t"
        ]
    );
    let view = objects.iter().find(|o| o.name == "cheap").unwrap();
    assert_eq!(view.kind, ObjectKind::View);
    assert_eq!(objects[1].kind, ObjectKind::Table);

    let items = session.describe_table(&table("items")).await.unwrap();
    assert_eq!(items.primary_key, ["id"]);
    assert_eq!(items.estimated_rows, None);
    let col = |name: &str| items.columns.iter().find(|c| c.name == name).unwrap();
    let shape = |name: &str| {
        let c = col(name);
        (
            c.data_type.as_str(),
            c.kind,
            c.nullable,
            c.generated,
            c.editable,
        )
    };
    // INTEGER PRIMARY KEY is the row id: SQLite fills it in.
    assert_eq!(
        shape("id"),
        ("INTEGER", ValueKind::Number, true, true, false)
    );
    assert_eq!(shape("name"), ("TEXT", ValueKind::Text, false, false, true));
    assert_eq!(
        shape("price"),
        ("DECIMAL(10,2)", ValueKind::Number, true, false, true)
    );
    assert_eq!(shape("qty"), ("INT", ValueKind::Number, false, false, true));
    assert_eq!(col("qty").default.as_deref(), Some("0"));
    assert_eq!(
        shape("ratio"),
        ("REAL", ValueKind::Number, true, false, true)
    );
    assert_eq!(
        shape("active"),
        ("BOOLEAN", ValueKind::Bool, true, false, true)
    );
    assert_eq!(
        shape("made"),
        ("DATETIME", ValueKind::Temporal, true, false, true)
    );
    assert_eq!(shape("data"), ("JSON", ValueKind::Json, true, false, true));
    assert_eq!(
        shape("photo"),
        ("BLOB", ValueKind::Binary, true, false, false)
    );
    // No declared type: shown, never edited.
    assert_eq!(shape("loose"), ("", ValueKind::Other, true, false, false));
    assert_eq!(
        shape("code"),
        ("VARCHAR(8)", ValueKind::Text, true, false, true)
    );

    // A composite key comes in key order, not column order.
    let pairs = session.describe_table(&table("pairs")).await.unwrap();
    assert_eq!(pairs.primary_key, ["b", "a"]);
    assert!(pairs.columns.iter().all(|c| !c.generated));
    // WITHOUT ROWID: an INTEGER PRIMARY KEY is an ordinary column there.
    let codes = session.describe_table(&table("codes")).await.unwrap();
    assert!(!codes.columns[0].generated && codes.columns[0].editable);
    // Generated columns, virtual and stored.
    let gen = session.describe_table(&table("gen")).await.unwrap();
    let generated: Vec<(&str, bool)> = gen
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c.generated))
        .collect();
    assert_eq!(
        generated,
        [
            ("id", true),
            ("w", false),
            ("h", false),
            ("area", true),
            ("kept", true)
        ]
    );
    // STRICT: `ANY` keeps values as they are, so it is not edited either.
    let strict = session.describe_table(&table("strict_t")).await.unwrap();
    assert_eq!(strict.columns[3].kind, ValueKind::Other);
    assert!(!strict.columns[3].editable && strict.columns[2].editable);

    let keys = session.unique_keys(&table("items")).await.unwrap();
    let listed: Vec<(bool, &[String])> = keys.iter().map(|k| (k.primary, &k.columns[..])).collect();
    // The index on an expression and the partial index are not row identity.
    assert_eq!(
        listed,
        [
            (true, &["id".to_string()][..]),
            (false, &["code".to_string()][..])
        ]
    );
    let keys = session.unique_keys(&table("pairs")).await.unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(
        (keys[0].primary, &keys[0].columns[..]),
        (true, &["b".to_string(), "a".to_string()][..])
    );
    assert_eq!(
        (keys[1].name.as_str(), &keys[1].columns[..]),
        ("pairs_note", &["note".to_string(), "a".to_string()][..])
    );
    assert!(session
        .unique_keys(&table("nokey"))
        .await
        .unwrap()
        .is_empty());

    let request = serde_json::from_value(json!({ "offset": 0, "limit": 10 })).unwrap();
    assert!(matches!(
        session.fetch_rows(&table("nothing"), &request).await,
        Err(Error::NotFound(_))
    ));
    session.close().await;
}

#[tokio::test]
async fn values_are_read_as_sqlite_renders_them() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let request = serde_json::from_value(json!({ "offset": 0, "limit": 2 })).unwrap();
    let page = session.fetch_rows(&table("items"), &request).await.unwrap();
    assert!(page.has_more);
    assert_eq!(page.ordered_by, ["id"]);
    assert_eq!(
        page.rows[0],
        [
            some("1"),
            some("กาแฟ"),
            some("12.5"),
            some("3"),
            some("9223372036854775807"),
            some("0.1"),
            some("1"),
            some("2026-01-31 13:45:00"),
            some("{\"a\": 1.10}"),
            some("0x00FF10"),
            some("42"),
            some("A-1"),
        ]
    );
    // Beyond 2^53, a REAL in SQLite's own spelling, a NULL blob.
    assert_eq!(page.rows[1][4], some("-9007199254740993"));
    assert_eq!(page.rows[1][5], some("1.0e+20"));
    assert_eq!(page.rows[1][9], None);
    assert_eq!(page.rows[1][10], some("text"));

    let request = serde_json::from_value(
        json!({ "offset": 2, "limit": 2, "sort": { "column": "name", "descending": true } }),
    )
    .unwrap();
    let page = session.fetch_rows(&table("items"), &request).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0][1], some("50% off_"));
    // An empty blob is not NULL.
    assert_eq!(page.rows[0][9], some("0x"));
    assert!(!page.has_more);

    // A view is read like a table; it has no key.
    let request = serde_json::from_value(json!({ "offset": 0, "limit": 10 })).unwrap();
    let page = session.fetch_rows(&table("cheap"), &request).await.unwrap();
    assert_eq!(page.rows, [[some("2"), some("Tea")]]);
    assert!(page.primary_key.is_empty());

    // A long value is cut in the page and whole in the cell.
    let long = "ก".repeat(1500);
    let mut console = session.open_console(None).await.unwrap();
    run(
        &mut console,
        &format!("UPDATE items SET name = '{long}' WHERE id = 2"),
    )
    .await;
    let page = session.fetch_rows(&table("items"), &request).await.unwrap();
    assert_eq!(page.truncated, [[1, 1]]);
    assert_eq!(page.rows[1][1].as_ref().unwrap().chars().count(), 1000);
    let key =
        |id: &str| vec![serde_json::from_value(json!({ "column": "id", "value": id })).unwrap()];
    let cell = session
        .fetch_cell(&table("items"), &key("2"), "name")
        .await
        .unwrap();
    assert_eq!(
        (cell.value.as_deref(), cell.cut),
        (Some(long.as_str()), false)
    );
    let cell = session
        .fetch_cell(&table("items"), &key("1"), "photo")
        .await
        .unwrap();
    assert_eq!(cell.value.as_deref(), Some("0x00FF10"));
    let cell = session
        .fetch_cell(&table("items"), &key("2"), "photo")
        .await
        .unwrap();
    assert_eq!(cell.value, None);
    assert!(matches!(
        session
            .fetch_cell(&table("items"), &key("99"), "name")
            .await,
        Err(Error::Conflict(_))
    ));
    session.close().await;
}

#[tokio::test]
async fn filters_compare_by_the_columns_own_type() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let f = |column: &str, op: &str, value: &str| json!([{ "column": column, "op": op, "value": value }]);
    let none: [&str; 0] = [];

    // Integers are compared as integers, to the last digit.
    assert_eq!(ids(&session, f("id", "equals", "2")).await.unwrap(), ["2"]);
    assert_eq!(
        ids(&session, f("big", "equals", "9223372036854775807"))
            .await
            .unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("big", "equals", "9223372036854775806"))
            .await
            .unwrap(),
        none
    );
    assert_eq!(ids(&session, f("big", "less", "0")).await.unwrap(), ["2"]);
    // Numbers by value, whatever the spelling.
    assert_eq!(
        ids(&session, f("price", "greater", "7")).await.unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("price", "greaterOrEqual", "7.00"))
            .await
            .unwrap(),
        ["1", "2"]
    );
    assert_eq!(
        ids(&session, f("price", "equals", "12.50")).await.unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("qty", "lessOrEqual", "3")).await.unwrap(),
        ["1", "2"]
    );
    // "Is not" keeps the rows that hold NULL.
    assert_eq!(
        ids(&session, f("price", "notEquals", "7")).await.unwrap(),
        ["1", "3"]
    );
    assert_eq!(
        ids(&session, f("name", "notEquals", "Tea")).await.unwrap(),
        ["1", "3"]
    );
    // Text that is not a number is refused, not compared as text after every number.
    for op in ["equals", "greater"] {
        let error = ids(&session, f("qty", op, "abc")).await.err();
        assert!(matches!(error, Some(Error::Invalid(_))), "{error:?}");
    }

    // Text: Thai, wildcards taken literally, ASCII letters in either case.
    assert_eq!(
        ids(&session, f("name", "equals", "กาแฟ")).await.unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("name", "contains", "แฟ")).await.unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("name", "contains", "%")).await.unwrap(),
        ["3"]
    );
    assert_eq!(
        ids(&session, f("name", "endsWith", "_")).await.unwrap(),
        ["3"]
    );
    assert_eq!(
        ids(&session, f("name", "startsWith", "50%")).await.unwrap(),
        ["3"]
    );
    assert_eq!(
        ids(&session, f("name", "contains", "tea")).await.unwrap(),
        ["2"]
    );
    assert_eq!(
        ids(&session, f("name", "notContains", "e")).await.unwrap(),
        ["1", "3"]
    );
    // Other types as the text the grid shows.
    assert_eq!(
        ids(&session, f("price", "contains", "2.")).await.unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("price", "notContains", "2")).await.unwrap(),
        ["2", "3"]
    );

    // Dates are text in SQLite and compare as text.
    assert_eq!(
        ids(&session, f("made", "greater", "2026-01-31 23:00"))
            .await
            .unwrap(),
        ["2"]
    );
    assert_eq!(
        ids(&session, f("made", "startsWith", "2026-01"))
            .await
            .unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("active", "equals", "1")).await.unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("data", "equals", "{\"a\": 1.10}"))
            .await
            .unwrap(),
        ["1"]
    );
    // A column without a type: equality on what is shown, and nothing else.
    assert_eq!(
        ids(&session, f("loose", "equals", "42")).await.unwrap(),
        ["1"]
    );
    assert_eq!(
        ids(&session, f("loose", "equals", "text")).await.unwrap(),
        ["2"]
    );
    assert!(ids(&session, f("loose", "greater", "1")).await.is_err());

    // Binary: NULL tests only.
    assert_eq!(
        ids(&session, json!([{ "column": "photo", "op": "isNull" }]))
            .await
            .unwrap(),
        ["2"]
    );
    assert_eq!(
        ids(&session, json!([{ "column": "photo", "op": "isNotNull" }]))
            .await
            .unwrap(),
        ["1", "3"]
    );
    assert!(ids(&session, f("photo", "equals", "0x00")).await.is_err());

    // Conditions are joined with AND.
    let both = json!([
        { "column": "qty", "op": "greater", "value": "0" },
        { "column": "price", "op": "isNull" },
    ]);
    assert_eq!(ids(&session, both).await.unwrap(), ["3"]);
    session.close().await;
}

#[tokio::test]
async fn staged_edits_are_saved_in_one_transaction() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let items = table("items");
    let changes = |value: Value| serde_json::from_value(value).unwrap();
    let key = |id: &str| json!([{ "column": "id", "value": id }]);

    // An update, an insert that takes its defaults, a delete: one save.
    let result = session
        .apply_changes(
            &items,
            &changes(json!({
                "updates": [{ "key": key("1"), "changes": [
                    { "column": "name", "original": "กาแฟ", "value": "กาแฟดำ" },
                    { "column": "price", "original": "12.5", "value": "13.25" },
                    { "column": "big", "original": "9223372036854775807", "value": "9007199254740993" },
                    { "column": "ratio", "original": "0.1", "value": null },
                ]}],
                "inserts": [{ "values": [
                    { "column": "name", "value": "New" },
                    { "column": "price", "value": "012.50" },
                ]}],
                "deletes": [{ "key": key("3") }],
            })),
        )
        .await
        .unwrap();
    assert_eq!((result.updated, result.inserted, result.deleted), (1, 1, 1));
    assert_eq!(
        scalar(&session, "SELECT name || '|' || price || '|' || big || '|' || coalesce(ratio, 'NULL') FROM items WHERE id = 1").await,
        some("กาแฟดำ|13.25|9007199254740993|NULL")
    );
    // The new row: the row id was assigned (deletes run first, so 3 was free again), text
    // became a number, defaults applied.
    assert_eq!(
        scalar(&session, "SELECT id || '|' || typeof(price) || '|' || price || '|' || qty || '|' || active FROM items WHERE name = 'New'").await,
        some("3|real|12.5|0|1")
    );
    assert_eq!(
        scalar(
            &session,
            "SELECT count(*) FROM items WHERE name = '50% off_'"
        )
        .await,
        some("0")
    );

    // The row changed since it was read: nothing of the save is kept.
    let error = session
        .apply_changes(
            &items,
            &changes(json!({
                "updates": [
                    { "key": key("2"), "changes": [{ "column": "qty", "original": "0", "value": "5" }] },
                    { "key": key("1"), "changes": [{ "column": "name", "original": "กาแฟ", "value": "x" }] },
                ],
            })),
        )
        .await
        .err();
    assert!(matches!(error, Some(Error::Conflict(_))), "{error:?}");
    assert_eq!(
        scalar(&session, "SELECT qty FROM items WHERE id = 2").await,
        some("0")
    );

    // NULL as the value seen, and an empty string, are different things.
    let result = session
        .apply_changes(
            &items,
            &changes(json!({ "updates": [{ "key": key("2"), "changes": [
                { "column": "data", "original": null, "value": "" },
            ]}]})),
        )
        .await
        .unwrap();
    assert_eq!(result.updated, 1);
    assert_eq!(
        scalar(&session, "SELECT typeof(data) FROM items WHERE id = 2").await,
        some("text")
    );

    // Text that is not a number is refused for a column that holds numbers: SQLite itself
    // would have stored it.
    for change in [
        json!({ "updates": [{ "key": key("2"), "changes": [{ "column": "qty", "original": "0", "value": "many" }] }] }),
        json!({ "inserts": [{ "values": [{ "column": "name", "value": "n" }, { "column": "price", "value": "1,5" }] }] }),
    ] {
        let error = session.apply_changes(&items, &changes(change)).await.err();
        match error {
            Some(Error::Invalid(m)) => assert!(m.contains("not a number"), "{m}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert_eq!(
        scalar(&session, "SELECT count(*) FROM items").await,
        some("3")
    );

    // Columns that are not edited here: the row id, a blob, a column without a type.
    for column in ["id", "photo", "loose"] {
        let change = json!({ "updates": [{ "key": key("2"), "changes": [{ "column": column, "original": null, "value": "1" }] }] });
        let error = session.apply_changes(&items, &changes(change)).await.err();
        assert!(
            matches!(error, Some(Error::Invalid(_))),
            "{column}: {error:?}"
        );
    }

    // A composite key, in a WITHOUT ROWID table.
    let pair = json!([{ "column": "a", "value": "y" }, { "column": "b", "value": "1" }]);
    session
        .apply_changes(
            &table("pairs"),
            &changes(json!({ "updates": [{ "key": pair, "changes": [{ "column": "note", "original": "two", "value": "สอง" }] }] })),
        )
        .await
        .unwrap();
    assert_eq!(
        scalar(&session, "SELECT note FROM pairs WHERE a = 'y'").await,
        some("สอง")
    );

    // No key: rows can be added, not changed.
    let nokey = table("nokey");
    session
        .apply_changes(
            &nokey,
            &changes(json!({ "inserts": [{ "values": [{ "column": "label", "value": "b" }] }] })),
        )
        .await
        .unwrap();
    let error = session
        .apply_changes(&nokey, &changes(json!({ "deletes": [{ "key": [] }] })))
        .await
        .err();
    assert!(matches!(error, Some(Error::Unsupported(_))), "{error:?}");

    // A row of nothing but defaults.
    session
        .apply_changes(
            &table("counted"),
            &changes(json!({ "inserts": [{ "values": [] }] })),
        )
        .await
        .unwrap();
    assert_eq!(
        scalar(&session, "SELECT max(id) FROM counted").await,
        some("2")
    );

    // A STRICT table is held by SQLite itself; foreign keys are enforced on this connection.
    let error = session
        .apply_changes(
            &table("strict_t"),
            &changes(json!({ "inserts": [{ "values": [{ "column": "n", "value": "1.5" }] }] })),
        )
        .await
        .err();
    assert!(matches!(error, Some(Error::Database(_))), "{error:?}");
    let error = session
        .apply_changes(
            &table("child"),
            &changes(
                json!({ "inserts": [{ "values": [{ "column": "parent_id", "value": "99" }] }] }),
            ),
        )
        .await
        .err();
    match error {
        Some(Error::Database(m)) => assert!(m.contains("FOREIGN KEY"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    session.close().await;
}

#[tokio::test]
async fn a_read_only_connection_cannot_write_the_file() {
    let scratch = Scratch::new(FIXTURE).await;
    let before = std::fs::read(&scratch.0).unwrap();
    let session = Session::connect(&profile(scratch.path(), true), None)
        .await
        .unwrap();
    assert!(session.server_info().await.unwrap().read_only);
    assert_eq!(ids(&session, json!([])).await.unwrap(), ["1", "2", "3"]);

    let change = serde_json::from_value(json!({
        "updates": [{ "key": [{ "column": "id", "value": "1" }], "changes": [
            { "column": "qty", "original": "3", "value": "4" },
        ]}],
    }))
    .unwrap();
    let error = session.apply_changes(&table("items"), &change).await.err();
    match error {
        Some(Error::Database(m)) => assert!(m.contains("readonly"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }

    let mut console = session.open_console(None).await.unwrap();
    for sql in [
        "DELETE FROM items WHERE id = 1",
        "CREATE TABLE made_here (x)",
        "PRAGMA user_version = 7",
    ] {
        let outcome = run(&mut console, sql).await;
        let statement = &outcome.statements[0];
        assert_eq!(statement.status, Status::Error, "{sql}");
        assert!(
            statement.message.as_deref().unwrap().contains("readonly"),
            "{sql}: {:?}",
            statement.message
        );
    }
    drop(console);
    session.close().await;
    assert_eq!(
        std::fs::read(&scratch.0).unwrap(),
        before,
        "the file changed"
    );
}

#[tokio::test]
async fn smart_paste_matches_then_writes() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let items = table("items");
    let request = |mode: &str, key: Value, rows: Value| {
        serde_json::from_value(json!({
            "mode": mode, "columns": ["code", "name", "price"], "key": key,
            "update": ["name", "price"], "rows": rows,
        }))
        .unwrap()
    };

    // Matched on a unique index: one row changes, one is new, one is already as pasted
    // (12.50 is the stored 12.5), one cannot be a price.
    let paste = request(
        "upsert",
        json!(["code"]),
        json!([
            ["B-2", "Green tea", "7"],
            ["Z-9", "ชา", "30"],
            ["A-1", "กาแฟ", "12.50"],
            ["Q-1", "Bad", "cheap"],
        ]),
    );
    let analysis = session.analyze_paste(&items, &paste).await.unwrap();
    let statuses: Vec<RowStatus> = analysis.rows.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        [
            RowStatus::Update,
            RowStatus::Insert,
            RowStatus::Unchanged,
            RowStatus::Invalid
        ]
    );
    assert_eq!(analysis.rows[0].changed, ["name"]);
    // A row with a problem stops the run.
    assert!(session
        .apply_paste(&items, &paste, &analysis.fingerprint, &|_| true)
        .await
        .is_err());

    let paste = request(
        "upsert",
        json!(["code"]),
        json!([
            ["B-2", "Green tea", "7"],
            ["Z-9", "ชา", "30"],
            ["A-1", "กาแฟ", "12.50"]
        ]),
    );
    let analysis = session.analyze_paste(&items, &paste).await.unwrap();
    let result = session
        .apply_paste(&items, &paste, &analysis.fingerprint, &|_| true)
        .await
        .unwrap();
    assert_eq!((result.updated, result.inserted), (1, 1));
    assert_eq!(
        scalar(&session, "SELECT group_concat(id || ':' || name || ':' || price, ', ') FROM (SELECT * FROM items WHERE code IS NOT NULL ORDER BY id)").await,
        some("1:กาแฟ:12.5, 2:Green tea:7, 4:ชา:30")
    );
    // Pasting the same again changes nothing, and an old preview is not accepted.
    let again = session.analyze_paste(&items, &paste).await.unwrap();
    assert_eq!(
        (
            again.counts.update,
            again.counts.insert,
            again.counts.unchanged
        ),
        (0, 0, 3)
    );
    let error = session
        .apply_paste(&items, &paste, &analysis.fingerprint, &|_| true)
        .await
        .err();
    assert!(matches!(error, Some(Error::Conflict(_))), "{error:?}");

    // Matched on the row id: integers to the last digit, and a blank id is a new row.
    let by_id = serde_json::from_value(json!({
        "mode": "upsert", "columns": ["id", "name"], "key": ["id"], "update": ["name"],
        "rows": [["1", "Espresso"], [null, "Blank id"], ["1.5", "Not an id"]],
    }))
    .unwrap();
    let analysis = session.analyze_paste(&items, &by_id).await.unwrap();
    let statuses: Vec<RowStatus> = analysis.rows.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        [RowStatus::Update, RowStatus::Insert, RowStatus::Invalid]
    );

    // Cancelling in the middle leaves nothing behind.
    let many: Vec<Value> = (0..50)
        .map(|i| json!([format!("N-{i}"), format!("n{i}"), "1"]))
        .collect();
    let paste = request("insert", json!([]), json!(many));
    let analysis = session.analyze_paste(&items, &paste).await.unwrap();
    assert_eq!(analysis.counts.insert, 50);
    let error = session
        .apply_paste(&items, &paste, &analysis.fingerprint, &|done| done < 20)
        .await
        .err();
    assert!(matches!(error, Some(Error::Invalid(_))), "{error:?}");
    assert_eq!(
        scalar(&session, "SELECT count(*) FROM items WHERE code LIKE 'N-%'").await,
        some("0")
    );
    session.close().await;
}

#[tokio::test]
async fn the_console_runs_scripts_with_its_guards() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let mut console = session.open_console(None).await.unwrap();
    let idle = AtomicBool::new(false);

    // Each value is shown as what it is: SQLite types values, not columns.
    let outcome = run(
        &mut console,
        "SELECT id, big, ratio, photo, loose, data, NULL AS empty FROM items ORDER BY id; -- a comment; with a semicolon\n\
         SELECT 'a;b' AS \"semi;colon\", [id], `name` FROM items WHERE id = 1",
    )
    .await;
    assert_eq!(
        outcome.statements.len(),
        2,
        "{:?}",
        outcome.statements[0].message
    );
    assert!(outcome
        .statements
        .iter()
        .all(|s| s.status == Status::Ok && s.kind == Kind::Read));
    let first = &outcome.statements[0].results[0];
    assert_eq!(
        first.rows[0],
        [
            some("1"),
            some("9223372036854775807"),
            some("0.1"),
            some("0x00ff10"),
            some("42"),
            some("{\"a\": 1.10}"),
            None
        ]
    );
    assert_eq!(first.rows[1][2], some("1.0e+20"));
    assert_eq!(first.rows[2][3], some("0x"));
    assert_eq!(rows(&outcome), [[some("a;b"), some("1"), some("กาแฟ")]]);
    assert_eq!(
        outcome.statements[1].results[0].columns[0].name,
        "semi;colon"
    );

    // A query without rows still has its column names.
    let outcome = run(&mut console, "SELECT id, name FROM items WHERE id < 0").await;
    let names: Vec<&str> = outcome.statements[0].results[0]
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["id", "name"]);

    // PRAGMA may change something, so it is not a "read"; its rows are shown all the same.
    let outcome = run(&mut console, "PRAGMA table_info(pairs)").await;
    assert_eq!(outcome.statements[0].kind, Kind::Other);
    assert_eq!(rows(&outcome).len(), 3);

    // A trigger's body has semicolons of its own: it is one statement.
    let outcome = run(
        &mut console,
        "CREATE TABLE log (what TEXT);\n\
         CREATE TRIGGER items_gone AFTER DELETE ON items BEGIN\n\
           INSERT INTO log VALUES ('deleted ' || old.id);\n\
           UPDATE nokey SET n = n + 1;\n\
         END;\n\
         DELETE FROM items WHERE id = 3;\n\
         SELECT what FROM log",
    )
    .await;
    let kinds: Vec<Kind> = outcome.statements.iter().map(|s| s.kind).collect();
    assert_eq!(kinds, [Kind::Ddl, Kind::Ddl, Kind::Dml, Kind::Read]);
    assert_eq!(rows(&outcome), [[some("deleted 3")]]);

    // The impact guard: more rows than allowed is rolled back, and the rest waits.
    let outcome = console
        .run(
            "UPDATE items SET qty = qty + 1; SELECT 1",
            &options(100, Some(1)),
            &idle,
        )
        .await
        .unwrap();
    assert_eq!(outcome.statements[0].status, Status::Blocked);
    assert_eq!(outcome.statements[0].rows_affected, Some(2));
    assert_eq!(outcome.skipped, 1);
    assert!(outcome.resume.as_deref().unwrap().starts_with("UPDATE"));
    assert!(!outcome.in_transaction);
    assert_eq!(
        scalar(&session, "SELECT sum(qty) FROM items").await,
        some("3")
    );
    // Within the limit it is committed.
    let outcome = console
        .run(
            "UPDATE items SET qty = 9 WHERE id = 2",
            &options(100, Some(1)),
            &idle,
        )
        .await
        .unwrap();
    assert_eq!(
        (
            outcome.statements[0].status,
            outcome.statements[0].rows_affected
        ),
        (Status::Ok, Some(1))
    );
    assert_eq!(
        scalar(&session, "SELECT qty FROM items WHERE id = 2").await,
        some("9")
    );

    // A transaction the user opens is theirs: seen, not guarded, and DDL is part of it.
    let outcome = run(
        &mut console,
        "BEGIN; UPDATE items SET qty = 100; CREATE TABLE gone (x)",
    )
    .await;
    assert!(outcome.in_transaction);
    assert!(outcome.statements.iter().all(|s| s.status == Status::Ok));
    // A result cut at the row limit does not disturb that transaction.
    let started = Instant::now();
    let outcome = console
        .run(
            &format!("{ENDLESS} SELECT x FROM c"),
            &options(50, None),
            &idle,
        )
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    let result = &outcome.statements[0].results[0];
    assert_eq!(
        (
            outcome.statements[0].status,
            result.rows.len(),
            result.capped
        ),
        (Status::Ok, 50, true)
    );
    assert!(outcome.in_transaction);
    let outcome = run(&mut console, "SELECT sum(qty) FROM items; ROLLBACK").await;
    assert_eq!(outcome.statements[0].results[0].rows, [[some("200")]]);
    assert!(!outcome.in_transaction);
    assert_eq!(
        scalar(&session, "SELECT sum(qty) FROM items").await,
        some("12")
    );
    assert_eq!(
        scalar(
            &session,
            "SELECT count(*) FROM sqlite_master WHERE name = 'gone'"
        )
        .await,
        some("0")
    );

    // Extensions are never loaded: SQLite refuses the function, as it does by default.
    let outcome = run(&mut console, "SELECT load_extension('nothing')").await;
    assert_eq!(outcome.statements[0].status, Status::Error);
    assert!(
        outcome.statements[0]
            .message
            .as_deref()
            .unwrap()
            .contains("not authorized"),
        "{:?}",
        outcome.statements[0].message
    );

    // An error names the statement and stops the script.
    let outcome = run(&mut console, "SELECT nope FROM items; SELECT 1").await;
    assert_eq!(outcome.statements[0].status, Status::Error);
    assert!(outcome.statements[0]
        .message
        .as_deref()
        .unwrap()
        .contains("nope"));
    assert_eq!(outcome.skipped, 1);
    assert!(!outcome.connection_lost);
    session.close().await;
}

/// Ask a running statement to stop, the way the app does: the tab's stop flag and its canceller.
fn stop_after(console: &Console, stop: &Arc<AtomicBool>, wait: Duration) {
    let (canceller, stop) = (console.canceller(), stop.clone());
    tokio::spawn(async move {
        tokio::time::sleep(wait).await;
        stop.store(true, Ordering::Relaxed);
        canceller.cancel().await.unwrap();
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_statement_can_be_stopped() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let mut console = session.open_console(None).await.unwrap();
    let soon = Duration::from_millis(200);

    // A query that would never end.
    let stop = Arc::new(AtomicBool::new(false));
    stop_after(&console, &stop, soon);
    let started = Instant::now();
    let outcome = console
        .run(
            &format!("{ENDLESS} SELECT count(*) FROM c; SELECT 1"),
            &options(100, None),
            &stop,
        )
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(
        outcome.statements[0].status,
        Status::Cancelled,
        "{:?}",
        outcome.statements[0].message
    );
    assert_eq!(outcome.skipped, 1);
    // The stop was for that statement only.
    assert_eq!(rows(&run(&mut console, "SELECT 2").await), [[some("2")]]);

    // A write under the guard: stopped, undone, and reported as cancelled.
    let stop = Arc::new(AtomicBool::new(false));
    stop_after(&console, &stop, soon);
    let endless_write =
        format!("UPDATE items SET qty = 77 WHERE id IN ({ENDLESS} SELECT x FROM c)");
    let outcome = console
        .run(&endless_write, &options(100, Some(1000)), &stop)
        .await
        .unwrap();
    assert_eq!(
        outcome.statements[0].status,
        Status::Cancelled,
        "{:?}",
        outcome.statements[0].message
    );
    assert!(!outcome.in_transaction);
    assert_eq!(
        scalar(&session, "SELECT count(*) FROM items WHERE qty = 77").await,
        some("0")
    );

    // The same inside the user's own transaction: SQLite ends that transaction, and the
    // console says so instead of showing a transaction that is no longer there.
    let outcome = run(
        &mut console,
        "BEGIN; INSERT INTO nokey VALUES ('pending', 5)",
    )
    .await;
    assert!(outcome.in_transaction);
    let stop = Arc::new(AtomicBool::new(false));
    stop_after(&console, &stop, soon);
    let outcome = console
        .run(&endless_write, &options(100, Some(1000)), &stop)
        .await
        .unwrap();
    let statement = &outcome.statements[0];
    assert_eq!(statement.status, Status::Cancelled);
    assert!(!outcome.in_transaction);
    assert!(
        statement
            .message
            .as_deref()
            .unwrap()
            .contains("rolled the open transaction back"),
        "{:?}",
        statement.message
    );
    // The insert made earlier in that transaction went with it.
    assert_eq!(
        rows(&run(&mut console, "SELECT count(*) FROM nokey").await),
        [[some("1")]]
    );
    session.close().await;
}

#[tokio::test]
async fn the_ai_reader_cannot_write() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let idle = AtomicBool::new(false);

    let mut reader = session.open_reader(None).await.unwrap();
    let (set, capped) = reader
        .read_only("SELECT id, name FROM items ORDER BY id", 2, &idle)
        .await
        .unwrap();
    assert_eq!((set.rows.len(), capped), (2, true));
    assert_eq!(set.rows[0], [some("1"), some("กาแฟ")]);
    let (set, _) = reader
        .read_only(
            "WITH t AS (SELECT count(*) AS n FROM items) SELECT n FROM t;",
            10,
            &idle,
        )
        .await
        .unwrap();
    assert_eq!(set.rows, [[some("3")]]);
    let (set, _) = reader
        .read_only(
            "EXPLAIN QUERY PLAN SELECT * FROM items WHERE id = 1",
            10,
            &idle,
        )
        .await
        .unwrap();
    assert!(!set.rows.is_empty());

    for sql in [
        "DELETE FROM items",
        "SELECT 1; DELETE FROM items",
        "SELECT 1 /* /* */ ; DELETE FROM items; -- */",
        "SELECT ';' ; DELETE FROM items",
        "SELECT ';'",
        "PRAGMA writable_schema = 1",
        "ATTACH DATABASE '/tmp/other.db' AS other",
        "WITH gone AS (SELECT 1) DELETE FROM items",
        "INSERT INTO nokey VALUES ('x', 1) RETURNING *",
        "SELECT load_extension('x')",
        "VACUUM",
    ] {
        let error = reader.read_only(sql, 10, &idle).await.err();
        assert!(matches!(error, Some(Error::Invalid(_))), "{sql}: {error:?}");
    }

    // Past the reading of the text, the connection itself cannot write: the file is open
    // read-only, whatever is sent on it.
    for sql in [
        "DELETE FROM items",
        "CREATE TABLE made_here (x)",
        "ATTACH DATABASE '' AS scratch; CREATE TABLE scratch.t (x)",
    ] {
        let outcome = run(&mut reader, sql).await;
        let last = outcome.statements.last().unwrap();
        assert_eq!(last.status, Status::Error, "{sql}");
    }
    assert_eq!(
        scalar(&session, "SELECT count(*) FROM items").await,
        some("3")
    );

    // An ordinary console of a writable session still refuses writes while `read_only`
    // runs, and writes again afterwards.
    let mut console = session.open_console(None).await.unwrap();
    console.read_only("SELECT 1", 10, &idle).await.unwrap();
    let outcome = run(&mut console, "UPDATE items SET qty = 1 WHERE id = 1").await;
    assert_eq!(outcome.statements[0].status, Status::Ok);
    session.close().await;
}

#[tokio::test]
async fn a_lock_held_by_another_program_is_waited_for_then_reported() {
    let scratch = Scratch::new(FIXTURE).await;
    let session = open(&scratch).await;
    let mut other = SqliteConnectOptions::new()
        .filename(&scratch.0)
        .connect()
        .await
        .unwrap();
    other.execute("BEGIN EXCLUSIVE").await.unwrap();

    let change = || {
        serde_json::from_value(json!({
            "updates": [{ "key": [{ "column": "id", "value": "1" }], "changes": [
                { "column": "qty", "original": "3", "value": "4" },
            ]}],
        }))
        .unwrap()
    };
    // The connection timeout of these tests is one second.
    let started = Instant::now();
    let error = session
        .apply_changes(&table("items"), &change())
        .await
        .err();
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
    match error {
        Some(Error::Database(m)) => assert!(m.contains("locked"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }

    other.execute("ROLLBACK").await.unwrap();
    other.close().await.unwrap();
    session
        .apply_changes(&table("items"), &change())
        .await
        .unwrap();
    assert_eq!(
        scalar(&session, "SELECT qty FROM items WHERE id = 1").await,
        some("4")
    );
    session.close().await;
}

#[tokio::test]
async fn a_file_in_wal_mode_is_shared_with_the_program_that_owns_it() {
    let scratch = Scratch::new(FIXTURE).await;
    // Another program has the file open in WAL mode, as most applications do.
    let mut other = SqliteConnectOptions::new()
        .filename(&scratch.0)
        .connect()
        .await
        .unwrap();
    other.execute("PRAGMA journal_mode = WAL").await.unwrap();
    other
        .execute("INSERT INTO nokey VALUES ('theirs', 2)")
        .await
        .unwrap();
    let session = open(&scratch).await;
    let count = "SELECT count(*) FROM nokey";
    assert_eq!(scalar(&session, count).await, some("2"));

    // While they are in the middle of a write, reading goes on (and sees what is committed);
    // a save waits for them and then says why it could not go through.
    other
        .execute("BEGIN IMMEDIATE; INSERT INTO nokey VALUES ('pending', 3)")
        .await
        .unwrap();
    let started = Instant::now();
    assert_eq!(scalar(&session, count).await, some("2"));
    assert!(started.elapsed() < Duration::from_millis(900));
    let insert = || {
        serde_json::from_value(json!({
            "inserts": [{ "values": [{ "column": "label", "value": "ours" }] }],
        }))
        .unwrap()
    };
    let error = session
        .apply_changes(&table("nokey"), &insert())
        .await
        .err();
    match error {
        Some(Error::Database(m)) => assert!(m.contains("locked"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    other.execute("COMMIT").await.unwrap();

    // Our save is seen by them, and the file is still in the mode they chose.
    session
        .apply_changes(&table("nokey"), &insert())
        .await
        .unwrap();
    let theirs: i64 = sqlx::query_scalar(count)
        .fetch_one(&mut other)
        .await
        .unwrap();
    assert_eq!(theirs, 4);
    assert_eq!(scalar(&session, "PRAGMA journal_mode").await, some("wal"));

    // A read-only connection reads such a file too, and still cannot write it.
    let reader = Session::connect(&profile(scratch.path(), true), None)
        .await
        .unwrap();
    assert_eq!(scalar(&reader, count).await, some("4"));
    assert!(reader
        .apply_changes(&table("nokey"), &insert())
        .await
        .is_err());
    reader.close().await;
    session.close().await;
    other.close().await.unwrap();
}
