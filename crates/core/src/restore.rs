//! Loading a backup PuriCat made into a database that was created for it ([ADR 0022]).
//!
//! The file is loaded by the vendor's own client, the one installed next to the dump tool:
//! `mysql` (or `mariadb`) for a MySQL dump, `psql` for a PostgreSQL SQL dump, `pg_restore`
//! for a PostgreSQL archive. As for a backup, the client runs with a fixed argument vector
//! and no shell, and the password is never an argument.
//!
//! This module only loads. Creating the database, and refusing one that already exists, is
//! the caller's part: nothing here is told to drop or replace anything.
//!
//! [ADR 0022]: ../../../docs/adr/0022-restore-into-a-new-database.md

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::ai_detect::path_with;
use crate::backup::{
    create_private, dump_is_complete, first_number, list_archive, mysql_option_file,
    mysql_tls_args, pg_env, Client, DumpTool, PrivateDir, STDERR_KEPT, TAIL_KEPT,
};
use crate::model::{ConnectionProfile, Engine};
use crate::{Error, Result};

/// What a backup file is, read from the name PuriCat gave it.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// SQL statements.
    Sql,
    /// SQL statements, gzipped.
    SqlGz,
    /// PostgreSQL's custom archive.
    Archive,
}

impl Kind {
    pub fn of(file: &Path) -> Option<Kind> {
        let name = file.file_name()?.to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".sql.gz") {
            Some(Kind::SqlGz)
        } else if name.ends_with(".sql") {
            Some(Kind::Sql)
        } else if name.ends_with(".dump") {
            Some(Kind::Archive)
        } else {
            None
        }
    }
}

/// The program that loads a backup of this kind: the client installed with `tool`.
pub fn client_for(tool: &DumpTool, kind: Kind) -> Result<PathBuf> {
    let names: &[&str] = match (tool.client, kind) {
        (Client::Postgres, Kind::Archive) => &["pg_restore"],
        (Client::Postgres, _) => &["psql"],
        (_, Kind::Archive) => {
            return Err(Error::Invalid(
                "An archive (.dump) is a PostgreSQL backup; it cannot be loaded into MySQL".into(),
            ))
        }
        (Client::Mariadb, _) => &["mariadb", "mysql"],
        (Client::Mysql, _) => &["mysql"],
    };
    names
        .iter()
        .map(|name| Path::new(&tool.path).with_file_name(name))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{} was not found next to {}. It loads a backup, and comes with the same client tools.",
                names[0], tool.path
            ))
        })
}

/// A PostgreSQL archive is read by a `pg_restore` at least as new as the `pg_dump` that
/// wrote it. `made_by` is what the history recorded, for example `pg_dump 16.13`.
pub fn check_made_by(made_by: &str, tool: &DumpTool) -> Result<()> {
    if tool.client != Client::Postgres {
        return Ok(());
    }
    match first_number(made_by) {
        Some(major) if major > tool.major => Err(Error::Unsupported(format!(
            "This backup was made by {made_by}, and the PostgreSQL client tools found here are {}. Loading it needs the tools of PostgreSQL {major} or newer.",
            tool.version
        ))),
        _ => Ok(()),
    }
}

// ---- argument vectors ------------------------------------------------------------------

/// The arguments of `mysql` (or `mariadb`) reading a dump on standard input. `option_file`
/// is read instead of the user's own option files.
pub fn mysql_args(
    profile: &ConnectionProfile,
    tool: &DumpTool,
    option_file: &Path,
    database: &str,
) -> Vec<String> {
    let mut args = vec![
        format!("--defaults-file={}", option_file.display()),
        format!("--host={}", profile.host),
        format!("--port={}", profile.port),
        format!("--user={}", profile.username),
        "--protocol=tcp".into(),
        "--default-character-set=utf8mb4".into(),
        // The dump's bytes as they are: no `\r\n` translation, no stop at a NUL byte.
        "--binary-mode".into(),
    ];
    args.extend(mysql_tls_args(profile, tool.client));
    // After `--`, a database whose name starts with a dash is still a name.
    args.push("--".into());
    args.push(database.to_string());
    args
}

fn pg_target(profile: &ConnectionProfile, database: &str) -> Vec<String> {
    vec![
        format!("--host={}", profile.host),
        format!("--port={}", profile.port),
        format!("--username={}", profile.username),
        "--no-password".into(),
        format!("--dbname={database}"),
    ]
}

/// The arguments of `psql` reading a SQL dump on standard input: none of the user's own
/// start-up file, the first error ends it, and all of it is one transaction.
pub fn psql_args(profile: &ConnectionProfile, database: &str) -> Vec<String> {
    let mut args = pg_target(profile, database);
    args.extend(
        [
            "--no-psqlrc",
            "--quiet",
            "--set=ON_ERROR_STOP=1",
            "--single-transaction",
        ]
        .map(String::from),
    );
    args
}

/// The arguments of `pg_restore` for an archive: one transaction, ended by the first error;
/// the objects belong to the role that restores them.
pub fn pg_restore_args(profile: &ConnectionProfile, database: &str, archive: &Path) -> Vec<String> {
    let mut args = pg_target(profile, database);
    args.extend(["--no-owner", "--single-transaction", "--exit-on-error"].map(String::from));
    args.push(archive.to_string_lossy().into_owned());
    args
}

// ---- a run -----------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub duration_ms: u64,
}

/// Whether a failed or stopped load leaves nothing in the database. PostgreSQL loads in one
/// transaction; MySQL commits every `CREATE TABLE` as it goes.
pub fn all_or_nothing(engine: Engine) -> bool {
    engine == Engine::Postgres
}

/// What the database holds after a load that did not reach its end.
fn left_behind(engine: Engine) -> &'static str {
    if all_or_nothing(engine) {
        "Nothing was loaded."
    } else {
        "The database holds what was loaded up to then."
    }
}

fn stopped(engine: Engine) -> Error {
    Error::Conflict(format!("The restore was stopped. {}", left_behind(engine)))
}

fn client_name(client: &Path) -> String {
    client
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A file that counts the bytes read from it, for the progress of a load.
struct Counted {
    file: std::fs::File,
    read: Arc<AtomicU64>,
}

impl Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.file.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

fn sql_reader(file: std::fs::File, kind: Kind, read: Arc<AtomicU64>) -> Box<dyn Read + Send> {
    let counted = Counted { file, read };
    match kind {
        Kind::SqlGz => Box::new(flate2::read::GzDecoder::new(counted)),
        _ => Box::new(counted),
    }
}

/// Read a SQL dump to its end before any of it is loaded: a file that does not unpack, or
/// that lacks the dump tool's closing line, is not a whole backup. Stops early when asked.
fn check_sql_file(file: &Path, kind: Kind, client: Client, stop: &AtomicBool) -> Result<()> {
    let unreadable = |e: std::io::Error| {
        Error::Invalid(format!(
            "The backup file cannot be read: {e}. Nothing was loaded."
        ))
    };
    let mut source = sql_reader(
        std::fs::File::open(file).map_err(unreadable)?,
        kind,
        Arc::new(AtomicU64::new(0)),
    );
    let mut chunk = vec![0u8; 256 * 1024];
    let mut tail: Vec<u8> = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Conflict(
                "The restore was stopped. Nothing was loaded.".into(),
            ));
        }
        let n = source.read(&mut chunk).map_err(unreadable)?;
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..n]);
        if tail.len() > TAIL_KEPT {
            tail.drain(..tail.len() - TAIL_KEPT);
        }
    }
    if dump_is_complete(client, &tail) {
        Ok(())
    } else {
        Err(Error::Invalid(
            "The backup file does not end with the dump tool's closing line: it is incomplete or was changed. Nothing was loaded.".into(),
        ))
    }
}

enum Feed {
    Chunk(Vec<u8>),
    Failed(String),
}

/// Load `file` into `database`, which exists and is empty. `on_progress` is told how many
/// bytes of the file were read so far, for a SQL dump; an archive is read by `pg_restore`
/// itself. Setting `stop` ends the client.
///
/// A SQL dump is read to its closing line before any of it is loaded, so a file that was
/// cut short loads nothing. An archive is listed first; `pg_restore` finds a cut in its data
/// only while loading, which ends the transaction and so loads nothing either.
pub async fn run(
    profile: &ConnectionProfile,
    password: Option<&str>,
    tool: &DumpTool,
    file: &Path,
    database: &str,
    stop: &Arc<AtomicBool>,
    mut on_progress: impl FnMut(u64),
) -> Result<RestoreOutcome> {
    let engine = profile.engine;
    if !matches!(engine, Engine::Mysql | Engine::Postgres) {
        return Err(Error::Unsupported(
            "Backups are restored into MySQL and PostgreSQL connections.".into(),
        ));
    }
    if database.is_empty() || database.contains('\0') {
        return Err(Error::Invalid("A database name is needed".into()));
    }
    let kind = Kind::of(file).ok_or_else(|| {
        Error::Invalid(format!(
            "{} is not a backup PuriCat made (.sql, .sql.gz or .dump).",
            file.display()
        ))
    })?;
    if !file.is_file() {
        return Err(Error::NotFound(format!(
            "The backup file {}",
            file.display()
        )));
    }
    let client = client_for(tool, kind)?;
    let started = Instant::now();

    // ---- the file, before anything is loaded from it ----
    if kind == Kind::Archive {
        list_archive(profile, tool, file)
            .await
            .map_err(|e| match e {
                Error::Database(text) => Error::Invalid(
                    text.replace("so it was not kept", "so nothing was loaded")
                        .replace(" No file was kept.", " Nothing was loaded."),
                ),
                other => other,
            })?;
    } else {
        let (path, flag, made_by) = (file.to_path_buf(), stop.clone(), tool.client);
        tokio::task::spawn_blocking(move || check_sql_file(&path, kind, made_by, &flag))
            .await
            .map_err(|e| Error::Unsupported(e.to_string()))??;
    }
    if stop.load(Ordering::Relaxed) {
        return Err(Error::Conflict(
            "The restore was stopped. Nothing was loaded.".into(),
        ));
    }

    // ---- the client ----
    let private = PrivateDir::new()?;
    let mut cmd = Command::new(&client);
    cmd.env_clear()
        .env("PATH", path_with(client.parent()))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    match engine {
        Engine::Mysql => {
            let option_file = private.0.join("client.cnf");
            create_private(&option_file)?.write_all(mysql_option_file(password)?.as_bytes())?;
            cmd.args(mysql_args(profile, tool, &option_file, database));
        }
        _ => {
            cmd.args(match kind {
                Kind::Archive => pg_restore_args(profile, database, file),
                _ => psql_args(profile, database),
            });
            cmd.envs(pg_env(profile, password));
            cmd.env("PGAPPNAME", "PuriCat restore");
        }
    }
    let streamed = kind != Kind::Archive;
    cmd.stdin(if streamed {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = cmd.spawn().map_err(|e| {
        Error::Unsupported(format!("{} could not be started: {e}", client.display()))
    })?;
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let errors = tokio::spawn(async move {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            let room = STDERR_KEPT.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..n.min(room)]);
        }
        kept
    });

    // A thread reads (and unpacks) the file; the client gets it as fast as it takes it.
    let read = Arc::new(AtomicU64::new(0));
    let (sender, mut feed) = tokio::sync::mpsc::channel::<Feed>(4);
    if streamed {
        let (path, counter) = (file.to_path_buf(), read.clone());
        std::thread::spawn(move || {
            let mut source = match std::fs::File::open(&path) {
                Ok(f) => sql_reader(f, kind, counter),
                Err(e) => {
                    let _ = sender.blocking_send(Feed::Failed(e.to_string()));
                    return;
                }
            };
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                let message = match source.read(&mut chunk) {
                    Ok(0) => return,
                    Ok(n) => Feed::Chunk(chunk[..n].to_vec()),
                    Err(e) => Feed::Failed(e.to_string()),
                };
                let last = matches!(message, Feed::Failed(_));
                if sender.blocking_send(message).is_err() || last {
                    return;
                }
            }
        });
    } else {
        drop(sender);
    }

    let mut stdin = child.stdin.take();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let status = loop {
        tokio::select! {
            fed = feed.recv(), if stdin.is_some() => match fed {
                Some(Feed::Chunk(bytes)) => {
                    let written = {
                        let pipe = stdin.as_mut().expect("checked by the guard");
                        let write = pipe.write_all(&bytes);
                        tokio::pin!(write);
                        // A client that waits (for a lock on the server) must still be stoppable.
                        loop {
                            tokio::select! {
                                written = &mut write => break written,
                                _ = tick.tick() => {
                                    if stop.load(Ordering::Relaxed) {
                                        let _ = child.kill().await;
                                        return Err(stopped(engine));
                                    }
                                }
                            }
                        }
                    };
                    if written.is_err() {
                        // The client ended before the end of the file: its own words say why.
                        stdin = None;
                        feed.close();
                    }
                    on_progress(read.load(Ordering::Relaxed));
                }
                Some(Feed::Failed(text)) => {
                    let _ = child.kill().await;
                    return Err(Error::Database(format!(
                        "The backup file could not be read to its end: {text}. {}",
                        left_behind(engine)
                    )));
                }
                // The end of the file: the client is told by closing its input.
                None => stdin = None,
            },
            status = child.wait() => break status?,
            _ = tick.tick() => {
                if stop.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    return Err(stopped(engine));
                }
            }
        }
    };
    let stderr = errors.await.unwrap_or_default();
    if !status.success() {
        let text = String::from_utf8_lossy(&stderr);
        let text = text.trim();
        let name = client_name(&client);
        return Err(Error::Database(if text.is_empty() {
            format!(
                "{name} ended with {status} and said nothing. {}",
                left_behind(engine)
            )
        } else {
            format!("{name} failed: {text}\n{}", left_behind(engine))
        }));
    }
    Ok(RestoreOutcome {
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(value: serde_json::Value) -> ConnectionProfile {
        serde_json::from_value(value).unwrap()
    }

    fn tool(client: Client, version: &str) -> DumpTool {
        DumpTool {
            path: format!("/tools/{version}/dump"),
            client,
            version: version.into(),
            major: first_number(version).unwrap(),
        }
    }

    #[test]
    fn a_backup_is_known_by_the_name_puricat_gave_it() {
        let kind = |name: &str| Kind::of(Path::new(name));
        assert_eq!(
            kind("/b/Shop_shopdb_2026-11-19_140509.sql.gz"),
            Some(Kind::SqlGz)
        );
        assert_eq!(
            kind("/b/Shop_shopdb_2026-11-19_140509.sql"),
            Some(Kind::Sql)
        );
        assert_eq!(
            kind("/b/Shop_shopdb_2026-11-19_140509.dump"),
            Some(Kind::Archive)
        );
        assert_eq!(kind("/b/SHOP.SQL.GZ"), Some(Kind::SqlGz));
        assert_eq!(kind("/b/notes.txt"), None);
        assert_eq!(kind("/b/archive.gz"), None);
    }

    #[test]
    fn the_password_is_never_an_argument_of_the_mysql_client() {
        let p = profile(json!({
            "name": "Shop", "engine": "mysql", "host": "db.example", "port": 3307, "username": "app",
            "tls": "verify-full", "tlsCaPath": "/certs/ca.pem",
        }));
        let args = mysql_args(
            &p,
            &tool(Client::Mysql, "9.3.0"),
            Path::new("/private/x/client.cnf"),
            "-odd name",
        );
        // The option file comes first, or the client would read the user's own files too.
        assert_eq!(args[0], "--defaults-file=/private/x/client.cnf");
        assert!(args.contains(&"--host=db.example".to_string()));
        assert!(args.contains(&"--port=3307".to_string()));
        assert!(args.contains(&"--protocol=tcp".to_string()));
        assert!(args.contains(&"--binary-mode".to_string()));
        assert!(args.contains(&"--ssl-mode=VERIFY_IDENTITY".to_string()));
        assert!(args.contains(&"--ssl-ca=/certs/ca.pem".to_string()));
        // The database is the last argument, after `--`: a leading dash is not an option.
        assert_eq!(&args[args.len() - 2..], ["--", "-odd name"]);
        assert!(!args.iter().any(|a| a.contains("password")));
        // Nothing of the dump tool's own options: this program loads.
        assert!(!args
            .iter()
            .any(|a| a.starts_with("--single-transaction") || a.starts_with("--set-gtid")));

        let maria = mysql_args(&p, &tool(Client::Mariadb, "11.4.2"), Path::new("/c"), "db");
        assert!(maria.contains(&"--ssl-verify-server-cert".to_string()));
        assert!(!maria.iter().any(|a| a.starts_with("--ssl-mode")));
    }

    #[test]
    fn postgres_loads_in_one_transaction_and_stops_at_the_first_error() {
        let p = profile(json!({
            "name": "x", "engine": "postgres", "host": "pg.example", "port": 5433, "username": "app",
        }));
        assert_eq!(
            psql_args(&p, "shop copy"),
            [
                "--host=pg.example",
                "--port=5433",
                "--username=app",
                "--no-password",
                "--dbname=shop copy",
                "--no-psqlrc",
                "--quiet",
                "--set=ON_ERROR_STOP=1",
                "--single-transaction",
            ]
        );
        assert_eq!(
            pg_restore_args(&p, "shop_copy", Path::new("/b/shop.dump")),
            [
                "--host=pg.example",
                "--port=5433",
                "--username=app",
                "--no-password",
                "--dbname=shop_copy",
                "--no-owner",
                "--single-transaction",
                "--exit-on-error",
                "/b/shop.dump",
            ]
        );
        assert!(all_or_nothing(Engine::Postgres));
        assert!(!all_or_nothing(Engine::Mysql));
    }

    #[test]
    fn the_client_is_the_one_installed_with_the_dump_tool() {
        let dir = std::env::temp_dir().join(format!(
            "puricat-restore-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&dir).unwrap();
        let at = |client, name: &str| DumpTool {
            path: dir.join(name).to_string_lossy().into_owned(),
            client,
            version: "1.0".into(),
            major: 1,
        };
        // Nothing installed beside the tool: said, with the name of what is missing.
        let missing = client_for(&at(Client::Mysql, "mysqldump"), Kind::SqlGz).unwrap_err();
        assert!(
            matches!(&missing, Error::Unsupported(m) if m.starts_with("mysql was not found next to")),
            "{missing}"
        );

        for name in ["mysql", "psql", "pg_restore"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let found = |client, tool: &str, kind| client_for(&at(client, tool), kind).unwrap();
        assert_eq!(
            found(Client::Mysql, "mysqldump", Kind::SqlGz),
            dir.join("mysql")
        );
        // MariaDB's own client when it is there, `mysql` otherwise.
        assert_eq!(
            found(Client::Mariadb, "mariadb-dump", Kind::Sql),
            dir.join("mysql")
        );
        std::fs::write(dir.join("mariadb"), "").unwrap();
        assert_eq!(
            found(Client::Mariadb, "mariadb-dump", Kind::Sql),
            dir.join("mariadb")
        );
        assert_eq!(
            found(Client::Postgres, "pg_dump", Kind::Sql),
            dir.join("psql")
        );
        assert_eq!(
            found(Client::Postgres, "pg_dump", Kind::Archive),
            dir.join("pg_restore")
        );
        // An archive is PostgreSQL's.
        assert!(matches!(
            client_for(&at(Client::Mysql, "mysqldump"), Kind::Archive),
            Err(Error::Invalid(_))
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_archive_needs_tools_as_new_as_the_ones_that_made_it() {
        let pg16 = tool(Client::Postgres, "16.13");
        assert!(check_made_by("pg_dump 16.13", &pg16).is_ok());
        assert!(check_made_by("pg_dump 15.8", &pg16).is_ok());
        let newer = check_made_by("pg_dump 17.2", &pg16).unwrap_err();
        assert!(
            matches!(&newer, Error::Unsupported(m) if m.contains("PostgreSQL 17 or newer")),
            "{newer}"
        );
        // Nothing recorded, or MySQL: no rule to apply.
        assert!(check_made_by("", &pg16).is_ok());
        assert!(check_made_by("mysqldump 9.3.0", &tool(Client::Mysql, "8.0.36")).is_ok());
    }

    #[test]
    fn a_dump_that_is_not_whole_is_refused_before_anything_is_loaded() {
        let dir = std::env::temp_dir().join(format!(
            "puricat-restore-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&dir).unwrap();
        let stop = AtomicBool::new(false);
        let whole = "CREATE TABLE t (id INT);\n-- Dump completed on 2026-11-19 14:05:09\n";

        let plain = dir.join("a.sql");
        std::fs::write(&plain, whole).unwrap();
        assert!(check_sql_file(&plain, Kind::Sql, Client::Mysql, &stop).is_ok());
        // The closing line of another vendor's tool is not this one's.
        assert!(check_sql_file(&plain, Kind::Sql, Client::Postgres, &stop).is_err());

        let gz = dir.join("a.sql.gz");
        let mut packed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        packed.write_all(whole.as_bytes()).unwrap();
        let bytes = packed.finish().unwrap();
        std::fs::write(&gz, &bytes).unwrap();
        assert!(check_sql_file(&gz, Kind::SqlGz, Client::Mysql, &stop).is_ok());

        // Cut short: it does not unpack to its end.
        let cut = dir.join("cut.sql.gz");
        std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        let refused = check_sql_file(&cut, Kind::SqlGz, Client::Mysql, &stop).unwrap_err();
        assert!(
            matches!(&refused, Error::Invalid(m) if m.contains("Nothing was loaded")),
            "{refused}"
        );
        // Whole as a file, but the dump inside never reached its end.
        let partial = dir.join("partial.sql");
        std::fs::write(
            &partial,
            "CREATE TABLE t (id INT);\nINSERT INTO t VALUES (1",
        )
        .unwrap();
        let refused = check_sql_file(&partial, Kind::Sql, Client::Mysql, &stop).unwrap_err();
        assert!(
            matches!(&refused, Error::Invalid(m) if m.contains("closing line")),
            "{refused}"
        );
        // Asked to stop: nothing more is read.
        let halted =
            check_sql_file(&plain, Kind::Sql, Client::Mysql, &AtomicBool::new(true)).unwrap_err();
        assert!(matches!(halted, Error::Conflict(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
