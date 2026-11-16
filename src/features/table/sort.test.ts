import { describe, expect, it } from "vitest";

import { nextSort } from "./sort";

describe("nextSort", () => {
  it("cycles ascending, descending, then back to the default order", () => {
    const asc = nextSort(null, "name");
    expect(asc).toEqual({ column: "name", descending: false });
    const desc = nextSort(asc, "name");
    expect(desc).toEqual({ column: "name", descending: true });
    expect(nextSort(desc, "name")).toBeNull();
  });

  it("starts ascending when another column is clicked", () => {
    expect(nextSort({ column: "name", descending: true }, "id")).toEqual({ column: "id", descending: false });
  });
});
