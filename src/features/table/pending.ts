// Staged edits for one page of a table. Nothing here talks to the database: the grid only
// builds up a `Pending` value, and saving turns it into a `ChangeSet` for the backend.
//
// Rows are addressed by index: `0 .. page.rows.length-1` are the loaded rows, anything above
// is a new row. That is stable because the page cannot change while edits are pending.

import type { ChangeSet, ColumnMeta, RowPage } from "../../lib/types";

/** `undefined` in a new row means "leave it to the server default". */
export type Draft = string | null | undefined;

export interface Pending {
  /** row -> column -> new value, for loaded rows. A cell equal to its original is not kept. */
  edits: Record<number, Record<number, string | null>>;
  inserts: Draft[][];
  deletes: Record<number, true>;
}

export const EMPTY: Pending = { edits: {}, inserts: [], deletes: {} };

export interface Cell {
  r: number;
  c: number;
}

export type Blocker = "column" | "truncated" | "deleted" | "key";

const truncatedCache = new WeakMap<RowPage, Set<number>>();

export function isTruncated(page: RowPage, r: number, c: number): boolean {
  let set = truncatedCache.get(page);
  if (!set) {
    set = new Set(page.truncated.map(([row, col]) => row * page.columns.length + col));
    truncatedCache.set(page, set);
  }
  return set.has(r * page.columns.length + c);
}

export function rowCount(page: RowPage, p: Pending): number {
  return page.rows.length + p.inserts.length;
}

export function isInserted(page: RowPage, r: number): boolean {
  return r >= page.rows.length;
}

export function cellValue(page: RowPage, p: Pending, r: number, c: number): Draft {
  if (isInserted(page, r)) return p.inserts[r - page.rows.length]?.[c];
  const edit = p.edits[r];
  return edit && c in edit ? edit[c] : page.rows[r]?.[c];
}

export function isEdited(p: Pending, r: number, c: number): boolean {
  const edit = p.edits[r];
  return !!edit && c in edit;
}

function keyColumns(page: RowPage): number[] {
  return page.primaryKey.map((name) => page.columns.findIndex((col) => col.name === name));
}

/** Why a cell cannot be edited, or `null` when it can. */
export function blocker(page: RowPage, p: Pending, r: number, c: number): Blocker | null {
  const col = page.columns[c];
  if (!col?.editable) return "column";
  if (isInserted(page, r)) return null;
  if (p.deletes[r]) return "deleted";
  // The optimistic check needs the complete original value.
  if (isTruncated(page, r, c)) return "truncated";
  // The row must be addressable by a complete, untruncated primary key.
  for (const k of keyColumns(page)) {
    if (k < 0 || page.rows[r]?.[k] == null || isTruncated(page, r, k)) return "key";
  }
  return null;
}

/**
 * What an empty input means for a column: an empty string is a real value only for text;
 * for numbers, dates and the like it can only mean "no value".
 */
export function typedValue(col: ColumnMeta, text: string): string | null {
  return text === "" && col.nullable && col.kind !== "text" ? null : text;
}

/** The value "Clear" writes: NULL where allowed, otherwise an empty string. */
export function clearedValue(col: ColumnMeta): string | null {
  return col.nullable ? null : "";
}

export interface SetResult {
  pending: Pending;
  applied: number;
  skipped: number;
}

export function setCells(page: RowPage, p: Pending, cells: (Cell & { value: string | null })[]): SetResult {
  const edits = { ...p.edits };
  const inserts = p.inserts.slice();
  const copied = new Set<number>(); // rows already cloned in this call
  let applied = 0;
  let skipped = 0;

  for (const { r, c, value } of cells) {
    if (r < 0 || r >= rowCount(page, p) || blocker(page, p, r, c)) {
      skipped++;
      continue;
    }
    applied++;
    if (isInserted(page, r)) {
      const i = r - page.rows.length;
      if (!copied.has(r)) {
        inserts[i] = inserts[i]!.slice();
        copied.add(r);
      }
      inserts[i]![c] = value;
      continue;
    }
    if (!copied.has(r)) {
      edits[r] = { ...edits[r] };
      copied.add(r);
    }
    if (value === page.rows[r]![c]) delete edits[r]![c];
    else edits[r]![c] = value;
    if (Object.keys(edits[r]!).length === 0) delete edits[r];
  }
  return { pending: { ...p, edits, inserts }, applied, skipped };
}

/** Mark loaded rows for deletion (or restore them); new rows are simply dropped. */
export function setDeleted(page: RowPage, p: Pending, rows: number[], deleted: boolean): Pending {
  const deletes = { ...p.deletes };
  const dropped = new Set<number>();
  for (const r of rows) {
    if (isInserted(page, r)) {
      if (deleted) dropped.add(r - page.rows.length);
    } else if (r >= 0) {
      if (deleted) deletes[r] = true;
      else delete deletes[r];
    }
  }
  const inserts = dropped.size ? p.inserts.filter((_, i) => !dropped.has(i)) : p.inserts;
  return { ...p, deletes, inserts };
}

export function addRows(page: RowPage, p: Pending, count = 1): Pending {
  const blank = (): Draft[] => new Array<Draft>(page.columns.length).fill(undefined);
  return { ...p, inserts: [...p.inserts, ...Array.from({ length: count }, blank)] };
}

export interface PasteResult extends SetResult {
  addedRows: number;
  /** Source columns that fell off the right edge of the table. */
  droppedColumns: number;
}

/**
 * Paste a matrix with its top-left cell at `at`. A single copied cell fills the whole
 * selection `to`. Rows past the end of the table become new rows, like in a spreadsheet.
 */
export function pasteMatrix(page: RowPage, p: Pending, matrix: string[][], at: Cell, to: Cell = at): PasteResult {
  const columns = page.columns.length;
  const none: PasteResult = { pending: p, applied: 0, skipped: 0, addedRows: 0, droppedColumns: 0 };
  if (matrix.length === 0 || matrix[0]!.length === 0) return none;

  const single = matrix.length === 1 && matrix[0]!.length === 1;
  const height = single ? to.r - at.r + 1 : matrix.length;
  const width = single ? to.c - at.c + 1 : matrix[0]!.length;
  const droppedColumns = Math.max(0, at.c + width - columns);

  const addedRows = Math.max(0, at.r + height - rowCount(page, p));
  const grown = addedRows ? addRows(page, p, addedRows) : p;

  const cells: (Cell & { value: string | null })[] = [];
  for (let i = 0; i < height; i++) {
    for (let j = 0; j < width && at.c + j < columns; j++) {
      const text = single ? matrix[0]![0]! : matrix[i]![j]!;
      cells.push({ r: at.r + i, c: at.c + j, value: typedValue(page.columns[at.c + j]!, text) });
    }
  }
  return { ...setCells(page, grown, cells), addedRows, droppedColumns };
}

export interface Summary {
  editedCells: number;
  editedRows: number;
  inserted: number;
  deleted: number;
  /** Rows the save will touch. */
  rows: number;
}

export function summarize(p: Pending): Summary {
  const live = Object.entries(p.edits).filter(([r]) => !p.deletes[Number(r)]);
  const editedCells = live.reduce((n, [, cols]) => n + Object.keys(cols).length, 0);
  const deleted = Object.keys(p.deletes).length;
  return {
    editedCells,
    editedRows: live.length,
    inserted: p.inserts.length,
    deleted,
    rows: live.length + p.inserts.length + deleted,
  };
}

export function isDirty(p: Pending): boolean {
  return summarize(p).rows > 0;
}

/** Translate staged edits into the backend's change set. */
export function toChangeSet(page: RowPage, p: Pending): ChangeSet {
  const keys = keyColumns(page);
  const keyOf = (r: number) => keys.map((k) => ({ column: page.columns[k]!.name, value: page.rows[r]![k]! }));
  const changes: ChangeSet = { updates: [], inserts: [], deletes: [] };

  for (const r of Object.keys(p.deletes).map(Number).sort((a, b) => a - b)) {
    changes.deletes.push({ key: keyOf(r) });
  }
  for (const r of Object.keys(p.edits).map(Number).sort((a, b) => a - b)) {
    if (p.deletes[r]) continue; // a deleted row's edits are moot
    changes.updates.push({
      key: keyOf(r),
      changes: Object.entries(p.edits[r]!).map(([c, value]) => ({
        column: page.columns[Number(c)]!.name,
        original: page.rows[r]![Number(c)]!,
        value,
      })),
    });
  }
  for (const row of p.inserts) {
    const values = row.flatMap((value, c) => (value === undefined ? [] : [{ column: page.columns[c]!.name, value }]));
    changes.inserts.push({ values });
  }
  return changes;
}

// ---- undo / redo -------------------------------------------------------------------

export interface History {
  stack: Pending[];
  index: number;
}

export const NO_HISTORY: History = { stack: [EMPTY], index: 0 };
const HISTORY_LIMIT = 200;

export const current = (h: History): Pending => h.stack[h.index]!;
export const canUndo = (h: History): boolean => h.index > 0;
export const canRedo = (h: History): boolean => h.index < h.stack.length - 1;

export function record(h: History, next: Pending): History {
  if (next === current(h)) return h;
  const stack = [...h.stack.slice(0, h.index + 1), next].slice(-HISTORY_LIMIT);
  return { stack, index: stack.length - 1 };
}

export const undo = (h: History): History => (canUndo(h) ? { ...h, index: h.index - 1 } : h);
export const redo = (h: History): History => (canRedo(h) ? { ...h, index: h.index + 1 } : h);
