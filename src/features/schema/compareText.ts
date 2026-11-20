import type { ColumnShape, Comparison, IndexShape, KeyShape } from "../../lib/types";

/** A column in one line: its type, NULL, default, extras and comment. */
export function describeColumn(c: ColumnShape): string {
  return [
    c.columnType,
    c.nullable ? "NULL" : "NOT NULL",
    c.default !== null ? `DEFAULT ${c.default}` : null,
    c.extra || null,
    c.collation ? `COLLATE ${c.collation}` : null,
    c.comment ? `COMMENT ${JSON.stringify(c.comment)}` : null,
  ]
    .filter(Boolean)
    .join(" ");
}

export function describeIndex(i: IndexShape): string {
  const kind = i.primary ? "PRIMARY KEY" : i.unique ? "UNIQUE" : "INDEX";
  return `${kind} ${i.name} (${i.columns.join(", ")})${i.predicate ? ` WHERE ${i.predicate}` : ""}`;
}

export function describeKey(k: KeyShape): string {
  return `${k.name} (${k.columns.join(", ")}) → ${k.references} (${k.refColumns.join(", ")}) ON DELETE ${k.onDelete} ON UPDATE ${k.onUpdate}`;
}

/** How many tables differ, are alike, and are on one side only. */
export function tally(c: Comparison) {
  return {
    same: c.same.length,
    changed: c.changed.length,
    onlyA: c.onlyA.length,
    onlyB: c.onlyB.length,
    identical: c.changed.length === 0 && c.onlyA.length === 0 && c.onlyB.length === 0 && c.viewsOnlyA.length === 0 && c.viewsOnlyB.length === 0,
  };
}

/** The report as plain text, to paste into a ticket or a chat. */
export function reportText(c: Comparison, a: string, b: string): string {
  const out: string[] = [`Structure compare`, `A: ${a}`, `B: ${b}`, ""];
  const t = tally(c);
  out.push(`Tables: ${t.same} alike, ${t.changed} differ, ${t.onlyA} only in A, ${t.onlyB} only in B`);
  if (c.onlyA.length) out.push("", "Tables only in A:", ...c.onlyA.map((n) => `  ${n}`));
  if (c.onlyB.length) out.push("", "Tables only in B:", ...c.onlyB.map((n) => `  ${n}`));
  if (c.viewsOnlyA.length) out.push("", "Views only in A:", ...c.viewsOnlyA.map((n) => `  ${n}`));
  if (c.viewsOnlyB.length) out.push("", "Views only in B:", ...c.viewsOnlyB.map((n) => `  ${n}`));
  for (const table of c.changed) {
    out.push("", `Table ${table.name}:`);
    for (const col of table.columns) {
      if (col.a && col.b)
        out.push(`  column ${col.name} differs (${col.fields.join(", ")})`, `    A: ${describeColumn(col.a)}`, `    B: ${describeColumn(col.b)}`);
      else out.push(`  column ${col.name} only in ${col.a ? "A" : "B"}: ${describeColumn((col.a ?? col.b)!)}`);
    }
    for (const i of table.indexes) out.push(`  index only in ${i.a ? "A" : "B"}: ${describeIndex((i.a ?? i.b)!)}`);
    for (const k of table.foreignKeys) out.push(`  foreign key only in ${k.a ? "A" : "B"}: ${describeKey((k.a ?? k.b)!)}`);
  }
  return out.join("\n");
}
