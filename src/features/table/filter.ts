import type { ColumnMeta, FilterOp, RowFilter, ValueKind } from "../../lib/types";

/** Operators that take no value. */
export const takesNoValue = (op: FilterOp) => op === "isNull" || op === "isNotNull" || op === "isEmpty" || op === "isNotEmpty";
/** Operators that take a list of values. */
export const takesList = (op: FilterOp) => op === "in" || op === "notIn";

/** The longest list one condition takes (`db::filter::MAX_VALUES`). */
export const MAX_VALUES = 200;

// Columns whose value is compared as the text the grid shows: JSON, uuid, arrays, …
const SHOWN_OPS: FilterOp[] = ["contains", "notContains", "equals", "notEquals", "startsWith", "endsWith", "in", "notIn", "isNull", "isNotNull"];
const TEXT_OPS: FilterOp[] = [
  "contains",
  "notContains",
  "equals",
  "notEquals",
  "startsWith",
  "endsWith",
  "in",
  "notIn",
  "between",
  "isEmpty",
  "isNotEmpty",
  "isNull",
  "isNotNull",
];
const ORDERED_OPS: FilterOp[] = [
  "equals",
  "notEquals",
  "greater",
  "greaterOrEqual",
  "less",
  "lessOrEqual",
  "between",
  "in",
  "notIn",
  "contains",
  "startsWith",
  "isNull",
  "isNotNull",
];

/** What the backend accepts for a column of this kind, most useful first (see `db::filter`). */
export function opsFor(kind: ValueKind): FilterOp[] {
  switch (kind) {
    case "number":
    case "temporal":
      return ORDERED_OPS;
    case "bool":
      return ["equals", "notEquals", "isNull", "isNotNull"];
    case "binary":
      return ["isNull", "isNotNull"];
    case "text":
      return TEXT_OPS;
    default:
      return SHOWN_OPS;
  }
}

/** One row of the filter bar: a condition being written, not yet sent. */
export interface Draft {
  id: number;
  column: string;
  op: FilterOp;
  /** The value; for a list, the values as typed (see `parseValues`); for a range, its first end. */
  value: string;
  /** The last end of a range ("is between"). */
  to?: string;
  /** Switched off: kept in the bar, not sent. */
  off?: boolean;
  /**
   * The condition really is about the empty text (it came from an empty cell). Without
   * this, an empty value box means "not written yet".
   */
  emptyText?: boolean;
}

let nextId = 1;

export function newDraft(column: ColumnMeta): Draft {
  return { id: nextId++, column: column.name, op: opsFor(column.kind)[0]!, value: "" };
}

/** Moving a condition to another column keeps its operator when the new column allows it. */
export function withColumn(draft: Draft, column: ColumnMeta): Draft {
  const ops = opsFor(column.kind);
  return { ...draft, column: column.name, op: ops.includes(draft.op) ? draft.op : ops[0]! };
}

/**
 * The values of a list as typed or pasted: separated by commas or line breaks (a column
 * copied from a spreadsheet arrives as lines), spaces around each trimmed, empty entries
 * dropped, each value once. Double quotes keep a value as it is — commas, spaces and all;
 * a quote inside them is written twice.
 */
export function parseValues(text: string): string[] {
  const values: string[] = [];
  let quoted: string | null = null; // the quoted part of the entry, once its quotes closed
  let plain = "";
  let open = false;
  const end = () => {
    const value = quoted === null ? plain.trim() : quoted + plain.trim();
    if ((quoted !== null || value !== "") && !values.includes(value)) values.push(value);
    quoted = null;
    plain = "";
  };
  for (let i = 0; i < text.length; i++) {
    const ch = text[i]!;
    if (open) {
      if (ch !== '"') quoted += ch;
      else if (text[i + 1] === '"') {
        quoted += '"';
        i++;
      } else open = false;
    } else if (ch === '"' && quoted === null && plain.trim() === "") {
      open = true;
      quoted = "";
    } else if (ch === "," || ch === "\n" || ch === "\r") end();
    else plain += ch;
  }
  end();
  return values;
}

/** The text `parseValues` reads back as these values. */
export function formatValues(values: string[]): string {
  return values.map((v) => (v === "" || v !== v.trim() || /[",\n\r]/.test(v) ? `"${v.replaceAll('"', '""')}"` : v)).join(", ");
}

export function toDrafts(filters: RowFilter[]): Draft[] {
  return filters.map((f) => {
    const draft = { id: nextId++, column: f.column, op: f.op };
    if (takesList(f.op)) return { ...draft, value: formatValues(f.values ?? []) };
    if (f.op === "between") return { ...draft, value: f.values?.[0] ?? "", to: f.values?.[1] ?? "" };
    return { ...draft, value: f.value ?? "", emptyText: f.value === "" };
  });
}

/**
 * The conditions to send. A row that is switched off is not one, nor is a row that is not
 * written yet: an empty value box, an empty list, a range with one end.
 */
export function toFilters(drafts: Draft[]): RowFilter[] {
  return drafts.flatMap((d): RowFilter[] => {
    if (d.off) return [];
    if (takesNoValue(d.op)) return [{ column: d.column, op: d.op, value: null }];
    if (takesList(d.op)) {
      const values = parseValues(d.value);
      return values.length > 0 ? [{ column: d.column, op: d.op, value: null, values }] : [];
    }
    if (d.op === "between") {
      const ends = [d.value.trim(), (d.to ?? "").trim()];
      return ends.every((v) => v !== "") ? [{ column: d.column, op: d.op, value: null, values: ends }] : [];
    }
    return d.value !== "" || d.emptyText ? [{ column: d.column, op: d.op, value: d.value }] : [];
  });
}

export function sameFilters(a: RowFilter[], b: RowFilter[]): boolean {
  const same = (x: string[] = [], y: string[] = []) => x.length === y.length && x.every((v, i) => v === y[i]);
  return a.length === b.length && a.every((f, i) => f.column === b[i]!.column && f.op === b[i]!.op && f.value === b[i]!.value && same(f.values, b[i]!.values));
}

/**
 * "Show the rows that hold what this cell holds." `null` when that cannot be asked:
 * a binary value, or one cut for preview (the cell no longer shows all of it).
 */
export function cellFilter(column: ColumnMeta, value: string | null, truncated: boolean): RowFilter | null {
  if (value === null) return { column: column.name, op: "isNull", value: null };
  if (column.kind === "binary" || truncated) return null;
  return { column: column.name, op: "equals", value };
}

/** "Hide the rows that hold what this cell holds." `null` as for `cellFilter`. */
export function cellExclusion(column: ColumnMeta, value: string | null, truncated: boolean): RowFilter | null {
  if (value === null) return { column: column.name, op: "isNotNull", value: null };
  if (column.kind === "binary" || truncated) return null;
  return { column: column.name, op: "notEquals", value };
}

/** Why several cells of a column cannot be made a list of values to show. */
export type NoList = "binary" | "truncated" | "onlyNull" | "tooMany";

/**
 * "Show the rows that hold any of these values": the values of several cells of one column,
 * each once. NULL is not a value a list can hold, so NULL cells are left out.
 */
export function cellsFilter(column: ColumnMeta, cells: { value: string | null; truncated: boolean }[]): RowFilter | NoList {
  if (column.kind === "binary") return "binary";
  if (cells.some((c) => c.truncated)) return "truncated";
  const values = [...new Set(cells.flatMap((c) => (c.value === null ? [] : [c.value])))];
  if (values.length === 0) return "onlyNull";
  if (values.length > MAX_VALUES) return "tooMany";
  return { column: column.name, op: "in", value: null, values };
}
