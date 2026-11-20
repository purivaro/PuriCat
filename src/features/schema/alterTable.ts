import type { ColumnDefault, ColumnDefinition, ColumnMeta, FkAction, IndexInfo, TableEdit, TableRef, TypeChoice } from "../../lib/types";
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
  changed: TableEdit[] = [],
  droppedKeys: string[] = [],
  keys: TableEdit[] = [],
): TableEdit[] {
  const columns = tableSpec(database, schema, table, "", added).columns;
  return [
    ...droppedKeys.map((name): TableEdit => ({ kind: "dropForeignKey", name })),
    ...columns.map((column): TableEdit => ({ kind: "addColumn", column })),
    ...existing
      .filter((c) => !c.drop && c.rename.trim() !== "" && c.rename.trim() !== c.name)
      .map((c): TableEdit => ({ kind: "renameColumn", from: c.name, to: c.rename.trim() })),
    ...changed,
    ...droppedIndexes.map((name): TableEdit => ({ kind: "dropIndex", name })),
    ...existing.filter((c) => c.drop).map((c): TableEdit => ({ kind: "dropColumn", name: c.name })),
    ...indexes.map((i): TableEdit => ({ kind: "addIndex", name: i.name.trim() || null, columns: i.columns, unique: i.unique })),
    ...keys,
  ];
}

/** An index that would lose a column the change drops. */
export function indexesLosing(indexes: IndexInfo[], existing: ColumnChange[]): string[] {
  const dropped = new Set(existing.filter((c) => c.drop).map((c) => c.name));
  return indexes.filter((i) => i.columns.some((c) => dropped.has(c))).map((i) => i.name);
}

/** A change of one column that is there now, as the form holds it. */
export interface ColumnEditDraft {
  /** "" keeps the type. */
  type: string;
  length: string;
  scale: string;
  unsigned: boolean;
  nullable: boolean;
  defaultKind: "keep" | ColumnDefault["kind"];
  defaultValue: string;
  comment: string;
}

export function startEdit(def: ColumnDefinition): ColumnEditDraft {
  return { type: "", length: "", scale: "", unsigned: false, nullable: def.nullable, defaultKind: "keep", defaultValue: "", comment: def.comment ?? "" };
}

const num = (text: string): number | null => (/^\d+$/.test(text.trim()) ? Number(text.trim()) : null);

/** The change the form describes, or `null` when it changes nothing. Only what differs is sent. */
export function columnEdit(def: ColumnDefinition, d: ColumnEditDraft, types: TypeChoice[]): TableEdit | null {
  const edit: Extract<TableEdit, { kind: "changeColumn" }> = { kind: "changeColumn", name: def.name };
  if (d.type !== "") {
    const choice = types.find((t) => t.name === d.type);
    edit.type = {
      type: d.type,
      length: choice?.params.kind === "none" ? null : num(d.length),
      scale: choice?.params.kind === "precision" ? num(d.scale) : null,
      unsigned: d.unsigned,
    };
  }
  if (d.nullable !== def.nullable) edit.nullable = d.nullable;
  if (d.defaultKind !== "keep") {
    edit.default =
      d.defaultKind === "value"
        ? { kind: "value", value: d.defaultValue }
        : d.defaultKind === "null"
          ? { kind: "null" }
          : d.defaultKind === "currentTimestamp"
            ? { kind: "currentTimestamp" }
            : { kind: "none" };
  }
  if (d.comment.trim() !== (def.comment ?? "").trim()) edit.comment = d.comment.trim();
  return Object.keys(edit).length > 2 ? edit : null;
}

export interface KeyDraft {
  key: string;
  name: string;
  columns: string[];
  /** The referenced table's name, in the same database (and schema). */
  table: string;
  refColumns: string[];
  onDelete: FkAction;
  onUpdate: FkAction;
}

let keyCounter = 0;
export const newKey = (): KeyDraft => ({
  key: `k${++keyCounter}`,
  name: "",
  columns: [],
  table: "",
  refColumns: [],
  onDelete: "noAction",
  onUpdate: "noAction",
});

/** A new key with a table and columns on both sides; until then it is left out of the change. */
export const keyReady = (k: KeyDraft): boolean => k.table !== "" && k.columns.length > 0 && k.refColumns.length > 0;

export function keyEdit(table: TableRef, k: KeyDraft): TableEdit {
  return {
    kind: "addForeignKey",
    name: k.name.trim() || null,
    columns: k.columns,
    references: { database: table.database, schema: table.schema, name: k.table },
    refColumns: k.refColumns,
    onDelete: k.onDelete,
    onUpdate: k.onUpdate,
  };
}
