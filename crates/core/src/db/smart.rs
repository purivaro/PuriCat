//! Smart Paste: turn rows pasted from a spreadsheet into INSERTs and UPDATEs, matched on a
//! key the user chose.
//!
//! PuriCat does the matching itself and then runs plain parameterised INSERT and UPDATE
//! statements in one transaction. It deliberately does not use `ON DUPLICATE KEY UPDATE`,
//! `ON CONFLICT` or `MERGE`: those decide "same row" by whichever unique index fires, while
//! here only the chosen key does, and the user sees the exact outcome before anything runs.
//!
//! `Draft` holds the pure logic (validation, duplicate detection, classification); the
//! session supplies the catalogue and runs the lookups it asks for.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::write::{self, CellChange, CellValue, ChangeSet, RowInsert, RowUpdate, MAX_STATEMENTS};
use super::{ColumnMeta, TableMeta, TableRef, ValueKind};
use crate::model::Engine;
use crate::{Error, Result};

/// Pasted rows matched per lookup query.
const LOOKUP_CHUNK: usize = 40;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Add every pasted row.
    Insert,
    /// Change rows that exist; pasted rows without a match are skipped.
    Update,
    /// Change rows that exist and add the rest.
    Upsert,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PasteRequest {
    pub mode: Mode,
    /// Target column of each pasted column, in pasted order.
    pub columns: Vec<String>,
    /// Columns that identify a row; must be a primary key or unique index of the table.
    #[serde(default)]
    pub key: Vec<String>,
    /// Columns an existing row may have changed.
    #[serde(default)]
    pub update: Vec<String>,
    /// Row-major values aligned with `columns`; `null` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UniqueKey {
    pub name: String,
    pub columns: Vec<String>,
    pub primary: bool,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RowStatus {
    Insert,
    Update,
    /// Matches a row that already holds exactly these values.
    Unchanged,
    /// No row has this key; nothing is done with it.
    Unmatched,
    MissingKey,
    Duplicate,
    /// Insert mode: a row with this key is already there.
    Exists,
    Invalid,
}

impl RowStatus {
    fn is_problem(self) -> bool {
        matches!(
            self,
            RowStatus::MissingKey | RowStatus::Duplicate | RowStatus::Exists | RowStatus::Invalid
        )
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RowReport {
    pub status: RowStatus,
    pub message: Option<String>,
    /// Columns whose stored value differs from the pasted one (updates only).
    pub changed: Vec<String>,
}

impl RowReport {
    fn new(status: RowStatus, message: impl Into<Option<String>>) -> Self {
        RowReport {
            status,
            message: message.into(),
            changed: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub insert: u32,
    pub update: u32,
    pub unchanged: u32,
    pub unmatched: u32,
    /// Rows that block the run until the source is fixed.
    pub problems: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    /// One report per pasted row, in pasted order. Empty when `problems` is not.
    pub rows: Vec<RowReport>,
    pub counts: Counts,
    /// Problems with the set-up itself (mapping, key, required columns).
    pub problems: Vec<String>,
    pub elapsed_ms: u64,
}

/// One lookup query: SQL, its parameters, and the pasted row indexes it covers.
pub type Lookup = (String, Vec<Option<String>>, Vec<usize>);

enum Slot {
    Done(RowReport),
    /// Becomes an insert without asking the database.
    New,
    /// Needs a lookup; `Some` once a row with this key was found (its fetched values).
    Lookup(Option<Vec<Option<String>>>),
}

pub struct Draft<'a> {
    engine: Engine,
    meta: &'a TableMeta,
    request: &'a PasteRequest,
    /// Position of each key column in `request.columns`.
    key_at: Vec<usize>,
    /// Columns read back for matched rows: the key, then the update columns.
    fetch: Vec<String>,
    /// A key column is filled in by the server, so a blank key means "new row".
    generated_key: bool,
    slots: Vec<Slot>,
    problems: Vec<String>,
}

fn column<'m>(meta: &'m TableMeta, name: &str) -> Option<&'m ColumnMeta> {
    meta.columns.iter().find(|c| c.name == name)
}

fn shown(value: &str) -> String {
    match value.char_indices().nth(40) {
        Some((cut, _)) => format!("{}…", &value[..cut]),
        None => value.to_string(),
    }
}

fn is_number(text: &str) -> bool {
    let t = text.trim();
    if matches!(
        t.trim_start_matches(['+', '-'])
            .to_ascii_lowercase()
            .as_str(),
        "nan" | "inf" | "infinity"
    ) {
        return true;
    }
    let t = t.strip_prefix(['+', '-']).unwrap_or(t);
    let (mantissa, exponent) = match t.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (t, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, frac)) => {
            (digits(whole) || whole.is_empty())
                && (digits(frac) || frac.is_empty())
                && !(whole.is_empty() && frac.is_empty())
        }
        None => digits(mantissa),
    };
    mantissa_ok && exponent.is_none_or(|e| digits(e.strip_prefix(['+', '-']).unwrap_or(e)))
}

/// Declared length of a character column, e.g. 20 for `varchar(20)`.
fn max_chars(col: &ColumnMeta) -> Option<usize> {
    let base = col.base_type.as_str();
    if !matches!(
        base,
        "varchar" | "char" | "character varying" | "character" | "bpchar"
    ) {
        return None;
    }
    let (_, rest) = col.data_type.split_once('(')?;
    rest.split_once(')')?.0.trim().parse().ok()
}

/// Cheap checks that catch the usual spreadsheet mistakes before anything runs.
/// The server remains the authority: whatever passes here can still be refused on save.
fn check_value(col: &ColumnMeta, value: &str) -> Option<String> {
    if col.kind == ValueKind::Number
        && !matches!(col.base_type.as_str(), "money" | "bit")
        && !is_number(value)
    {
        return Some(format!("{}: “{}” is not a number", col.name, shown(value)));
    }
    if let Some(limit) = max_chars(col) {
        if value.chars().count() > limit {
            return Some(format!("{}: longer than {limit} characters", col.name));
        }
    }
    None
}

impl<'a> Draft<'a> {
    pub fn new(
        engine: Engine,
        meta: &'a TableMeta,
        keys: &[UniqueKey],
        request: &'a PasteRequest,
    ) -> Result<Self> {
        let width = request.columns.len();
        if width == 0 || request.rows.is_empty() {
            return Err(Error::Invalid("There is nothing to paste".into()));
        }
        if request.rows.len() > MAX_STATEMENTS {
            return Err(Error::Invalid(format!(
                "{} rows in one paste exceeds the limit of {MAX_STATEMENTS}",
                request.rows.len()
            )));
        }
        if request.rows.iter().any(|r| r.len() != width) {
            return Err(Error::Invalid(
                "Every pasted row must have one value per mapped column".into(),
            ));
        }

        let mut problems = Vec::new();
        let position = |name: &str| request.columns.iter().position(|c| c == name);
        let is_key = |name: &str| request.key.iter().any(|k| k == name);

        for (i, name) in request.columns.iter().enumerate() {
            match column(meta, name) {
                None => problems.push(format!("Unknown column: {name}")),
                Some(_) if request.columns[..i].contains(name) => {
                    problems.push(format!("Column {name} is mapped more than once"))
                }
                Some(col) if !col.editable && !is_key(name) => problems.push(format!(
                    "Column {name} cannot be written (generated by the server, or binary). Skip it, or use it only to match rows."
                )),
                Some(_) => {}
            }
        }

        if request.mode != Mode::Insert && request.key.is_empty() {
            problems.push("Choose the key that identifies a row".into());
        }
        let mut key_at = Vec::new();
        if !request.key.is_empty() {
            let unique = keys.iter().any(|k| {
                k.columns.len() == request.key.len() && k.columns.iter().all(|c| is_key(c))
            });
            if !unique {
                problems.push(format!(
                    "({}) is not a primary key or unique index of this table, so it cannot identify a row",
                    request.key.join(", ")
                ));
            }
            for name in &request.key {
                match position(name) {
                    Some(at) => key_at.push(at),
                    None => problems.push(format!("Key column {name} is not mapped")),
                }
            }
        }

        if request.mode != Mode::Insert {
            for name in &request.update {
                match column(meta, name) {
                    _ if position(name).is_none() => {
                        problems.push(format!("Column {name} is not mapped"))
                    }
                    _ if is_key(name) => {
                        problems.push(format!("Key column {name} cannot also be updated"))
                    }
                    Some(col) if !col.editable => {
                        problems.push(format!("Column {name} cannot be written"))
                    }
                    _ => {}
                }
            }
            if request.mode == Mode::Update && request.update.is_empty() {
                problems.push("Choose at least one column to update".into());
            }
        }

        let generated_key = request
            .key
            .iter()
            .any(|k| column(meta, k).is_some_and(|c| c.generated));
        let mut fetch = request.key.clone();
        if request.mode != Mode::Insert {
            fetch.extend(request.update.iter().cloned());
        }

        let mut draft = Draft {
            engine,
            meta,
            request,
            key_at,
            fetch,
            generated_key,
            slots: Vec::new(),
            problems,
        };
        if draft.problems.is_empty() {
            draft.presort();
        }
        Ok(draft)
    }

    /// Decide which rows need a lookup, and catch missing and repeated keys.
    fn presort(&mut self) {
        let request = self.request;
        if request.key.is_empty() {
            self.slots = request.rows.iter().map(|_| Slot::New).collect();
            return;
        }
        let mut first_seen: HashMap<Vec<&str>, usize> = HashMap::new();
        let mut slots: Vec<Slot> = Vec::with_capacity(request.rows.len());
        for (i, row) in request.rows.iter().enumerate() {
            let key: Vec<&str> = self
                .key_at
                .iter()
                .filter_map(|&at| row[at].as_deref().filter(|v| !v.is_empty()))
                .collect();
            if key.len() != self.key_at.len() {
                // A blank key means "new row" when the server fills the key in, or when plainly inserting.
                let blank = key.is_empty();
                let new_row = request.mode == Mode::Insert
                    || (request.mode == Mode::Upsert && self.generated_key && blank);
                slots.push(if new_row {
                    Slot::New
                } else {
                    Slot::Done(RowReport::new(
                        RowStatus::MissingKey,
                        format!("No value for the key ({})", request.key.join(", ")),
                    ))
                });
                continue;
            }
            match first_seen.get(&key) {
                Some(&first) => {
                    let message = |other: usize| format!("Same key as pasted row {}", other + 1);
                    slots[first] = Slot::Done(RowReport::new(RowStatus::Duplicate, message(i)));
                    slots.push(Slot::Done(RowReport::new(
                        RowStatus::Duplicate,
                        message(first),
                    )));
                }
                None => {
                    first_seen.insert(key, i);
                    slots.push(Slot::Lookup(None));
                }
            }
        }
        self.slots = slots;
    }

    /// The lookup queries to run: `(sql, parameters, pasted row indexes covered)`.
    ///
    /// Each query returns, for every existing row matched by the chunk, one flag per pasted
    /// row ("is this the row with that key?") followed by the fetched columns as text. The
    /// database does the comparison, so its own collation and type rules decide equality.
    pub fn lookups(&self, table: &TableRef) -> Result<Vec<Lookup>> {
        let pending: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| matches!(s, Slot::Lookup(_)).then_some(i))
            .collect();
        let engine = self.engine;
        let target = write::table_name(engine, table)?;
        let mut key_columns = Vec::new();
        for name in &self.request.key {
            key_columns.push((write::quote(engine, name)?, write::column(self.meta, name)?));
        }
        let mut fetched = Vec::new();
        for name in &self.fetch {
            let col = write::column(self.meta, name)?;
            fetched.push(write::rendered(engine, col, &write::quote(engine, name)?));
        }

        let mut out = Vec::new();
        for chunk in pending.chunks(LOOKUP_CHUNK) {
            let mut params: Vec<Option<String>> = Vec::new();
            // The same condition appears in the SELECT list (as a flag) and in WHERE.
            let conditions = |params: &mut Vec<Option<String>>| -> Vec<String> {
                chunk
                    .iter()
                    .map(|&row| {
                        let parts: Vec<String> = key_columns
                            .iter()
                            .zip(&self.key_at)
                            .map(|((quoted, col), &at)| {
                                params.push(self.request.rows[row][at].clone());
                                let placeholder = match engine {
                                    Engine::Postgres => format!("${}", params.len()),
                                    _ => "?".to_string(),
                                };
                                format!("{quoted} = {}", write::key_expr(engine, col, &placeholder))
                            })
                            .collect();
                        format!("({})", parts.join(" AND "))
                    })
                    .collect()
            };
            let flags: Vec<String> = conditions(&mut params)
                .into_iter()
                .map(|c| match engine {
                    Engine::Postgres => format!("{c}::text"),
                    _ => format!("CAST({c} AS CHAR)"),
                })
                .collect();
            let filter = conditions(&mut params).join(" OR ");
            let sql = format!(
                "SELECT {}, {} FROM {target} WHERE {filter}",
                flags.join(", "),
                fetched.join(", ")
            );
            out.push((sql, params, chunk.to_vec()));
        }
        Ok(out)
    }

    /// Feed back the result of one lookup query.
    pub fn matched(&mut self, rows: &[usize], found: Vec<Vec<Option<String>>>) {
        for existing in found {
            let (flags, values) = existing.split_at(rows.len().min(existing.len()));
            let hits: Vec<usize> = rows
                .iter()
                .zip(flags)
                .filter(|(_, flag)| matches!(flag.as_deref(), Some("true" | "t" | "1")))
                .map(|(&row, _)| row)
                .collect();
            match hits.as_slice() {
                [row] => {
                    self.slots[*row] = match &self.slots[*row] {
                        Slot::Lookup(None) => Slot::Lookup(Some(values.to_vec())),
                        _ => Slot::Done(RowReport::new(
                            RowStatus::Invalid,
                            "This key matches more than one existing row".to_string(),
                        )),
                    };
                }
                // Two pasted keys that differ as text but are the same value to the database
                // ("7" and "007", or "abc" and "ABC" under a case-insensitive collation).
                several => {
                    for &row in several {
                        let other = several.iter().find(|&&r| r != row).copied().unwrap_or(row);
                        self.slots[row] = Slot::Done(RowReport::new(
                            RowStatus::Duplicate,
                            format!("Same existing row as pasted row {}", other + 1),
                        ));
                    }
                }
            }
        }
    }

    fn insert_row(&self, row: &[Option<String>]) -> std::result::Result<RowInsert, String> {
        // A column every new row needs but the paste does not provide.
        for col in &self.meta.columns {
            let provided = self.request.columns.contains(&col.name);
            if !provided && !col.nullable && col.default.is_none() && !col.generated {
                return Err(format!(
                    "New rows need a value for {}, which is not mapped",
                    col.name
                ));
            }
        }
        let mut values = Vec::new();
        for (name, value) in self.request.columns.iter().zip(row) {
            let Some(col) = column(self.meta, name).filter(|c| c.editable) else {
                continue; // generated key column: the server assigns it
            };
            match value {
                None if !col.nullable && col.default.is_some() => continue, // empty cell: take the default
                None if !col.nullable => return Err(format!("{} is required", col.name)),
                None => {}
                Some(text) => {
                    if let Some(problem) = check_value(col, text) {
                        return Err(problem);
                    }
                }
            }
            values.push(CellValue {
                column: name.clone(),
                value: value.clone(),
            });
        }
        Ok(RowInsert { values })
    }

    fn update_row(
        &self,
        row: &[Option<String>],
        existing: &[Option<String>],
    ) -> std::result::Result<Option<RowUpdate>, String> {
        let keys = self.request.key.len();
        let mut changes = Vec::new();
        for (offset, name) in self.request.update.iter().enumerate() {
            let at = self
                .request
                .columns
                .iter()
                .position(|c| c == name)
                .expect("validated");
            let (pasted, stored) = (&row[at], &existing[keys + offset]);
            if pasted == stored {
                continue;
            }
            let col = column(self.meta, name).expect("validated");
            match pasted {
                None if !col.nullable => return Err(format!("{} cannot be empty", col.name)),
                Some(text) => {
                    if let Some(problem) = check_value(col, text) {
                        return Err(problem);
                    }
                }
                None => {}
            }
            changes.push(CellChange {
                column: name.clone(),
                original: stored.clone(),
                value: pasted.clone(),
            });
        }
        if changes.is_empty() {
            return Ok(None);
        }
        // Address the row by the key exactly as the database renders it.
        let key = self
            .request
            .key
            .iter()
            .zip(existing)
            .map(|(column, value)| CellValue {
                column: column.clone(),
                value: value.clone(),
            })
            .collect();
        Ok(Some(RowUpdate { key, changes }))
    }

    /// Classify every row and build the change set to run.
    pub fn finish(self) -> (Analysis, ChangeSet) {
        let mut changes = ChangeSet::default();
        let mut reports = Vec::with_capacity(self.slots.len());
        if !self.problems.is_empty() {
            let analysis = Analysis {
                rows: reports,
                counts: Counts::default(),
                problems: self.problems,
                elapsed_ms: 0,
            };
            return (analysis, changes);
        }
        let mode = self.request.mode;
        for (slot, row) in self.slots.iter().zip(&self.request.rows) {
            let adding = |changes: &mut ChangeSet| match self.insert_row(row) {
                Ok(insert) => {
                    changes.inserts.push(insert);
                    RowReport::new(RowStatus::Insert, None)
                }
                Err(problem) => RowReport::new(RowStatus::Invalid, problem),
            };
            reports.push(match slot {
                Slot::Done(report) => report.clone(),
                Slot::New => adding(&mut changes),
                Slot::Lookup(None) => match mode {
                    Mode::Insert => adding(&mut changes),
                    Mode::Upsert if !self.generated_key => adding(&mut changes),
                    Mode::Upsert => RowReport::new(
                        RowStatus::Unmatched,
                        "No row has this key. Leave the key empty to add a new row.".to_string(),
                    ),
                    Mode::Update => RowReport::new(RowStatus::Unmatched, None),
                },
                Slot::Lookup(Some(_)) if mode == Mode::Insert => RowReport::new(
                    RowStatus::Exists,
                    "A row with this key already exists".to_string(),
                ),
                Slot::Lookup(Some(existing)) => match self.update_row(row, existing) {
                    Ok(Some(update)) => {
                        let changed = update.changes.iter().map(|c| c.column.clone()).collect();
                        changes.updates.push(update);
                        RowReport {
                            status: RowStatus::Update,
                            message: None,
                            changed,
                        }
                    }
                    Ok(None) => RowReport::new(RowStatus::Unchanged, None),
                    Err(problem) => RowReport::new(RowStatus::Invalid, problem),
                },
            });
        }

        let mut counts = Counts::default();
        for report in &reports {
            match report.status {
                RowStatus::Insert => counts.insert += 1,
                RowStatus::Update => counts.update += 1,
                RowStatus::Unchanged => counts.unchanged += 1,
                RowStatus::Unmatched => counts.unmatched += 1,
                s if s.is_problem() => counts.problems += 1,
                _ => {}
            }
        }
        (
            Analysis {
                rows: reports,
                counts,
                problems: Vec::new(),
                elapsed_ms: 0,
            },
            changes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, data_type: &str, kind: ValueKind) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            data_type: data_type.into(),
            kind,
            nullable: true,
            default: None,
            primary_key: false,
            generated: false,
            editable: true,
            base_type: data_type.split('(').next().unwrap().into(),
        }
    }

    fn meta() -> TableMeta {
        let mut id = col("id", "bigint", ValueKind::Number);
        (id.nullable, id.generated, id.editable, id.primary_key) = (false, true, false, true);
        let mut sku = col("sku", "varchar(8)", ValueKind::Text);
        sku.nullable = false;
        let mut qty = col("qty", "int", ValueKind::Number);
        (qty.nullable, qty.default) = (false, Some("0".into()));
        TableMeta {
            columns: vec![id, sku, col("name", "text", ValueKind::Text), qty],
            primary_key: vec!["id".into()],
            estimated_rows: None,
        }
    }

    fn keys() -> Vec<UniqueKey> {
        vec![
            UniqueKey {
                name: "PRIMARY".into(),
                columns: vec!["id".into()],
                primary: true,
            },
            UniqueKey {
                name: "sku".into(),
                columns: vec!["sku".into()],
                primary: false,
            },
        ]
    }

    fn request(
        mode: Mode,
        columns: &[&str],
        key: &[&str],
        update: &[&str],
        rows: &[&[Option<&str>]],
    ) -> PasteRequest {
        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        PasteRequest {
            mode,
            columns: strings(columns),
            key: strings(key),
            update: strings(update),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|v| v.map(String::from)).collect())
                .collect(),
        }
    }

    fn table() -> TableRef {
        TableRef {
            database: "shop".into(),
            schema: Some("public".into()),
            name: "items".into(),
        }
    }

    fn statuses(analysis: &Analysis) -> Vec<RowStatus> {
        analysis.rows.iter().map(|r| r.status).collect()
    }

    #[test]
    fn recognises_numbers() {
        for ok in [
            "0",
            "-12",
            "+3.5",
            ".5",
            "5.",
            "1e5",
            "1.2E-3",
            " 42 ",
            "NaN",
            "-Infinity",
        ] {
            assert!(is_number(ok), "{ok}");
        }
        for bad in [
            "", "abc", "1,234", "12x", "1.2.3", "e5", ".", "--1", "1e", "๑๒",
        ] {
            assert!(!is_number(bad), "{bad}");
        }
    }

    #[test]
    fn upsert_on_a_unique_key_classifies_every_row() {
        let m = meta();
        let req = request(
            Mode::Upsert,
            &["sku", "name", "qty"],
            &["sku"],
            &["name", "qty"],
            &[
                &[Some("A1"), Some("Apple"), Some("5")], // matches, name differs  -> update
                &[Some("B2"), Some("Pear"), Some("2")],  // matches, identical     -> unchanged
                &[Some("C3"), Some("Fig"), None],        // no match, qty default  -> insert
                &[None, Some("Nameless"), Some("1")],    // no key                 -> missing key
                &[Some("D4"), Some("x"), Some("1")],     // repeated key           -> duplicate
                &[Some("D4"), Some("y"), Some("1")],
                &[Some("E5"), Some("Bad"), Some("many")], // not a number           -> invalid
                &[Some("TOOLONG99"), Some("z"), Some("1")], // longer than varchar(8) -> invalid
            ],
        );
        let mut draft = Draft::new(Engine::Postgres, &m, &keys(), &req).unwrap();
        let lookups = draft.lookups(&table()).unwrap();
        assert_eq!(lookups.len(), 1);
        let (sql, params, rows) = &lookups[0];
        assert_eq!(rows, &vec![0, 1, 2, 6, 7]);
        assert!(
            sql.starts_with("SELECT (\"sku\" = $1::varchar)::text, (\"sku\" = $2::varchar)::text")
        );
        assert!(sql.contains("\"sku\"::text, \"name\"::text, \"qty\"::text FROM \"public\".\"items\" WHERE (\"sku\" = $6::varchar) OR"));
        assert_eq!(params.len(), 10);
        assert!(!sql.contains("Apple"), "values travel as parameters");

        let flag = |hit: usize| {
            (0..5).map(move |i| Some(if i == hit { "true" } else { "false" }.to_string()))
        };
        let found = vec![
            flag(0)
                .chain([
                    Some("A1".into()),
                    Some("Old apple".into()),
                    Some("5".into()),
                ])
                .collect(),
            flag(1)
                .chain([Some("B2".into()), Some("Pear".into()), Some("2".into())])
                .collect(),
        ];
        draft.matched(rows, found);
        let (analysis, changes) = draft.finish();

        use RowStatus::*;
        assert_eq!(
            statuses(&analysis),
            vec![Update, Unchanged, Insert, MissingKey, Duplicate, Duplicate, Invalid, Invalid]
        );
        assert_eq!(
            analysis.counts,
            Counts {
                insert: 1,
                update: 1,
                unchanged: 1,
                unmatched: 0,
                problems: 5
            }
        );
        assert_eq!(analysis.rows[0].changed, vec!["name"]);
        assert!(analysis.rows[6]
            .message
            .as_deref()
            .unwrap()
            .contains("not a number"));
        assert!(analysis.rows[7]
            .message
            .as_deref()
            .unwrap()
            .contains("longer than 8"));

        assert_eq!(changes.updates.len(), 1);
        assert_eq!(changes.updates[0].key[0].value.as_deref(), Some("A1"));
        assert_eq!(
            changes.updates[0].changes[0].original.as_deref(),
            Some("Old apple")
        );
        // The empty qty is left out so the column default applies.
        let inserted: Vec<_> = changes.inserts[0]
            .values
            .iter()
            .map(|v| v.column.as_str())
            .collect();
        assert_eq!(inserted, vec!["sku", "name"]);
    }

    #[test]
    fn a_generated_key_makes_blank_mean_new_and_unknown_mean_skip() {
        let m = meta();
        let req = request(
            Mode::Upsert,
            &["id", "sku", "name"],
            &["id"],
            &["sku", "name"],
            &[
                &[Some("1"), Some("A1"), Some("Renamed")],
                &[None, Some("N1"), Some("New")],
                &[Some("999"), Some("X9"), Some("Ghost")],
            ],
        );
        let mut draft = Draft::new(Engine::Mysql, &m, &keys(), &req).unwrap();
        let lookups = draft.lookups(&table()).unwrap();
        let (sql, _, rows) = &lookups[0];
        assert_eq!(rows, &vec![0, 2]);
        assert!(sql.contains("CAST((`id` = CAST(? AS SIGNED)) AS CHAR)"));
        draft.matched(
            rows,
            vec![vec![
                Some("1".into()),
                Some("0".into()),
                Some("1".into()),
                Some("A1".into()),
                Some("Old".into()),
            ]],
        );
        let (analysis, changes) = draft.finish();
        use RowStatus::*;
        assert_eq!(statuses(&analysis), vec![Update, Insert, Unmatched]);
        // The generated id is never written.
        assert!(changes.inserts[0].values.iter().all(|v| v.column != "id"));
        assert_eq!(changes.updates[0].changes.len(), 1);
    }

    #[test]
    fn insert_and_update_modes() {
        let m = meta();
        // Insert with a key: rows that exist are refused instead of failing halfway.
        let req = request(
            Mode::Insert,
            &["sku", "name"],
            &["sku"],
            &[],
            &[&[Some("A1"), Some("x")], &[Some("Z9"), Some("y")]],
        );
        let mut draft = Draft::new(Engine::Postgres, &m, &keys(), &req).unwrap();
        let rows = draft.lookups(&table()).unwrap().remove(0).2;
        draft.matched(
            &rows,
            vec![vec![
                Some("true".into()),
                Some("false".into()),
                Some("A1".into()),
            ]],
        );
        let (analysis, changes) = draft.finish();
        assert_eq!(
            statuses(&analysis),
            vec![RowStatus::Exists, RowStatus::Insert]
        );
        assert_eq!(changes.inserts.len(), 1);

        // Insert without a key needs no lookup at all.
        let req = request(
            Mode::Insert,
            &["sku", "name"],
            &[],
            &[],
            &[&[Some("A1"), None]],
        );
        let draft = Draft::new(Engine::Postgres, &m, &keys(), &req).unwrap();
        assert!(draft.lookups(&table()).unwrap().is_empty());
        assert_eq!(statuses(&draft.finish().0), vec![RowStatus::Insert]);

        // Update never inserts: unmatched rows are reported and left alone.
        let req = request(
            Mode::Update,
            &["sku", "name"],
            &["sku"],
            &["name"],
            &[&[Some("Q1"), Some("x")]],
        );
        let draft = Draft::new(Engine::Postgres, &m, &keys(), &req).unwrap();
        let (analysis, changes) = draft.finish();
        assert_eq!(statuses(&analysis), vec![RowStatus::Unmatched]);
        assert!(changes.updates.is_empty() && changes.inserts.is_empty());
    }

    #[test]
    fn two_spellings_of_one_key_are_duplicates() {
        let m = meta();
        let req = request(
            Mode::Update,
            &["id", "name"],
            &["id"],
            &["name"],
            &[&[Some("7"), Some("a")], &[Some("007"), Some("b")]],
        );
        let mut draft = Draft::new(Engine::Mysql, &m, &keys(), &req).unwrap();
        let rows = draft.lookups(&table()).unwrap().remove(0).2;
        draft.matched(
            &rows,
            vec![vec![
                Some("1".into()),
                Some("1".into()),
                Some("7".into()),
                Some("old".into()),
            ]],
        );
        assert_eq!(
            statuses(&draft.finish().0),
            vec![RowStatus::Duplicate, RowStatus::Duplicate]
        );
    }

    #[test]
    fn refuses_a_set_up_that_cannot_identify_or_write_rows() {
        let m = meta();
        let problems = |req: PasteRequest| {
            Draft::new(Engine::Mysql, &m, &keys(), &req)
                .unwrap()
                .finish()
                .0
                .problems
        };
        let row: &[&[Option<&str>]] = &[&[Some("a"), Some("b")]];

        assert!(problems(request(
            Mode::Update,
            &["name", "qty"],
            &["name"],
            &["qty"],
            row
        ))[0]
            .contains("not a primary key or unique index"));
        assert!(
            problems(request(Mode::Update, &["sku", "name"], &[], &["name"], row))[0]
                .contains("Choose the key")
        );
        assert!(
            problems(request(Mode::Update, &["sku", "name"], &["sku"], &[], row))[0]
                .contains("at least one column")
        );
        assert!(
            problems(request(Mode::Insert, &["sku", "nope"], &[], &[], row))[0]
                .contains("Unknown column")
        );
        assert!(
            problems(request(Mode::Insert, &["sku", "sku"], &[], &[], row))[0]
                .contains("more than once")
        );
        assert!(
            problems(request(Mode::Insert, &["id", "sku"], &[], &[], row))[0]
                .contains("cannot be written")
        );
        assert!(problems(request(
            Mode::Upsert,
            &["sku", "name"],
            &["sku"],
            &["sku"],
            row
        ))[0]
            .contains("cannot also be updated"));

        // Rows that would be inserted without a required column are flagged one by one.
        let req = request(Mode::Insert, &["name", "qty"], &[], &[], row);
        let (analysis, _) = Draft::new(Engine::Mysql, &m, &keys(), &req)
            .unwrap()
            .finish();
        assert!(analysis.rows[0]
            .message
            .as_deref()
            .unwrap()
            .contains("need a value for sku"));

        let ragged = request(Mode::Insert, &["sku", "name"], &[], &[], &[&[Some("a")]]);
        assert!(Draft::new(Engine::Mysql, &m, &keys(), &ragged).is_err());
    }
}
