import type { ColumnMeta, IndexInfo, TableEdit } from "../../lib/types";
import { type ColumnDraft, tableSpec } from "./newTable";

/** What happens to a column that is there now. */
export interface ColumnChange {
  name: string;
  /** The new name, or "" to keep it. */
  rename: string;
  drop: boolean;
}

export interface IndexDraft {
  key: string;
  name: string;
  columns: string[];
  unique: boolean;
}

let counter = 0;
export const newIndex = (): IndexDraft => ({ key: `i${++counter}`, name: "", columns: [], unique: false });

export function startColumns(columns: ColumnMeta[]): ColumnChange[] {
  return columns.map((c) => ({ name: c.name, rename: "", drop: false }));
}

/**
 * The columns the table will have, by name, in order: what is kept (under its new name),
 * then what is added. An index can be made of these.
 */
export function columnsAfter(existing: ColumnChange[], added: ColumnDraft[]): string[] {
  return [...existing.filter((c) => !c.drop).map((c) => c.rename.trim() || c.name), ...added.map((c) => c.name.trim()).filter((n) => n !== "")];
}

/**
 * The changes, in the order that keeps names valid: columns added and renamed first, then
 * indexes dropped, then columns dropped, then indexes made (they may name the new columns).
 */
export function tableEdits(
  database: string,
  schema: string | null,
  table: string,
  existing: ColumnChange[],
  added: ColumnDraft[],
  droppedIndexes: string[],
  indexes: IndexDraft[],
): TableEdit[] {
  const columns = tableSpec(database, schema, table, "", added).columns;
  return [
    ...columns.map((column): TableEdit => ({ kind: "addColumn", column })),
    ...existing
      .filter((c) => !c.drop && c.rename.trim() !== "" && c.rename.trim() !== c.name)
      .map((c): TableEdit => ({ kind: "renameColumn", from: c.name, to: c.rename.trim() })),
    ...droppedIndexes.map((name): TableEdit => ({ kind: "dropIndex", name })),
    ...existing.filter((c) => c.drop).map((c): TableEdit => ({ kind: "dropColumn", name: c.name })),
    ...indexes.map((i): TableEdit => ({ kind: "addIndex", name: i.name.trim() || null, columns: i.columns, unique: i.unique })),
  ];
}

/** An index that would lose a column the change drops. */
export function indexesLosing(indexes: IndexInfo[], existing: ColumnChange[]): string[] {
  const dropped = new Set(existing.filter((c) => c.drop).map((c) => c.name));
  return indexes.filter((i) => i.columns.some((c) => dropped.has(c))).map((i) => i.name);
}
