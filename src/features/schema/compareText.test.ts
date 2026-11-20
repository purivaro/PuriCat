import { describe, expect, it } from "vitest";

import type { ColumnShape, Comparison } from "../../lib/types";
import { describeColumn, describeIndex, describeKey, reportText, tally } from "./compareText";

const col = (more: Partial<ColumnShape> = {}): ColumnShape => ({
  name: "total",
  columnType: "decimal(12,2)",
  nullable: false,
  default: null,
  extra: "",
  comment: null,
  collation: null,
  ...more,
});

const comparison: Comparison = {
  onlyA: ["only_a"],
  onlyB: [],
  same: ["customers"],
  changed: [
    {
      name: "orders",
      columns: [
        { name: "total", a: col(), b: col({ columnType: "decimal(14,2)", nullable: true }), fields: ["type", "nullable"] },
        { name: "note", a: col({ name: "note", columnType: "text", nullable: true, comment: "ห้ามว่าง" }), b: null, fields: [] },
      ],
      indexes: [{ a: null, b: { name: "u_total", columns: ["total"], unique: true, primary: false, predicate: null } }],
      foreignKeys: [],
    },
  ],
  viewsOnlyA: [],
  viewsOnlyB: ["v_b"],
};

describe("structure compare", () => {
  it("describes columns, indexes and keys in one line", () => {
    expect(describeColumn(col({ default: "'0'", extra: "auto_increment", comment: "ยอด" }))).toBe(
      "decimal(12,2) NOT NULL DEFAULT '0' auto_increment COMMENT \"ยอด\"",
    );
    expect(describeIndex({ name: "recent", columns: ["id"], unique: false, primary: false, predicate: "(total > 0)" })).toBe(
      "INDEX recent (id) WHERE (total > 0)",
    );
    expect(describeKey({ name: "fk", columns: ["c"], references: "customers", refColumns: ["id"], onDelete: "CASCADE", onUpdate: "NO ACTION" })).toBe(
      "fk (c) → customers (id) ON DELETE CASCADE ON UPDATE NO ACTION",
    );
  });

  it("counts and tells when nothing differs", () => {
    expect(tally(comparison)).toEqual({ same: 1, changed: 1, onlyA: 1, onlyB: 0, identical: false });
    expect(tally({ onlyA: [], onlyB: [], same: ["t"], changed: [], viewsOnlyA: [], viewsOnlyB: [] }).identical).toBe(true);
  });

  it("writes the report as text", () => {
    expect(reportText(comparison, "local › shop", "prod › shop")).toBe(
      [
        "Structure compare",
        "A: local › shop",
        "B: prod › shop",
        "",
        "Tables: 1 alike, 1 differ, 1 only in A, 0 only in B",
        "",
        "Tables only in A:",
        "  only_a",
        "",
        "Views only in B:",
        "  v_b",
        "",
        "Table orders:",
        "  column total differs (type, nullable)",
        "    A: decimal(12,2) NOT NULL",
        "    B: decimal(14,2) NULL",
        '  column note only in A: text NULL COMMENT "ห้ามว่าง"',
        "  index only in B: UNIQUE u_total (total)",
      ].join("\n"),
    );
  });
});
