import type { FindMode, NameHit, ObjectKind, RowFilter, TableFind, TableRef } from "../../lib/types";

/** The most conditions a table tab's filter takes (the backend's `MAX_FILTERS`). */
export const MAX_FILTERS = 20;

/** One table of a search: its result, or why it could not be read. */
export interface Searched {
  table: TableRef;
  result: TableFind | null;
  error: string | null;
}

/**
 * The filter that shows the rows a search counted: the columns that hold the value, any one
 * of them, tested the way the search tested them. At most `MAX_FILTERS` columns.
 */
export function findFilters(columns: string[], mode: FindMode, text: string): RowFilter[] {
  return columns.slice(0, MAX_FILTERS).map((column) => ({ column, op: mode, value: text }));
}

/** `orders`, or `shop.orders` when the search covers more than one schema. */
export const tableLabel = (table: TableRef, withSchema: boolean) => (withSchema && table.schema ? `${table.schema}.${table.name}` : table.name);

/** What a search has found so far. */
export function tally(list: Searched[]) {
  return {
    found: list.filter((s) => (s.result?.rows ?? 0) > 0),
    failed: list.filter((s) => s.error !== null),
    /** Tables without a column the value can be in: not read at all. */
    skipped: list.filter((s) => s.result !== null && s.result.searched.length === 0).length,
  };
}

export interface NameGroup {
  schema: string | null;
  table: string;
  kind: ObjectKind;
  /** The table's own name holds the text. */
  named: boolean;
  columns: { column: string; dataType: string | null }[];
}

/** Name matches by table, in the order the backend gave them (tables before their columns). */
export function groupNames(hits: NameHit[]): NameGroup[] {
  const groups: NameGroup[] = [];
  for (const h of hits) {
    let g = groups.find((x) => x.schema === h.schema && x.table === h.table);
    if (!g) {
      g = { schema: h.schema, table: h.table, kind: h.kind, named: false, columns: [] };
      groups.push(g);
    }
    if (h.column === null) g.named = true;
    else g.columns.push({ column: h.column, dataType: h.dataType });
  }
  return groups;
}
