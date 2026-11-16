import type { SortSpec } from "../../lib/types";

/** Sort cycle of a header click: ascending -> descending -> back to the default order. */
export function nextSort(current: SortSpec | null, column: string): SortSpec | null {
  if (current?.column !== column) return { column, descending: false };
  return current.descending ? null : { column, descending: true };
}
