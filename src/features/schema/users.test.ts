import { describe, expect, it } from "vitest";

import type { Account } from "../../lib/types";
import { accountLabel, filterAccounts, generatePassword, groupSteps, sameAccount, untouchable, userSpec } from "./users";

const account = (name: string, host: string | null, flags: Partial<Account> = {}): Account => ({
  name,
  host,
  you: false,
  system: false,
  locked: false,
  superuser: false,
  ...flags,
});

describe("accounts", () => {
  it("are written user@host on MySQL and by name on PostgreSQL", () => {
    expect(accountLabel({ name: "app", host: "%" })).toBe("app@%");
    expect(accountLabel({ name: "app", host: null })).toBe("app");
    expect(accountLabel({ name: "app" })).toBe("app");
  });

  it("are the same only with the same host", () => {
    expect(sameAccount({ name: "app", host: "%" }, { name: "app", host: "%" })).toBe(true);
    expect(sameAccount({ name: "app", host: "%" }, { name: "app", host: "localhost" })).toBe(false);
    expect(sameAccount({ name: "app" }, { name: "app", host: null })).toBe(true);
  });

  it("are found by a part of the name or of the host, whatever the case", () => {
    const list = [account("root", "localhost"), account("Shop_App", "%"), account("reader", "10.0.0.%")];
    expect(filterAccounts(list, "  ")).toBe(list);
    expect(filterAccounts(list, "shop").map((a) => a.name)).toEqual(["Shop_App"]);
    expect(filterAccounts(list, "LOCAL").map((a) => a.name)).toEqual(["root"]);
    expect(filterAccounts(list, "@10.").map((a) => a.name)).toEqual(["reader"]);
    expect(filterAccounts(list, "nobody")).toEqual([]);
  });

  it("that are yours, the server's own or a superuser are left alone", () => {
    expect(untouchable(account("app", "%"))).toBe(false);
    expect(untouchable(account("root", "localhost", { you: true }))).toBe(true);
    expect(untouchable(account("mysql.sys", "localhost", { system: true }))).toBe(true);
    expect(untouchable(account("postgres", null, { superuser: true }))).toBe(true);
    // A locked account can still be dropped or given a password.
    expect(untouchable(account("old", "%", { locked: true }))).toBe(false);
  });
});

describe("userSpec", () => {
  const draft = { name: "app", host: "localhost", template: "app" as const, database: "shop", password: "pw" };

  it("sends the host on MySQL only", () => {
    expect(userSpec("mysql", draft)).toEqual({ account: { name: "app", host: "localhost" }, template: "app", database: "shop", password: "pw" });
    expect(userSpec("postgres", draft).account).toEqual({ name: "app", host: null });
  });

  it("sends no database for a user without privileges, and none when none is chosen", () => {
    expect(userSpec("mysql", { ...draft, template: "none" }).database).toBeNull();
    expect(userSpec("mysql", { ...draft, template: "readOnly", database: "" }).database).toBeNull();
    expect(userSpec("mysql", { ...draft, template: "readOnly" }).database).toBe("shop");
  });

  it("sends the password as typed", () => {
    expect(userSpec("mysql", { ...draft, password: "  a b  " }).password).toBe("  a b  ");
  });
});

describe("generatePassword", () => {
  it("is 24 letters and digits from the random source", () => {
    const password = generatePassword();
    expect(password).toMatch(/^[A-Za-z0-9]{24}$/);
    expect(generatePassword()).not.toBe(password);
    expect(generatePassword(40)).toHaveLength(40);
  });

  it("throws away the bytes that would favour some characters", () => {
    // 62 characters: bytes 248–255 would make the first eight more likely than the rest.
    const fed: number[] = [];
    const source = (bytes: Uint8Array<ArrayBuffer>) => {
      for (let i = 0; i < bytes.length; i++) {
        bytes[i] = [255, 248, 0, 61, 62, 247][(fed.length + i) % 6]!;
      }
      fed.push(bytes.length);
    };
    // 0 → A, 61 → 9, 62 → A again (62 % 62), 247 → 247 % 62 = 61 → 9; 255 and 248 are dropped.
    expect(generatePassword(8, source)).toBe("A9A9A9A9");
  });
});

describe("groupSteps", () => {
  it("keeps the order and puts neighbours of one database together", () => {
    expect(
      groupSteps([
        { sql: "CREATE ROLE", database: null },
        { sql: "GRANT CONNECT", database: null },
        { sql: "GRANT USAGE", database: "shop" },
        { sql: "GRANT SELECT", database: "shop" },
      ]),
    ).toEqual([
      { database: null, sql: ["CREATE ROLE", "GRANT CONNECT"] },
      { database: "shop", sql: ["GRANT USAGE", "GRANT SELECT"] },
    ]);
    expect(groupSteps([])).toEqual([]);
  });
});
