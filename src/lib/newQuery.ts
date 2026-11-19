import type { Tab } from "../stores/workspace";

export interface QueryTarget {
  connectionId: string;
  /** `null`: the connection's default database. */
  database: string | null;
}

/**
 * Where "New query" opens without asking: on the connection and database of the tab in
 * front, else on the only connection that is connected. `null` when nothing is connected,
 * or when several are and the tab in front belongs to none of them.
 */
export function newQueryTarget(tabs: Tab[], activeId: string, connected: string[]): QueryTarget | null {
  const current = tabs.find((t) => t.id === activeId);
  if (current && current.kind !== "welcome" && connected.includes(current.connectionId)) {
    return { connectionId: current.connectionId, database: current.kind === "table" ? current.table.database : current.database };
  }
  return connected.length === 1 ? { connectionId: connected[0]!, database: null } : null;
}
