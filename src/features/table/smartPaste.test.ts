import { describe, expect, it } from "vitest";

import type { ColumnMeta, UniqueKey } from "../../lib/types";
import { autoMap, buildRequest, columnLetter, looksLikeHeaders, updatableColumns, usableKeys } from "./smartPaste";

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

const columns = [
  col("id", { kind: "number", nullable: false, generated: true, editable: false, primaryKey: true }),
  col("order_no", { nullable: false }),
  col("status"),
  col("total", { kind: "number" }),
];
const keys: UniqueKey[] = [
  { name: "PRIMARY", columns: ["id"], primary: true },
  { name: "uq_no", columns: ["order_no"], primary: false },
  { name: "uq_pair", columns: ["order_no", "status"], primary: false },
];

describe("reading the pasted block", () => {
  it("names columns like a spreadsheet", () => {
    expect([0, 25, 26, 27, 701, 702].map(columnLetter)).toEqual(["A", "Z", "AA", "AB", "ZZ", "AAA"]);
  });

  it("detects a header row by column names, not by guessing from data", () => {
    expect(looksLikeHeaders([["Order_No ", "STATUS", "note"], ["SO-1", "Done", "x"]], columns)).toBe(true);
    expect(looksLikeHeaders([["SO-1", "Done"], ["SO-2", "New"]], columns)).toBe(false);
    expect(looksLikeHeaders([["order_no", "status"]], columns)).toBe(false); // a header with no data is data
    expect(looksLikeHeaders([], columns)).toBe(false);
  });

  it("maps by header name, or by position without headers", () => {
    expect(autoMap([["Total", "order_no", "unknown", "total"]], true, columns)).toEqual(["total", "order_no", null, null]);
    expect(autoMap([["1", "SO-1", "Done", "5", "extra"]], false, columns)).toEqual(["id", "order_no", "status", "total", null]);
  });
});

describe("keys and update columns", () => {
  it("offers only keys whose columns are all mapped", () => {
    expect(usableKeys(keys, ["order_no", "total"]).map((k) => k.name)).toEqual(["uq_no"]);
    expect(usableKeys(keys, ["id", "order_no", "status"]).map((k) => k.name)).toEqual(["PRIMARY", "uq_no", "uq_pair"]);
    expect(usableKeys(keys, ["total", null])).toEqual([]);
  });

  it("updates writable non-key columns only", () => {
    expect(updatableColumns(["id", "order_no", null, "total"], ["order_no"], columns)).toEqual(["total"]);
  });
});

describe("buildRequest", () => {
  const matrix = [
    ["order_no", "ignored", "total", "status"],
    ["SO-1", "x", "10.50", "Done"],
    ["", "", "", ""],
    ["SO-2", "y", "", "NULL"],
    ["", "z", "7", ""],
  ];
  const base = { matrix, hasHeaders: true, mapping: ["order_no", null, "total", "status"], columns, nullText: false };

  it("skips the header, unmapped columns and blank rows, and types empty cells", () => {
    const built = buildRequest({ ...base, mode: "upsert", key: ["order_no"], update: ["total", "status", "order_no"] });
    expect(built.request).toEqual({
      mode: "upsert",
      columns: ["order_no", "total", "status"],
      key: ["order_no"],
      update: ["total", "status"], // the key is never updated
      rows: [
        ["SO-1", "10.50", "Done"],
        ["SO-2", null, "NULL"], // empty number -> NULL; the text NULL stays text
        [null, "7", ""], // blank key -> no key; empty text stays an empty string
      ],
    });
    expect(built.blankRows).toBe(1);
    expect(built.sourceRows).toEqual([1, 3, 4]);
  });

  it("reads the text NULL as NULL when asked, and sends no update list for inserts", () => {
    const built = buildRequest({ ...base, mode: "insert", key: [], update: ["total"], nullText: true });
    expect(built.request.rows[1]).toEqual(["SO-2", null, null]);
    expect(built.request.update).toEqual([]);
    expect(built.request.rows[2]).toEqual(["", "7", ""]); // no key chosen: an empty order_no is just text
  });

  it("treats the first row as data without headers", () => {
    const built = buildRequest({ ...base, hasHeaders: false, mode: "insert", key: [], update: [] });
    expect(built.request.rows).toHaveLength(4);
    expect(built.request.rows[0]).toEqual(["order_no", "total", "status"]);
  });
});
