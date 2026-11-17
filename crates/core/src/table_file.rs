//! Reads a table of text out of a file the user chose: delimited text (CSV, TSV) or a
//! workbook (XLSX, XLS, ODS). The result is what a block copied from a spreadsheet would be
//! — rows of cells, as text — so Smart Paste can take it from here.
//!
//! Nothing is guessed silently:
//! - text that is not UTF-8 (or UTF-16 with a byte-order mark) is refused until the caller
//!   names its encoding, and bytes that encoding cannot read are an error, not `�`;
//! - a workbook cell is the stored value, not its formatting: `0.5` for a cell shown as
//!   `50%`. Dates and times are written the ISO way a database reads them.

use std::path::Path;

use calamine::{Data, Reader};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Larger files are for an import tool that streams; a workbook is read into memory whole.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

const TEXT_EXTENSIONS: &[&str] = &["csv", "tsv", "tab", "txt"];
const WORKBOOK_EXTENSIONS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];
const DELIMITERS: &[char] = &[',', ';', '\t', '|'];

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Text,
    Workbook,
}

/// An unguessable name for a chosen file, handed out in place of its path.
pub fn new_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Every extension `kind_of` accepts, for the file dialog's filter.
pub fn extensions() -> Vec<&'static str> {
    [TEXT_EXTENSIONS, WORKBOOK_EXTENSIONS].concat()
}

/// What a file is, by its extension. The content is only looked at by `read`.
pub fn kind_of(path: &Path) -> Result<FileKind> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if TEXT_EXTENSIONS.contains(&ext.as_str()) {
        Ok(FileKind::Text)
    } else if WORKBOOK_EXTENSIONS.contains(&ext.as_str()) {
        Ok(FileKind::Workbook)
    } else {
        Err(Error::Invalid(
            "Only CSV or TSV text and Excel or OpenDocument spreadsheets can be opened".into(),
        ))
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReadOptions {
    /// Workbooks: the sheet to read. The first one when absent.
    #[serde(default)]
    pub sheet: Option<String>,
    /// Text: an encoding label such as `windows-874`. When absent the file must be UTF-8,
    /// or UTF-16 with a byte-order mark.
    #[serde(default)]
    pub encoding: Option<String>,
    /// Text: the character between fields. Detected from the first lines when absent.
    #[serde(default)]
    pub delimiter: Option<char>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileTable {
    pub kind: FileKind,
    /// The first rows, rectangular (short rows padded with empty text).
    pub rows: Vec<Vec<String>>,
    /// Rows in the file or sheet; more than `rows` holds when it was cut at the limit.
    pub total_rows: usize,
    /// Workbooks: every sheet, in order.
    pub sheets: Vec<String>,
    /// Workbooks: the sheet that was read.
    pub sheet: Option<String>,
    /// Text: the encoding the bytes were read as.
    pub encoding: Option<String>,
    /// Text: the character taken as the field separator.
    pub delimiter: Option<char>,
}

/// Read at most `max_rows` rows; `total_rows` still counts all of them.
pub fn read(path: &Path, options: &ReadOptions, max_rows: usize) -> Result<FileTable> {
    let kind = kind_of(path)?;
    let size = std::fs::metadata(path)?.len();
    if size > MAX_FILE_BYTES {
        return Err(Error::Invalid(format!(
            "The file is {} MB; files up to {} MB can be opened",
            size / (1024 * 1024),
            MAX_FILE_BYTES / (1024 * 1024)
        )));
    }
    let mut table = match kind {
        FileKind::Text => {
            let tabbed = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tsv") || e.eq_ignore_ascii_case("tab"));
            read_text(&std::fs::read(path)?, options, tabbed, max_rows)?
        }
        FileKind::Workbook => read_workbook(path, options, max_rows)?,
    };
    let width = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut table.rows {
        row.resize(width, String::new());
    }
    Ok(table)
}

fn read_text(
    bytes: &[u8],
    options: &ReadOptions,
    tabbed: bool,
    max_rows: usize,
) -> Result<FileTable> {
    let (text, encoding) = decode(bytes, options.encoding.as_deref())?;
    let delimiter = match options.delimiter {
        Some(d) => d,
        None if tabbed => '\t',
        None => detect_delimiter(&text),
    };
    if !delimiter.is_ascii() || matches!(delimiter, '"' | '\r' | '\n') {
        return Err(Error::Invalid(
            "The field separator must be one plain character".into(),
        ));
    }
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter as u8)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    let mut total_rows = 0;
    for record in reader.records() {
        let record = record.map_err(|e| Error::Invalid(format!("Could not read the file: {e}")))?;
        total_rows += 1;
        if rows.len() < max_rows {
            rows.push(record.iter().map(str::to_string).collect());
        }
    }
    Ok(FileTable {
        kind: FileKind::Text,
        rows,
        total_rows,
        sheets: Vec::new(),
        sheet: None,
        encoding: Some(encoding),
        delimiter: Some(delimiter),
    })
}

/// Bytes to text, and the name of the encoding used. A byte-order mark decides; without one
/// the bytes must be valid UTF-8 unless the caller named another encoding.
fn decode(bytes: &[u8], label: Option<&str>) -> Result<(String, String)> {
    let encoding = match (encoding_rs::Encoding::for_bom(bytes), label) {
        (Some((by_mark, _)), _) => by_mark,
        (None, Some(label)) => encoding_rs::Encoding::for_label(label.as_bytes())
            .ok_or_else(|| Error::Invalid(format!("Unknown text encoding: {label}")))?,
        (None, None) => encoding_rs::UTF_8,
    };
    // `decode` also removes the byte-order mark.
    let (text, used, damaged) = encoding.decode(bytes);
    if damaged {
        return Err(Error::Invalid(if label.is_none() {
            "This file is not UTF-8 text. Choose the encoding it was saved in.".into()
        } else {
            format!(
                "This file is not valid {} text. Choose another encoding.",
                used.name()
            )
        }));
    }
    Ok((text.into_owned(), used.name().to_string()))
}

/// The candidate that splits the first lines into the most fields, counted outside quotes.
/// A comma when nothing else is clearly used.
fn detect_delimiter(text: &str) -> char {
    let mut counts = [0usize; DELIMITERS.len()];
    let mut quoted = false;
    let mut lines = 0;
    for ch in text.chars() {
        match ch {
            '"' => quoted = !quoted,
            '\n' if !quoted => {
                lines += 1;
                if lines == 10 {
                    break;
                }
            }
            _ if !quoted => {
                if let Some(i) = DELIMITERS.iter().position(|d| *d == ch) {
                    counts[i] += 1;
                }
            }
            _ => {}
        }
    }
    let mut best = 0;
    for i in 1..DELIMITERS.len() {
        if counts[i] > counts[best] {
            best = i;
        }
    }
    DELIMITERS[best]
}

fn read_workbook(path: &Path, options: &ReadOptions, max_rows: usize) -> Result<FileTable> {
    let unreadable =
        |e: &dyn std::fmt::Display| Error::Invalid(format!("Could not read the workbook: {e}"));
    let mut workbook = calamine::open_workbook_auto(path).map_err(|e| unreadable(&e))?;
    let sheets = workbook.sheet_names();
    let sheet = match &options.sheet {
        Some(name) if sheets.contains(name) => name.clone(),
        Some(name) => return Err(Error::NotFound(format!("Sheet {name}"))),
        None => sheets
            .first()
            .cloned()
            .ok_or_else(|| Error::Invalid("The workbook has no sheets".into()))?,
    };
    let range = workbook
        .worksheet_range(&sheet)
        .map_err(|e| unreadable(&e))?;
    let mut all: Vec<Vec<String>> = range
        .rows()
        .map(|row| row.iter().map(cell_text).collect())
        .collect();
    // Formatting below the data makes a sheet's used range longer than its content.
    while all
        .last()
        .is_some_and(|row| row.iter().all(String::is_empty))
    {
        all.pop();
    }
    let total_rows = all.len();
    all.truncate(max_rows);
    Ok(FileTable {
        kind: FileKind::Workbook,
        rows: all,
        total_rows,
        sheets,
        sheet: Some(sheet),
        encoding: None,
        delimiter: None,
    })
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(n) => n.to_string(),
        Data::Float(v) => number_text(*v),
        // What the cell shows, and what a copy of it puts on the clipboard.
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::Error(e) => e.to_string(),
        Data::DateTime(moment) => {
            if moment.is_duration() {
                let seconds = (moment.as_f64() * 86_400.0).round() as i64;
                let sign = if seconds < 0 { "-" } else { "" };
                let s = seconds.abs();
                return format!("{sign}{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60);
            }
            let Some(at) = moment.as_datetime() else {
                return number_text(moment.as_f64());
            };
            // Displayed as `2026-01-31 13:45:07`, with a fraction only when there is one.
            let shown = at.to_string();
            let (day, clock) = shown.split_once(' ').unwrap_or((&shown, "00:00:00"));
            // Excel counts days from 1900: a value below one day is a time of day.
            if (0.0..1.0).contains(&moment.as_f64()) {
                clock.to_string()
            } else if clock == "00:00:00" {
                day.to_string()
            } else {
                shown
            }
        }
    }
}

/// A stored number as Excel itself would write it: whole numbers without a fraction, and at
/// most 15 significant digits — the 17 a double can carry would turn `0.1 + 0.2` into
/// `0.30000000000000004`.
fn number_text(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if v == v.trunc() {
        // Also turns -0 into 0. Display prints every digit, without an exponent.
        return format!("{}", if v == 0.0 { 0.0 } else { v });
    }
    let decimals = 14 - v.abs().log10().floor() as i32;
    if !(1..=40).contains(&decimals) {
        return v.to_string();
    }
    let text = format!("{v:.*}", decimals as usize);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const THAI: &str = "สมชาย \"ใจดี\", เชียงใหม่";

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("puricat-file-{}-{name}", uuid::Uuid::new_v4()))
    }

    fn with_file<T>(name: &str, bytes: &[u8], test: impl FnOnce(&Path) -> T) -> T {
        let path = temp(name);
        std::fs::write(&path, bytes).unwrap();
        let out = test(&path);
        let _ = std::fs::remove_file(&path);
        out
    }

    fn rows(table: &FileTable) -> Vec<Vec<&str>> {
        table
            .rows
            .iter()
            .map(|r| r.iter().map(String::as_str).collect())
            .collect()
    }

    #[test]
    fn csv_with_quotes_line_breaks_and_a_byte_order_mark() {
        let text = format!(
            "\u{feff}id,name,note\r\n1,\"{}\",\"two\nlines\"\r\n2,plain\r\n\r\n3,,007\r\n",
            THAI.replace('"', "\"\"")
        );
        let table = with_file("a.csv", text.as_bytes(), |p| {
            read(p, &ReadOptions::default(), 100).unwrap()
        });
        assert_eq!(table.kind, FileKind::Text);
        assert_eq!(
            rows(&table),
            vec![
                vec!["id", "name", "note"],
                vec!["1", THAI, "two\nlines"],
                vec!["2", "plain", ""], // padded
                vec!["3", "", "007"],   // leading zeros kept: nothing is reinterpreted
            ]
        );
        assert_eq!(table.total_rows, 4);
        assert_eq!(table.encoding.as_deref(), Some("UTF-8"));
        assert_eq!(table.delimiter, Some(','));
    }

    #[test]
    fn the_separator_is_detected_or_given() {
        let read_as = |name: &str, text: &str, delimiter: Option<char>| {
            let options = ReadOptions {
                delimiter,
                ..Default::default()
            };
            with_file(name, text.as_bytes(), |p| read(p, &options, 100).unwrap())
        };
        // Semicolons, as Excel writes CSV where the comma is the decimal mark.
        let table = read_as("a.csv", "id;price\n1;\"1,50\"\n2;2,75\n", None);
        assert_eq!(table.delimiter, Some(';'));
        assert_eq!(rows(&table)[1], vec!["1", "1,50"]);
        // A separator inside quotes does not count.
        let table = read_as("b.csv", "a,b\n\"x;y;z;w\",2\n", None);
        assert_eq!(table.delimiter, Some(','));
        // A .tsv file is tab-separated whatever else it contains.
        let table = read_as("c.tsv", "a,b,c\td\n1,2,3\t4\n", None);
        assert_eq!(rows(&table), vec![vec!["a,b,c", "d"], vec!["1,2,3", "4"]]);
        // One column, nothing to detect: a comma is assumed and nothing is split.
        let table = read_as("d.txt", "name\nalpha\n", None);
        assert_eq!(rows(&table), vec![vec!["name"], vec!["alpha"]]);
        // The caller's choice wins.
        let table = read_as("e.csv", "a|b,c\n", Some('|'));
        assert_eq!(rows(&table), vec![vec!["a", "b,c"]]);
        let bad = ReadOptions {
            delimiter: Some('ก'),
            ..Default::default()
        };
        assert!(with_file("f.csv", b"a,b\n", |p| read(p, &bad, 100)).is_err());
    }

    #[test]
    fn other_encodings_are_never_guessed() {
        let (thai_bytes, _, _) = encoding_rs::WINDOWS_874.encode("รหัส,ชื่อ\n1,สมชาย\n");
        // Not UTF-8: refused until an encoding is named.
        let refused = with_file("a.csv", &thai_bytes, |p| {
            read(p, &ReadOptions::default(), 100)
        });
        assert!(matches!(refused, Err(Error::Invalid(m)) if m.contains("not UTF-8")));
        let named = ReadOptions {
            encoding: Some("windows-874".into()),
            ..Default::default()
        };
        let table = with_file("b.csv", &thai_bytes, |p| read(p, &named, 100).unwrap());
        assert_eq!(rows(&table), vec![vec!["รหัส", "ชื่อ"], vec!["1", "สมชาย"]]);
        assert_eq!(table.encoding.as_deref(), Some("windows-874"));
        // Bytes the named encoding has no character for are an error, not replacement
        // characters. (0xFC is unassigned in windows-874.)
        assert!(with_file("c.csv", b"a,\xFC\n", |p| read(p, &named, 100)).is_err());
        let unknown = ReadOptions {
            encoding: Some("klingon".into()),
            ..Default::default()
        };
        assert!(with_file("d.csv", b"a\n", |p| read(p, &unknown, 100)).is_err());

        // UTF-16 with a byte-order mark, as Excel's "Unicode Text" writes it.
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in "ชื่อ\tx\nก\t1\n".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        let table = with_file("e.txt", &utf16, |p| {
            read(p, &ReadOptions::default(), 100).unwrap()
        });
        assert_eq!(rows(&table), vec![vec!["ชื่อ", "x"], vec!["ก", "1"]]);
        assert_eq!(table.encoding.as_deref(), Some("UTF-16LE"));
    }

    #[test]
    fn the_row_limit_cuts_but_still_counts() {
        let text: String = (1..=25).map(|n| format!("{n},x\n")).collect();
        let table = with_file("a.csv", text.as_bytes(), |p| {
            read(p, &ReadOptions::default(), 10).unwrap()
        });
        assert_eq!(table.rows.len(), 10);
        assert_eq!(table.total_rows, 25);
        assert_eq!(table.rows[9][0], "10");
    }

    #[test]
    fn only_table_files_are_opened() {
        for name in ["id_rsa", "notes.md", "data.json", ".env"] {
            assert!(
                matches!(kind_of(Path::new(name)), Err(Error::Invalid(_))),
                "{name}"
            );
        }
        assert_eq!(kind_of(Path::new("A.CSV")).unwrap(), FileKind::Text);
        assert_eq!(kind_of(Path::new("b.Xlsx")).unwrap(), FileKind::Workbook);
        // A refused extension is refused before the file is touched.
        assert!(matches!(
            read(
                Path::new("/nonexistent/secret.pem"),
                &ReadOptions::default(),
                10
            ),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn numbers_are_written_the_way_excel_shows_them() {
        for (value, text) in [
            (0.1 + 0.2, "0.3"),
            (1.0, "1"),
            (-0.0, "0"),
            (1234567.0, "1234567"),
            (1e15, "1000000000000000"),
            (9007199254740992.0, "9007199254740992"),
            (0.5, "0.5"),
            (-12.345, "-12.345"),
            (1234.5678, "1234.5678"),
            (1e-7, "0.0000001"),
            (123456789012.345, "123456789012.345"),
        ] {
            assert_eq!(number_text(value), text);
        }
    }

    /// A workbook from another writer (openpyxl; `tests/fixtures/make_openpyxl.py` made it):
    /// shared strings, dates stored as serial numbers with a date format, and a sheet
    /// name in Thai.
    #[test]
    fn a_workbook_from_another_writer() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openpyxl.xlsx");
        let table = read(&path, &ReadOptions::default(), 100).unwrap();
        assert_eq!(table.sheets, ["ลูกค้า", "Empty"]);
        assert_eq!(
            rows(&table),
            vec![
                vec!["id", "name", "joined", "seen", "opens", "balance", "active", "share", "note"],
                vec![
                    "1",
                    "สมชาย \"ใจดี\"",
                    "2026-01-31",
                    "2026-01-31 13:45:07",
                    "08:30:00",
                    "1234.5",
                    "TRUE",
                    "0.25",
                    ""
                ],
                vec![
                    "2",
                    "Ananya Wong",
                    "2024-02-29",
                    "",
                    "",
                    "-0.1",
                    "FALSE",
                    "1",
                    "two\nlines"
                ],
                vec![
                    // The file says 9007199254740993. A workbook number is a double, which
                    // cannot hold it: long identifiers must be text cells to stay exact.
                    "9007199254740992",
                    "big id as text? no: a number",
                    "",
                    "",
                    "",
                    "0.3",
                    "",
                    "",
                    "007"
                ],
            ]
        );
        let empty = ReadOptions {
            sheet: Some("Empty".into()),
            ..Default::default()
        };
        let table = read(&path, &empty, 100).unwrap();
        assert!(table.rows.is_empty() && table.total_rows == 0);
    }

    #[test]
    fn workbook_cells_become_the_text_a_database_reads() {
        use rust_xlsxwriter::{ExcelDateTime, Format, Formula, Workbook};
        let path = temp("book.xlsx");
        let mut book = Workbook::new();
        let sheet = book.add_worksheet().set_name("Orders").unwrap();
        let date = Format::new().set_num_format("yyyy-mm-dd");
        let stamp = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
        let clock = Format::new().set_num_format("hh:mm");
        let percent = Format::new().set_num_format("0%");
        for (c, name) in [
            "id", "name", "price", "day", "seen", "at", "paid", "share", "sum",
        ]
        .iter()
        .enumerate()
        {
            sheet.write_string(0, c as u16, *name).unwrap();
        }
        sheet.write_number(1, 0, 1).unwrap();
        sheet.write_string(1, 1, THAI).unwrap();
        sheet.write_number(1, 2, 0.1 + 0.2).unwrap();
        let day = ExcelDateTime::from_ymd(2026, 1, 31).unwrap();
        sheet.write_datetime_with_format(1, 3, &day, &date).unwrap();
        let seen = ExcelDateTime::from_ymd(2026, 1, 31)
            .unwrap()
            .and_hms(13, 45, 7)
            .unwrap();
        sheet
            .write_datetime_with_format(1, 4, &seen, &stamp)
            .unwrap();
        let at = ExcelDateTime::from_hms(13, 45, 0).unwrap();
        sheet.write_datetime_with_format(1, 5, &at, &clock).unwrap();
        sheet.write_boolean(1, 6, true).unwrap();
        sheet.write_number_with_format(1, 7, 0.5, &percent).unwrap();
        sheet
            .write_formula(1, 8, Formula::new("=A2+41").set_result("42"))
            .unwrap();
        // Row 3 is short; row 5 holds only formatting.
        sheet.write_number(2, 0, 2).unwrap();
        sheet.write_string(2, 1, "007").unwrap();
        sheet.write_blank(4, 0, &percent).unwrap();
        let other = book.add_worksheet().set_name("ลูกค้า").unwrap();
        other.write_string(0, 0, "only").unwrap();
        book.save(&path).unwrap();

        let table = read(&path, &ReadOptions::default(), 100).unwrap();
        assert_eq!(table.kind, FileKind::Workbook);
        assert_eq!(table.sheets, ["Orders", "ลูกค้า"]);
        assert_eq!(table.sheet.as_deref(), Some("Orders"));
        assert_eq!(table.total_rows, 3);
        assert_eq!(
            rows(&table)[1],
            vec![
                "1",
                THAI,
                "0.3",
                "2026-01-31",
                "2026-01-31 13:45:07",
                "13:45:00",
                "TRUE",
                "0.5", // the stored value, not "50%"
                "42",
            ]
        );
        assert_eq!(
            rows(&table)[2],
            vec!["2", "007", "", "", "", "", "", "", ""]
        );

        let second = ReadOptions {
            sheet: Some("ลูกค้า".into()),
            ..Default::default()
        };
        assert_eq!(
            rows(&read(&path, &second, 100).unwrap()),
            vec![vec!["only"]]
        );
        let missing = ReadOptions {
            sheet: Some("Nope".into()),
            ..Default::default()
        };
        assert!(matches!(
            read(&path, &missing, 100),
            Err(Error::NotFound(_))
        ));
        let _ = std::fs::remove_file(&path);

        // A file that only has the name of a workbook.
        let fake = with_file("fake.xlsx", b"not a zip", |p| {
            read(p, &ReadOptions::default(), 10)
        });
        assert!(matches!(fake, Err(Error::Invalid(m)) if m.contains("Could not read")));
    }
}
