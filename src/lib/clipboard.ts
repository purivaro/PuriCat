import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

/**
 * Parse spreadsheet clipboard text: tab-separated cells, one row per line, as written by
 * Excel, Google Sheets and Numbers. A cell containing a tab, a line break or a leading quote
 * arrives wrapped in double quotes with inner quotes doubled.
 *
 * Returns a rectangular matrix (short rows are padded with empty strings).
 */
export function parseClipboardTable(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  let atCellStart = true;

  for (let i = 0; i < text.length; i++) {
    const ch = text[i]!;
    if (quoted) {
      if (ch !== '"') cell += ch;
      else if (text[i + 1] === '"') {
        cell += '"';
        i++;
      } else quoted = false;
      continue;
    }
    if (ch === '"' && atCellStart) {
      quoted = true;
      atCellStart = false;
    } else if (ch === "\t") {
      row.push(cell);
      cell = "";
      atCellStart = true;
    } else if (ch === "\n" || ch === "\r") {
      if (ch === "\r" && text[i + 1] === "\n") i++;
      row.push(cell);
      rows.push(row);
      row = [];
      cell = "";
      atCellStart = true;
    } else {
      cell += ch;
      atCellStart = false;
    }
  }
  // An unterminated quote was a literal quote character after all.
  if (quoted) cell = `"${cell}`;
  // The final line break of a copied range does not start another row.
  if (cell !== "" || row.length > 0) {
    row.push(cell);
    rows.push(row);
  }

  const width = rows.reduce((w, r) => Math.max(w, r.length), 0);
  for (const r of rows) while (r.length < width) r.push("");
  return rows;
}

/** Inverse of `parseClipboardTable`: text that pastes back into a spreadsheet cell by cell. */
export function toClipboardTable(rows: string[][]): string {
  const cell = (v: string) => (/[\t\n\r]/.test(v) || v.startsWith('"') ? `"${v.replaceAll('"', '""')}"` : v);
  return rows.map((r) => r.map(cell).join("\t")).join("\n");
}

// The system clipboard, through the native plugin: no browser permission prompt.
export const clipboard = { read: readText, write: writeText };
