//! Backups of one database with the vendor's own dump tool ([ADR 0021]): `mysqldump` (or
//! MariaDB's) and `pg_dump`. The tools are found on this computer, never shipped.
//!
//! A tool runs with a fixed argument vector and no shell. The password is never an
//! argument: `mysqldump` reads it from an option file that only this user can read and that
//! is removed afterwards, `pg_dump` from `PGPASSWORD`. A dump is written under a temporary
//! name, checked for the tool's own end marker (or listed by `pg_restore`), and only then
//! given its name; a run that fails or is stopped leaves no file.
//!
//! [ADR 0021]: ../../../docs/adr/0021-backups-with-dump-tools.md

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::ai_detect::path_with;
use crate::model::{ConnectionProfile, Engine, TlsMode};
use crate::{Error, Result};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// How much of a tool's error output is kept for the message of a failed run.
pub(crate) const STDERR_KEPT: usize = 8 * 1024;
/// The end of a dump is looked at for the tool's "completed" line.
pub(crate) const TAIL_KEPT: usize = 512;

// ---- the tools -------------------------------------------------------------------------

/// Whose client a dump tool is. MariaDB ships a `mysqldump` of its own with other options.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    Mysql,
    Mariadb,
    Postgres,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DumpTool {
    pub path: String,
    pub client: Client,
    /// As the tool prints it, for example `9.3.0` or `16.13`.
    pub version: String,
    pub major: u32,
}

/// What a server said it is, reduced to what choosing a tool needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub mariadb: bool,
    pub major: u32,
}

impl Server {
    /// From `SELECT version()` (MySQL, MariaDB) or `server_version` (PostgreSQL).
    pub fn parse(version: &str) -> Server {
        Server {
            mariadb: version.to_ascii_lowercase().contains("mariadb"),
            major: first_number(version).unwrap_or(0),
        }
    }
}

pub(crate) fn first_number(text: &str) -> Option<u32> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The dotted number that follows `marker` in `text`.
fn version_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let rest = text[text.find(marker)? + marker.len()..].trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let version = rest[..end].trim_end_matches('.');
    (!version.is_empty()).then_some(version)
}

/// Read a tool's `--version` line.
pub fn parse_tool_version(engine: Engine, line: &str) -> Option<(Client, String)> {
    match engine {
        Engine::Postgres => {
            version_after(line, "(PostgreSQL)").map(|v| (Client::Postgres, v.to_string()))
        }
        Engine::Mysql if line.contains("MariaDB") => {
            // "mysqldump  Ver 10.19 Distrib 10.11.6-MariaDB, …" / "mariadb-dump from 11.4.2-MariaDB, …"
            version_after(line, "Distrib")
                .or_else(|| version_after(line, " from"))
                .map(|v| (Client::Mariadb, v.to_string()))
        }
        Engine::Mysql => version_after(line, "Ver").map(|v| (Client::Mysql, v.to_string())),
        _ => None,
    }
}

/// Where dump tools are installed besides `PATH`. An app started from Finder has a bare
/// `PATH`, and Homebrew keeps versioned clients out of it on purpose.
fn tool_dirs(engine: Engine) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    let prefixes: &[&str] = match engine {
        Engine::Postgres => &["postgresql", "libpq"],
        _ => &["mysql", "mariadb"],
    };
    for cellar in ["/opt/homebrew/opt", "/usr/local/opt"] {
        let Ok(entries) = std::fs::read_dir(cellar) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if prefixes.iter().any(|p| name.starts_with(p)) {
                dirs.push(entry.path().join("bin"));
            }
        }
    }
    match engine {
        Engine::Postgres => {
            dirs.push("/Applications/Postgres.app/Contents/Versions/latest/bin".into());
            if let Ok(entries) = std::fs::read_dir("/Library/PostgreSQL") {
                dirs.extend(entries.flatten().map(|e| e.path().join("bin")));
            }
        }
        _ => dirs.push("/usr/local/mysql/bin".into()),
    }
    dirs
}

fn tool_names(engine: Engine) -> &'static [&'static str] {
    match engine {
        Engine::Postgres => &["pg_dump"],
        Engine::Mysql => &["mysqldump", "mariadb-dump"],
        _ => &[],
    }
}

/// Every dump tool for `engine` found on this computer, each asked for its version.
pub async fn find_tools(engine: Engine) -> Vec<DumpTool> {
    let mut seen = std::collections::HashSet::new();
    let mut tools = Vec::new();
    for dir in tool_dirs(engine) {
        for name in tool_names(engine) {
            let path = dir.join(name);
            // The same file is reached through several directories (links of Homebrew).
            let Ok(real) = std::fs::canonicalize(&path) else {
                continue;
            };
            if !real.is_file() || !seen.insert(real) {
                continue;
            }
            if let Some(tool) = probe(engine, &path).await {
                tools.push(tool);
            }
        }
    }
    tools
}

async fn probe(engine: Engine, path: &Path) -> Option<DumpTool> {
    let mut cmd = Command::new(path);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(PROBE_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (client, version) = parse_tool_version(engine, text.lines().next()?)?;
    Some(DumpTool {
        path: path.to_string_lossy().into_owned(),
        client,
        major: first_number(&version)?,
        version,
    })
}

/// The tool to use for a server, with what the user should know about the pairing.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    /// `None`: no tool on this computer can back this server up; `problem` says why.
    pub tool: Option<DumpTool>,
    pub problem: Option<String>,
    /// The pairing works but is not the one the vendor recommends.
    pub warning: Option<String>,
}

/// Pick among `tools` for a server of `engine` that reported `server_version`.
pub fn choose(engine: Engine, server_version: &str, tools: &[DumpTool]) -> Choice {
    let server = Server::parse(server_version);
    let none = |problem: String| Choice {
        tool: None,
        problem: Some(problem),
        warning: None,
    };
    match engine {
        Engine::Postgres => {
            // pg_dump refuses a server newer than itself. The same major is the best match;
            // a newer pg_dump is supported by PostgreSQL for older servers.
            let mut usable: Vec<&DumpTool> =
                tools.iter().filter(|t| t.major >= server.major).collect();
            usable.sort_by_key(|t| (t.major != server.major, std::cmp::Reverse(t.major)));
            match usable.first() {
                Some(tool) => Choice {
                    tool: Some((*tool).clone()),
                    problem: None,
                    warning: None,
                },
                None if tools.is_empty() => none(
                    "pg_dump was not found on this computer. It comes with PostgreSQL's client tools (for example `brew install libpq`).".into(),
                ),
                None => none(format!(
                    "The server is PostgreSQL {}, and the newest pg_dump found here is {}. pg_dump cannot back up a server newer than itself: install the client tools of PostgreSQL {} or newer.",
                    server.major,
                    tools.iter().map(|t| t.major).max().unwrap_or(0),
                    server.major
                )),
            }
        }
        Engine::Mysql => {
            let wanted = if server.mariadb {
                Client::Mariadb
            } else {
                Client::Mysql
            };
            let newest = |client: Client| {
                tools
                    .iter()
                    .filter(|t| t.client == client)
                    .max_by_key(|t| t.major)
            };
            let other = if server.mariadb {
                Client::Mysql
            } else {
                Client::Mariadb
            };
            if let Some(tool) = newest(wanted) {
                let warning = (!server.mariadb && tool.major < server.major).then(|| {
                    format!(
                        "mysqldump {} is older than the server (MySQL {}). Newer features of the server may be missing from the backup.",
                        tool.version, server.major
                    )
                });
                return Choice {
                    tool: Some(tool.clone()),
                    problem: None,
                    warning,
                };
            }
            match newest(other) {
                Some(tool) => Choice {
                    tool: Some(tool.clone()),
                    problem: None,
                    warning: Some(if server.mariadb {
                        format!(
                            "The server is MariaDB and only MySQL's mysqldump {} was found. Tables and data are backed up; MariaDB's own tool (`mariadb-dump`) is the supported one for this server.",
                            tool.version
                        )
                    } else {
                        format!(
                            "The server is MySQL and only MariaDB's dump tool {} was found. Tables and data are backed up; MySQL's own mysqldump is the supported one for this server.",
                            tool.version
                        )
                    }),
                },
                None => none(
                    "mysqldump was not found on this computer. It comes with MySQL's client tools (for example `brew install mysql-client`).".into(),
                ),
            }
        }
        _ => none("Backups are available for MySQL and PostgreSQL connections.".into()),
    }
}

// ---- what a run is asked for -----------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// SQL statements. Readable, and restorable with the plain client.
    Sql,
    /// PostgreSQL's custom archive: compressed, and `pg_restore` can restore parts of it.
    Custom,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BackupRequest {
    pub database: String,
    /// The folder the file is written to. It must exist.
    pub destination: PathBuf,
    pub format: Format,
    /// Gzip a SQL dump. A custom archive is compressed by `pg_dump` itself.
    pub compress: bool,
}

impl BackupRequest {
    pub fn extension(&self) -> &'static str {
        match (self.format, self.compress) {
            (Format::Custom, _) => "dump",
            (Format::Sql, true) => "sql.gz",
            (Format::Sql, false) => "sql",
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BackupOutcome {
    pub file: PathBuf,
    pub bytes: u64,
    pub duration_ms: u64,
}

/// A file name made of the connection, the database and the local time, safe on every
/// file system: spaces and the characters a path or a shell gives a meaning become `-`.
pub fn file_name(
    connection: &str,
    database: &str,
    at: chrono::NaiveDateTime,
    extension: &str,
) -> String {
    fn clean(text: &str, fallback: &str) -> String {
        let mut out = String::new();
        for c in text.trim().chars() {
            // What a path or a shell would read as something else; every script's letters stay.
            let unsafe_here =
                c.is_whitespace() || c.is_control() || "/\\:*?\"<>|'`$&;!#%{}[]()".contains(c);
            if !unsafe_here {
                out.push(c);
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
        let out = out.trim_matches(|c| c == '-' || c == '.').to_string();
        if out.is_empty() {
            fallback.to_string()
        } else {
            out.chars().take(60).collect()
        }
    }
    format!(
        "{}_{}_{}.{extension}",
        clean(connection, "connection"),
        clean(database, "database"),
        at.format("%Y-%m-%d_%H%M%S")
    )
}

// ---- argument vectors ------------------------------------------------------------------

/// A value inside double quotes in a MySQL option file.
fn option_file_value(value: &str) -> Result<String> {
    if value.contains(['\n', '\r', '\0']) {
        return Err(Error::Invalid(
            "The password contains a line break, which the dump tool's option file cannot hold"
                .into(),
        ));
    }
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The option file that carries what must not be an argument.
pub(crate) fn mysql_option_file(password: Option<&str>) -> Result<String> {
    let mut text = String::from("[client]\n");
    if let Some(password) = password {
        text.push_str(&format!("password=\"{}\"\n", option_file_value(password)?));
    }
    Ok(text)
}

/// The TLS options of MySQL's and MariaDB's command-line programs, from the connection's
/// own setting. The same for the tool that dumps and the client that loads a backup.
pub(crate) fn mysql_tls_args(profile: &ConnectionProfile, client: Client) -> Vec<String> {
    let mut args = Vec::new();
    match client {
        Client::Mariadb => match profile.tls {
            TlsMode::Disable => args.push("--skip-ssl".into()),
            TlsMode::Prefer => {}
            TlsMode::Require | TlsMode::VerifyCa => args.push("--ssl".into()),
            TlsMode::VerifyFull => {
                args.push("--ssl".into());
                args.push("--ssl-verify-server-cert".into());
            }
        },
        _ => args.push(format!(
            "--ssl-mode={}",
            match profile.tls {
                TlsMode::Disable => "DISABLED",
                TlsMode::Prefer => "PREFERRED",
                TlsMode::Require => "REQUIRED",
                TlsMode::VerifyCa => "VERIFY_CA",
                TlsMode::VerifyFull => "VERIFY_IDENTITY",
            }
        )),
    }
    if let Some(ca) = &profile.tls_ca_path {
        if matches!(profile.tls, TlsMode::VerifyCa | TlsMode::VerifyFull) {
            args.push(format!("--ssl-ca={ca}"));
        }
    }
    args
}

/// `mysqldump`'s arguments. `option_file` is read instead of the user's own option files,
/// so nothing in `~/.my.cnf` changes where the dump comes from.
pub fn mysqldump_args(
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
        // One consistent snapshot of the transactional tables, without locking them.
        "--single-transaction".into(),
        "--quick".into(),
        "--routines".into(),
        "--triggers".into(),
        "--events".into(),
        "--hex-blob".into(),
        "--default-character-set=utf8mb4".into(),
    ];
    args.extend(mysql_tls_args(profile, tool.client));
    if tool.client != Client::Mariadb {
        // A dump meant to be restored anywhere: no GTID state of this server in it.
        args.push("--set-gtid-purged=OFF".into());
        if tool.major >= 8 {
            // Needs no PROCESS privilege; and no query for statistics the server may lack.
            args.push("--no-tablespaces".into());
            args.push("--column-statistics=0".into());
        }
    }
    // After `--`, a database whose name starts with a dash is still a name.
    args.push("--".into());
    args.push(database.to_string());
    args
}

/// `pg_dump`'s arguments. `file` is given for the custom format, which `pg_dump` writes
/// itself; a SQL dump comes on standard output.
pub fn pg_dump_args(
    profile: &ConnectionProfile,
    database: &str,
    format: Format,
    file: Option<&Path>,
) -> Vec<String> {
    let mut args = vec![
        format!("--host={}", profile.host),
        format!("--port={}", profile.port),
        format!("--username={}", profile.username),
        "--no-password".into(),
        format!("--dbname={database}"),
        format!(
            "--format={}",
            match format {
                Format::Sql => "plain",
                Format::Custom => "custom",
            }
        ),
    ];
    if let Some(file) = file {
        args.push(format!("--file={}", file.display()));
    }
    args
}

/// The environment `pg_dump` and `pg_restore` run in: the password and the TLS policy, and
/// nothing of the user's own `PG*` variables.
pub(crate) fn pg_env(
    profile: &ConnectionProfile,
    password: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut env = vec![
        (
            "PGSSLMODE",
            match profile.tls {
                TlsMode::Disable => "disable",
                TlsMode::Prefer => "prefer",
                TlsMode::Require => "require",
                TlsMode::VerifyCa => "verify-ca",
                TlsMode::VerifyFull => "verify-full",
            }
            .to_string(),
        ),
        (
            "PGCONNECT_TIMEOUT",
            profile.connect_timeout_secs.to_string(),
        ),
        ("PGAPPNAME", "PuriCat backup".to_string()),
        ("LC_MESSAGES", "C".to_string()),
    ];
    if let Some(password) = password {
        env.push(("PGPASSWORD", password.to_string()));
    }
    if let Some(ca) = &profile.tls_ca_path {
        env.push(("PGSSLROOTCERT", ca.clone()));
    }
    env
}

// ---- a run -----------------------------------------------------------------------------

/// A directory only this user can enter, removed with everything in it when dropped.
pub(crate) struct PrivateDir(pub(crate) PathBuf);

impl PrivateDir {
    pub(crate) fn new() -> Result<Self> {
        let path =
            std::env::temp_dir().join(format!("puricat-backup-{}", uuid::Uuid::new_v4().simple()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        Ok(PrivateDir(path))
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A file that only its owner can read, which must not exist yet.
pub(crate) fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// The unfinished file: removed unless the run reaches the end.
struct Partial {
    path: PathBuf,
    keep: bool,
}

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

enum Sink {
    Plain(std::fs::File),
    Gzip(Box<flate2::write::GzEncoder<std::fs::File>>),
}

impl Sink {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Sink::Plain(f) => f.write_all(bytes),
            Sink::Gzip(g) => g.write_all(bytes),
        }
    }

    fn finish(self) -> std::io::Result<std::fs::File> {
        match self {
            Sink::Plain(f) => Ok(f),
            Sink::Gzip(g) => g.finish(),
        }
    }
}

fn stopped() -> Error {
    Error::Conflict("The backup was stopped. No file was kept.".into())
}

/// What the tool said when it failed, without the lines that only repeat a warning.
fn tool_failure(tool: &DumpTool, status: std::process::ExitStatus, stderr: &[u8]) -> Error {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    let name = Path::new(&tool.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Error::Database(if text.is_empty() {
        format!("{name} ended with {status} and said nothing. No file was kept.")
    } else {
        format!("{name} failed: {text}")
    })
}

/// Whether the end of a SQL dump is the tool's own last line. A dump that was cut short
/// (a lost connection, a full disk on the way) does not have it.
pub fn dump_is_complete(client: Client, tail: &[u8]) -> bool {
    let tail = String::from_utf8_lossy(tail);
    match client {
        Client::Postgres => tail.contains("PostgreSQL database dump complete"),
        Client::Mysql | Client::Mariadb => tail.contains("-- Dump completed"),
    }
}

/// Back one database up into `request.destination`. `on_progress` is told how many bytes of
/// the dump exist so far (before compression, for a SQL dump). Setting `stop` ends the tool; nothing is kept then, nor after any failure.
pub async fn run(
    profile: &ConnectionProfile,
    password: Option<&str>,
    tool: &DumpTool,
    request: &BackupRequest,
    stop: &AtomicBool,
    mut on_progress: impl FnMut(u64),
) -> Result<BackupOutcome> {
    if !matches!(profile.engine, Engine::Mysql | Engine::Postgres) {
        return Err(Error::Unsupported(
            "Backups are available for MySQL and PostgreSQL connections.".into(),
        ));
    }
    if request.format == Format::Custom && profile.engine != Engine::Postgres {
        return Err(Error::Invalid(
            "The custom archive format is PostgreSQL's".into(),
        ));
    }
    if request.database.is_empty() || request.database.contains('\0') {
        return Err(Error::Invalid("A database name is needed".into()));
    }
    if !request.destination.is_dir() {
        return Err(Error::Invalid(format!(
            "The folder {} does not exist.",
            request.destination.display()
        )));
    }

    let started = Instant::now();
    let name = file_name(
        &profile.name,
        &request.database,
        chrono::Local::now().naive_local(),
        request.extension(),
    );
    let target = request.destination.join(&name);
    if target.exists() {
        return Err(Error::Conflict(format!(
            "{} already exists.",
            target.display()
        )));
    }
    let mut partial = Partial {
        path: request.destination.join(format!(".{name}.partial")),
        keep: false,
    };
    let _ = std::fs::remove_file(&partial.path);

    let private = PrivateDir::new()?;
    let mut cmd = Command::new(&tool.path);
    cmd.env_clear()
        .env("PATH", path_with(Path::new(&tool.path).parent()))
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    // For the custom format pg_dump writes the file itself (it can then seek in it).
    let streamed = request.format == Format::Sql;
    match profile.engine {
        Engine::Mysql => {
            let option_file = private.0.join("client.cnf");
            create_private(&option_file)?.write_all(mysql_option_file(password)?.as_bytes())?;
            cmd.args(mysqldump_args(
                profile,
                tool,
                &option_file,
                &request.database,
            ));
        }
        _ => {
            cmd.args(pg_dump_args(
                profile,
                &request.database,
                request.format,
                (!streamed).then_some(partial.path.as_path()),
            ));
            cmd.envs(pg_env(profile, password));
        }
    }
    cmd.stdout(if streamed {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut sink = if streamed {
        let file = create_private(&partial.path)?;
        Some(if request.compress {
            Sink::Gzip(Box::new(flate2::write::GzEncoder::new(
                file,
                flate2::Compression::default(),
            )))
        } else {
            Sink::Plain(file)
        })
    } else {
        None
    };

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Unsupported(format!("{} could not be started: {e}", tool.path)))?;
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let errors = tokio::spawn(async move {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            // The first lines say what went wrong; a flood after them adds nothing.
            let room = STDERR_KEPT.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..n.min(room)]);
        }
        kept
    });

    let mut written = 0u64;
    let mut tail: Vec<u8> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    if let Some(mut stdout) = child.stdout.take() {
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            tokio::select! {
                read = stdout.read(&mut chunk) => {
                    let n = read?;
                    if n == 0 {
                        break;
                    }
                    sink.as_mut().expect("a streamed dump has a sink").write_all(&chunk[..n])?;
                    written += n as u64;
                    tail.extend_from_slice(&chunk[..n]);
                    if tail.len() > TAIL_KEPT {
                        tail.drain(..tail.len() - TAIL_KEPT);
                    }
                    on_progress(written);
                }
                _ = tick.tick() => {
                    if stop.load(Ordering::Relaxed) {
                        let _ = child.kill().await;
                        return Err(stopped());
                    }
                }
            }
        }
    }
    let status = loop {
        tokio::select! {
            status = child.wait() => break status?,
            _ = tick.tick() => {
                if stop.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    return Err(stopped());
                }
                if !streamed {
                    if let Ok(meta) = std::fs::metadata(&partial.path) {
                        on_progress(meta.len());
                    }
                }
            }
        }
    };
    let stderr = errors.await.unwrap_or_default();
    if !status.success() {
        return Err(tool_failure(tool, status, &stderr));
    }

    if let Some(sink) = sink.take() {
        if !dump_is_complete(tool.client, &tail) {
            return Err(Error::Database(
                "The dump tool ended without its closing line: the backup is incomplete. No file was kept.".into(),
            ));
        }
        sink.finish()?.sync_all()?;
    } else {
        // pg_dump created the file with the process's default permissions.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&partial.path, std::fs::Permissions::from_mode(0o600))?;
        }
        list_archive(profile, tool, &partial.path).await?;
    }

    std::fs::rename(&partial.path, &target)?;
    partial.keep = true;
    let bytes = std::fs::metadata(&target)?.len();
    if !streamed {
        on_progress(bytes);
    }
    Ok(BackupOutcome {
        file: target,
        bytes,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// Have `pg_restore` read the archive's table of contents. A file that is not an archive,
/// or whose front is damaged, does not list; a cut further in, inside the data, still does
/// (`live_restore` shows it), so this is a check of the file's form, not of all its data.
pub(crate) async fn list_archive(
    profile: &ConnectionProfile,
    tool: &DumpTool,
    archive: &Path,
) -> Result<()> {
    let pg_restore = Path::new(&tool.path).with_file_name("pg_restore");
    if !pg_restore.is_file() {
        return Err(Error::Unsupported(format!(
            "pg_restore was not found next to {}; the archive could not be checked. No file was kept.",
            tool.path
        )));
    }
    let mut cmd = Command::new(&pg_restore);
    cmd.env_clear()
        .env("PATH", path_with(pg_restore.parent()))
        .envs(pg_env(profile, None))
        .arg("--list")
        .arg(archive)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(120), cmd.output())
        .await
        .map_err(|_| {
            Error::Database(
                "pg_restore did not finish listing the archive. No file was kept.".into(),
            )
        })??;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Database(format!(
            "The archive did not pass pg_restore's check, so it was not kept: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
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
    fn reads_the_version_lines_of_the_tools() {
        let v = |engine, line| parse_tool_version(engine, line);
        assert_eq!(
            v(
                Engine::Mysql,
                "mysqldump  Ver 9.3.0 for macos15.2 on arm64 (Homebrew)"
            ),
            Some((Client::Mysql, "9.3.0".into()))
        );
        assert_eq!(
            v(
                Engine::Mysql,
                "mysqldump  Ver 8.0.36 for Linux on x86_64 (MySQL Community Server - GPL)"
            ),
            Some((Client::Mysql, "8.0.36".into()))
        );
        assert_eq!(
            v(
                Engine::Mysql,
                "mysqldump  Ver 10.19 Distrib 10.11.6-MariaDB, for osx10.19 (arm64)"
            ),
            Some((Client::Mariadb, "10.11.6".into()))
        );
        assert_eq!(
            v(
                Engine::Mysql,
                "mariadb-dump from 11.4.2-MariaDB, client 10.19 for osx10.19 (arm64)"
            ),
            Some((Client::Mariadb, "11.4.2".into()))
        );
        assert_eq!(
            v(Engine::Postgres, "pg_dump (PostgreSQL) 16.13 (Homebrew)"),
            Some((Client::Postgres, "16.13".into()))
        );
        assert_eq!(
            v(Engine::Postgres, "pg_dump (PostgreSQL) 17.2"),
            Some((Client::Postgres, "17.2".into()))
        );
        assert_eq!(v(Engine::Postgres, "something else"), None);
        assert_eq!(v(Engine::Sqlite, "3.43.2"), None);
    }

    #[test]
    fn reads_what_a_server_is() {
        assert_eq!(
            Server::parse("9.3.0"),
            Server {
                mariadb: false,
                major: 9
            }
        );
        assert_eq!(
            Server::parse("8.0.36-0ubuntu0.22.04.1"),
            Server {
                mariadb: false,
                major: 8
            }
        );
        assert_eq!(
            Server::parse("10.11.6-MariaDB-1:10.11.6+maria~ubu2204"),
            Server {
                mariadb: true,
                major: 10
            }
        );
        assert_eq!(
            Server::parse("16.13 (Homebrew)"),
            Server {
                mariadb: false,
                major: 16
            }
        );
    }

    #[test]
    fn pg_dump_must_not_be_older_than_the_server() {
        let found = [
            tool(Client::Postgres, "16.13"),
            tool(Client::Postgres, "17.2"),
            tool(Client::Postgres, "18.0"),
        ];
        // The same major first; otherwise the newest.
        assert_eq!(
            choose(Engine::Postgres, "17.4", &found)
                .tool
                .unwrap()
                .version,
            "17.2"
        );
        assert_eq!(
            choose(Engine::Postgres, "15.8", &found)
                .tool
                .unwrap()
                .version,
            "18.0"
        );
        let refused = choose(Engine::Postgres, "19.1", &found);
        assert_eq!(refused.tool, None);
        assert!(refused
            .problem
            .unwrap()
            .contains("newest pg_dump found here is 18"));
        assert!(choose(Engine::Postgres, "16.1", &[])
            .problem
            .unwrap()
            .contains("was not found"));
    }

    #[test]
    fn a_mysql_tool_is_chosen_for_what_the_server_is() {
        let both = [
            tool(Client::Mysql, "9.3.0"),
            tool(Client::Mariadb, "11.4.2"),
            tool(Client::Mysql, "8.4.3"),
        ];
        let plain = choose(Engine::Mysql, "8.0.36", &both);
        assert_eq!(
            (plain.tool.unwrap().version.as_str(), plain.warning),
            ("9.3.0", None)
        );
        let maria = choose(Engine::Mysql, "10.11.6-MariaDB", &both);
        assert_eq!(
            (maria.tool.unwrap().client, maria.warning),
            (Client::Mariadb, None)
        );

        // Only the other vendor's tool: it is used, and the user is told.
        let crossed = choose(Engine::Mysql, "10.11.6-MariaDB", &both[..1]);
        assert_eq!(crossed.tool.unwrap().client, Client::Mysql);
        assert!(crossed.warning.unwrap().contains("mariadb-dump"));
        // An older mysqldump than the server.
        let old = choose(Engine::Mysql, "9.3.0", &[tool(Client::Mysql, "8.0.36")]);
        assert!(old.warning.unwrap().contains("older than the server"));

        assert!(choose(Engine::Mysql, "9.3.0", &[])
            .problem
            .unwrap()
            .contains("mysqldump was not found"));
        assert!(choose(Engine::Sqlite, "3.43", &[]).problem.is_some());
    }

    #[test]
    fn the_password_is_in_the_option_file_and_never_an_argument() {
        let p = profile(json!({
            "name": "Shop", "engine": "mysql", "host": "db.example", "port": 3307, "username": "app",
            "tls": "verify-full", "tlsCaPath": "/certs/ca.pem",
        }));
        let args = mysqldump_args(
            &p,
            &tool(Client::Mysql, "9.3.0"),
            Path::new("/private/x/client.cnf"),
            "-odd name",
        );
        assert_eq!(args[0], "--defaults-file=/private/x/client.cnf");
        assert!(args.contains(&"--single-transaction".to_string()));
        assert!(args.contains(&"--ssl-mode=VERIFY_IDENTITY".to_string()));
        assert!(args.contains(&"--ssl-ca=/certs/ca.pem".to_string()));
        assert!(args.contains(&"--column-statistics=0".to_string()));
        // The database is the last argument, after `--`: a leading dash is not an option.
        assert_eq!(&args[args.len() - 2..], ["--", "-odd name"]);
        assert!(!args.iter().any(|a| a.contains("password")));

        assert_eq!(
            mysql_option_file(Some(r#"p"a\ss"#)).unwrap(),
            "[client]\npassword=\"p\\\"a\\\\ss\"\n"
        );
        assert_eq!(mysql_option_file(None).unwrap(), "[client]\n");
        assert!(mysql_option_file(Some("two\nlines")).is_err());
    }

    #[test]
    fn mysqldump_options_follow_the_client() {
        let p = profile(
            json!({ "name": "x", "engine": "mysql", "host": "h", "port": 3306, "username": "u", "tls": "require" }),
        );
        let maria = mysqldump_args(&p, &tool(Client::Mariadb, "11.4.2"), Path::new("/c"), "db");
        assert!(maria.contains(&"--ssl".to_string()));
        // Options MariaDB's tool does not have.
        for absent in [
            "--ssl-mode",
            "--set-gtid-purged",
            "--column-statistics",
            "--no-tablespaces",
        ] {
            assert!(!maria.iter().any(|a| a.starts_with(absent)), "{absent}");
        }
        let old = mysqldump_args(&p, &tool(Client::Mysql, "5.7.44"), Path::new("/c"), "db");
        assert!(old.contains(&"--ssl-mode=REQUIRED".to_string()));
        assert!(!old
            .iter()
            .any(|a| a.starts_with("--column-statistics") || a == "--no-tablespaces"));
    }

    #[test]
    fn pg_dump_gets_the_password_and_tls_through_the_environment() {
        let p = profile(json!({
            "name": "x", "engine": "postgres", "host": "pg.example", "port": 5433, "username": "app",
            "tls": "verify-ca", "tlsCaPath": "/certs/ca.pem",
        }));
        let args = pg_dump_args(&p, "shop", Format::Custom, Some(Path::new("/b/.x.partial")));
        assert_eq!(
            args,
            [
                "--host=pg.example",
                "--port=5433",
                "--username=app",
                "--no-password",
                "--dbname=shop",
                "--format=custom",
                "--file=/b/.x.partial"
            ]
        );
        assert_eq!(
            pg_dump_args(&p, "shop", Format::Sql, None).last().unwrap(),
            "--format=plain"
        );
        let env = pg_env(&p, Some("s3cret"));
        assert!(env.contains(&("PGPASSWORD", "s3cret".to_string())));
        assert!(env.contains(&("PGSSLMODE", "verify-ca".to_string())));
        assert!(env.contains(&("PGSSLROOTCERT", "/certs/ca.pem".to_string())));
        assert!(!pg_env(&p, None).iter().any(|(k, _)| *k == "PGPASSWORD"));
    }

    #[test]
    fn file_names_are_safe_and_say_what_and_when() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 19)
            .unwrap()
            .and_hms_opt(14, 5, 9)
            .unwrap();
        assert_eq!(
            file_name("Main Shop", "shopdb", at, "sql.gz"),
            "Main-Shop_shopdb_2026-11-19_140509.sql.gz"
        );
        // Thai letters stay; separators and dots at the ends do not make a hidden or odd file.
        assert_eq!(
            file_name("ฐานข้อมูล วัด", "../etc/passwd", at, "dump"),
            "ฐานข้อมูล-วัด_etc-passwd_2026-11-19_140509.dump"
        );
        assert_eq!(
            file_name("  ", "...", at, "sql"),
            "connection_database_2026-11-19_140509.sql"
        );
    }

    #[test]
    fn a_dump_without_its_last_line_is_not_a_backup() {
        assert!(dump_is_complete(
            Client::Mysql,
            b"UNLOCK TABLES;\n-- Dump completed on 2026-11-19 14:05:09\n"
        ));
        assert!(dump_is_complete(
            Client::Mariadb,
            b"\n-- Dump completed on 2026-11-19 14:05:09\n"
        ));
        assert!(!dump_is_complete(
            Client::Mysql,
            b"INSERT INTO `orders` VALUES (1,'SO-1"
        ));
        assert!(dump_is_complete(
            Client::Postgres,
            b"--\n-- PostgreSQL database dump complete\n--\n\n"
        ));
        assert!(!dump_is_complete(
            Client::Postgres,
            b"COPY shop.orders (id) FROM stdin;\n1\n"
        ));
    }
}
