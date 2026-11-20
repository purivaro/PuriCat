import { describe, expect, it } from "vitest";

import type { ColumnDefinition, TypeChoice } from "../../lib/types";
import { columnEdit, columnsAfter, indexesLosing, keyEdit, keyReady, newKey, startColumns, startEdit, tableEdits } from "./alterTable";
import type { ColumnDraft } from "./newTable";

const meta = (name: string) => ({
  name,
  dataType: "int",
  kind: "number" as const,
  nullable: false,
  default: null,
  primaryKey: name === "id",
  generated: false,
  editable: true,
});
const added: ColumnDraft = {
  key: "n",
  name: " note ",
  type: "TEXT",
  length: "",
  scale: "",
  unsigned: false,
  nullable: true,
  defaultKind: "none",
  defaultValue: "",
  auto: false,
  primary: false,
  comment: "",
};

describe("the change form", () => {
  it("orders the changes so every name is valid when it is used", () => {
    const existing = startColumns([meta("id"), meta("a"), meta("old")]);
    existing[1]!.rename = " amount ";
    existing[2]!.drop = true;
    const edits = tableEdits("shop", null, "t", existing, [added], ["ix_old"], [{ key: "i", name: " ", columns: ["amount", "note"], unique: true }]);
    expect(edits.map((e) => e.kind)).toEqual(["addColumn", "renameColumn", "dropIndex", "dropColumn", "addIndex"]);
    expect(edits[1]).toEqual({ kind: "renameColumn", from: "a", to: "amount" });
    expect(edits[4]).toEqual({ kind: "addIndex", name: null, columns: ["amount", "note"], unique: true });
  });

  it("leaves out a rename to the same name, and a rename of a column that is dropped", () => {
    const existing = startColumns([meta("a"), meta("b")]);
    existing[0]!.rename = "a";
    existing[1]!.rename = "c";
    existing[1]!.drop = true;
    expect(tableEdits("d", null, "t", existing, [], [], []).map((e) => e.kind)).toEqual(["dropColumn"]);
  });

  it("offers an index the columns the table will have", () => {
    const existing = startColumns([meta("id"), meta("a"), meta("b")]);
    existing[1]!.rename = "x";
    existing[2]!.drop = true;
    expect(columnsAfter(existing, [added, { ...added, name: "" }])).toEqual(["id", "x", "note"]);
  });

  it("names the indexes a dropped column takes away from", () => {
    const existing = startColumns([meta("a"), meta("b")]);
    existing[1]!.drop = true;
    const indexes = [
      { name: "ia", columns: ["a"], unique: false, primary: false },
      { name: "iab", columns: ["a", "b"], unique: true, primary: false },
    ];
    expect(indexesLosing(indexes, existing)).toEqual(["iab"]);
  });

  const def: ColumnDefinition = {
    name: "qty",
    columnType: "varchar(20)",
    nullable: true,
    default: "1",
    defaultExpression: false,
    extra: "",
    comment: "how many",
    charset: "utf8mb4",
    collation: "utf8mb4_0900_ai_ci",
    generated: false,
  };
  const types: TypeChoice[] = [
    { name: "INT", params: { kind: "none" }, family: "integer", timestamp: false },
    { name: "DECIMAL", params: { kind: "precision", defaultPrecision: 10, defaultScale: 2, max: 65 }, family: "number", timestamp: false },
  ];

  it("sends nothing for a column the form leaves as it is", () => {
    expect(columnEdit(def, startEdit(def), types)).toBeNull();
    expect(columnEdit(def, { ...startEdit(def), comment: " how many " }, types)).toBeNull();
  });

  it("sends only what the form changes", () => {
    expect(columnEdit(def, { ...startEdit(def), nullable: false }, types)).toEqual({ kind: "changeColumn", name: "qty", nullable: false });
    expect(columnEdit(def, { ...startEdit(def), comment: "" }, types)).toEqual({ kind: "changeColumn", name: "qty", comment: "" });
    expect(columnEdit(def, { ...startEdit(def), type: "INT", length: "11", unsigned: true, defaultKind: "value", defaultValue: "0" }, types)).toEqual({
      kind: "changeColumn",
      name: "qty",
      type: { type: "INT", length: null, scale: null, unsigned: true },
      default: { kind: "value", value: "0" },
    });
    expect(columnEdit(def, { ...startEdit(def), type: "DECIMAL", length: "8", scale: "x", defaultKind: "none" }, types)).toEqual({
      kind: "changeColumn",
      name: "qty",
      type: { type: "DECIMAL", length: 8, scale: null, unsigned: false },
      default: { kind: "none" },
    });
  });

  it("makes a foreign key only once both sides are chosen, after everything else", () => {
    const k = { ...newKey(), columns: ["customer_id"], onDelete: "cascade" as const };
    expect(keyReady(k)).toBe(false);
    expect(keyReady({ ...k, table: "customers" })).toBe(false);
    const done = { ...k, table: "customers", refColumns: ["id"] };
    expect(keyReady(done)).toBe(true);
    const table = { database: "shop", schema: null, name: "orders" };
    const fk = keyEdit(table, done);
    expect(fk).toEqual({
      kind: "addForeignKey",
      name: null,
      columns: ["customer_id"],
      references: { database: "shop", schema: null, name: "customers" },
      refColumns: ["id"],
      onDelete: "cascade",
      onUpdate: "noAction",
    });
    const change = { kind: "changeColumn" as const, name: "qty", nullable: false };
    const edits = tableEdits("shop", null, "orders", startColumns([meta("id"), meta("qty")]), [], [], [], [change], ["fk_old"], [fk]);
    expect(edits.map((e) => e.kind)).toEqual(["dropForeignKey", "changeColumn", "addForeignKey"]);
  });
});
