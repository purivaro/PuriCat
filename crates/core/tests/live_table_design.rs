//! New tables from the table form, against real servers: what the statement makes is what
//! the server then describes. Every database created here is named `puricat_it_<random>` and
//! dropped again.

use puricat_core::db::table_design::{AlterTable, NewTable};
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

fn change(
    database: &str,
    schema: Option<&str>,
    name: &str,
    edits: serde_json::Value,
) -> AlterTable {
    serde_json::from_value(json!({
        "table": { "database": database, "schema": schema, "name": name },
        "edits": edits,
    }))
    .unwrap()
}

#[tokio::test]
async fn mysql_table_changes() {
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
        let mut conn = sqlx::MySqlConnection::connect_with(&opts.database(&d)).await.unwrap();
        conn.execute("CREATE TABLE t (id INT PRIMARY KEY, a INT, old TEXT, KEY ix_old (a)); INSERT INTO t VALUES (1, 10, 'x')").await.unwrap();
        let table = TableRef { database: d.clone(), schema: None, name: "t".into() };
        let before = session.indexes(&table).await.unwrap();
        assert_eq!(before.iter().map(|i| (i.name.as_str(), i.primary, i.unique)).collect::<Vec<_>>(), [("PRIMARY", true, true), ("ix_old", false, false)]);

        let ran = session
            .alter_table(&change(&d, None, "t", json!([
                { "kind": "addColumn", "column": { "name": "หมายเหตุ", "type": "VARCHAR", "length": 50, "default": { "kind": "value", "value": "ไม่มี" } } },
                { "kind": "dropColumn", "name": "old" },
                { "kind": "renameColumn", "from": "a", "to": "amount" },
                { "kind": "dropIndex", "name": "ix_old" },
                { "kind": "addIndex", "name": "ux_amount", "columns": ["amount", "หมายเหตุ"], "unique": true }
            ])))
            .await
            .unwrap();
        assert_eq!(ran.len(), 1);
        let meta = session.describe_table(&table).await.unwrap();
        assert_eq!(meta.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["id", "amount", "หมายเหตุ"]);
        let row = sqlx::query("SELECT amount, `หมายเหตุ` FROM t").fetch_one(&mut conn).await.unwrap();
        assert_eq!((row.get::<i32, _>(0), row.get::<String, _>(1)), (10, "ไม่มี".to_string()));
        let after = session.indexes(&table).await.unwrap();
        assert_eq!(after.iter().map(|i| (i.name.clone(), i.unique, i.columns.join(","))).collect::<Vec<_>>(), [("PRIMARY".to_string(), true, "id".to_string()), ("ux_amount".to_string(), true, "amount,หมายเหตุ".to_string())]);

        // One statement: when a clause fails, none of the others is applied.
        let failed = session
            .alter_table(&change(&d, None, "t", json!([
                { "kind": "addColumn", "column": { "name": "added", "type": "INT", "nullable": true } },
                { "kind": "dropColumn", "name": "no_such_column" }
            ])))
            .await
            .unwrap_err();
        assert!(failed.to_string().contains("no_such_column"), "{failed}");
        let meta = session.describe_table(&table).await.unwrap();
        assert!(!meta.columns.iter().any(|c| c.name == "added"));
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
async fn postgres_table_changes() {
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
        let mut conn = sqlx::PgConnection::connect_with(&o).await.unwrap();
        conn.execute("CREATE TABLE t (id int PRIMARY KEY, a int, old text); CREATE INDEX ix_old ON t (a); INSERT INTO t VALUES (1, 10, 'x')").await.unwrap();
        let table = TableRef { database: d.clone(), schema: Some("public".into()), name: "t".into() };
        let before = session.indexes(&table).await.unwrap();
        assert_eq!(before.iter().map(|i| (i.name.as_str(), i.primary)).collect::<Vec<_>>(), [("t_pkey", true), ("ix_old", false)]);

        let ran = session
            .alter_table(&change(&d, Some("public"), "t", json!([
                { "kind": "addColumn", "column": { "name": "note", "type": "text", "default": { "kind": "value", "value": "none" }, "comment": "หมายเหตุ" } },
                { "kind": "dropColumn", "name": "old" },
                { "kind": "renameColumn", "from": "a", "to": "amount" },
                { "kind": "dropIndex", "name": "ix_old" },
                { "kind": "addIndex", "name": "ux_amount", "columns": ["amount"], "unique": true }
            ])))
            .await
            .unwrap();
        assert_eq!(ran.len(), 6, "{ran:?}");
        let row = sqlx::query("SELECT amount, note, col_description('public.t'::regclass, 4) FROM t").fetch_one(&mut conn).await.unwrap();
        assert_eq!((row.get::<i32, _>(0), row.get::<String, _>(1), row.get::<String, _>(2)), (10, "none".into(), "หมายเหตุ".into()));
        let after = session.indexes(&table).await.unwrap();
        assert_eq!(after.iter().map(|i| (i.name.clone(), i.unique, i.columns.join(","))).collect::<Vec<_>>(), [("t_pkey".to_string(), true, "id".to_string()), ("ux_amount".to_string(), true, "amount".to_string())]);

        // One transaction: when a later statement fails, the earlier ones are undone.
        let failed = session
            .alter_table(&change(&d, Some("public"), "t", json!([
                { "kind": "addColumn", "column": { "name": "added", "type": "integer", "nullable": true } },
                { "kind": "dropColumn", "name": "no_such_column" }
            ])))
            .await
            .unwrap_err();
        assert!(failed.to_string().contains("no_such_column"), "{failed}");
        let meta = session.describe_table(&table).await.unwrap();
        assert!(!meta.columns.iter().any(|c| c.name == "added"), "the first change outlived the failed one");
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}

/// Changing a column keeps what is not changed, and foreign keys are made and dropped.
#[tokio::test]
async fn mysql_column_changes_and_foreign_keys() {
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
        let mut conn = sqlx::MySqlConnection::connect_with(&opts.database(&d)).await.unwrap();
        conn.execute(
            "CREATE TABLE customers (id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL);
             CREATE TABLE orders (
               id INT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
               customer_id INT UNSIGNED NOT NULL,
               title VARCHAR(120) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL DEFAULT 'ไม่มีชื่อ' COMMENT 'ชื่อ',
               at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3));
             INSERT INTO customers (name) VALUES ('ก'), ('ข');
             INSERT INTO orders (customer_id) VALUES (1), (2);",
        )
        .await
        .unwrap();
        let table = TableRef { database: d.clone(), schema: None, name: "orders".into() };
        let before = session.column_definitions(&table).await.unwrap();

        // Only NULL changes on two columns, and only the type on the id: the rest stays.
        session
            .alter_table(&change(&d, None, "orders", json!([
                { "kind": "changeColumn", "name": "title", "nullable": true },
                { "kind": "changeColumn", "name": "at", "nullable": true },
                { "kind": "changeColumn", "name": "id", "type": { "type": "BIGINT", "unsigned": true } },
                { "kind": "addForeignKey", "name": "fk_customer", "columns": ["customer_id"], "references": { "database": d, "schema": null, "name": "customers" }, "refColumns": ["id"], "onDelete": "cascade" }
            ])))
            .await
            .unwrap();
        let after = session.column_definitions(&table).await.unwrap();
        let col = |list: &[puricat_core::db::table_design::ColumnDefinition], n: &str| list.iter().find(|c| c.name == n).unwrap().clone();
        let (t0, t1) = (col(&before, "title"), col(&after, "title"));
        assert!(t1.nullable && !t0.nullable);
        assert_eq!((t1.column_type.as_str(), t1.default.as_deref(), t1.comment.as_deref(), t1.collation.as_deref()), ("varchar(120)", Some("ไม่มีชื่อ"), Some("ชื่อ"), Some("utf8mb4_bin")));
        let a1 = col(&after, "at");
        assert!(a1.nullable);
        assert_eq!(a1.default.as_deref(), Some("CURRENT_TIMESTAMP(3)"));
        assert!(a1.extra.contains("on update CURRENT_TIMESTAMP(3)"), "{}", a1.extra);
        let i1 = col(&after, "id");
        assert_eq!((i1.column_type.as_str(), i1.extra.as_str()), ("bigint unsigned", "auto_increment"));

        let fks = session.foreign_keys(&table).await.unwrap();
        assert_eq!(fks.len(), 1);
        assert_eq!((fks[0].name.as_str(), fks[0].columns.join(","), fks[0].references.name.as_str(), fks[0].on_delete), ("fk_customer", "customer_id".to_string(), "customers", Some(puricat_core::db::table_design::FkAction::Cascade)));
        // The key works: a customer's orders go with the customer.
        conn.execute("DELETE FROM customers WHERE id = 1").await.unwrap();
        let left: i64 = sqlx::query("SELECT COUNT(*) FROM orders").fetch_one(&mut conn).await.unwrap().get(0);
        assert_eq!(left, 1);

        session.alter_table(&change(&d, None, "orders", json!([{ "kind": "dropForeignKey", "name": "fk_customer" }]))).await.unwrap();
        assert!(session.foreign_keys(&table).await.unwrap().is_empty());
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
async fn postgres_column_changes_and_foreign_keys() {
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
        let mut conn = sqlx::PgConnection::connect_with(&o).await.unwrap();
        conn.execute(
            "CREATE TABLE customers (id int PRIMARY KEY);
             CREATE TABLE orders (id int PRIMARY KEY, customer_id int, qty text NOT NULL DEFAULT '1');
             COMMENT ON COLUMN orders.qty IS 'จำนวน';
             INSERT INTO customers VALUES (1), (2); INSERT INTO orders VALUES (1, 1, '5'), (2, 2, '7');",
        )
        .await
        .unwrap();
        let table = TableRef { database: d.clone(), schema: Some("public".into()), name: "orders".into() };
        session
            .alter_table(&change(&d, Some("public"), "orders", json!([
                { "kind": "changeColumn", "name": "qty", "type": { "type": "integer" }, "default": { "kind": "value", "value": "0" }, "comment": "" },
                { "kind": "addForeignKey", "name": "fk_c", "columns": ["customer_id"], "references": { "database": d, "schema": "public", "name": "customers" }, "refColumns": ["id"], "onDelete": "setNull" }
            ])))
            .await
            .unwrap();
        let q = session.column_definitions(&table).await.unwrap().into_iter().find(|c| c.name == "qty").unwrap();
        assert_eq!((q.column_type.as_str(), q.default.as_deref(), q.comment.as_deref(), q.nullable), ("integer", Some("0"), None, false));
        let sum: i64 = sqlx::query("SELECT SUM(qty)::bigint FROM orders").fetch_one(&mut conn).await.unwrap().get(0);
        assert_eq!(sum, 12, "the text values were converted");
        let fks = session.foreign_keys(&table).await.unwrap();
        assert_eq!((fks.len(), fks[0].on_delete), (1, Some(puricat_core::db::table_design::FkAction::SetNull)));
        conn.execute("DELETE FROM customers WHERE id = 2").await.unwrap();
        let nulls: i64 = sqlx::query("SELECT COUNT(*) FROM orders WHERE customer_id IS NULL").fetch_one(&mut conn).await.unwrap().get(0);
        assert_eq!(nulls, 1);
        // A conversion that fails leaves the column as it was: one transaction.
        conn.execute("ALTER TABLE orders ADD COLUMN code text; UPDATE orders SET code = 'x'").await.unwrap();
        let failed = session
            .alter_table(&change(&d, Some("public"), "orders", json!([
                { "kind": "changeColumn", "name": "qty", "comment": "จะไม่เกิด" },
                { "kind": "changeColumn", "name": "code", "type": { "type": "integer" } }
            ])))
            .await
            .unwrap_err();
        assert!(failed.to_string().contains("invalid input syntax"), "{failed}");
        let q = session.column_definitions(&table).await.unwrap().into_iter().find(|c| c.name == "qty").unwrap();
        assert_eq!(q.comment, None, "the comment of the failed change outlived it");
        session.alter_table(&change(&d, Some("public"), "orders", json!([{ "kind": "dropForeignKey", "name": "fk_c" }]))).await.unwrap();
        assert!(session.foreign_keys(&table).await.unwrap().is_empty());
        session.close().await;
    })
    .await;
    admin
        .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.unwrap();
}
