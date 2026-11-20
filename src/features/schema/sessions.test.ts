import { describe, expect, it } from "vitest";

import type { ServerProcess } from "../../lib/types";
import { duration, oneLine, visibleSessions } from "./sessions";

const p = (id: number, more: Partial<ServerProcess> = {}): ServerProcess => ({
  id,
  user: "app",
  host: "10.0.0.5:51000",
  database: "shop",
  command: "Query",
  seconds: 3,
  state: null,
  query: "SELECT 1",
  client: null,
  own: false,
  idle: false,
  ...more,
});

describe("server sessions", () => {
  it("writes a duration the way a clock does", () => {
    expect(duration(null)).toBe("");
    expect(duration(-1)).toBe("");
    expect(duration(0)).toBe("0 s");
    expect(duration(45)).toBe("45 s");
    expect(duration(723)).toBe("12:03");
    expect(duration(11045)).toBe("3:04:05");
  });

  it("hides idle sessions unless asked, but never the one reading the list", () => {
    const list = [p(1), p(2, { idle: true, command: "Sleep" }), p(3, { idle: true, own: true })];
    expect(visibleSessions(list, "", false).map((x) => x.id)).toEqual([1, 3]);
    expect(visibleSessions(list, "", true).map((x) => x.id)).toEqual([1, 2, 3]);
  });

  it("finds a session by any of what it shows, whatever the case", () => {
    const list = [p(1, { query: "UPDATE orders SET x = 1" }), p(22, { user: "Report" }), p(3, { client: "psql" })];
    expect(visibleSessions(list, "orders", true).map((x) => x.id)).toEqual([1]);
    expect(visibleSessions(list, "REPORT", true).map((x) => x.id)).toEqual([22]);
    expect(visibleSessions(list, "psql", true).map((x) => x.id)).toEqual([3]);
    expect(visibleSessions(list, "22", true).map((x) => x.id)).toEqual([22]);
  });

  it("puts a query on one line", () => {
    expect(oneLine("SELECT *\n  FROM t\n WHERE a = 1")).toBe("SELECT * FROM t WHERE a = 1");
    expect(oneLine(null)).toBe("");
  });
});
