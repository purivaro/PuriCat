import { describe, expect, it } from "vitest";

import { fileName, fullPlace, homeDatabase, place } from "./connection";
import type { ConnectionProfile } from "./types";

const base: ConnectionProfile = {
  id: "c1",
  name: "Shop",
  engine: "mysql",
  environment: "development",
  host: "db.local",
  port: 3306,
  username: "app",
  database: null,
  file: null,
  tls: "prefer",
  tlsCaPath: null,
  readOnly: false,
  favorite: false,
  tags: [],
  connectTimeoutSecs: 10,
  bulkReviewRows: 10,
  createdAt: 0,
  updatedAt: 0,
  lastUsedAt: null,
};
const file: ConnectionProfile = { ...base, engine: "sqlite", host: "", port: 0, username: "", file: "/Users/me/แอป/data.sqlite" };

describe("where a connection points", () => {
  it("is host and port for a server", () => {
    expect(place(base)).toBe("db.local:3306");
    expect(fullPlace(base)).toBe("app@db.local:3306");
  });

  it("is the file for SQLite: its name in a list, its path in full", () => {
    expect(place(file)).toBe("data.sqlite");
    expect(fullPlace(file)).toBe("/Users/me/แอป/data.sqlite");
    expect(fileName("C:\\data\\shop.db")).toBe("shop.db");
    expect(fileName("notes")).toBe("notes");
  });
});

describe("the database a query tab starts on", () => {
  it("is the profile's own, or the engine's default", () => {
    expect(homeDatabase(base, false)).toBeNull();
    expect(homeDatabase({ ...base, database: "shop" }, false)).toBe("shop");
    expect(homeDatabase({ ...base, engine: "postgres" }, true)).toBe("postgres");
    expect(homeDatabase(file, false)).toBe("main");
  });
});
