import { describe, expect, it } from "vitest";

import type { TypeChoice } from "../../lib/types";
import { blankColumn, firstColumns, move, tableSpec, withType } from "./newTable";

const types: TypeChoice[] = [
  { name: "INT", params: { kind: "none" }, family: "integer", timestamp: false },
  { name: "VARCHAR", params: { kind: "length", required: true, default: 255, max: 16383 }, family: "text", timestamp: false },
  { name: "DECIMAL", params: { kind: "precision", defaultPrecision: 12, defaultScale: 2, max: 65 }, family: "number", timestamp: false },
  { name: "DATETIME", params: { kind: "none" }, family: "time", timestamp: true },
  { name: "TEXT", params: { kind: "none" }, family: "text", timestamp: false },
];

describe("the table form", () => {
  it("starts with a self-numbering id and an empty text column", () => {
    const [id, second] = firstColumns("mysql", types);
    expect(id).toMatchObject({ name: "id", type: "INT", auto: true, primary: true, unsigned: true });
    expect(second).toMatchObject({ name: "", type: "VARCHAR", length: "255" });
  });

  it("gives a new type its first offer and drops what no longer fits", () => {
    const id = firstColumns("mysql", types)[0]!;
    const asText = withType({ ...id, defaultKind: "value" }, types[1]!);
    expect(asText).toMatchObject({ type: "VARCHAR", length: "255", auto: false, unsigned: false, defaultKind: "value" });
    expect(withType(asText, types[2]!)).toMatchObject({ length: "12", scale: "2" });
    const stamped = withType({ ...blankColumn(types, "DATETIME"), defaultKind: "currentTimestamp" }, types[4]!);
    expect(stamped.defaultKind).toBe("none");
  });

  it("shapes the spec: numbers as numbers, a key column never NULL, empty comments left out", () => {
    const columns = [
      { ...blankColumn(types, "VARCHAR"), name: "code", length: " 20 ", primary: true, nullable: true },
      { ...blankColumn(types, "DECIMAL"), name: "total", length: "10", scale: "x", defaultKind: "value" as const, defaultValue: "0", comment: "  " },
    ];
    const spec = tableSpec("shop", null, " orders ", "", columns);
    expect(spec.comment).toBeNull();
    expect(spec.table).toEqual({ database: "shop", schema: null, name: " orders " });
    expect(spec.columns[0]).toMatchObject({ name: "code", length: 20, nullable: false, key: true });
    expect(spec.columns[1]).toMatchObject({ length: 10, scale: null, default: { kind: "value", value: "0" }, comment: null });
  });

  it("moves a column one place, and not past either end", () => {
    expect(move(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
    expect(move(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
    expect(move(["a", "b"], 0, -1)).toEqual(["a", "b"]);
  });
});
