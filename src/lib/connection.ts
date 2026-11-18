import type { ConnectionProfile } from "./types";

/** The database a SQLite file holds, as SQLite itself names it. */
export const SQLITE_MAIN = "main";

/** The last part of a path, with either kind of separator. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Where a connection points, short enough for a list: `host:port`, or the file's name. */
export function place(c: ConnectionProfile): string {
  return c.engine === "sqlite" ? fileName(c.file ?? "") : `${c.host}:${c.port}`;
}

/** The same in full, for a tooltip: `user@host:port`, or the whole path of the file. */
export function fullPlace(c: ConnectionProfile): string {
  return c.engine === "sqlite" ? (c.file ?? "") : `${c.username}@${c.host}:${c.port}`;
}

/** The database a new query tab starts on. `null`: none is selected (MySQL without a default). */
export function homeDatabase(c: ConnectionProfile, hasSchemas: boolean): string | null {
  if (c.engine === "sqlite") return SQLITE_MAIN;
  return c.database ?? (hasSchemas ? "postgres" : null);
}
