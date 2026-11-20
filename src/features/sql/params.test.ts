import { describe, expect, it } from "vitest";

import { autoKind, remember, rememberedFor, toValues } from "./params";

describe("query parameters", () => {
  it("writes a value as a number when it reads as one", () => {
    expect(autoKind("42")).toBe("number");
    expect(autoKind(" -3.5 ")).toBe("number");
    expect(autoKind("007x")).toBe("text");
    expect(autoKind("")).toBe("text");
  });

  it("gives each name its value, as the kind chosen", () => {
    const out = toValues(["id", "name", "code", "gone"], {
      id: { kind: "auto", value: "7" },
      name: { kind: "auto", value: "O'Neil" },
      code: { kind: "text", value: "007" },
      gone: { kind: "null", value: "x" },
    });
    expect(out).toEqual({
      values: { id: { kind: "number", value: "7" }, name: { kind: "text", value: "O'Neil" }, code: { kind: "text", value: "007" }, gone: { kind: "null" } },
    });
    // A value never typed is the empty text.
    expect(toValues(["x"], {})).toEqual({ values: { x: { kind: "text", value: "" } } });
    expect(toValues(["n"], { n: { kind: "number", value: "4x" } })).toEqual({ notNumber: "n" });
  });

  it("remembers the last values per tab", () => {
    remember("t1", { a: { kind: "auto", value: "1" } });
    remember("t1", { b: { kind: "text", value: "2" } });
    expect(rememberedFor("t1")).toEqual({ a: { kind: "auto", value: "1" }, b: { kind: "text", value: "2" } });
    expect(rememberedFor("t2")).toEqual({});
  });
});
