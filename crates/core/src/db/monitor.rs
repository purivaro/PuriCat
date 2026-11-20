//! The sessions on a server, and ending one ([ADR 0024]).
//!
//! What the server reports about each session, read with one query; and the statement that
//! stops a session's query or ends the session. The statement is built here from a number,
//! so the one shown to the user before it runs is the one that runs.
//!
//! [ADR 0024]: ../../../../docs/adr/0024-server-sessions.md

use serde::{Deserialize, Serialize};

use crate::model::Engine;
use crate::{Error, Result};

/// The longest piece of a session's current statement that is read.
pub const MAX_QUERY_CHARS: usize = 4000;

/// One session on the server, as the server reports it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    /// MySQL's connection id, PostgreSQL's process id.
    pub id: u64,
    pub user: Option<String>,
    /// Where the client connects from.
    pub host: Option<String>,
    pub database: Option<String>,
    /// MySQL's `COMMAND` (`Query`, `Sleep`, …); PostgreSQL's `state` (`active`, `idle`, …).
    pub command: Option<String>,
    /// Seconds in that command or state (MySQL), since the current or last statement
    /// started (PostgreSQL).
    pub seconds: Option<i64>,
    /// MySQL's `STATE`; PostgreSQL's wait event.
    pub state: Option<String>,
    /// The statement it runs, or ran last (PostgreSQL), cut at `MAX_QUERY_CHARS`.
    pub query: Option<String>,
    /// PostgreSQL's `application_name`.
    pub client: Option<String>,
    /// The session that read this list.
    pub own: bool,
    /// Doing nothing: MySQL `Sleep`, PostgreSQL `idle`.
    pub idle: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum End {
    /// Stop what the session is running; the session stays.
    Query,
    /// Close the session. A transaction it has open is rolled back by the server.
    Connection,
}

/// The statement that ends `id`'s query or the session itself.
pub fn end_sql(engine: Engine, id: u64, what: End) -> Result<String> {
    if id == 0 {
        return Err(Error::Invalid("There is no session 0".into()));
    }
    match (engine, what) {
        (Engine::Mysql, End::Query) => Ok(format!("KILL QUERY {id}")),
        (Engine::Mysql, End::Connection) => Ok(format!("KILL CONNECTION {id}")),
        (Engine::Postgres, End::Query) => Ok(format!("SELECT pg_cancel_backend({id})")),
        (Engine::Postgres, End::Connection) => Ok(format!("SELECT pg_terminate_backend({id})")),
        _ => Err(Error::Unsupported(
            "The sessions of a server are listed for MySQL and PostgreSQL".into(),
        )),
    }
}

/// MySQL 8.0.22 and later: `performance_schema.processlist`. The `information_schema`
/// table, which MariaDB and older servers have, is the fallback.
pub fn mysql_list_sql(table: &str) -> String {
    format!(
        "SELECT CAST(ID AS CHAR), USER, HOST, DB, COMMAND, CAST(TIME AS CHAR), STATE, \
         LEFT(INFO, {MAX_QUERY_CHARS}), ID = CONNECTION_ID() \
         FROM {table} ORDER BY COMMAND = 'Sleep', TIME DESC, ID"
    )
}

/// Client sessions only: no autovacuum, no WAL writer.
pub fn postgres_list_sql() -> String {
    format!(
        "SELECT pid::text, usename::text, \
           CASE WHEN client_addr IS NOT NULL THEN host(client_addr) \
                WHEN client_port = -1 THEN 'local socket' END, \
           datname::text, state, \
           (EXTRACT(EPOCH FROM now() - COALESCE(query_start, backend_start)))::bigint::text, \
           CASE WHEN wait_event IS NULL THEN NULL ELSE wait_event_type || ': ' || wait_event END, \
           left(query, {MAX_QUERY_CHARS}), application_name, pid = pg_backend_pid() \
         FROM pg_stat_activity WHERE backend_type = 'client backend' \
         ORDER BY state = 'idle', query_start NULLS LAST, pid"
    )
}

/// Turn the text columns both queries return into a `Process`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process(
    engine: Engine,
    id: Option<String>,
    user: Option<String>,
    host: Option<String>,
    database: Option<String>,
    command: Option<String>,
    seconds: Option<String>,
    state: Option<String>,
    query: Option<String>,
    client: Option<String>,
    own: bool,
) -> Option<Process> {
    let id = id?.trim().parse().ok()?;
    let blank = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
    let idle = match engine {
        Engine::Mysql => command.as_deref() == Some("Sleep"),
        _ => command.as_deref() == Some("idle"),
    };
    Some(Process {
        id,
        user: blank(user),
        host: blank(host),
        database: blank(database),
        command: blank(command),
        seconds: seconds.and_then(|s| s.trim().parse().ok()),
        state: blank(state),
        query: blank(query),
        client: blank(client),
        own,
        idle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_statement_is_made_from_a_number() {
        assert_eq!(
            end_sql(Engine::Mysql, 42, End::Query).unwrap(),
            "KILL QUERY 42"
        );
        assert_eq!(
            end_sql(Engine::Mysql, 42, End::Connection).unwrap(),
            "KILL CONNECTION 42"
        );
        assert_eq!(
            end_sql(Engine::Postgres, 7, End::Query).unwrap(),
            "SELECT pg_cancel_backend(7)"
        );
        assert_eq!(
            end_sql(Engine::Postgres, 7, End::Connection).unwrap(),
            "SELECT pg_terminate_backend(7)"
        );
        assert!(end_sql(Engine::Mysql, 0, End::Query).is_err());
        assert!(end_sql(Engine::Sqlite, 1, End::Query).is_err());
        // The direction of the API: an id is a number in JSON, never text.
        assert!(serde_json::from_str::<u64>("\"1; DROP TABLE t\"").is_err());
    }

    #[test]
    fn rows_become_processes() {
        let s = |v: &str| Some(v.to_string());
        let p = process(
            Engine::Mysql,
            s("12"),
            s("app"),
            s("10.0.0.5:51234"),
            None,
            s("Sleep"),
            s("300"),
            s(""),
            None,
            None,
            false,
        )
        .unwrap();
        assert_eq!(
            (p.id, p.idle, p.seconds, p.state.clone(), p.database.clone()),
            (12, true, Some(300), None, None)
        );
        let q = process(
            Engine::Postgres,
            s("4021"),
            s("u"),
            s("local socket"),
            s("shop"),
            s("active"),
            s("-1"),
            None,
            s("SELECT 1"),
            s("psql"),
            true,
        )
        .unwrap();
        assert_eq!(
            (q.idle, q.own, q.seconds, q.client.as_deref()),
            (false, true, Some(-1), Some("psql"))
        );
        assert!(process(
            Engine::Mysql,
            s("x"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false
        )
        .is_none());
    }
}
