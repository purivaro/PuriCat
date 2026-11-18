import { describe, expect, it } from "vitest";

import { formatJson } from "./json";

describe("formatJson", () => {
  it("puts one member on a line, two spaces per level", () => {
    expect(formatJson('{"a": 1, "b": [true, null, {"c": "d"}], "e": {}}')).toBe(
      ['{', '  "a": 1,', '  "b": [', "    true,", "    null,", "    {", '      "c": "d"', "    }", "  ],", '  "e": {}', "}"].join("\n"),
    );
    expect(formatJson("[]")).toBe("[]");
    expect(formatJson("[ [ ], { } ]")).toBe("[\n  [],\n  {}\n]");
  });

  it("copies numbers as they are written, whatever JavaScript would make of them", () => {
    const text = '{"id":12345678901234567890,"price":1.10,"big":1e400,"neg":-0.0}';
    expect(formatJson(text)).toBe('{\n  "id": 12345678901234567890,\n  "price": 1.10,\n  "big": 1e400,\n  "neg": -0.0\n}');
  });

  it("leaves what is inside a string alone", () => {
    const text = '{"note":"a, b: {c} [d] \\"quoted\\" \\\\","ไทย":"บรรทัด\\nใหม่  สองช่อง"}';
    expect(formatJson(text)).toBe('{\n  "note": "a, b: {c} [d] \\"quoted\\" \\\\",\n  "ไทย": "บรรทัด\\nใหม่  สองช่อง"\n}');
  });

  it("gives the same layout for text that already has one", () => {
    const once = formatJson('{"a":[1,2,{"b":null}]}')!;
    expect(formatJson(once)).toBe(once);
    expect(formatJson(`\n  ${once}  \n`)).toBe(once);
  });

  it("is nothing for text that is not a JSON object or array", () => {
    for (const text of ["", "plain text", "42", '"a string"', "null", "{a: 1}", '{"a": 1,}', '{"a": 1} trailing', "[1, 2", "{'a': 1}"]) {
      expect(formatJson(text), text).toBeNull();
    }
  });
});
