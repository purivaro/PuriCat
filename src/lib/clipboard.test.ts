import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ readText: vi.fn(), writeText: vi.fn() }));

import { parseClipboardTable, toClipboardTable } from "./clipboard";

describe("parseClipboardTable", () => {
  it("reads a plain range and ignores the final line break", () => {
    expect(parseClipboardTable("a\tb\r\n1\t2\r\n")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
    expect(parseClipboardTable("a\tb\n1\t2")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
    expect(parseClipboardTable("solo")).toEqual([["solo"]]);
    expect(parseClipboardTable("")).toEqual([]);
  });

  it("keeps empty cells, empty rows and Unicode", () => {
    expect(parseClipboardTable("\tb\t\r\n\r\nสวัสดี\t𝒳\t日本\r\n")).toEqual([
      ["", "b", ""],
      ["", "", ""],
      ["สวัสดี", "𝒳", "日本"],
    ]);
  });

  it("unwraps quoted cells with tabs, line breaks and doubled quotes", () => {
    const text = '"line 1\nline 2"\t"has\ttab"\t"say ""hi"""\r\nplain\t5" nail\t""\r\n';
    expect(parseClipboardTable(text)).toEqual([
      ["line 1\nline 2", "has\ttab", 'say "hi"'],
      ["plain", '5" nail', ""],
    ]);
  });

  it("pads ragged rows and survives an unterminated quote", () => {
    expect(parseClipboardTable("a\tb\tc\nd")).toEqual([
      ["a", "b", "c"],
      ["d", "", ""],
    ]);
    expect(parseClipboardTable('"oops')).toEqual([['"oops']]);
  });

  it("does not reinterpret numbers, dates or leading zeros", () => {
    expect(parseClipboardTable("007\t1e5\t2026-01-02\t 1,234.50 \tTRUE")).toEqual([
      ["007", "1e5", "2026-01-02", " 1,234.50 ", "TRUE"],
    ]);
  });
});

describe("toClipboardTable", () => {
  it("round-trips awkward cells", () => {
    const rows = [
      ["plain", "tab\there", "two\nlines", '"quoted" start'],
      ["", 'mid "quote"', "ไทย", "end"],
    ];
    expect(parseClipboardTable(toClipboardTable(rows))).toEqual(rows);
  });

  it("only quotes when it has to", () => {
    expect(toClipboardTable([["a", 'b "c"', "d\te"]])).toBe('a\tb "c"\t"d\te"');
  });
});
