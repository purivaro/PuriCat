import type { ServerProcess } from "../../lib/types";

/** `45 s`, `12:03`, `3:04:05`; nothing for a time the server did not give. */
export function duration(seconds: number | null): string {
  if (seconds === null || seconds < 0) return "";
  if (seconds < 60) return `${seconds} s`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = String(seconds % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/**
 * The sessions to show: idle ones only when asked (the session reading the list is always
 * shown), and those whose user, host, database, client, command or query contains `text`.
 */
export function visibleSessions(list: ServerProcess[], text: string, showIdle: boolean): ServerProcess[] {
  const needle = text.trim().toLowerCase();
  return list.filter((p) => {
    if (p.idle && !showIdle && !p.own) return false;
    if (!needle) return true;
    return [String(p.id), p.user, p.host, p.database, p.client, p.command, p.state, p.query].some((v) => v?.toLowerCase().includes(needle));
  });
}

/** The query on one line, for the list. */
export function oneLine(query: string | null): string {
  return (query ?? "").replace(/\s+/g, " ").trim();
}
