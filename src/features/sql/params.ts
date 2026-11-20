import type { ParamValue } from "../../lib/types";

/** How a parameter's value is written: "auto" is a number when it reads as one, else text. */
export type ParamKind = "auto" | "text" | "number" | "null";

export interface ParamDraft {
  kind: ParamKind;
  value: string;
}

const NUMBER = /^\s*-?\d+(\.\d+)?\s*$/;

/** What `auto` makes of a value. */
export const autoKind = (value: string): "number" | "text" => (NUMBER.test(value) ? "number" : "text");

/** The values to send, or the name of the first one that is not a number when it must be. */
export function toValues(names: string[], drafts: Record<string, ParamDraft>): { values: Record<string, ParamValue> } | { notNumber: string } {
  const values: Record<string, ParamValue> = {};
  for (const name of names) {
    const d = drafts[name] ?? { kind: "auto", value: "" };
    const kind = d.kind === "auto" ? autoKind(d.value) : d.kind;
    if (kind === "null") values[name] = { kind: "null" };
    else if (kind === "number") {
      if (!NUMBER.test(d.value)) return { notNumber: name };
      values[name] = { kind: "number", value: d.value.trim() };
    } else values[name] = { kind: "text", value: d.value };
  }
  return { values };
}

/** The values typed last, by tab and name, so running again asks with them filled in. */
const remembered = new Map<string, Record<string, ParamDraft>>();
export const rememberedFor = (tab: string) => remembered.get(tab) ?? {};
export const remember = (tab: string, drafts: Record<string, ParamDraft>) => remembered.set(tab, { ...rememberedFor(tab), ...drafts });
