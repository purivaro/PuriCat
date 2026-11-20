import { describe, expect, it } from "vitest";

import type { TableFind } from "../../lib/types";
import { findFilters, groupNames, MAX_FILTERS, tableLabel, tally } from "./find";

const table = (name: string, schema: string | null = null) => ({ database: "shop", schema, name });
const result = (rows: number, searched = ["a"]): TableFind => ({ searched, hits: rows ? [{ column: "a", rows }] : [], rows, elapsedMs: 1 });

describe("find in database", () => {
  it("opens a table on the columns that hold the value, any one of them, as the search tested them", () => {
    expect(findFilters(["name", "email"], "contains", "ann")).toEqual([
      { column: "name", op: "contains", value: "ann" },
      { column: "email", op: "contains", value: "ann" },
    ]);
    expect(findFilters(["id"], "equals", "42")).toEqual([{ column: "id", op: "equals", value: "42" }]);
    const many = Array.from({ length: 30 }, (_, i) => `c${i}`);
    expect(findFilters(many, "startsWith", "x")).toHaveLength(MAX_FILTERS);
  });

  it("names a table with its schema only when the search covers several", () => {
    expect(tableLabel(table("orders", "shop"), true)).toBe("shop.orders");
    expect(tableLabel(table("orders", "shop"), false)).toBe("orders");
    expect(tableLabel(table("orders"), true)).toBe("orders");
  });

  it("counts what was found, what failed and what had nowhere to look", () => {
    const list = [
      { table: table("a"), result: result(3), error: null },
      { table: table("b"), result: result(0), error: null },
      { table: table("c"), result: result(0, []), error: null },
      { table: table("d"), result: null, error: "denied" },
    ];
    const t = tally(list);
    expect(t.found.map((s) => s.table.name)).toEqual(["a"]);
    expect(t.failed.map((s) => s.table.name)).toEqual(["d"]);
    expect(t.skipped).toBe(1);
  });

  it("groups name matches by table", () => {
    const groups = groupNames([
      { schema: "s", table: "orders", kind: "table", column: null, dataType: null },
      { schema: "s", table: "orders", kind: "table", column: "order_id", dataType: "integer" },
      { schema: "s", table: "lines", kind: "view", column: "order_id", dataType: "bigint" },
      { schema: "t", table: "orders", kind: "table", column: "order_no", dataType: "text" },
    ]);
    expect(groups.map((g) => [g.schema, g.table, g.named, g.columns.map((c) => c.column).join()])).toEqual([
      ["s", "orders", true, "order_id"],
      ["s", "lines", false, "order_id"],
      ["t", "orders", false, "order_no"],
    ]);
  });
});
