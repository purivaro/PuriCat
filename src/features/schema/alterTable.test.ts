import { describe, expect, it } from "vitest";

import { columnsAfter, indexesLosing, startColumns, tableEdits } from "./alterTable";
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
});
