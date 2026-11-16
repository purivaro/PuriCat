// Pure helpers for the Smart Paste wizard: reading the pasted block, mapping its columns
// to table columns, and building the request the backend analyses and runs.

import type { ColumnMeta, PasteMode, PasteRequest, UniqueKey } from "../../lib/types";

/** Target column name for each pasted column; `null` skips that column. */
export type Mapping = (string | null)[];

const norm = (s: string) => s.trim().toLocaleLowerCase();

/** Spreadsheet-style column letter: 0 -> A, 25 -> Z, 26 -> AA. */
export function columnLetter(index: number): string {
  let n = index;
  let out = "";
  do {
    out = String.fromCharCode(65 + (n % 26)) + out;
    n = Math.floor(n / 26) - 1;
  } while (n >= 0);
  return out;
}

/** The first row is a header row when most of its cells name a column of the table. */
export function looksLikeHeaders(matrix: string[][], columns: ColumnMeta[]): boolean {
  const first = (matrix[0] ?? []).map(norm).filter(Boolean);
  if (first.length === 0 || matrix.length < 2) return false;
  const names = new Set(columns.map((c) => norm(c.name)));
  return first.filter((cell) => names.has(cell)).length * 2 >= first.length;
}

/** With headers, map by name; without, by position. A table column is used at most once. */
export function autoMap(matrix: string[][], hasHeaders: boolean, columns: ColumnMeta[]): Mapping {
  const width = matrix[0]?.length ?? 0;
  const used = new Set<string>();
  const take = (col: ColumnMeta | undefined) => {
    if (!col || used.has(col.name)) return null;
    used.add(col.name);
    return col.name;
  };
  return Array.from({ length: width }, (_, i) =>
    hasHeaders ? take(columns.find((c) => norm(c.name) === norm(matrix[0]![i] ?? ""))) : take(columns[i]),
  );
}

/** Keys whose columns are all present in the paste, primary key first. */
export function usableKeys(keys: UniqueKey[], mapping: Mapping): UniqueKey[] {
  const mapped = new Set(mapping.filter(Boolean));
  return keys.filter((k) => k.columns.every((c) => mapped.has(c)));
}

/** Mapped columns an existing row may have changed: writable, and not part of the key. */
export function updatableColumns(mapping: Mapping, key: string[], columns: ColumnMeta[]): string[] {
  return mapping.filter(
    (name): name is string => !!name && !key.includes(name) && !!columns.find((c) => c.name === name)?.editable,
  );
}

export interface BuildOptions {
  mode: PasteMode;
  matrix: string[][];
  hasHeaders: boolean;
  mapping: Mapping;
  /** Columns of the chosen unique key; empty for a plain insert. */
  key: string[];
  /** Columns to update on existing rows. */
  update: string[];
  columns: ColumnMeta[];
  /** Read the literal text `NULL` as SQL NULL. */
  nullText: boolean;
}

export interface Built {
  request: PasteRequest;
  /** Entirely empty pasted rows, which are left out. */
  blankRows: number;
  /** Index in the pasted block (0-based, counting a header row) of each request row. */
  sourceRows: number[];
}

export function buildRequest(o: BuildOptions): Built {
  const picked = o.mapping.flatMap((name, i) => (name ? [{ name, i }] : []));
  const meta = picked.map((p) => o.columns.find((c) => c.name === p.name)!);
  const rows: (string | null)[][] = [];
  const sourceRows: number[] = [];
  let blankRows = 0;

  o.matrix.forEach((source, r) => {
    if (o.hasHeaders && r === 0) return;
    const cells = picked.map((p) => source[p.i] ?? "");
    // Selections dragged past the data end in empty rows; they are not rows to insert.
    if (cells.every((c) => c === "")) {
      blankRows++;
      return;
    }
    rows.push(
      cells.map((text, j) => {
        if (o.nullText && text === "NULL") return null;
        // A blank key is "no key", whatever the column type.
        if (text === "" && o.key.includes(picked[j]!.name)) return null;
        // An empty cell is "no value" for anything but text, also where the column is NOT NULL:
        // the backend then applies the column default, or reports the cell as required.
        return text === "" && meta[j]!.kind !== "text" ? null : text;
      }),
    );
    sourceRows.push(r);
  });

  const columns = picked.map((p) => p.name);
  return {
    request: {
      mode: o.mode,
      columns,
      key: o.key,
      update: o.mode === "insert" ? [] : o.update.filter((c) => columns.includes(c) && !o.key.includes(c)),
      rows,
    },
    blankRows,
    sourceRows,
  };
}
