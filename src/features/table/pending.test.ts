import { describe, expect, it } from "vitest";

import type { ColumnMeta, RowPage } from "../../lib/types";
import {
  addRows,
  blocker,
  canRedo,
  canUndo,
  cellIndex,
  cellValue,
  current,
  EMPTY,
  isDirty,
  NO_HISTORY,
  pasteMatrix,
  record,
  redo,
  rowKey,
  setCells,
  setDeleted,
  summarize,
  toChangeSet,
  typedValue,
  undo,
  withFull,
} from "./pending";

const col = (name: string, over: Partial<ColumnMeta> = {}): ColumnMeta => ({
  name,
  dataType: "text",
  kind: "text",
  nullable: true,
  default: null,
  primaryKey: false,
  generated: false,
  editable: true,
  ...over,
});

const page = (over: Partial<RowPage> = {}): RowPage => ({
  columns: [
    col("id", { kind: "number", nullable: false, primaryKey: true, generated: true, editable: false }),
    col("name", { nullable: false }),
    col("qty", { kind: "number" }),
    col("note"),
  ],
  rows: [
    ["1", "apple", "5", null],
    ["2", "pear", null, "long…"],
    ["3", "fig", "0", ""],
  ],
  truncated: [[1, 3]],
  primaryKey: ["id"],
  estimatedRows: 3,
  offset: 0,
  hasMore: false,
  orderedBy: ["id"],
  elapsedMs: 1,
  ...over,
});

describe("editing cells", () => {
  it("stages an edit and forgets it when the original value comes back", () => {
    const p = page();
    const a = setCells(p, EMPTY, [{ r: 0, c: 1, value: "apricot" }]);
    expect(a.applied).toBe(1);
    expect(cellValue(p, a.pending, 0, 1)).toBe("apricot");
    expect(isDirty(a.pending)).toBe(true);

    const b = setCells(p, a.pending, [{ r: 0, c: 1, value: "apple" }]);
    expect(b.pending.edits).toEqual({});
    expect(isDirty(b.pending)).toBe(false);
  });

  it("does not mutate the previous state (undo relies on it)", () => {
    const p = page();
    const a = setCells(p, EMPTY, [{ r: 0, c: 1, value: "x" }]).pending;
    const b = setCells(p, a, [{ r: 0, c: 2, value: "9" }]).pending;
    expect(a.edits).toEqual({ 0: { 1: "x" } });
    expect(b.edits).toEqual({ 0: { 1: "x", 2: "9" } });
    expect(EMPTY).toEqual({ edits: {}, inserts: [], deletes: {} });
  });

  it("refuses generated columns, truncated cells, deleted rows and unaddressable rows", () => {
    const p = page();
    expect(blocker(p, EMPTY, 0, 0)).toBe("column");
    expect(blocker(p, EMPTY, 1, 3)).toBe("truncated");
    expect(blocker(p, EMPTY, 0, 1)).toBeNull();
    const deleted = setDeleted(p, EMPTY, [0], true);
    expect(blocker(p, deleted, 0, 1)).toBe("deleted");

    const nullKey = page({ rows: [[null, "a", "1", null]], truncated: [] });
    expect(blocker(nullKey, EMPTY, 0, 1)).toBe("key");
    const cutKey = page({ truncated: [[0, 0]] });
    expect(blocker(cutKey, EMPTY, 0, 1)).toBe("key");

    const r = setCells(p, EMPTY, [
      { r: 0, c: 0, value: "9" },
      { r: 1, c: 3, value: "x" },
      { r: 99, c: 1, value: "x" },
      { r: 2, c: 1, value: "ok" },
    ]);
    expect([r.applied, r.skipped]).toEqual([1, 3]);
  });

  it("treats an empty input as NULL only where an empty string cannot be a value", () => {
    const [, name, qty, note] = page().columns;
    expect(typedValue(qty!, "")).toBeNull();
    expect(typedValue(note!, "")).toBe("");
    expect(typedValue(name!, "")).toBe("");
    expect(typedValue(col("n", { kind: "number", nullable: false }), "")).toBe("");
    expect(typedValue(qty!, "0")).toBe("0");
  });
});

describe("rows", () => {
  it("adds, fills and drops new rows; marks and restores loaded rows", () => {
    const p = page();
    let s = addRows(p, EMPTY, 2);
    s = setCells(p, s, [{ r: 3, c: 1, value: "kiwi" }]).pending;
    expect(cellValue(p, s, 3, 1)).toBe("kiwi");
    expect(cellValue(p, s, 3, 2)).toBeUndefined();

    s = setDeleted(p, s, [1, 4], true); // one loaded row, one new row
    expect(s.inserts).toHaveLength(1);
    expect(s.deletes).toEqual({ 1: true });
    expect(summarize(s)).toEqual({ editedCells: 0, editedRows: 0, inserted: 1, deleted: 1, rows: 2 });

    s = setDeleted(p, s, [1], false);
    expect(s.deletes).toEqual({});
  });
});

describe("pasting", () => {
  it("pastes a block, appends rows past the end and reports what did not fit", () => {
    const p = page();
    const r = pasteMatrix(
      p,
      EMPTY,
      [
        ["plum", "7", "n1", "extra"],
        ["lime", "", "n2", "extra"],
      ],
      { r: 2, c: 1 },
    );
    expect(r.addedRows).toBe(1);
    expect(r.droppedColumns).toBe(1);
    expect(r.applied).toBe(6);
    expect(cellValue(p, r.pending, 2, 1)).toBe("plum");
    expect(cellValue(p, r.pending, 2, 2)).toBe("7");
    expect(r.pending.inserts[0]).toEqual([undefined, "lime", null, "n2"]); // "" in a number column -> NULL
  });

  it("fills the selection with a single copied cell, skipping read-only cells", () => {
    const p = page();
    const r = pasteMatrix(p, EMPTY, [["x"]], { r: 0, c: 0 }, { r: 2, c: 1 });
    expect([r.applied, r.skipped]).toEqual([3, 3]);
    expect([0, 1, 2].map((row) => cellValue(p, r.pending, row, 1))).toEqual(["x", "x", "x"]);
  });

  it("ignores an empty clipboard", () => {
    expect(pasteMatrix(page(), EMPTY, [], { r: 0, c: 0 }).pending).toBe(EMPTY);
  });
});

describe("toChangeSet", () => {
  it("addresses rows by primary key and carries the original values", () => {
    const p = page();
    let s = setCells(p, EMPTY, [
      { r: 0, c: 1, value: "apricot" },
      { r: 0, c: 3, value: "hi" },
      { r: 2, c: 2, value: null },
    ]).pending;
    s = setDeleted(p, s, [2], true); // edits of a deleted row are dropped
    s = addRows(p, s);
    s = setCells(p, s, [
      { r: 3, c: 1, value: "kiwi" },
      { r: 3, c: 2, value: null },
    ]).pending;

    expect(toChangeSet(p, s)).toEqual({
      deletes: [{ key: [{ column: "id", value: "3" }] }],
      updates: [
        {
          key: [{ column: "id", value: "1" }],
          changes: [
            { column: "name", original: "apple", value: "apricot" },
            { column: "note", original: null, value: "hi" },
          ],
        },
      ],
      inserts: [
        {
          values: [
            { column: "name", value: "kiwi" },
            { column: "qty", value: null },
          ],
        },
      ],
    });
  });

  it("uses composite keys in key order", () => {
    const p = page({ primaryKey: ["name", "id"] });
    const s = setDeleted(p, EMPTY, [1], true);
    expect(toChangeSet(p, s).deletes[0]!.key).toEqual([
      { column: "name", value: "pear" },
      { column: "id", value: "2" },
    ]);
  });
});

describe("whole values", () => {
  it("a cell cut for preview becomes editable once its whole value is known", () => {
    const p = page();
    expect(blocker(p, EMPTY, 1, 3)).toBe("truncated");
    expect(withFull(p, {})).toBe(p); // nothing learned, nothing copied

    const whole = "long text, all of it";
    const known = withFull(p, { [cellIndex(p, 1, 3)]: whole });
    expect(known.rows[1]![3]).toBe(whole);
    expect(known.truncated).toEqual([]);
    expect(blocker(known, EMPTY, 1, 3)).toBeNull();
    // The loaded page is left as it was, and untouched rows are shared.
    expect(p.rows[1]![3]).toBe("long…");
    expect(known.rows[0]).toBe(p.rows[0]);
    expect(known.columns).toBe(p.columns);

    // The edit carries the complete original for the concurrency check.
    const edited = setCells(known, EMPTY, [{ r: 1, c: 3, value: "short now" }]).pending;
    expect(toChangeSet(known, edited).updates).toEqual([
      { key: [{ column: "id", value: "2" }], changes: [{ column: "note", original: whole, value: "short now" }] },
    ]);
    // Typing the whole original back is no edit at all.
    expect(setCells(known, edited, [{ r: 1, c: 3, value: whole }]).pending.edits).toEqual({});
  });

  it("addresses a row by its complete key, or not at all", () => {
    expect(rowKey(page(), 1)).toEqual([{ column: "id", value: "2" }]);
    expect(rowKey(page({ primaryKey: ["name", "id"] }), 1)).toEqual([
      { column: "name", value: "pear" },
      { column: "id", value: "2" },
    ]);
    expect(rowKey(page({ primaryKey: [] }), 1)).toBeNull();
    expect(rowKey(page({ primaryKey: ["qty"] }), 1)).toBeNull(); // NULL key value
    expect(rowKey(page({ primaryKey: ["note"] }), 1)).toBeNull(); // key cut for preview
  });
});

describe("history", () => {
  it("undoes, redoes and drops the redo branch on a new edit", () => {
    const p = page();
    const one = setCells(p, EMPTY, [{ r: 0, c: 1, value: "a" }]).pending;
    const two = setCells(p, one, [{ r: 0, c: 1, value: "b" }]).pending;
    let h = record(record(NO_HISTORY, one), two);
    expect(current(h)).toBe(two);
    h = undo(h);
    expect(current(h)).toBe(one);
    expect(canRedo(h)).toBe(true);
    h = redo(h);
    expect(current(h)).toBe(two);
    h = record(undo(undo(h)), one);
    expect(canRedo(h)).toBe(false);
    expect(canUndo(undo(h))).toBe(false);
    expect(record(h, current(h))).toBe(h); // no-op edits do not pollute the stack
  });
});
