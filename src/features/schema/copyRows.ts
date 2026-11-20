import type { ColumnMeta, Engine, PasteMode, PasteRequest } from "../../lib/types";
import type { Mapping } from "../table/smartPaste";

/** Rows read and written per page (one transaction on the target each). */
export const COPY_PAGE = 1000;

/** A column whose value can be copied as text (the backend's `copy::copyable`). */
export function copyable(engine: Engine, col: ColumnMeta): boolean {
  return !(col.kind === "binary" || (engine === "mysql" && col.kind === "other"));
}

const norm = (s: string) => s.trim().toLocaleLowerCase();

/** Each source column paired with the target column of the same name (whatever the case), if any. */
export function pairByName(engine: Engine, source: ColumnMeta[], target: ColumnMeta[]): Mapping {
  return source.map((s) => (copyable(engine, s) ? (target.find((t) => norm(t.name) === norm(s.name))?.name ?? null) : null));
}

/** The source columns read, and the request for the target (its rows are the source's, filled in page by page). */
export function copyRequest(o: { mode: PasteMode; source: ColumnMeta[]; mapping: Mapping; key: string[]; update: string[] }): {
  columns: string[];
  request: PasteRequest;
} {
  const pairs = o.source.flatMap((s, i) => (o.mapping[i] ? [{ from: s.name, to: o.mapping[i]! }] : []));
  const targets = pairs.map((p) => p.to);
  return {
    columns: pairs.map((p) => p.from),
    request: {
      mode: o.mode,
      columns: targets,
      key: o.key,
      update: o.mode === "insert" ? [] : o.update.filter((c) => targets.includes(c) && !o.key.includes(c)),
      rows: [],
    },
  };
}

/** A copied row named by its key values (or its first value), as the source has them. */
export function rowLabel(values: (string | null)[], at: number[]): string {
  const picked = at.length > 0 ? at.map((i) => values[i]) : values.slice(0, 1);
  return picked.map((v) => v ?? "NULL").join(", ");
}
