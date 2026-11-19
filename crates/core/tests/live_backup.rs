//! Backups against real servers, with the dump tools installed on this computer, and a
//! restore of every backup into a second scratch database: only a restore proves a backup.
//! Same opt-in environment variables and disposable-database rule as `live_db.rs`: every
//! database created here is named `puricat_it_<random>…` and dropped again.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;

use puricat_core::backup::{self, BackupRequest, DumpTool, Format};
use puricat_core::model::{ConnectionProfile, Engine};
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

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn gunzip(path: &Path) -> String {
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    text
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// A client program that was installed with the dump tool.
fn sibling(tool: &DumpTool, name: &str) -> PathBuf {
    let path = Path::new(&tool.path).with_file_name(name);
    assert!(
        path.is_file(),
        "{} is not installed next to {}",
        name,
        tool.path
    );
    path
}

async fn chosen(t: &Target, server_version: &str) -> DumpTool {
    let tools = backup::find_tools(t.profile.engine).await;
    let choice = backup::choose(t.profile.engine, server_version, &tools);
    choice
        .tool
        .unwrap_or_else(|| panic!("no dump tool for this server: {:?}", choice.problem))
}

#[tokio::test]
async fn mysql_backup_restores_to_the_same_data() {
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
    let (source, copy) = (scratch_name(), scratch_name());
    let dir = scratch_dir();
    let (s, c, d) = (source.clone(), copy.clone(), dir.clone());

    let outcome = tokio::spawn(async move {
        let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
        for db in [&s, &c] {
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
        let stop = AtomicBool::new(false);
        let mut reported = Vec::new();
        let done = backup::run(
            &t.profile,
            t.password.as_deref(),
            &tool,
            &request(&s, &d, Format::Sql, true),
            &stop,
            |bytes| reported.push(bytes),
        )
        .await
        .expect("the backup runs");

        // One file, named for the connection, the database and the time, readable by its
        // owner only; nothing unfinished is left beside it.
        let names = files_in(&d);
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with(&format!("Integration-ทดสอบ_{s}_20")) && names[0].ends_with(".sql.gz"), "{names:?}");
        assert_eq!(done.file, d.join(&names[0]));
        assert_eq!(done.bytes, std::fs::metadata(&done.file).unwrap().len());
        #[cfg(unix)]
        assert_eq!(mode(&done.file), 0o600);
        assert!(reported.len() > 1 && reported.windows(2).all(|w| w[0] <= w[1]), "{reported:?}");

        let sql = gunzip(&done.file);
        assert!(sql.contains("CREATE TABLE `orders`"));
        assert!(sql.contains("ลูกค้า 500"));
        assert!(sql.trim_end().lines().last().unwrap().starts_with("-- Dump completed"));

        // Restore into the second database with the client that came with the tool.
        let mysql = sibling(&tool, "mysql");
        let mut restore = Command::new(mysql);
        restore
            .args(["--no-defaults", "--protocol=tcp", "--default-character-set=utf8mb4"])
            .arg(format!("--host={}", t.profile.host))
            .arg(format!("--port={}", t.profile.port))
            .arg(format!("--user={}", t.profile.username))
            .arg(&c)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(pw) = &t.password {
            restore.env("MYSQL_PWD", pw);
        }
        let mut child = restore.spawn().unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), sql.as_bytes()).unwrap();
        let restored = child.wait_with_output().unwrap();
        assert!(restored.status.success(), "{}", String::from_utf8_lossy(&restored.stderr));

        let fingerprint = "SELECT COUNT(*), CAST(SUM(CRC32(CONCAT_WS('|', id, note, amount, HEX(payload), detail))) AS CHAR) FROM orders";
        let mut prints = Vec::new();
        for db in [&s, &c] {
            admin.execute(format!("USE `{db}`").as_str()).await.unwrap();
            let row = sqlx::query(fingerprint).fetch_one(&mut admin).await.unwrap();
            prints.push((row.get::<i64, _>(0), row.get::<String, _>(1)));
        }
        assert_eq!(prints[0].0, 500);
        assert_eq!(prints[0], prints[1], "the restored rows differ from the source");
        // The view, the trigger and the function came along.
        let kinds: Vec<String> = sqlx::query(
            "SELECT 'view' FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'big_orders'
             UNION ALL SELECT 'trigger' FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = ? AND TRIGGER_NAME = 'orders_touch'
             UNION ALL SELECT 'function' FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? AND ROUTINE_NAME = 'with_tax'",
        )
        .bind(&c)
        .bind(&c)
        .bind(&c)
        .fetch_all(&mut admin)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<String, _>(0))
        .collect();
        assert_eq!(kinds, ["view", "trigger", "function"]);

        // ---- what must leave nothing behind ----------------------------------------------
        let empty = scratch_dir();
        // A database that does not exist.
        let missing = backup::run(&t.profile, t.password.as_deref(), &tool, &request("puricat_it_no_such_db", &empty, Format::Sql, false), &stop, |_| {})
            .await
            .unwrap_err();
        assert!(matches!(&missing, Error::Database(m) if m.contains("mysqldump failed") && m.contains("puricat_it_no_such_db")), "{missing}");
        // A wrong password: refused, and the message does not repeat it.
        let wrong = backup::run(&t.profile, Some("not-the-p@ssw0rd"), &tool, &request(&s, &empty, Format::Sql, true), &stop, |_| {})
            .await
            .unwrap_err();
        assert!(matches!(&wrong, Error::Database(m) if m.contains("Access denied") && !m.contains("not-the-p@ssw0rd")), "{wrong}");
        // Stopped before it began.
        let halted = backup::run(&t.profile, t.password.as_deref(), &tool, &request(&s, &empty, Format::Sql, true), &AtomicBool::new(true), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(&halted, Error::Conflict(m) if m.contains("stopped")), "{halted}");
        // A folder that is not there, and the custom format, which is PostgreSQL's.
        let nowhere = backup::run(&t.profile, t.password.as_deref(), &tool, &request(&s, &empty.join("missing"), Format::Sql, true), &stop, |_| {})
            .await
            .unwrap_err();
        assert!(matches!(nowhere, Error::Invalid(_)));
        let custom = backup::run(&t.profile, t.password.as_deref(), &tool, &request(&s, &empty, Format::Custom, false), &stop, |_| {})
            .await
            .unwrap_err();
        assert!(matches!(custom, Error::Invalid(_)));
        assert_eq!(files_in(&empty), Vec::<String>::new(), "a failed run left a file");
        std::fs::remove_dir_all(&empty).unwrap();
    })
    .await;

    for db in [&source, &copy] {
        admin
            .execute(format!("DROP DATABASE IF EXISTS `{db}`").as_str())
            .await
            .unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    outcome.unwrap();
}

#[tokio::test]
async fn postgres_backup_restores_to_the_same_data() {
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
    let (source, copy_custom, copy_sql) = (scratch_name(), scratch_name(), scratch_name());
    for db in [&source, &copy_custom, &copy_sql] {
        admin
            .execute(format!("CREATE DATABASE \"{db}\"").as_str())
            .await
            .unwrap();
    }
    let dir = scratch_dir();
    let (s, cc, cs, d) = (
        source.clone(),
        copy_custom.clone(),
        copy_sql.clone(),
        dir.clone(),
    );
    let (source_options, custom_options, sql_options) =
        (options(&source), options(&copy_custom), options(&copy_sql));

    let outcome = tokio::spawn(async move {
        let mut db = sqlx::PgConnection::connect_with(&source_options).await.unwrap();
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
             CREATE FUNCTION shop.with_tax(x NUMERIC) RETURNS NUMERIC LANGUAGE sql IMMUTABLE AS 'SELECT x * 1.07';",
        )
        .await
        .unwrap();
        db.close().await.unwrap();

        let tool = chosen(&t, &version).await;
        let stop = AtomicBool::new(false);
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
        let env = |cmd: &mut Command| {
            cmd.arg(format!("--host={}", t.profile.host))
                .arg(format!("--port={}", t.profile.port))
                .arg(format!("--username={}", t.profile.username))
                .arg("--no-password")
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            if let Some(pw) = &t.password {
                cmd.env("PGPASSWORD", pw);
            }
        };

        // ---- the custom archive, restored with pg_restore --------------------------------
        let mut reported = Vec::new();
        let archive = backup::run(&t.profile, t.password.as_deref(), &tool, &request(&s, &d, Format::Custom, true), &stop, |b| reported.push(b))
            .await
            .expect("the custom archive is made");
        assert!(archive.file.to_string_lossy().ends_with(".dump"));
        #[cfg(unix)]
        assert_eq!(mode(&archive.file), 0o600);
        assert_eq!(reported.last(), Some(&archive.bytes));
        let mut restore = Command::new(sibling(&tool, "pg_restore"));
        env(&mut restore);
        let restored = restore.arg("--no-owner").arg(format!("--dbname={cc}")).arg(&archive.file).output().unwrap();
        assert!(restored.status.success(), "{}", String::from_utf8_lossy(&restored.stderr));

        // ---- the SQL dump, gzipped, restored with psql -----------------------------------
        let plain = backup::run(&t.profile, t.password.as_deref(), &tool, &request(&s, &d, Format::Sql, true), &stop, |_| {})
            .await
            .expect("the SQL dump is made");
        assert!(plain.file.to_string_lossy().ends_with(".sql.gz"));
        let sql = gunzip(&plain.file);
        assert!(sql.contains("CREATE TABLE shop.orders") && sql.contains("ลูกค้า 500"));
        assert!(sql.contains("PostgreSQL database dump complete"));
        let mut psql = Command::new(sibling(&tool, "psql"));
        env(&mut psql);
        let mut child = psql
            .args(["--no-psqlrc", "--quiet", "--set=ON_ERROR_STOP=1"])
            .arg(format!("--dbname={cs}"))
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), sql.as_bytes()).unwrap();
        let restored = child.wait_with_output().unwrap();
        assert!(restored.status.success(), "{}", String::from_utf8_lossy(&restored.stderr));

        // Both restores hold the same rows as the source, the sequence goes on where it
        // was, and the view and the function came along.
        let original = print_of(source_options.clone()).await;
        assert_eq!((original.0, original.2, original.3), (500, 501, 2));
        assert_eq!(print_of(custom_options).await, original, "the restored archive differs from the source");
        assert_eq!(print_of(sql_options).await, original, "the restored SQL dump differs from the source");
        assert_eq!(files_in(&d).len(), 2, "{:?}", files_in(&d));

        // ---- what must leave nothing behind ----------------------------------------------
        let empty = scratch_dir();
        for format in [Format::Custom, Format::Sql] {
            let missing = backup::run(&t.profile, t.password.as_deref(), &tool, &request("puricat_it_no_such_db", &empty, format, true), &stop, |_| {})
                .await
                .unwrap_err();
            assert!(matches!(&missing, Error::Database(m) if m.contains("pg_dump failed") && m.contains("puricat_it_no_such_db")), "{missing}");
            let halted = backup::run(&t.profile, t.password.as_deref(), &tool, &request(&s, &empty, format, true), &AtomicBool::new(true), |_| {})
                .await
                .unwrap_err();
            assert!(matches!(&halted, Error::Conflict(m) if m.contains("stopped")), "{halted}");
        }
        assert_eq!(files_in(&empty), Vec::<String>::new(), "a failed run left a file");
        std::fs::remove_dir_all(&empty).unwrap();
    })
    .await;

    for db in [&source, &copy_custom, &copy_sql] {
        admin
            .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
            .await
            .unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    outcome.unwrap();
}

/// The schedule's runner against a real server: a job runs once per scheduled time, keeps
/// the newest backups only, and writes every failure into the history with its reason.
#[tokio::test]
async fn scheduled_jobs_run_when_due_and_keep_the_newest() {
    use puricat_core::backup_schedule::{run_due, Schedule};
    use puricat_core::store::{BackupJob, Store};

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
    let database = scratch_name();
    admin
        .execute(format!("CREATE DATABASE `{database}`; CREATE TABLE `{database}`.t (id INT PRIMARY KEY); INSERT INTO `{database}`.t VALUES (1), (2)").as_str())
        .await
        .unwrap();
    let dir = scratch_dir();
    let (db, d, password) = (database.clone(), dir.clone(), t.password.clone());

    // Not a spawned task: the runner's future borrows the password reader. A panic is caught
    // so that the scratch database is dropped all the same.
    let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async move {
        let store = Store::open_in_memory().await.unwrap();
        let profile = store.save_connection(t.profile.clone()).await.unwrap();
        let job = |database: &str, destination: &Path, connection_id: &str| BackupJob {
            id: 0,
            connection_id: connection_id.to_string(),
            database: database.to_string(),
            destination: destination.to_string_lossy().into_owned(),
            format: Format::Sql,
            compress: true,
            schedule: Schedule::Daily { hour: 2, minute: 0 },
            keep: 2,
            updated_at: 0,
        };
        let saved = store
            .save_backup_job(&job(&db, &d, &profile.id))
            .await
            .unwrap();
        let password_of = |_: &ConnectionProfile| Ok(password.clone());
        const DAY: i64 = 24 * 60 * 60 * 1000;

        // Right after saving, nothing is due: the time that passed today is not made up for.
        assert!(run_due(&store, &password_of, saved.updated_at + 1000)
            .await
            .unwrap()
            .is_empty());

        // One run per day for four days; asking twice on the same day runs nothing more.
        for day in 1..=4 {
            let now = saved.updated_at + day * DAY;
            let ran = run_due(&store, &password_of, now).await.unwrap();
            assert_eq!(ran.len(), 1, "day {day}");
            assert_eq!(
                (ran[0].status.as_str(), ran[0].scheduled),
                ("ok", true),
                "{}",
                ran[0].message
            );
            assert!(ran[0].tool.starts_with("mysqldump "), "{}", ran[0].tool);
            assert!(
                run_due(&store, &password_of, now + 60_000)
                    .await
                    .unwrap()
                    .is_empty(),
                "day {day}, again"
            );
            // File names carry the time to the second.
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        }
        // Four ran, two are kept: the two newest files are there, the older two are gone and
        // the history says so.
        let history = store
            .list_backup_runs(Some((&profile.id, &db)), 50)
            .await
            .unwrap();
        assert_eq!(history.len(), 4);
        let on_disk = files_in(&d);
        assert_eq!(on_disk.len(), 2, "{on_disk:?}");
        for (i, run) in history.iter().enumerate() {
            let file = Path::new(run.file.as_ref().unwrap());
            assert_eq!(file.exists(), i < 2, "run {i}");
            assert_eq!(run.removed_at.is_some(), i >= 2, "run {i}");
        }

        // A job whose folder is gone, and one whose connection was deleted: both are failed
        // runs with a reason, and the good job still runs.
        let gone = d.join("unplugged-disk");
        store
            .save_backup_job(&job("puricat_it_other", &gone, &profile.id))
            .await
            .unwrap();
        store
            .save_backup_job(&job(&db, &d, "no-such-connection"))
            .await
            .unwrap();
        let now = store
            .list_backup_jobs()
            .await
            .unwrap()
            .iter()
            .map(|j| j.updated_at)
            .max()
            .unwrap()
            + 10 * DAY;
        let mut ran = run_due(&store, &password_of, now).await.unwrap();
        ran.sort_by(|a, b| a.status.cmp(&b.status).then(a.message.cmp(&b.message)));
        assert_eq!(
            ran.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(),
            ["failed", "failed", "ok"]
        );
        assert!(
            ran[0].message.contains("no longer exists"),
            "{}",
            ran[0].message
        );
        assert!(
            ran[1].message.contains("unplugged-disk") && ran[1].message.contains("does not exist"),
            "{}",
            ran[1].message
        );
        assert_eq!(files_in(&d).len(), 2);
    }))
    .await;

    admin
        .execute(format!("DROP DATABASE IF EXISTS `{database}`").as_str())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
