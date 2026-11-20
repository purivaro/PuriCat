import { describe, expect, it } from "vitest";

import type { BatchOutcome } from "../../lib/types";
import { addUp, failedLines, failedTable, nothingWritten } from "./largeImport";

const batch = (more: Partial<BatchOutcome> = {}): BatchOutcome => ({
  inserted: 0,
  updated: 0,
  unchanged: 0,
  unmatched: 0,
  failed: [],
  stopped: false,
  cancelled: false,
  elapsedMs: 1,
  ...more,
});

describe("large import", () => {
  it("adds up what each batch wrote", () => {
    const sum = addUp(addUp(nothingWritten, batch({ inserted: 3, unchanged: 1 })), batch({ inserted: 2, updated: 4, unmatched: 1 }));
    expect(sum).toEqual({ inserted: 5, updated: 4, unchanged: 1, unmatched: 1 });
  });

  it("names a failed row by its line in the file, blank rows and the header counted", () => {
    // The page starts at the file's row 2000; its row 3 is the request's row 1 (rows 1 and 2 were blank).
    const page = [
      ["1", "a"],
      ["", ""],
      ["", ""],
      ["4", "d"],
    ];
    const out = batch({ failed: [{ row: 1, status: null, message: "refused" }] });
    expect(failedLines(out, page, 2000, [0, 3])).toEqual([{ line: 2004, cells: ["4", "d"], status: null, message: "refused" }]);
  });

  it("gives the lines back with the header and a reason, or as they were", () => {
    const failed = [{ line: 5, cells: ["7", "x"], status: "invalid" as const, message: "Too long" }];
    expect(failedTable(failed, ["id", "name"], "Problem", (f) => f.message ?? "")).toEqual([
      ["id", "name", "Problem"],
      ["7", "x", "Too long"],
    ]);
    expect(failedTable(failed, null, "Problem", () => "")).toEqual([["7", "x"]]);
  });
});
