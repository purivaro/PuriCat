//! Writing a result set to a file: CSV, JSON or XLSX.
//!
//! Values are exported as the server rendered them. Nothing is reformatted on the way
//! out, with one deliberate exception: XLSX cells of numeric columns become real numbers
//! when — and only when — a spreadsheet's 64-bit float can hold them exactly.

use std::io::{BufWriter, Write};
use std::path::Path;

use serde::Deserialize;

use super::console::ResultSet;
use super::ValueKind;
use crate::{Error, Result};

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Csv,
    Json,
    Xlsx,
}

/// Write `set` to `path`. Returns the number of data rows written.
pub fn write(path: &Path, format: Format, set: &ResultSet) -> Result<usize> {
    match format {
        Format::Csv => csv(path, set)?,
        Format::Json => json(path, set)?,
        Format::Xlsx => xlsx(path, set)?,
    }
    Ok(set.rows.len())
}

fn csv_field(value: &str) -> std::borrow::Cow<'_, str> {
    if value.contains([',', '"', '\n', '\r']) || value.starts_with(' ') || value.ends_with(' ') {
        format!("\"{}\"", value.replace('"', "\"\"")).into()
    } else {
        value.into()
    }
}

/// RFC 4180, UTF-8 with a byte-order mark so spreadsheet apps read non-Latin text
/// correctly. NULL is an empty field; an empty string is `""`, so the two stay distinct.
fn csv(path: &Path, set: &ResultSet) -> Result<()> {
    let mut out = BufWriter::new(std::fs::File::create(path)?);
    out.write_all("\u{feff}".as_bytes())?;
    let header: Vec<_> = set.columns.iter().map(|c| csv_field(&c.name)).collect();
    writeln!(out, "{}\r", header.join(","))?;
    for row in &set.rows {
        let line: Vec<_> = row
            .iter()
            .map(|cell| match cell.as_deref() {
                None => "".into(),
                Some("") => "\"\"".into(),
                Some(v) => csv_field(v),
            })
            .collect();
        writeln!(out, "{}\r", line.join(","))?;
    }
    Ok(out.flush()?)
}

/// Object keys for the columns. A query may return two columns with one name (`a.id`,
/// `b.id`); the later ones get a numeric suffix so no value is dropped.
fn json_keys(set: &ResultSet) -> Vec<String> {
    let mut keys: Vec<String> = Vec::with_capacity(set.columns.len());
    for column in &set.columns {
        let mut key = column.name.clone();
        let mut n = 2;
        while keys.contains(&key) {
            key = format!("{}_{n}", column.name);
            n += 1;
        }
        keys.push(key);
    }
    keys
}

/// An array of objects. Every value is a string or null: JSON numbers would silently lose
/// digits of large integers and decimals in most consumers.
fn json(path: &Path, set: &ResultSet) -> Result<()> {
    let mut out = BufWriter::new(std::fs::File::create(path)?);
    let keys = json_keys(set);
    out.write_all(b"[")?;
    for (i, row) in set.rows.iter().enumerate() {
        let mut object = serde_json::Map::with_capacity(row.len());
        for (key, cell) in keys.iter().zip(row) {
            let value = cell
                .clone()
                .map_or(serde_json::Value::Null, serde_json::Value::String);
            object.insert(key.clone(), value);
        }
        out.write_all(if i == 0 { b"\n  " } else { b",\n  " })?;
        serde_json::to_writer(&mut out, &object).map_err(|e| Error::Io(e.into()))?;
    }
    out.write_all(b"\n]\n")?;
    Ok(out.flush()?)
}

/// A number a spreadsheet can store without changing it: plain decimal notation with at
/// most 15 significant digits.
fn exact_number(text: &str) -> Option<f64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let plain = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && digits.bytes().filter(|&b| b == b'.').count() <= 1
        && !digits.starts_with('.')
        && !digits.ends_with('.');
    let significant = digits.bytes().filter(u8::is_ascii_digit).count();
    if !plain || significant > 15 {
        return None;
    }
    text.parse().ok()
}

fn xlsx(path: &Path, set: &ResultSet) -> Result<()> {
    use rust_xlsxwriter::{Format as CellFormat, Workbook};
    let fail = |e: rust_xlsxwriter::XlsxError| {
        Error::Invalid(format!("Could not write the spreadsheet: {e}"))
    };
    // A worksheet holds 1,048,576 rows including the header.
    if set.rows.len() >= 1_048_576 {
        return Err(Error::Invalid(
            "Too many rows for one worksheet; export as CSV instead".into(),
        ));
    }
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let bold = CellFormat::new().set_bold();
    for (c, column) in set.columns.iter().enumerate() {
        sheet
            .write_string_with_format(0, c as u16, &column.name, &bold)
            .map_err(fail)?;
    }
    for (r, row) in set.rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let Some(value) = cell else { continue }; // NULL stays an empty cell
            let number = (set.columns[c].kind == ValueKind::Number)
                .then(|| exact_number(value))
                .flatten();
            match number {
                Some(n) => sheet
                    .write_number(r as u32 + 1, c as u16, n)
                    .map_err(fail)?,
                None => sheet
                    .write_string(r as u32 + 1, c as u16, value)
                    .map_err(fail)?,
            };
        }
    }
    sheet.set_freeze_panes(1, 0).map_err(fail)?;
    workbook.save(path).map_err(fail)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::console::ResultColumn;
    use super::*;

    fn sample() -> ResultSet {
        let col = |name: &str, kind| ResultColumn {
            name: name.into(),
            type_name: "x".into(),
            kind,
        };
        ResultSet {
            columns: vec![
                col("id", ValueKind::Number),
                col("ชื่อ, \"name\"", ValueKind::Text),
                col("note", ValueKind::Text),
            ],
            rows: vec![
                vec![
                    Some("1".into()),
                    Some("สมชาย, \"ใจดี\"".into()),
                    Some("two\nlines".into()),
                ],
                vec![Some("9007199254740993".into()), Some("".into()), None],
                vec![
                    Some("12.50".into()),
                    Some(" padded ".into()),
                    Some("plain".into()),
                ],
            ],
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("puricat-export-{}-{name}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn csv_keeps_text_exact_and_null_distinct_from_empty() {
        let path = temp("a.csv");
        assert_eq!(write(&path, Format::Csv, &sample()).unwrap(), 3);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            text,
            "\u{feff}id,\"ชื่อ, \"\"name\"\"\",note\r\n\
             1,\"สมชาย, \"\"ใจดี\"\"\",\"two\nlines\"\r\n\
             9007199254740993,\"\",\r\n\
             12.50,\" padded \",plain\r\n"
        );
    }

    #[test]
    fn json_is_lossless_strings_and_nulls() {
        let path = temp("a.json");
        write(&path, Format::Json, &sample()).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(parsed[1]["id"], "9007199254740993");
        assert_eq!(parsed[1]["note"], serde_json::Value::Null);
        assert_eq!(parsed[1]["ชื่อ, \"name\""], "");
        assert_eq!(parsed[0]["note"], "two\nlines");
        assert_eq!(parsed.as_array().unwrap().len(), 3);

        // Two columns with one name both survive.
        let mut twice = sample();
        twice.columns[2].name = "id".into();
        twice.columns.push(twice.columns[0].clone());
        for row in &mut twice.rows {
            row.push(Some("last".into()));
        }
        assert_eq!(json_keys(&twice), ["id", "ชื่อ, \"name\"", "id_2", "id_3"]);
    }

    #[test]
    fn xlsx_only_turns_exactly_representable_values_into_numbers() {
        assert_eq!(exact_number("12.50"), Some(12.5));
        assert_eq!(exact_number("-7"), Some(-7.0));
        assert_eq!(exact_number("123456789012345"), Some(123456789012345.0));
        for kept_as_text in [
            "9007199254740993",
            "1234567890.123456",
            "1e5",
            "NaN",
            "",
            ".5",
            "5.",
            "1,000",
            "+1",
        ] {
            assert_eq!(exact_number(kept_as_text), None, "{kept_as_text}");
        }
        let path = temp("a.xlsx");
        write(&path, Format::Xlsx, &sample()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(&bytes[..2], b"PK", "an xlsx file is a zip archive");
        assert!(bytes.len() > 1000);
    }
}
