import { describe, expect, it } from "vitest";

import type { ColumnMeta, ValueKind } from "../../lib/types";
import { cellFilter, newDraft, opsFor, sameFilters, toDrafts, toFilters, withColumn } from "./filter";

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
});
