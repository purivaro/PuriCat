//! Reading SQL text: splitting a script into statements and classifying each one.
//!
//! The classification drives conveniences (which statements need a confirmation, which
//! run under the row-impact guard). It is never the only safeguard: read-only sessions are
//! enforced by the server, and the impact guard measures what a statement actually did.

use serde::Serialize;

use crate::model::Engine;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Only reads data (SELECT, SHOW, EXPLAIN without ANALYZE, ...).
    Read,
    /// Changes rows (INSERT, UPDATE, DELETE, REPLACE, MERGE, data-modifying WITH).
    Dml,
    /// Changes the schema or privileges.
    Ddl,
    /// BEGIN / COMMIT / ROLLBACK / SAVEPOINT.
    Transaction,
    /// Everything else (SET, USE, CALL, ...). Treated as "may change something".
    Other,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Statement {
    pub text: String,
    /// Byte offset of the statement in the script.
    pub start: usize,
    /// Byte offset of its first word: where the statement proper begins, after the
    /// comments that lead it.
    pub body: usize,
    pub kind: Kind,
    /// Why this statement deserves a second look, if it does.
    pub danger: Option<&'static str>,
}

/// Walk the script outside of strings and comments, reporting keywords-like words with
/// their parenthesis depth and every statement-ending semicolon.
fn scan(
    engine: Engine,
    sql: &str,
    mut on_word: impl FnMut(&str, usize, usize),
    mut on_end: impl FnMut(usize),
) {
    let bytes = sql.as_bytes();
    let mysql = engine == Engine::Mysql;
    let sqlite = engine == Engine::Sqlite;
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            // SQLite accepts MySQL's backticks around a name.
            b'\'' | b'"' | b'`' if c != b'`' || mysql || sqlite => {
                // PostgreSQL treats backslashes literally unless the string is E'...';
                // SQLite always does.
                let escapes = c != b'`'
                    && !sqlite
                    && (mysql || (c == b'\'' && i > 0 && matches!(bytes[i - 1], b'e' | b'E')));
                i += 1;
                while i < bytes.len() {
                    if escapes && bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == c {
                        if bytes.get(i + 1) == Some(&c) {
                            i += 2; // doubled quote
                        } else {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                i += 1;
            }
            // MySQL starts a `--` comment only when a space or a control character follows
            // the dashes (its lexer asks `isspace || iscntrl`); `5--1` is a subtraction there.
            b'-' if bytes.get(i + 1) == Some(&b'-')
                && (!mysql
                    || bytes
                        .get(i + 2)
                        .is_none_or(|b| b.is_ascii_whitespace() || b.is_ascii_control())) =>
            {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'#' if mysql => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // SQLite also accepts SQL Server's brackets around a name; nothing escapes in them.
            b'[' if sqlite => {
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                // PostgreSQL block comments nest; the others end at the first `*/`.
                let nests = !mysql && !sqlite;
                let mut level = 1;
                i += 2;
                while i < bytes.len() && level > 0 {
                    if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        level -= 1;
                        i += 2;
                    } else if nests && bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        level += 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'$' if !mysql && !sqlite => {
                // Dollar-quoted string: $tag$ ... $tag$. `$1` is a parameter, not a quote.
                let tag_end = (i + 1..bytes.len())
                    .find(|&j| !(bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_'));
                match tag_end {
                    Some(end)
                        if bytes[end] == b'$'
                            && !bytes.get(i + 1).is_some_and(u8::is_ascii_digit) =>
                    {
                        let tag = &sql[i..=end];
                        i = match sql[end + 1..].find(tag) {
                            Some(close) => end + 1 + close + tag.len(),
                            None => bytes.len(),
                        };
                    }
                    _ => i += 1,
                }
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b';' => {
                on_end(i);
                depth = 0;
                i += 1;
            }
            _ if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                on_word(&sql[start..i], depth, start);
            }
            _ => i += 1,
        }
    }
}

/// Split a script on semicolons that are not inside strings, comments or dollar quotes.
/// Statements that contain nothing but whitespace and comments are dropped.
pub fn split(engine: Engine, sql: &str) -> Vec<Statement> {
    let mut ends = Vec::new();
    if engine == Engine::Sqlite {
        ends = sqlite_ends(sql);
    } else {
        scan(engine, sql, |_, _, _| {}, |at| ends.push(at));
    }
    ends.push(sql.len());

    let mut out = Vec::new();
    let mut from = 0;
    for end in ends {
        let piece = &sql[from..end];
        let mut first_word = None;
        scan(
            engine,
            piece,
            |_, _, at| {
                first_word.get_or_insert(at);
            },
            |_| {},
        );
        if let Some(word) = first_word {
            let lead = piece.len() - piece.trim_start().len();
            let text = piece.trim().to_string();
            let (kind, danger) = classify(engine, &text);
            out.push(Statement {
                text,
                start: from + lead,
                body: from + word,
                kind,
                danger,
            });
        }
        from = (end + 1).min(sql.len());
    }
    out
}

/// Where the statements of a SQLite script end. A trigger's body has statements of its own,
/// each closed by a semicolon: `CREATE TRIGGER` runs on to the `END` that stands alone
/// between two semicolons, the rule SQLite's own `sqlite3_complete()` applies.
fn sqlite_ends(sql: &str) -> Vec<usize> {
    use std::cell::RefCell;
    // The words since the last semicolon: the first three, and how many there were.
    let piece: RefCell<(Vec<String>, usize)> = RefCell::new((Vec::new(), 0));
    let mut in_trigger = false;
    let mut ends = Vec::new();
    scan(
        Engine::Sqlite,
        sql,
        |word, _, _| {
            let mut piece = piece.borrow_mut();
            piece.1 += 1;
            if piece.0.len() < 3 {
                piece.0.push(word.to_ascii_uppercase());
            }
        },
        |at| {
            let (words, count) = piece.replace((Vec::new(), 0));
            let word = |i: usize| words.get(i).map(String::as_str);
            if in_trigger {
                // Still inside the body unless this piece is the closing END.
                if count != 1 || word(0) != Some("END") {
                    return;
                }
                in_trigger = false;
            } else if word(0) == Some("CREATE")
                && (word(1) == Some("TRIGGER")
                    || (matches!(word(1), Some("TEMP" | "TEMPORARY"))
                        && word(2) == Some("TRIGGER")))
            {
                in_trigger = true;
                return;
            }
            ends.push(at);
        },
    );
    ends
}

const DML: &[&str] = &["INSERT", "UPDATE", "DELETE", "REPLACE", "MERGE"];

pub fn classify(engine: Engine, statement: &str) -> (Kind, Option<&'static str>) {
    let mut words: Vec<(String, usize)> = Vec::new();
    scan(
        engine,
        statement,
        |w, depth, _| words.push((w.to_ascii_uppercase(), depth)),
        |_| {},
    );
    let Some((first, _)) = words.first() else {
        return (Kind::Other, None);
    };
    let top = |name: &str| words.iter().any(|(w, depth)| *depth == 0 && w == name);
    let anywhere = |names: &[&str]| words.iter().any(|(w, _)| names.contains(&w.as_str()));

    match first.as_str() {
        "SELECT" | "TABLE" | "VALUES" | "SHOW" | "DESCRIBE" | "DESC" => {
            // SELECT ... INTO creates a table (PostgreSQL) or writes a file (MySQL).
            if first == "SELECT" && top("INTO") {
                (Kind::Other, None)
            } else {
                (Kind::Read, None)
            }
        }
        // EXPLAIN ANALYZE really executes the statement it explains.
        "EXPLAIN" if top("ANALYZE") && anywhere(DML) => (Kind::Dml, None),
        "EXPLAIN" => (Kind::Read, None),
        // PostgreSQL allows data-modifying statements inside WITH.
        "WITH" if anywhere(DML) => (Kind::Dml, None),
        "WITH" => (Kind::Read, None),
        "UPDATE" | "DELETE" if !top("WHERE") => (
            Kind::Dml,
            Some("No WHERE clause: every row of the table is affected"),
        ),
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "MERGE" => (Kind::Dml, None),
        "DROP" => (Kind::Ddl, Some("Drops an object, with everything in it")),
        "TRUNCATE" => (Kind::Ddl, Some("Removes every row of the table")),
        "ALTER" if top("DROP") => (
            Kind::Ddl,
            Some("Drops part of an object (a column, constraint or partition)"),
        ),
        "CREATE" | "ALTER" | "RENAME" | "COMMENT" | "GRANT" | "REVOKE" => (Kind::Ddl, None),
        "BEGIN" | "START" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "END" | "ABORT" => {
            (Kind::Transaction, None)
        }
        _ => (Kind::Other, None),
    }
}

/// Functions a read-only transaction does not hold back: ending other sessions, running
/// SQL over another connection, reading or writing files on the server.
const OUTSIDE_EFFECTS: &[&str] = &[
    "pg_terminate_backend",
    "pg_cancel_backend",
    "dblink",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "lo_export",
    "lo_import",
    "pg_reload_conf",
    "load_file",
    "load_extension",
];

/// The one statement a read-only tool may run, or why the text is refused.
///
/// This reads the text, so it is a first filter and not the guarantee: the caller also has
/// the server confirm that the text is a single statement, and runs it in a read-only
/// transaction on a connection that is closed afterwards.
pub fn single_read(engine: Engine, sql: &str) -> std::result::Result<Statement, &'static str> {
    // MySQL and MariaDB execute what is inside these comments; a reader skips it.
    if engine == Engine::Mysql && (sql.contains("/*!") || sql.contains("/*M!")) {
        return Err("Version comments (/*! ... */) are not accepted here");
    }
    // SQLite prepares a whole script, so the server cannot be asked to confirm that the text
    // is one statement. A statement cannot follow another without a semicolon between them:
    // the text may end with one, and hold no other, not even inside a string.
    if engine == Engine::Sqlite && sql.trim_end().trim_end_matches(';').contains(';') {
        return Err("A semicolon inside the statement is not accepted here");
    }
    let mut statements = split(engine, sql);
    if statements.len() != 1 {
        return Err("Exactly one statement is accepted");
    }
    let statement = statements.remove(0);
    if statement.kind != Kind::Read {
        return Err(
            "Only a statement that reads is accepted (SELECT, WITH, SHOW, EXPLAIN, DESCRIBE, TABLE, VALUES)",
        );
    }
    // Locking reads hold locks other sessions wait on.
    let mut words: Vec<String> = Vec::new();
    scan(
        engine,
        &statement.text,
        |w, _, _| words.push(w.to_ascii_uppercase()),
        |_| {},
    );
    let follows = |first: &str, next: &[&str]| {
        words
            .windows(2)
            .any(|pair| pair[0] == first && next.contains(&pair[1].as_str()))
    };
    if follows("FOR", &["UPDATE", "SHARE", "NO", "KEY"]) || follows("LOCK", &["IN"]) {
        return Err("Locking reads (FOR UPDATE, FOR SHARE) are not accepted here");
    }
    // INTO writes somewhere: a table, a variable, or a file on the server (`TABLE t INTO
    // OUTFILE` is not a SELECT, and a read-only transaction does not stop it).
    if words.iter().any(|w| w == "INTO") {
        return Err("INTO is not accepted here");
    }
    // Functions that act outside the transaction or read the server's files. This catches
    // the plain spelling only (an escaped identifier gets past it), so it is a courtesy
    // and not a boundary: what the account may do is decided by the server.
    let lowered = statement.text.to_ascii_lowercase();
    if OUTSIDE_EFFECTS.iter().any(|name| lowered.contains(name)) {
        return Err("That function acts outside the query and is not accepted here");
    }
    Ok(statement)
}

/// The query of a stored `CREATE VIEW … AS <query>` statement: what follows the first `AS`
/// that stands outside parentheses, strings and comments (a column list has its own
/// parentheses, and a quoted name is not a word). `None` when there is no such `AS`.
pub fn view_query(engine: Engine, statement: &str) -> Option<&str> {
    let mut after = None;
    scan(
        engine,
        statement,
        |word, depth, start| {
            if after.is_none() && depth == 0 && word.eq_ignore_ascii_case("AS") {
                after = Some(start + word.len());
            }
        },
        |_| {},
    );
    let query = statement[after?..].trim().trim_end_matches(';').trim_end();
    (!query.is_empty()).then_some(query)
}

/// Is this `CREATE TEMPORARY …` / `DROP TEMPORARY …`? Judged by position, so a table that
/// happens to be called `temporary` is not mistaken for one.
pub fn is_temporary_ddl(engine: Engine, statement: &str) -> bool {
    let mut words = 0;
    let mut second = false;
    scan(
        engine,
        statement,
        |w, _, _| {
            words += 1;
            second |= words == 2 && w.eq_ignore_ascii_case("TEMPORARY");
        },
        |_| {},
    );
    second
}

/// Does a transaction-control statement leave the session inside a transaction?
/// `None` when the statement does not change that (SAVEPOINT, ROLLBACK TO, ...).
///
/// The statement is read the way the server reads it: a leading comment is not a keyword.
pub fn transaction_effect(engine: Engine, statement: &str) -> Option<bool> {
    let mut words: Vec<String> = Vec::new();
    scan(
        engine,
        statement,
        |w, _, _| words.push(w.to_ascii_uppercase()),
        |_| {},
    );
    let has = |name: &str| words.iter().any(|w| w == name);
    // COMMIT AND CHAIN / ROLLBACK AND CHAIN end one transaction and open the next.
    let chains = has("CHAIN") && !has("NO");
    match words.first()?.as_str() {
        "BEGIN" => Some(true),
        // START TRANSACTION, but not START REPLICA and friends.
        "START" => (words.get(1).map(String::as_str) == Some("TRANSACTION")).then_some(true),
        "COMMIT" | "END" | "ABORT" => Some(chains),
        "ROLLBACK" => (!has("TO")).then_some(chains),
        _ => None,
    }
}

/// Replace password literals before a statement is written to the local history.
/// It errs on the side of hiding too much: any quoted literal (single quotes, or the double
/// quotes MySQL also accepts) shortly after the word `password` or `identified by/with` is
/// masked.
pub fn redact(sql: &str) -> String {
    let lower = sql.to_ascii_lowercase();
    // Byte ranges of the literal contents to mask.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for marker in ["password", "identified by", "identified with"] {
        let mut from = 0;
        while let Some(found) = lower[from..].find(marker) {
            let at = from + found;
            let mut after = at + marker.len();
            from = after;
            // The word itself may be a quoted name ("password" = 'x'): its closing quote
            // does not open the literal.
            let bytes = lower.as_bytes();
            if at > 0
                && matches!(bytes[at - 1], b'"' | b'`')
                && bytes.get(after) == Some(&bytes[at - 1])
            {
                after += 1;
            }
            let Some(open) = lower[after..].find(['\'', '"']).filter(|&gap| gap <= 48) else {
                continue;
            };
            let quote = bytes[after + open] as char;
            let start = after + open + 1;
            if let Some(len) = sql[start..].find(quote) {
                spans.push((start, start + len));
                from = start + len;
            }
        }
    }
    spans.sort_unstable();
    let mut out = String::with_capacity(sql.len());
    let mut copied = 0;
    for (start, end) in spans {
        if start < copied {
            continue; // overlaps a literal already masked
        }
        out.push_str(&sql[copied..start]);
        out.push_str("***");
        copied = end;
    }
    out.push_str(&sql[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(engine: Engine, sql: &str) -> Vec<String> {
        split(engine, sql).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn a_read_tool_accepts_one_reading_statement_only() {
        for (engine, sql) in [
            (
                Engine::Mysql,
                "SELECT id, total FROM orders WHERE total > 10 LIMIT 5",
            ),
            (Engine::Mysql, "SELECT 1; -- trailing comment"),
            (Engine::Mysql, "SHOW CREATE TABLE orders"),
            (Engine::Mysql, "EXPLAIN SELECT * FROM orders"),
            (
                Engine::Postgres,
                "WITH t AS (SELECT 1 AS n) SELECT n FROM t",
            ),
            (Engine::Postgres, "/* why */ SELECT 'for update' AS words"),
            (Engine::Postgres, "TABLE orders"),
        ] {
            assert!(single_read(engine, sql).is_ok(), "{sql}");
        }
        for (engine, sql) in [
            (Engine::Mysql, ""),
            (Engine::Mysql, "-- nothing"),
            (Engine::Mysql, "SELECT 1; SELECT 2"),
            (Engine::Mysql, "SELECT 1; DELETE FROM orders"),
            (Engine::Mysql, "DELETE FROM orders"),
            (Engine::Mysql, "UPDATE orders SET total = 0 WHERE id = 1"),
            (Engine::Mysql, "SELECT * FROM orders INTO OUTFILE '/tmp/x'"),
            (Engine::Mysql, "TABLE orders INTO OUTFILE '/tmp/x'"),
            (Engine::Mysql, "SELECT COUNT(*) FROM orders INTO @n"),
            (Engine::Mysql, "SELECT LOAD_FILE('/etc/passwd')"),
            (
                Engine::Postgres,
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity",
            ),
            (Engine::Postgres, "SELECT \"pg_read_file\"('/etc/passwd')"),
            (
                Engine::Postgres,
                "SELECT * FROM dblink('dbname=x', 'DELETE FROM orders') AS t(n int)",
            ),
            (Engine::Mysql, "SELECT 1 /*! ; DELETE FROM orders */"),
            (Engine::Mysql, "SELECT 1 /*M!100100 ; DELETE FROM orders */"),
            (Engine::Mysql, "SELECT * FROM orders FOR UPDATE"),
            (Engine::Mysql, "SELECT * FROM orders LOCK IN SHARE MODE"),
            (Engine::Mysql, "SET SESSION TRANSACTION READ WRITE"),
            (Engine::Mysql, "CALL refresh_totals()"),
            (Engine::Postgres, "COMMIT; DELETE FROM orders"),
            (
                Engine::Postgres,
                "WITH gone AS (DELETE FROM orders RETURNING id) SELECT * FROM gone",
            ),
            (Engine::Postgres, "SELECT * INTO copy FROM orders"),
            (Engine::Postgres, "SELECT * FROM orders FOR NO KEY UPDATE"),
            (Engine::Postgres, "SELECT * FROM orders FOR SHARE"),
            (Engine::Postgres, "EXPLAIN ANALYZE DELETE FROM orders"),
            (Engine::Postgres, "SET default_transaction_read_only = off"),
            (Engine::Postgres, "DO $$ BEGIN DELETE FROM orders; END $$"),
            (Engine::Postgres, "COPY orders TO PROGRAM 'sh'"),
        ] {
            assert!(single_read(engine, sql).is_err(), "{sql}");
        }
    }

    #[test]
    fn splits_on_real_semicolons_only() {
        let sql = "SELECT 1; -- a; comment\nSELECT ';' AS a, \"b;c\" /* ; */ FROM t;\n\n  ;  UPDATE t SET a = 'it''s; fine' WHERE id = 1";
        assert_eq!(
            texts(Engine::Postgres, sql),
            vec![
                "SELECT 1",
                "-- a; comment\nSELECT ';' AS a, \"b;c\" /* ; */ FROM t",
                "UPDATE t SET a = 'it''s; fine' WHERE id = 1",
            ]
        );
        assert!(split(Engine::Postgres, " ; -- nothing\n /* still nothing */ ;").is_empty());
        let second = &split(Engine::Postgres, "SELECT 1;\n  SELECT 2")[1];
        assert_eq!((second.start, second.text.as_str()), (12, "SELECT 2"));
        assert_eq!(second.body, 12);
        // A statement starts with the comments that lead it; its body is its first word.
        let sql = "SELECT 'ก'; -- เสร็จ\n/* สอง */ (SELECT 2)";
        let second = &split(Engine::Mysql, sql)[1];
        assert_eq!(&sql[second.start..second.start + 2], "--");
        assert_eq!(&sql[second.body..], "SELECT 2)");
    }

    #[test]
    fn respects_each_dialect() {
        // Backticks, # comments and backslash escapes are MySQL; `--x` is not a comment there.
        let mysql = "SELECT `a;b`, 'x\\';y' FROM t # c;\n; SELECT 5--1";
        assert_eq!(
            texts(Engine::Mysql, mysql),
            vec!["SELECT `a;b`, 'x\\';y' FROM t # c;", "SELECT 5--1"]
        );
        // Dollar quotes, nested comments and literal backslashes are PostgreSQL.
        let pg = "DO $body$ BEGIN PERFORM 1; END $body$; SELECT $$a;b$$, 'c\\'; /* x /* ; */ y */ SELECT $1";
        assert_eq!(
            texts(Engine::Postgres, pg),
            vec![
                "DO $body$ BEGIN PERFORM 1; END $body$",
                "SELECT $$a;b$$, 'c\\'",
                "/* x /* ; */ y */ SELECT $1"
            ]
        );
        assert_eq!(
            texts(Engine::Postgres, "SELECT E'a\\';b'; SELECT 2").len(),
            2
        );
    }

    #[test]
    fn mysql_double_dash_comments_are_read_as_the_server_reads_them() {
        // MySQL's lexer starts a `--` comment when a space or a control character follows
        // the dashes. Read otherwise, the quote after an invisible control character would
        // open a string that hides the second statement on the next line.
        let hidden = "SELECT id FROM t --\u{1}'\n; DROP TABLE t; -- '";
        assert_eq!(
            texts(Engine::Mysql, hidden),
            ["SELECT id FROM t --\u{1}'", "DROP TABLE t"]
        );
        assert!(single_read(Engine::Mysql, hidden).is_err());
        for control in ['\u{0}', '\u{1}', '\u{b}', '\u{1f}', '\u{7f}', '\t', '\r'] {
            let sql = format!("SELECT 1 --{control}'\n; SELECT 2");
            assert_eq!(texts(Engine::Mysql, &sql).len(), 2, "{control:?}");
        }
        // Still not a comment: `--` followed by a letter or a digit is two minus signs.
        assert_eq!(texts(Engine::Mysql, "SELECT 5--1; SELECT --x").len(), 2);
    }

    #[test]
    fn sqlite_scripts_are_read_the_way_sqlite_reads_them() {
        let split = |sql: &str| texts(Engine::Sqlite, sql);
        // Names in double quotes, backticks and brackets; a backslash escapes nothing.
        assert_eq!(
            split("SELECT 'a;b\\'; SELECT \"c;d\", `e;f`, [g;h] FROM t; -- x; y\nSELECT 3"),
            [
                "SELECT 'a;b\\'",
                "SELECT \"c;d\", `e;f`, [g;h] FROM t",
                "-- x; y\nSELECT 3"
            ]
        );
        // A block comment ends at the first `*/`: what follows it is SQL.
        assert_eq!(
            split("SELECT 1 /* /* */ ; DELETE FROM t; -- */"),
            ["SELECT 1 /* /* */", "DELETE FROM t"]
        );
        // `#` and `$` are ordinary characters.
        assert_eq!(split("SELECT $a$; SELECT 2 -- $a$").len(), 2);

        // A trigger runs on to the END that stands alone between two semicolons.
        let script = "CREATE TEMP TRIGGER IF NOT EXISTS t AFTER INSERT ON a BEGIN\n\
                      UPDATE b SET n = CASE WHEN n > 1 THEN 0 ELSE n END;\n\
                      DELETE FROM c; -- END;\n\
                      END; SELECT 1; create trigger u after delete on a begin select 1; end; SELECT 2";
        let statements = split(script);
        assert_eq!(statements.len(), 4, "{statements:?}");
        assert!(statements[0].ends_with("END") && statements[0].contains("DELETE FROM c"));
        assert_eq!(statements[1], "SELECT 1");
        assert_eq!(statements[3], "SELECT 2");
        // A table that is merely called `trigger`, and an unfinished trigger.
        assert_eq!(split("CREATE TABLE trigger (x); SELECT 1").len(), 2);
        assert_eq!(
            split("CREATE TRIGGER t AFTER INSERT ON a BEGIN SELECT 1; SELECT 2").len(),
            1
        );

        let kind = |sql: &str| classify(Engine::Sqlite, sql).0;
        assert_eq!(kind("SELECT 1"), Kind::Read);
        assert_eq!(kind("EXPLAIN QUERY PLAN DELETE FROM t"), Kind::Read);
        assert_eq!(kind("INSERT OR REPLACE INTO t VALUES (1)"), Kind::Dml);
        assert_eq!(kind("REPLACE INTO t VALUES (1)"), Kind::Dml);
        assert_eq!(kind("WITH x AS (SELECT 1) DELETE FROM t"), Kind::Dml);
        assert_eq!(
            kind("CREATE TRIGGER t AFTER INSERT ON a BEGIN DELETE FROM b; END"),
            Kind::Ddl
        );
        assert_eq!(kind("BEGIN IMMEDIATE"), Kind::Transaction);
        for sql in [
            "PRAGMA journal_mode = WAL",
            "VACUUM",
            "ATTACH 'x.db' AS x",
            "ANALYZE",
        ] {
            assert_eq!(kind(sql), Kind::Other, "{sql}");
        }

        // The read-only tool: one reading statement, and no semicolon but a last one.
        for sql in [
            "SELECT 1",
            "SELECT 1;  ",
            "WITH t AS (SELECT 1) SELECT * FROM t;",
        ] {
            assert!(single_read(Engine::Sqlite, sql).is_ok(), "{sql}");
        }
        for sql in [
            "SELECT 1; DELETE FROM t",
            "SELECT ';'",
            "SELECT 1 /* ; */",
            "SELECT [a;b] FROM t",
            "PRAGMA table_info(t)",
            "ATTACH 'x.db' AS x",
            "SELECT load_extension('x')",
            "DELETE FROM t RETURNING *",
        ] {
            assert!(single_read(Engine::Sqlite, sql).is_err(), "{sql}");
        }
    }

    #[test]
    fn classifies_by_what_a_statement_does() {
        let kind = |sql: &str| classify(Engine::Postgres, sql).0;
        for read in [
            "select 1",
            "  /* c */ SELECT * FROM t",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "SHOW tables",
            "EXPLAIN DELETE FROM t",
            "VALUES (1)",
        ] {
            assert_eq!(kind(read), Kind::Read, "{read}");
        }
        for dml in [
            "INSERT INTO t VALUES (1)",
            "update t set a = 1 where id = 2",
            "WITH gone AS (DELETE FROM t RETURNING *) SELECT * FROM gone",
            "EXPLAIN ANALYZE UPDATE t SET a = 1",
            "MERGE INTO t USING s ON true WHEN MATCHED THEN DELETE",
        ] {
            assert_eq!(kind(dml), Kind::Dml, "{dml}");
        }
        assert_eq!(kind("CREATE TABLE t (a int)"), Kind::Ddl);
        assert_eq!(kind("begin"), Kind::Transaction);
        assert_eq!(kind("SELECT * INTO backup FROM t"), Kind::Other);
        assert_eq!(kind("CALL do_things()"), Kind::Other);
        // A keyword inside a string or an identifier does not count.
        assert_eq!(
            kind("SELECT 'DELETE FROM t', update_count FROM t"),
            Kind::Read
        );
    }

    #[test]
    fn flags_statements_that_deserve_a_second_look() {
        let danger = |sql: &str| classify(Engine::Mysql, sql).1.is_some();
        assert!(danger("DELETE FROM t"));
        assert!(danger("UPDATE t SET a = (SELECT b FROM s WHERE s.id = 1)")); // WHERE only in a subquery
        assert!(danger("DROP TABLE t"));
        assert!(danger("TRUNCATE t"));
        assert!(danger("ALTER TABLE t DROP COLUMN a"));
        assert!(!danger("DELETE FROM t WHERE id = 1"));
        assert!(!danger("UPDATE t SET note = 'no where here' WHERE id = 1"));
        assert!(!danger("ALTER TABLE t ADD COLUMN dropped_at date"));
        assert!(!danger("SELECT 1"));
    }

    #[test]
    fn tracks_transactions() {
        let effect = |sql: &str| transaction_effect(Engine::Postgres, sql);
        assert_eq!(effect("BEGIN"), Some(true));
        assert_eq!(effect("start transaction read only"), Some(true));
        assert_eq!(effect("COMMIT"), Some(false));
        assert_eq!(effect("ROLLBACK"), Some(false));
        assert_eq!(effect("ROLLBACK TO SAVEPOINT a"), None);
        assert_eq!(effect("SAVEPOINT a"), None);
        // A comment in front does not hide the statement, and a keyword inside one is not it.
        assert_eq!(effect("-- open it\nBEGIN"), Some(true));
        assert_eq!(effect("/* COMMIT */ begin;"), Some(true));
        assert_eq!(
            transaction_effect(Engine::Mysql, "# done\nCOMMIT"),
            Some(false)
        );
        // AND CHAIN opens the next transaction straight away.
        assert_eq!(effect("COMMIT AND CHAIN"), Some(true));
        assert_eq!(effect("ROLLBACK AND NO CHAIN"), Some(false));
        assert_eq!(transaction_effect(Engine::Mysql, "START REPLICA"), None);
    }

    #[test]
    fn redacts_password_literals_for_the_history() {
        assert_eq!(
            redact("CREATE USER app IDENTIFIED BY 's3cret' ; ALTER ROLE x WITH PASSWORD 'p@ss' VALID UNTIL 'infinity'"),
            "CREATE USER app IDENTIFIED BY '***' ; ALTER ROLE x WITH PASSWORD '***' VALID UNTIL 'infinity'"
        );
        assert_eq!(
            redact("SELECT password FROM users WHERE name = 'bob'"),
            "SELECT password FROM users WHERE name = '***'"
        );
        assert_eq!(
            redact(r#"CREATE USER app IDENTIFIED BY "s3cret""#),
            r#"CREATE USER app IDENTIFIED BY "***""#
        );
        // A quoted column named password does not shield the value that follows it.
        assert_eq!(
            redact(r#"UPDATE users SET "password" = 'hunter2' WHERE id = 1"#),
            r#"UPDATE users SET "password" = '***' WHERE id = 1"#
        );
        assert_eq!(
            redact("UPDATE users SET `password` = 'hunter2'"),
            "UPDATE users SET `password` = '***'"
        );
        assert_eq!(redact("SELECT 1"), "SELECT 1");
        assert!(is_temporary_ddl(
            Engine::Mysql,
            "-- scratch\ncreate temporary table t (a int)"
        ));
        assert!(is_temporary_ddl(Engine::Mysql, "DROP TEMPORARY TABLE t"));
        assert!(!is_temporary_ddl(
            Engine::Mysql,
            "CREATE TABLE temporary (a int) COMMENT 'temporary'"
        ));
    }

    #[test]
    fn the_query_of_a_stored_view() {
        let q = |sql| view_query(Engine::Sqlite, sql);
        assert_eq!(q("CREATE VIEW v AS SELECT 1 AS n"), Some("SELECT 1 AS n"));
        assert_eq!(
            q("create view if not exists \"a as b\" (x, y) as\n  select a, b from t;\n"),
            Some("select a, b from t")
        );
        // An `AS` inside the column list, a string or a comment is not the one.
        assert_eq!(
            q("CREATE VIEW v /* as it was */ AS WITH w AS (SELECT 'x AS y') SELECT * FROM w"),
            Some("WITH w AS (SELECT 'x AS y') SELECT * FROM w")
        );
        assert_eq!(q("CREATE TABLE t (a int)"), None);
        assert_eq!(q("CREATE VIEW v AS ;"), None);
    }
}
