import { describe, expect, it } from "vitest";

import { spans, statementAt } from "./statements";

/**
 * What the backend answers for these scripts: byte offsets, split on `;`, a statement's text
 * starting with the `--` comments that lead it.
 */
function split(script: string) {
  const bytes = (to: number) => new TextEncoder().encode(script.slice(0, to)).length;
  const out: { start: number; body: number; text: string }[] = [];
  let from = 0;
  for (const piece of script.split(";")) {
    const text = piece.trim();
    const word = /^(?:\s|--.*\n)*/.exec(piece)![0].length;
    if (word < piece.length) out.push({ start: bytes(from + piece.length - piece.trimStart().length), body: bytes(from + word), text });
    from += piece.length + 1;
  }
  return out;
}

describe("spans", () => {
  it("places statements by UTF-16 offset, whatever comes before them", () => {
    const script = "SELECT 'ก้อน 😀';\n  SELECT 2;\n-- จบ\nSELECT 3";
    const list = spans(script, split(script));
    expect(list.map((s) => script.slice(s.start, s.end))).toEqual(["SELECT 'ก้อน 😀'", "SELECT 2", "-- จบ\nSELECT 3"]);
  });

  it("leaves a comment after a semicolon to the statement on that line", () => {
    const script = "SELECT 1; -- done\n\n-- about the second\nSELECT 2";
    const [, second] = spans(script, split(script));
    expect(script.slice(second!.start, second!.end).trim()).toBe("-- about the second\nSELECT 2");
    // A block comment may run over the line break: it stays whole.
    const block = "SELECT 1; /* a\nb */ SELECT 2";
    const answer = [
      { start: 0, body: 0, text: "SELECT 1" },
      { start: 10, body: 19, text: "/* a\nb */ SELECT 2" },
    ];
    expect(spans(block, answer)[1]).toEqual({ start: 10, body: 19, end: block.length });
  });

  it("is empty for a script with no statement", () => {
    expect(spans("  \n", [])).toEqual([]);
  });
});

describe("statementAt", () => {
  const script = "SELECT 1;\n\nUPDATE t\n  SET a = 2; -- done\n\n\nSELECT 3";
  const list = spans(script, split(script));
  const at = (marker: string, after = 0) => statementAt(script, list, script.indexOf(marker) + after);

  it("is the statement the cursor is inside", () => {
    expect(at("SELECT 1")).toBe(0);
    expect(at("SET a")).toBe(1);
    expect(statementAt(script, list, script.length)).toBe(2);
  });

  it("is the statement just ended, on the line it ends on", () => {
    expect(at("SELECT 1;", 9)).toBe(0); // right after the semicolon
    expect(at("-- done", 3)).toBe(1); // in the comment that follows it
    expect(at("-- done", 7)).toBe(1);
  });

  it("is the next statement once the cursor is on a later line", () => {
    expect(at("\n\nUPDATE", 1)).toBe(1); // the empty line between the first two
    expect(at("\n\n\nSELECT 3", 2)).toBe(2);
  });

  it("gives a comment above a statement to that statement", () => {
    const noted = "SELECT 1;\n-- the second\nSELECT 2";
    const two = spans(noted, split(noted));
    expect(statementAt(noted, two, noted.indexOf("the second"))).toBe(1);
    expect(two[1]).toEqual({ start: noted.indexOf("--"), body: noted.indexOf("SELECT 2"), end: noted.length });
  });

  it("is the first statement above it, and the last one below everything", () => {
    const padded = "\n\n  SELECT 1;\nSELECT 2;\n\n";
    const two = spans(padded, split(padded));
    expect(statementAt(padded, two, 0)).toBe(0);
    expect(statementAt(padded, two, padded.length)).toBe(1);
  });

  it("is nothing in a script without statements", () => {
    expect(statementAt("-- only a comment", [], 3)).toBe(-1);
  });
});
