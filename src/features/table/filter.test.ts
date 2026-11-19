import { describe, expect, it } from "vitest";

import type { ColumnMeta, ValueKind } from "../../lib/types";
import { cellExclusion, cellFilter, cellsFilter, formatValues, newDraft, opsFor, parseValues, sameFilters, toDrafts, toFilters, withColumn } from "./filter";

const col = (name: string, kind: ValueKind): ColumnMeta => ({
  name,
  dataType: "x",
  kind,
  nullable: true,
  default: null,
  primaryKey: false,
  generated: false,
  editable: true,
});

describe("filter drafts", () => {
  it("offers only what the backend accepts for the kind", () => {
    expect(opsFor("binary")).toEqual(["isNull", "isNotNull"]);
    expect(opsFor("bool")).not.toContain("contains");
    expect(opsFor("json")).not.toContain("greater");
    expect(opsFor("number")).toContain("greaterOrEqual");
    expect(opsFor("text")[0]).toBe("contains");
    expect(opsFor("temporal")[0]).toBe("equals");
  });

  it("keeps the operator across a column change only when it still applies", () => {
    const draft = { ...newDraft(col("price", "number")), op: "greater" as const, value: "5" };
    expect(withColumn(draft, col("total", "number"))).toMatchObject({ column: "total", op: "greater", value: "5" });
    expect(withColumn(draft, col("name", "text"))).toMatchObject({ column: "name", op: "contains" });
    expect(withColumn(draft, col("photo", "binary"))).toMatchObject({ op: "isNull" });
  });

  it("sends complete conditions only, and no value with a NULL test", () => {
    const drafts = [
      { id: 1, column: "name", op: "contains" as const, value: "a b " },
      { id: 2, column: "name", op: "equals" as const, value: "" },
      { id: 3, column: "note", op: "isNull" as const, value: "left over" },
    ];
    expect(toFilters(drafts)).toEqual([
      { column: "name", op: "contains", value: "a b " },
      { column: "note", op: "isNull", value: null },
    ]);
    expect(toFilters(toDrafts(toFilters(drafts)))).toEqual(toFilters(drafts));
    // A filter on the empty text (from an empty cell) survives a trip through the bar.
    const empty = [{ column: "name", op: "equals" as const, value: "" }];
    expect(toFilters(toDrafts(empty))).toEqual(empty);
  });

  it("compares applied filters by content", () => {
    const a = [{ column: "id", op: "equals" as const, value: "1" }];
    expect(sameFilters(a, [{ ...a[0]! }])).toBe(true);
    expect(sameFilters(a, [{ ...a[0]!, value: "2" }])).toBe(false);
    expect(sameFilters(a, [])).toBe(false);
  });

  it("builds a filter from a cell", () => {
    expect(cellFilter(col("name", "text"), "x", false)).toEqual({ column: "name", op: "equals", value: "x" });
    expect(cellFilter(col("name", "text"), "", false)).toEqual({ column: "name", op: "equals", value: "" });
    expect(cellFilter(col("name", "text"), null, false)).toEqual({ column: "name", op: "isNull", value: null });
    expect(cellFilter(col("note", "text"), "long…", true)).toBeNull();
    expect(cellFilter(col("photo", "binary"), "0x00", false)).toBeNull();
    expect(cellFilter(col("photo", "binary"), null, false)).toMatchObject({ op: "isNull" });
  });

  it("offers lists, ranges and the empty text only where the backend takes them", () => {
    for (const kind of ["number", "temporal", "text"] as const) {
      expect(opsFor(kind)).toEqual(expect.arrayContaining(["in", "notIn", "between"]));
    }
    expect(opsFor("text")).toEqual(expect.arrayContaining(["isEmpty", "isNotEmpty"]));
    expect(opsFor("number")).not.toContain("isEmpty");
    // Compared as the text the grid shows: a list of those, but no order and no empty text.
    expect(opsFor("json")).toEqual(expect.arrayContaining(["in", "notIn"]));
    for (const op of ["between", "isEmpty", "isNotEmpty"] as const) {
      expect(opsFor("json")).not.toContain(op);
      expect(opsFor("other")).not.toContain(op);
      expect(opsFor("bool")).not.toContain(op);
    }
    expect(opsFor("bool")).not.toContain("in");
    expect(opsFor("binary")).toEqual(["isNull", "isNotNull"]);
  });

  it("reads a list of values as typed or pasted", () => {
    expect(parseValues("a, b,c")).toEqual(["a", "b", "c"]);
    // A column copied from a spreadsheet: lines, Windows line ends, a trailing one.
    expect(parseValues("SO-1\r\nSO-2\nSO-3\n")).toEqual(["SO-1", "SO-2", "SO-3"]);
    expect(parseValues("  spaced out  ,, ,\n x ")).toEqual(["spaced out", "x"]);
    // Quotes keep commas, spaces and line breaks; a quote inside is written twice; "" is the empty text.
    expect(parseValues('"a, b", " c ", "say ""hi""", ""')).toEqual(["a, b", " c ", 'say "hi"', ""]);
    expect(parseValues('5" pipe, "x"y')).toEqual(['5" pipe', "xy"]);
    expect(parseValues("a, b, a")).toEqual(["a", "b"]);
    expect(parseValues("")).toEqual([]);
    expect(parseValues(" , \n")).toEqual([]);
    // What is shown for a list reads back as the same list.
    const values = ["plain", "a, b", " c ", 'say "hi"', "", "two\nlines"];
    expect(formatValues(values)).toBe('plain, "a, b", " c ", "say ""hi""", "", "two\nlines"');
    expect(parseValues(formatValues(values))).toEqual(values);
  });

  it("sends lists and ranges once they are written, and nothing that is switched off", () => {
    const drafts = [
      { id: 1, column: "status", op: "in" as const, value: "Pending,\nShipped" },
      { id: 2, column: "status", op: "notIn" as const, value: " , " },
      { id: 3, column: "total", op: "between" as const, value: " 10 ", to: "20" },
      { id: 4, column: "total", op: "between" as const, value: "10" },
      { id: 5, column: "note", op: "isEmpty" as const, value: "left over" },
      { id: 6, column: "note", op: "contains" as const, value: "x", off: true },
      { id: 7, column: "note", op: "isNull" as const, value: "", off: true },
    ];
    const sent = toFilters(drafts);
    expect(sent).toEqual([
      { column: "status", op: "in", value: null, values: ["Pending", "Shipped"] },
      { column: "total", op: "between", value: null, values: ["10", "20"] },
      { column: "note", op: "isEmpty", value: null },
    ]);
    expect(toFilters(toDrafts(sent))).toEqual(sent);
    expect(toDrafts(sent).map((d) => [d.value, d.to])).toEqual([
      ["Pending, Shipped", undefined],
      ["10", "20"],
      ["", undefined],
    ]);
  });

  it("compares the values of lists and ranges too", () => {
    const a = [{ column: "id", op: "in" as const, value: null, values: ["1", "2"] }];
    expect(sameFilters(a, [{ ...a[0]!, values: ["1", "2"] }])).toBe(true);
    expect(sameFilters(a, [{ ...a[0]!, values: ["1", "3"] }])).toBe(false);
    expect(sameFilters(a, [{ ...a[0]!, values: ["1"] }])).toBe(false);
    expect(sameFilters(a, [{ column: "id", op: "in", value: null }])).toBe(false);
  });

  it("builds the other cell-menu filters", () => {
    const name = col("name", "text");
    expect(cellExclusion(name, "x", false)).toEqual({ column: "name", op: "notEquals", value: "x" });
    expect(cellExclusion(name, null, false)).toEqual({ column: "name", op: "isNotNull", value: null });
    expect(cellExclusion(name, "long…", true)).toBeNull();
    expect(cellExclusion(col("photo", "binary"), "0x00", false)).toBeNull();

    const cell = (value: string | null, truncated = false) => ({ value, truncated });
    // Each value once, in the order met; NULL cells are left out.
    expect(cellsFilter(name, [cell("b"), cell(null), cell("a"), cell("b"), cell("")])).toEqual({
      column: "name",
      op: "in",
      value: null,
      values: ["b", "a", ""],
    });
    expect(cellsFilter(name, [cell(null), cell(null)])).toBe("onlyNull");
    expect(cellsFilter(name, [cell("a"), cell("lo…", true)])).toBe("truncated");
    expect(cellsFilter(col("photo", "binary"), [cell("0x00"), cell("0x01")])).toBe("binary");
    const many = Array.from({ length: 201 }, (_, i) => cell(String(i)));
    expect(cellsFilter(name, many)).toBe("tooMany");
    expect(cellsFilter(name, many.slice(0, 200))).toMatchObject({ op: "in" });
  });
});
