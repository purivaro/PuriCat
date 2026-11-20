import type { BatchOutcome, PasteRowStatus } from "../../lib/types";

/** Rows per transaction of a large import. */
export const BATCH_ROWS = 1000;

/** A row of the file that was not written. */
export interface FailedLine {
  /** Its line in the file, from 1 (a header row is line 1). */
  line: number;
  /** Its cells, as the file has them. */
  cells: string[];
  /** What the analysis found; null when the server refused the row. */
  status: PasteRowStatus | null;
  message: string | null;
}

export interface Written {
  inserted: number;
  updated: number;
  unchanged: number;
  unmatched: number;
}

export const nothingWritten: Written = { inserted: 0, updated: 0, unchanged: 0, unmatched: 0 };

export function addUp(sum: Written, batch: BatchOutcome): Written {
  return {
    inserted: sum.inserted + batch.inserted,
    updated: sum.updated + batch.updated,
    unchanged: sum.unchanged + batch.unchanged,
    unmatched: sum.unmatched + batch.unmatched,
  };
}

/**
 * The rows of a batch that were not written, by their line in the file. `page` is the file's
 * rows from `offset`; `sourceRows` maps each request row to its row in `page` (`Built.sourceRows`).
 */
export function failedLines(batch: BatchOutcome, page: string[][], offset: number, sourceRows: number[]): FailedLine[] {
  return batch.failed.map((f) => {
    const at = sourceRows[f.row] ?? 0;
    return { line: offset + at + 1, cells: page[at] ?? [], status: f.status, message: f.message };
  });
}

/**
 * The lines that were not written, ready for a spreadsheet: with the file's header row and a
 * last column saying why, when the file has a header; as they were otherwise, so that importing
 * them again maps the same columns.
 */
export function failedTable(failed: FailedLine[], header: string[] | null, reasonHeader: string, reason: (f: FailedLine) => string): string[][] {
  if (!header) return failed.map((f) => [...f.cells]);
  return [[...header, reasonHeader], ...failed.map((f) => [...f.cells, reason(f)])];
}
