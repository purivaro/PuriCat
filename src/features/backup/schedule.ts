import type { BackupFormat, BackupSchedule } from "../../lib/types";

export type ScheduleKind = BackupSchedule["kind"];

/** `HH:MM`, as a time input holds it. */
export function toTimeInput(hour: number, minute: number): string {
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}

export function fromTimeInput(value: string): { hour: number; minute: number } {
  const [hour, minute] = value.split(":").map((part) => Number.parseInt(part, 10));
  const within = (n: number | undefined, max: number) => (n !== undefined && Number.isInteger(n) && n >= 0 && n <= max ? n : 0);
  return { hour: within(hour, 23), minute: within(minute, 59) };
}

/** The schedule the form describes. A weekly schedule on every day is a daily one. */
export function scheduleOf(kind: ScheduleKind, time: string, days: number[]): BackupSchedule {
  if (kind === "off") return { kind: "off" };
  const at = fromTimeInput(time);
  const chosen = [...new Set(days)].filter((d) => d >= 1 && d <= 7).sort((a, b) => a - b);
  if (kind === "daily" || chosen.length === 7) return { kind: "daily", ...at };
  return { kind: "weekly", days: chosen, ...at };
}

/** A file size with the unit that reads best: `812 B`, `42 KB`, `1.5 MB`, `2.31 GB`. */
export function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${Math.round(bytes / 1024)} KB`;
  const mb = bytes / 1_048_576;
  if (mb < 1024) return `${mb >= 100 ? mb.toFixed(0) : mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}

/**
 * The name a backup made at `at` gets. It mirrors `backup::file_name` of the backend, which
 * is the one that names the file: this is a preview.
 */
export function fileName(connection: string, database: string, at: Date, format: BackupFormat, compress: boolean): string {
  const clean = (text: string, fallback: string) => {
    let out = "";
    for (const c of text.trim()) {
      // Whitespace, control characters, and what a path or a shell gives a meaning.
      if (/[\s\p{Cc}/\\:*?"<>|'`$&;!#%{}[\]()]/u.test(c)) {
        if (!out.endsWith("-")) out += "-";
      } else {
        out += c;
      }
    }
    out = out.replace(/^[-.]+|[-.]+$/g, "");
    return out === "" ? fallback : [...out].slice(0, 60).join("");
  };
  const two = (n: number) => String(n).padStart(2, "0");
  const stamp = `${at.getFullYear()}-${two(at.getMonth() + 1)}-${two(at.getDate())}_${two(at.getHours())}${two(at.getMinutes())}${two(at.getSeconds())}`;
  const extension = format === "custom" ? "dump" : compress ? "sql.gz" : "sql";
  return `${clean(connection, "connection")}_${clean(database, "database")}_${stamp}.${extension}`;
}
