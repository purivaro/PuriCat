import { describe, expect, it } from "vitest";

import type { ColumnMeta } from "../../lib/types";
import { copyable, copyRequest, pairByName, rowLabel } from "./copyRows";

const col = (name: string, kind: ColumnMeta["kind"] = "text", more: Partial<ColumnMeta> = {}): ColumnMeta => ({
  name,
  dataType: kind,
  kind,
  nullable: true,
  default: null,
  primaryKey: false,
  generated: false,
  editable: true,
  ...more,
});

describe("copying rows", () => {
  it("copies what the server can render as text the target reads back", () => {
    expect(copyable("mysql", col("photo", "binary"))).toBe(false);
    expect(copyable("mysql", col("spot", "other"))).toBe(false);
    expect(copyable("postgres", col("u", "other"))).toBe(true);
    expect(copyable("postgres", col("n", "number"))).toBe(true);
  });

  it("pairs columns by name, whatever the case, and leaves out what cannot be copied", () => {
    const source = [col("ID", "number"), col("Name"), col("photo", "binary"), col("only_here")];
    const target = [col("id", "number"), col("name"), col("photo", "binary"), col("extra")];
    expect(pairByName("mysql", source, target)).toEqual(["id", "name", null, null]);
  });

  it("reads the paired source columns and writes their targets", () => {
    const source = [col("id", "number"), col("full_name"), col("skip")];
    const { columns, request } = copyRequest({ mode: "upsert", source, mapping: ["id", "name", null], key: ["id"], update: ["name", "id", "gone"] });
    expect(columns).toEqual(["id", "full_name"]);
    expect(request).toEqual({ mode: "upsert", columns: ["id", "name"], key: ["id"], update: ["name"], rows: [] });
    expect(copyRequest({ mode: "insert", source, mapping: ["id", "name", null], key: [], update: ["name"] }).request.update).toEqual([]);
  });

  it("names a row by its key", () => {
    expect(rowLabel(["7", "Ann", null], [0])).toBe("7");
    expect(rowLabel(["7", "Ann", null], [2, 1])).toBe("NULL, Ann");
    expect(rowLabel(["7", "Ann"], [])).toBe("7");
  });
});
