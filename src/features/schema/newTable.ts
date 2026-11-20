import type { ColumnDefault, ColumnSpec, Engine, NewTable, TypeChoice } from "../../lib/types";

/** One row of the table form, as typed: numbers still text, the default split in two. */
export interface ColumnDraft {
  key: string;
  name: string;
  type: string;
  length: string;
  scale: string;
  unsigned: boolean;
  nullable: boolean;
  defaultKind: ColumnDefault["kind"];
  defaultValue: string;
  auto: boolean;
  primary: boolean;
  comment: string;
}

let counter = 0;
const nextKey = () => `c${++counter}`;

/** What a new form starts with: a self-numbering id, and an empty second column. */
export function firstColumns(engine: Engine, types: TypeChoice[]): ColumnDraft[] {
  const integer = engine === "mysql" ? "INT" : "bigint";
  const text = types.find((t) => t.family === "text" && t.params.kind === "length") ?? types[0]!;
  return [
    { ...blankColumn(types, integer), name: "id", auto: true, primary: true, unsigned: engine === "mysql" },
    { ...blankColumn(types, text.name), name: "" },
  ];
}

export function blankColumn(types: TypeChoice[], type?: string): ColumnDraft {
  const choice = types.find((t) => t.name === type) ?? types.find((t) => t.family === "text") ?? types[0]!;
  return withType({ key: nextKey(), name: "", type: choice.name, length: "", scale: "", unsigned: false, nullable: false, defaultKind: "none", defaultValue: "", auto: false, primary: false, comment: "" }, choice);
}

/** Another type: its first offer of a length or precision, and what no longer fits dropped. */
export function withType(c: ColumnDraft, choice: TypeChoice): ColumnDraft {
  const next = { ...c, type: choice.name };
  if (choice.params.kind === "length") {
    next.length = choice.params.required ? String(choice.params.default) : "";
    next.scale = "";
  } else if (choice.params.kind === "precision") {
    next.length = String(choice.params.defaultPrecision);
    next.scale = String(choice.params.defaultScale);
  } else {
    next.length = "";
    next.scale = "";
  }
  if (choice.family !== "integer") next.auto = false;
  if (choice.family !== "integer" && choice.family !== "number") next.unsigned = false;
  if (!choice.timestamp && next.defaultKind === "currentTimestamp") next.defaultKind = "none";
  return next;
}

const number = (text: string): number | null => {
  const t = text.trim();
  return /^\d+$/.test(t) ? Number(t) : null;
};

/** The spec the backend gets. It checks everything again; this only shapes it. */
export function tableSpec(database: string, schema: string | null, name: string, comment: string, columns: ColumnDraft[]): NewTable {
  return {
    table: { database, schema, name },
    comment: comment.trim() || null,
    columns: columns.map(
      (c): ColumnSpec => ({
        name: c.name,
        type: c.type,
        length: number(c.length),
        scale: number(c.scale),
        unsigned: c.unsigned,
        nullable: c.nullable && !c.primary && !c.auto,
        default:
          c.defaultKind === "value"
            ? { kind: "value", value: c.defaultValue }
            : c.defaultKind === "null"
              ? { kind: "null" }
              : c.defaultKind === "currentTimestamp"
                ? { kind: "currentTimestamp" }
                : { kind: "none" },
        auto: c.auto,
        key: c.primary,
        comment: c.comment.trim() || null,
      }),
    ),
  };
}

/** Move the column at `from` one place up (-1) or down (+1). */
export function move<T>(list: T[], from: number, by: -1 | 1): T[] {
  const to = from + by;
  if (to < 0 || to >= list.length) return list;
  const next = [...list];
  [next[from], next[to]] = [next[to]!, next[from]!];
  return next;
}
