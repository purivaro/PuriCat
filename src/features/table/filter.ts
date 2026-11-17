import type { ColumnMeta, FilterOp, RowFilter, ValueKind } from "../../lib/types";

/** Operators that take no value. */
export const takesNoValue = (op: FilterOp) => op === "isNull" || op === "isNotNull";

const TEXT_OPS: FilterOp[] = ["contains", "notContains", "equals", "notEquals", "startsWith", "endsWith", "isNull", "isNotNull"];
const ORDERED_OPS: FilterOp[] = [
  "equals",
  "notEquals",
  "greater",
  "greaterOrEqual",
  "less",
  "lessOrEqual",
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
    default:
      return TEXT_OPS;
  }
}

/** One row of the filter bar: a condition being written, not yet sent. */
export interface Draft {
  id: number;
  column: string;
  op: FilterOp;
  value: string;
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

export function toDrafts(filters: RowFilter[]): Draft[] {
  return filters.map((f) => ({ id: nextId++, column: f.column, op: f.op, value: f.value ?? "", emptyText: f.value === "" }));
}

/** The conditions to send. A row whose value box is still empty is not a condition yet. */
export function toFilters(drafts: Draft[]): RowFilter[] {
  return drafts
    .filter((d) => takesNoValue(d.op) || d.value !== "" || d.emptyText)
    .map((d) => ({ column: d.column, op: d.op, value: takesNoValue(d.op) ? null : d.value }));
}

export function sameFilters(a: RowFilter[], b: RowFilter[]): boolean {
  return a.length === b.length && a.every((f, i) => f.column === b[i]!.column && f.op === b[i]!.op && f.value === b[i]!.value);
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
