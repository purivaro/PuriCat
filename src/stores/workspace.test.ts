import { describe, expect, it } from "vitest";

import { closeTabIn, tableTabId, type Tab } from "./workspace";

const tab = (name: string): Tab => ({
  id: name,
  kind: "table",
  connectionId: "c1",
  table: { database: "db", schema: null, name },
  objectKind: "table",
});

describe("closeTabIn", () => {
  const tabs = [tab("a"), tab("b"), tab("c")];

  it("keeps the active tab when another tab closes", () => {
    expect(closeTabIn(tabs, "a", "c")).toEqual({ tabs: [tabs[0], tabs[1]], activeId: "a" });
  });

  it("activates the right neighbour, or the left one at the end", () => {
    expect(closeTabIn(tabs, "b", "b").activeId).toBe("c");
    expect(closeTabIn(tabs, "c", "c").activeId).toBe("b");
  });

  it("falls back to the welcome tab when the last tab closes", () => {
    const result = closeTabIn([tab("a")], "a", "a");
    expect(result.tabs).toEqual([{ id: "welcome", kind: "welcome" }]);
    expect(result.activeId).toBe("welcome");
  });

  it("ignores unknown ids", () => {
    expect(closeTabIn(tabs, "a", "zzz")).toEqual({ tabs, activeId: "a" });
  });
});

describe("tableTabId", () => {
  it("distinguishes schema, database and connection", () => {
    const ids = new Set([
      tableTabId("c1", { database: "db", schema: "public", name: "t" }),
      tableTabId("c1", { database: "db", schema: "audit", name: "t" }),
      tableTabId("c1", { database: "db2", schema: "public", name: "t" }),
      tableTabId("c2", { database: "db", schema: "public", name: "t" }),
      // a name containing the separator must not collide with a schema boundary
      tableTabId("c1", { database: "db", schema: null, name: "public/t" }),
    ]);
    expect(ids.size).toBe(5);
  });
});
