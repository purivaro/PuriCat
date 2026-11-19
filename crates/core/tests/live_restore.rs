//! Restores against real servers: a backup made by `backup::run` is loaded by `restore::run`
//! into a database created for it, and compared with the source. Then what must not load:
//! a file that is not whole, a stop, and a load that fails half way.
//! Same opt-in environment variables and disposable-database rule as `live_backup.rs`: every
//! database created here is named `puricat_it_<random>…` and dropped again.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use puricat_core::backup::{self, BackupRequest, DumpTool, Format};
use puricat_core::model::{ConnectionProfile, Engine};
use puricat_core::restore;
use puricat_core::Error;
use serde_json::json;
use sqlx::{Connection, Executor, Row};

struct Target {
    profile: ConnectionProfile,
    password: Option<String>,
}

fn target(engine: Engine, var: &str) -> Option<Target> {
    let spec = std::env::var(var).ok()?;
    let (user, rest) = spec.split_once('@')?;
    let (host, port) = rest.rsplit_once(':')?;
    let profile = serde_json::from_value(json!({
        "name": "Integration ทดสอบ", "engine": engine, "host": host,
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

fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(scratch_name());
    std::fs::create_dir(&dir).unwrap();
    dir
}

fn request(database: &str, destination: &Path, format: Format, compress: bool) -> BackupRequest {
    serde_json::from_value(json!({
        "database": database, "destination": destination, "format": format, "compress": compress,
    }))
    .unwrap()
}

async fn chosen(t: &Target, server_version: &str) -> DumpTool {
    let tools = backup::find_tools(t.profile.engine).await;
    let choice = backup::choose(t.profile.engine, server_version, &tools);
    choice
        .tool
        .unwrap_or_else(|| panic!("no dump tool for this server: {:?}", choice.problem))
}

fn going() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[tokio::test]
async fn mysql_restore_loads_a_backup_into_a_new_database() {
    let Some(t) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
    let version: String = sqlx::query("SELECT version()")
        .fetch_one(&mut admin)
        .await
        .unwrap()
        .get(0);
    // The source, a copy for the whole backup, and three that must stay as they are.
    let names: Vec<String> = (0..5).map(|_| scratch_name()).collect();
    let dir = scratch_dir();
    let (n, d) = (names.clone(), dir.clone());

    let outcome = tokio::spawn(async move {
        let (s, copy, untouched, halted_db, broken_db) = (&n[0], &n[1], &n[2], &n[3], &n[4]);
        let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
        for db in &n {
            admin
                .execute(format!("CREATE DATABASE `{db}` CHARACTER SET utf8mb4").as_str())
                .await
                .unwrap();
        }
        admin.execute(format!("USE `{s}`").as_str()).await.unwrap();
        admin
            .execute(
                "CREATE TABLE orders (
                   id INT PRIMARY KEY AUTO_INCREMENT, note VARCHAR(200) NOT NULL, amount DECIMAL(12,2),
                   payload VARBINARY(64), detail JSON, changed INT NOT NULL DEFAULT 0
                 ) ENGINE=InnoDB",
            )
            .await
            .unwrap();
        admin
            .execute(
                "INSERT INTO orders (note, amount, payload, detail)
                 WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 500)
                 SELECT CONCAT('ลูกค้า ', i, ' — ‘quote’ \\\\ back; -- not a comment 🐱'), i * 1.25,
                        UNHEX(SHA2(i, 256)), JSON_OBJECT('n', i, 'ชื่อ', 'ทดสอบ') FROM n",
            )
            .await
            .unwrap();
        admin
            .execute("CREATE TABLE zz_last (id INT PRIMARY KEY, label VARCHAR(20))")
            .await
            .unwrap();
        admin.execute("INSERT INTO zz_last VALUES (1, 'ท้ายสุด')").await.unwrap();
        admin
            .execute("CREATE VIEW big_orders AS SELECT id, amount FROM orders WHERE amount > 100")
            .await
            .unwrap();
        admin
            .execute("CREATE TRIGGER orders_touch BEFORE UPDATE ON orders FOR EACH ROW SET NEW.changed = OLD.changed + 1")
            .await
            .unwrap();
        admin
            .execute("CREATE FUNCTION with_tax(x DECIMAL(12,2)) RETURNS DECIMAL(12,2) DETERMINISTIC RETURN x * 1.07")
            .await
            .unwrap();

        let tool = chosen(&t, &version).await;
        let made = backup::run(&t.profile, t.password.as_deref(), &tool, &request(s, &d, Format::Sql, true), &AtomicBool::new(false), |_| {})
            .await
            .expect("the backup runs");

        // ---- the whole backup, into the database made for it -----------------------------
        let mut reported = Vec::new();
        restore::run(&t.profile, t.password.as_deref(), &tool, &made.file, copy, &going(), |bytes| reported.push(bytes))
            .await
            .expect("the restore runs");
        // Progress is the part of the file that was read: it ends at the file's size.
        assert_eq!(reported.last(), Some(&made.bytes), "{reported:?}");
        assert!(reported.windows(2).all(|w| w[0] <= w[1]), "{reported:?}");

        let fingerprint = "SELECT COUNT(*), CAST(SUM(CRC32(CONCAT_WS('|', id, note, amount, HEX(payload), detail))) AS CHAR) FROM orders";
        let mut prints = Vec::new();
        for db in [s, copy] {
            admin.execute(format!("USE `{db}`").as_str()).await.unwrap();
            let row = sqlx::query(fingerprint).fetch_one(&mut admin).await.unwrap();
            prints.push((row.get::<i64, _>(0), row.get::<String, _>(1)));
        }
        assert_eq!(prints[0].0, 500);
        assert_eq!(prints[0], prints[1], "the restored rows differ from the source");
        let kinds: Vec<String> = sqlx::query(
            "SELECT 'view' FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'big_orders'
             UNION ALL SELECT 'trigger' FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = ? AND TRIGGER_NAME = 'orders_touch'
             UNION ALL SELECT 'function' FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? AND ROUTINE_NAME = 'with_tax'",
        )
        .bind(copy)
        .bind(copy)
        .bind(copy)
        .fetch_all(&mut admin)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<String, _>(0))
        .collect();
        assert_eq!(kinds, ["view", "trigger", "function"]);

        let tables_of = |db: String| {
            let opts = opts.clone();
            async move {
                let mut c = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
                let rows = sqlx::query("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME")
                    .bind(db)
                    .fetch_all(&mut c)
                    .await
                    .unwrap();
                rows.iter().map(|r| r.get::<String, _>(0)).collect::<Vec<String>>()
            }
        };

        // ---- what must load nothing ------------------------------------------------------
        // A file cut short does not unpack to its end: refused before the client starts.
        let bytes = std::fs::read(&made.file).unwrap();
        let cut = d.join("cut_2026-11-19_000000.sql.gz");
        std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        let refused = restore::run(&t.profile, t.password.as_deref(), &tool, &cut, untouched, &going(), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(&refused, Error::Invalid(m) if m.contains("Nothing was loaded")), "{refused}");
        // A file that is not a backup by its name, and one that is not there.
        let other = d.join("notes.txt");
        std::fs::write(&other, "CREATE TABLE sneaked (id INT);").unwrap();
        assert!(matches!(
            restore::run(&t.profile, t.password.as_deref(), &tool, &other, untouched, &going(), |_| {}).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            restore::run(&t.profile, t.password.as_deref(), &tool, &d.join("gone.sql.gz"), untouched, &going(), |_| {}).await,
            Err(Error::NotFound(_))
        ));
        // Stopped before it began.
        let halted = restore::run(&t.profile, t.password.as_deref(), &tool, &made.file, untouched, &Arc::new(AtomicBool::new(true)), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(&halted, Error::Conflict(m) if m.contains("Nothing was loaded")), "{halted}");
        // A wrong password: the server's refusal, without the password in it.
        let wrong = restore::run(&t.profile, Some("not-the-p@ssw0rd"), &tool, &made.file, untouched, &going(), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(&wrong, Error::Database(m) if m.contains("Access denied") && !m.contains("not-the-p@ssw0rd")), "{wrong}");
        // A database that does not exist is not created by the load.
        let missing = restore::run(&t.profile, t.password.as_deref(), &tool, &made.file, "puricat_it_no_such_db", &going(), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(&missing, Error::Database(m) if m.contains("mysql failed") && m.contains("puricat_it_no_such_db")), "{missing}");
        assert_eq!(tables_of(untouched.clone()).await, Vec::<String>::new(), "a refused restore loaded something");

        // ---- a load that fails half way: the first error ends it -------------------------
        // The whole dump with a statement the server refuses put before its last table.
        let text = {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut text).unwrap();
            text
        };
        let at = text.find("DROP TABLE IF EXISTS `zz_last`").expect("the dump drops and creates zz_last");
        let broken = d.join("broken_2026-11-19_000000.sql");
        std::fs::write(&broken, format!("{}THIS IS NOT SQL;\n{}", &text[..at], &text[at..])).unwrap();
        let failed = restore::run(&t.profile, t.password.as_deref(), &tool, &broken, broken_db, &going(), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(&failed, Error::Database(m) if m.contains("mysql failed") && m.contains("ERROR 1064") && m.contains("holds what was loaded up to then")), "{failed}");
        // What came before the error is there (the table, and the stand-in mysqldump writes
        // for a view), what came after is not: MySQL is not all-or-nothing.
        assert_eq!(tables_of(broken_db.clone()).await, ["big_orders", "orders"]);

        // ---- Stop while the client loads -------------------------------------------------
        let stop = going();
        let flag = stop.clone();
        let stopped = restore::run(&t.profile, t.password.as_deref(), &tool, &made.file, halted_db, &stop, move |_| {
            flag.store(true, std::sync::atomic::Ordering::Relaxed)
        })
        .await;
        // A dump this small may be loaded before the stop is seen; when it is seen, it says so.
        if let Err(e) = &stopped {
            assert!(matches!(e, Error::Conflict(m) if m.contains("stopped")), "{e}");
        }
    })
    .await;

    for db in &names {
        admin
            .execute(format!("DROP DATABASE IF EXISTS `{db}`").as_str())
            .await
            .unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    outcome.unwrap();
}

#[tokio::test]
async fn postgres_restore_is_all_or_nothing() {
    let Some(t) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let options = |database: &str| {
        let mut o = sqlx::postgres::PgConnectOptions::new()
            .host(&t.profile.host)
            .port(t.profile.port)
            .username(&t.profile.username)
            .database(database);
        if let Some(pw) = &t.password {
            o = o.password(pw);
        }
        o
    };
    let mut admin = sqlx::PgConnection::connect_with(&options("postgres"))
        .await
        .unwrap();
    let version: String = sqlx::query("SELECT current_setting('server_version')")
        .fetch_one(&mut admin)
        .await
        .unwrap()
        .get(0);
    // The source, a copy per format, and one per format that already holds a table in the way.
    let names: Vec<String> = (0..5).map(|_| scratch_name()).collect();
    for db in &names {
        admin
            .execute(format!("CREATE DATABASE \"{db}\"").as_str())
            .await
            .unwrap();
    }
    let dir = scratch_dir();
    let (n, d) = (names.clone(), dir.clone());
    let all: Vec<sqlx::postgres::PgConnectOptions> = names.iter().map(|db| options(db)).collect();

    let outcome = tokio::spawn(async move {
        let mut db = sqlx::PgConnection::connect_with(&all[0]).await.unwrap();
        db.execute(
            "CREATE SCHEMA shop;
             CREATE TABLE shop.orders (
               id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, note TEXT NOT NULL, amount NUMERIC(12,2),
               payload BYTEA, detail JSONB, placed TIMESTAMPTZ NOT NULL DEFAULT '2026-01-02 03:04:05+07'
             );
             INSERT INTO shop.orders (note, amount, payload, detail)
               SELECT 'ลูกค้า ' || i || E' — ‘quote’ \\\\ back; -- not a comment 🐱', i * 1.25,
                      sha256(i::text::bytea), jsonb_build_object('n', i, 'ชื่อ', 'ทดสอบ')
               FROM generate_series(1, 500) AS i;
             CREATE VIEW shop.big_orders AS SELECT id, amount FROM shop.orders WHERE amount > 100;
             CREATE FUNCTION shop.with_tax(x NUMERIC) RETURNS NUMERIC LANGUAGE sql IMMUTABLE AS 'SELECT x * 1.07';
             CREATE TABLE public.zz_last (id INT PRIMARY KEY, label TEXT);
             INSERT INTO public.zz_last VALUES (1, 'ท้ายสุด');",
        )
        .await
        .unwrap();
        db.close().await.unwrap();

        let tool = chosen(&t, &version).await;
        let fingerprint = "SELECT COUNT(*), md5(string_agg(id || '|' || note || '|' || amount || '|' || encode(payload, 'hex') || '|' || detail || '|' || placed, ',' ORDER BY id)) FROM shop.orders";
        let print_of = |o: sqlx::postgres::PgConnectOptions| async move {
            let mut db = sqlx::PgConnection::connect_with(&o).await.unwrap();
            let row = sqlx::query(fingerprint).fetch_one(&mut db).await.unwrap();
            let next: i64 = sqlx::query("SELECT nextval(pg_get_serial_sequence('shop.orders', 'id'))").fetch_one(&mut db).await.unwrap().get(0);
            let extras: i64 = sqlx::query(
                "SELECT (SELECT COUNT(*) FROM pg_views WHERE schemaname = 'shop' AND viewname = 'big_orders')
                      + (SELECT COUNT(*) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = 'shop' AND p.proname = 'with_tax')",
            )
            .fetch_one(&mut db)
            .await
            .unwrap()
            .get(0);
            db.close().await.unwrap();
            (row.get::<i64, _>(0), row.get::<String, _>(1), next, extras)
        };
        // Every table outside the server's own schemas.
        let tables_of = |o: sqlx::postgres::PgConnectOptions| async move {
            let mut db = sqlx::PgConnection::connect_with(&o).await.unwrap();
            let rows = sqlx::query("SELECT schemaname || '.' || tablename FROM pg_tables WHERE schemaname NOT IN ('pg_catalog', 'information_schema') ORDER BY 1")
                .fetch_all(&mut db)
                .await
                .unwrap();
            db.close().await.unwrap();
            rows.iter().map(|r| r.get::<String, _>(0)).collect::<Vec<String>>()
        };
        // Both backups before the source is asked for its next sequence value, which moves it.
        let mut backups = Vec::new();
        for format in [Format::Custom, Format::Sql] {
            backups.push(
                backup::run(&t.profile, t.password.as_deref(), &tool, &request(&n[0], &d, format, true), &AtomicBool::new(false), |_| {})
                    .await
                    .expect("the backup runs"),
            );
        }
        let original = print_of(all[0].clone()).await;
        assert_eq!((original.0, original.2, original.3), (500, 501, 2));

        for ((format, copy, blocked), made) in [(Format::Custom, 1, 3), (Format::Sql, 2, 4)].into_iter().zip(backups) {
            restore::check_made_by(&format!("pg_dump {}", tool.version), &tool).unwrap();

            // ---- the whole backup, into the database made for it -------------------------
            let mut reported = Vec::new();
            restore::run(&t.profile, t.password.as_deref(), &tool, &made.file, &n[copy], &going(), |bytes| reported.push(bytes))
                .await
                .unwrap_or_else(|e| panic!("the restore of {format:?} runs: {e}"));
            // A SQL dump is fed by PuriCat, which knows how far it is; an archive is read by pg_restore.
            match format {
                Format::Sql => assert_eq!(reported.last(), Some(&made.bytes), "{reported:?}"),
                Format::Custom => assert!(reported.is_empty(), "{reported:?}"),
            }
            assert_eq!(print_of(all[copy].clone()).await, original, "the restored {format:?} backup differs from the source");

            // ---- a load that fails half way leaves nothing -------------------------------
            // `zz_last` is already there, so its CREATE TABLE fails — after shop.orders was
            // created and filled in the same transaction.
            let mut db = sqlx::PgConnection::connect_with(&all[blocked]).await.unwrap();
            db.execute("CREATE TABLE public.zz_last (already_here INT)").await.unwrap();
            db.close().await.unwrap();
            let failed = restore::run(&t.profile, t.password.as_deref(), &tool, &made.file, &n[blocked], &going(), |_| {})
                .await
                .unwrap_err();
            assert!(matches!(&failed, Error::Database(m) if m.contains("failed") && m.contains("zz_last") && m.contains("already exists") && m.contains("Nothing was loaded")), "{failed}");
            assert_eq!(tables_of(all[blocked].clone()).await, ["public.zz_last"], "a failed {format:?} restore left something behind");
            let mut db = sqlx::PgConnection::connect_with(&all[blocked]).await.unwrap();
            db.execute("DROP TABLE public.zz_last").await.unwrap();
            db.close().await.unwrap();

            // ---- what must not start -----------------------------------------------------
            let halted = restore::run(&t.profile, t.password.as_deref(), &tool, &made.file, &n[blocked], &Arc::new(AtomicBool::new(true)), |_| {})
                .await
                .unwrap_err();
            assert!(matches!(&halted, Error::Conflict(m) if m.contains("Nothing was loaded")), "{halted}");
            let bytes = std::fs::read(&made.file).unwrap();
            let cut = d.join(if format == Format::Sql { "cut_2026-11-19_000000.sql.gz" } else { "cut_2026-11-19_000000.dump" });
            std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
            let refused = restore::run(&t.profile, t.password.as_deref(), &tool, &cut, &n[blocked], &going(), |_| {})
                .await
                .unwrap_err();
            // A cut SQL dump is refused before the client starts. A cut archive still lists
            // (its table of contents is at the front): pg_restore finds the cut while it
            // loads, and the transaction ends with it.
            match format {
                Format::Sql => assert!(matches!(&refused, Error::Invalid(m) if m.contains("Nothing was loaded")), "{refused}"),
                Format::Custom => assert!(matches!(&refused, Error::Database(m) if m.contains("pg_restore failed") && m.contains("Nothing was loaded")), "{refused}"),
            }
            assert_eq!(tables_of(all[blocked].clone()).await, Vec::<String>::new(), "a refused {format:?} restore loaded something");
        }
        // The archive of a newer pg_dump is not read.
        let newer = restore::check_made_by("pg_dump 99.1", &tool).unwrap_err();
        assert!(matches!(newer, Error::Unsupported(_)));
    })
    .await;

    for db in &names {
        admin
            .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
            .await
            .unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    outcome.unwrap();
}
