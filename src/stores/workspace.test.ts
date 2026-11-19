import { describe, expect, it } from "vitest";

import { closeTabIn, databaseGoneIn, tableTabId, type Tab } from "./workspace";

const tab = (name: string, connectionId = "c1", database = "db"): Tab => ({
  id: name,
  kind: "table",
  connectionId,
  table: { database, schema: null, name },
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

describe("databaseGoneIn", () => {
  const query = (id: string, connectionId: string, database: string | null): Tab => ({ id, kind: "sql", connectionId, database, title: id });
  const elsewhere = tab("other", "c1", "db2");
  const otherConnection = tab("far", "c2");
  const tabs = [tab("a"), query("q1", "c1", "db"), elsewhere, otherConnection, query("q2", "c1", "db2"), query("q3", "c2", "db")];

  it("closes the table tabs of a dropped database and moves its query tabs to the default database", () => {
    const after = databaseGoneIn(tabs, "a", "c1", "db", null);
    expect(after.tabs.map((t) => t.id)).toEqual(["q1", "other", "far", "q2", "q3"]);
    expect(after.tabs[0]).toMatchObject({ id: "q1", database: null });
    // the active tab went away: its right neighbour takes over
    expect(after.activeId).toBe("q1");
  });

  it("leaves other databases and other connections alone", () => {
    const after = databaseGoneIn(tabs, "other", "c1", "db", null);
    expect(after.activeId).toBe("other");
    expect(after.tabs.slice(1)).toEqual([elsewhere, otherConnection, tabs[4], tabs[5]]);
  });

  it("moves query tabs to the new name of a renamed database", () => {
    const after = databaseGoneIn(tabs, "q1", "c1", "db", "db_new");
    expect(after.tabs.find((t) => t.id === "q1")).toMatchObject({ database: "db_new" });
    expect(after.tabs.some((t) => t.id === "a")).toBe(false);
    expect(after.activeId).toBe("q1");
  });

  it("falls back to the welcome tab when nothing else is open", () => {
    expect(databaseGoneIn([tab("a")], "a", "c1", "db", null)).toEqual({ tabs: [{ id: "welcome", kind: "welcome" }], activeId: "welcome" });
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
