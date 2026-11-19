import { describe, expect, it } from "vitest";

import type { Tab } from "../stores/workspace";
import { newQueryTarget } from "./newQuery";

const welcome: Tab = { id: "welcome", kind: "welcome" };
const table: Tab = { id: "t", kind: "table", connectionId: "c1", table: { database: "shop", schema: null, name: "orders" }, objectKind: "table" };
const query: Tab = { id: "q", kind: "sql", connectionId: "c2", database: "blog", title: "Query 1" };
const tabs = [welcome, table, query];

describe("newQueryTarget", () => {
  it("follows the tab in front: its connection and its database", () => {
    expect(newQueryTarget(tabs, "t", ["c1", "c2"])).toEqual({ connectionId: "c1", database: "shop" });
    expect(newQueryTarget(tabs, "q", ["c1", "c2"])).toEqual({ connectionId: "c2", database: "blog" });
  });

  it("falls back to the only connected connection, on its default database", () => {
    expect(newQueryTarget(tabs, "welcome", ["c2"])).toEqual({ connectionId: "c2", database: null });
    // the tab in front belongs to a connection that was closed
    expect(newQueryTarget(tabs, "t", ["c2"])).toEqual({ connectionId: "c2", database: null });
  });

  it("does not guess between several connections, and has nothing without one", () => {
    expect(newQueryTarget(tabs, "welcome", ["c1", "c2"])).toBeNull();
    expect(newQueryTarget(tabs, "welcome", [])).toBeNull();
  });
});
