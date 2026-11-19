import { create } from "zustand";

import { ipc } from "../lib/ipc";
import type { ObjectKind, TableRef } from "../lib/types";

export interface TableTab {
  id: string;
  kind: "table";
  connectionId: string;
  table: TableRef;
  objectKind: ObjectKind;
}

export interface WelcomeTab {
  id: "welcome";
  kind: "welcome";
}

export interface SqlTab {
  id: string;
  kind: "sql";
  connectionId: string;
  /** Database the tab's connection is on; `null` uses the connection's default. */
  database: string | null;
  title: string;
  /** The saved script this tab is editing, when it is one; the title is then its name. */
  scriptId?: number;
}

export type Tab = WelcomeTab | TableTab | SqlTab;

/** Settings key holding a query tab's text, so it survives a restart. */
export const sqlTextKey = (tabId: string) => `sql:${tabId}`;

/** Text a query tab was opened with (SQL sent over from the agent chat), until it closes. */
const seeds = new Map<string, string>();
export const sqlSeed = (tabId: string) => seeds.get(tabId);

/** What each open query tab's editor holds right now, for the agent's context. */
export const sqlEditors = new Map<string, () => string>();

const WELCOME: WelcomeTab = { id: "welcome", kind: "welcome" };

export function tableTabId(connectionId: string, t: TableRef): string {
  return JSON.stringify([connectionId, t.database, t.schema, t.name]);
}

/** Remove a tab and decide which tab becomes active: the right neighbour, else the left. */
/** A closed query tab gives its connection back and its saved text is cleared. */
function released(before: Tab[], after: Tab[]) {
  for (const t of before) {
    if (t.kind === "sql" && !after.some((x) => x.id === t.id)) {
      void ipc.closeConsole(t.id);
      void ipc.setSetting(sqlTextKey(t.id), "");
      seeds.delete(t.id);
    }
  }
}

const inDatabase = (t: Tab, connectionId: string, database: string) =>
  t.kind !== "welcome" && t.connectionId === connectionId && (t.kind === "table" ? t.table.database : t.database) === database;

/**
 * A database was dropped (`to` is `null`) or renamed: its table tabs close, and its query
 * tabs keep their text and move to `to` (`null`: the connection's default database).
 */
export function databaseGoneIn(
  tabs: Tab[],
  activeId: string,
  connectionId: string,
  database: string,
  to: string | null,
): { tabs: Tab[]; activeId: string } {
  let next = { tabs, activeId };
  for (const t of tabs) {
    if (t.kind === "table" && inDatabase(t, connectionId, database)) next = closeTabIn(next.tabs, next.activeId, t.id);
  }
  return {
    tabs: next.tabs.map((t) => (t.kind === "sql" && inDatabase(t, connectionId, database) ? { ...t, database: to } : t)),
    activeId: next.activeId,
  };
}

export function closeTabIn(tabs: Tab[], activeId: string, id: string): { tabs: Tab[]; activeId: string } {
  const index = tabs.findIndex((t) => t.id === id);
  if (index < 0) return { tabs, activeId };
  const rest = tabs.filter((t) => t.id !== id);
  if (rest.length === 0) return { tabs: [WELCOME], activeId: WELCOME.id };
  if (activeId !== id) return { tabs: rest, activeId };
  const neighbour = rest[Math.min(index, rest.length - 1)]!;
  return { tabs: rest, activeId: neighbour.id };
}

interface WorkspaceState {
  tabs: Tab[];
  activeId: string;
  /** Tabs with unsaved edits. */
  dirty: Record<string, true>;
  /** Query tabs whose text differs from the saved script they are editing. */
  edited: Record<string, true>;
  /** A dirty or edited tab the user asked to close; the workspace asks before discarding. */
  closing: string | null;
  setDirty: (id: string, dirty: boolean) => void;
  setEdited: (id: string, edited: boolean) => void;
  hasUnsavedFor: (connectionId: string) => boolean;
  /** Unsaved edits in a table of this database, or an open transaction in a query tab on it. */
  hasUnsavedInDatabase: (connectionId: string, database: string) => boolean;
  /** Close even if dirty (after the user confirmed). */
  forceClose: (id: string) => void;
  cancelClose: () => void;
  restore: () => Promise<void>;
  openTable: (connectionId: string, table: TableRef, objectKind: ObjectKind) => void;
  /** Open a new query tab, optionally with `text` already in the editor. */
  openSql: (connectionId: string, database: string | null, text?: string) => void;
  /** Show a saved script: the tab that already has it, or a new one. */
  openScript: (database: string | null, script: { id: number; connectionId: string; name: string; sql: string }) => void;
  /** A query tab becomes a saved script (it was saved under a name, or a script was loaded into it). */
  bindScript: (id: string, script: { id: number; name: string }) => void;
  /** A script was renamed: its tabs follow. */
  renameScript: (scriptId: number, name: string) => void;
  /** A script was deleted: its tabs keep their text and are plain query tabs again. */
  unbindScript: (scriptId: number) => void;
  setSqlDatabase: (id: string, database: string | null) => void;
  openWelcome: () => void;
  activate: (id: string) => void;
  close: (id: string) => void;
  closeOthers: (id: string) => void;
  closeAll: () => void;
  closeForConnection: (connectionId: string) => void;
  /** See `databaseGoneIn`. */
  databaseGone: (connectionId: string, database: string, to: string | null) => void;
}

function persist(state: Pick<WorkspaceState, "tabs" | "activeId">) {
  void ipc.setSetting("workspace", JSON.stringify({ tabs: state.tabs, activeId: state.activeId }));
}

function isTab(v: unknown): v is Tab {
  if (!v || typeof v !== "object") return false;
  const t = v as Record<string, unknown>;
  if (t.kind === "welcome") return t.id === "welcome";
  if (t.kind === "sql") {
    return (
      typeof t.id === "string" &&
      typeof t.connectionId === "string" &&
      typeof t.title === "string" &&
      (t.scriptId === undefined || typeof t.scriptId === "number")
    );
  }
  const table = t.table as Record<string, unknown> | undefined;
  return (
    t.kind === "table" &&
    typeof t.id === "string" &&
    typeof t.connectionId === "string" &&
    !!table &&
    typeof table.database === "string" &&
    typeof table.name === "string"
  );
}

export const useWorkspace = create<WorkspaceState>((set, get) => {
  const commit = (next: Pick<WorkspaceState, "tabs" | "activeId">) => {
    released(get().tabs, next.tabs);
    set(next);
    persist(next);
  };

  return {
    tabs: [WELCOME],
    activeId: WELCOME.id,

    restore: async () => {
      const all = await ipc.getSettings();
      if (!all.workspace) return;
      try {
        const saved = JSON.parse(all.workspace) as { tabs?: unknown[]; activeId?: unknown };
        const tabs = (saved.tabs ?? []).filter(isTab);
        if (tabs.length === 0) return;
        const activeId = tabs.some((t) => t.id === saved.activeId) ? (saved.activeId as string) : tabs[0]!.id;
        set({ tabs, activeId });
      } catch {
        /* corrupt value: keep the default workspace */
      }
    },

    openTable: (connectionId, table, objectKind) => {
      const id = tableTabId(connectionId, table);
      const { tabs } = get();
      if (tabs.some((t) => t.id === id)) return commit({ tabs, activeId: id });
      commit({ tabs: [...tabs, { id, kind: "table", connectionId, table, objectKind }], activeId: id });
    },

    openSql: (connectionId, database, text) => {
      const { tabs } = get();
      // "Query 3" is the next free number, so names stay unique while tabs are open.
      const used = new Set(tabs.flatMap((t) => (t.kind === "sql" ? [t.title] : [])));
      let n = 1;
      while (used.has(`Query ${n}`)) n++;
      const id = `sql:${crypto.randomUUID()}`;
      if (text) {
        seeds.set(id, text);
        void ipc.setSetting(sqlTextKey(id), text);
      }
      commit({ tabs: [...tabs, { id, kind: "sql", connectionId, database, title: `Query ${n}` }], activeId: id });
    },

    openScript: (database, script) => {
      const { tabs } = get();
      const open = tabs.find((t) => t.kind === "sql" && t.scriptId === script.id);
      if (open) return commit({ tabs, activeId: open.id });
      const id = `sql:${crypto.randomUUID()}`;
      seeds.set(id, script.sql);
      void ipc.setSetting(sqlTextKey(id), script.sql);
      const tab: SqlTab = { id, kind: "sql", connectionId: script.connectionId, database, title: script.name, scriptId: script.id };
      commit({ tabs: [...tabs, tab], activeId: id });
    },

    bindScript: (id, script) =>
      commit({
        tabs: get().tabs.map((t) => (t.id === id && t.kind === "sql" ? { ...t, title: script.name, scriptId: script.id } : t)),
        activeId: get().activeId,
      }),

    renameScript: (scriptId, name) =>
      commit({
        tabs: get().tabs.map((t) => (t.kind === "sql" && t.scriptId === scriptId ? { ...t, title: name } : t)),
        activeId: get().activeId,
      }),

    unbindScript: (scriptId) =>
      commit({
        tabs: get().tabs.map((t) => {
          if (t.kind !== "sql" || t.scriptId !== scriptId) return t;
          const { scriptId: _gone, ...plain } = t;
          return plain;
        }),
        activeId: get().activeId,
      }),

    setSqlDatabase: (id, database) =>
      commit({
        tabs: get().tabs.map((t) => (t.id === id && t.kind === "sql" ? { ...t, database } : t)),
        activeId: get().activeId,
      }),

    openWelcome: () => {
      const { tabs } = get();
      commit({ tabs: tabs.some((t) => t.id === WELCOME.id) ? tabs : [WELCOME, ...tabs], activeId: WELCOME.id });
    },

    dirty: {},
    edited: {},
    closing: null,
    setDirty: (id, dirty) =>
      set((s) => {
        if (!!s.dirty[id] === dirty) return s;
        const next = { ...s.dirty };
        if (dirty) next[id] = true;
        else delete next[id];
        return { dirty: next };
      }),
    setEdited: (id, edited) =>
      set((s) => {
        if (!!s.edited[id] === edited) return s;
        const next = { ...s.edited };
        if (edited) next[id] = true;
        else delete next[id];
        return { edited: next };
      }),
    hasUnsavedFor: (connectionId) =>
      get().tabs.some((t) => t.kind !== "welcome" && t.connectionId === connectionId && get().dirty[t.id]),
    hasUnsavedInDatabase: (connectionId, database) =>
      get().tabs.some((t) => inDatabase(t, connectionId, database) && get().dirty[t.id]),
    cancelClose: () => set({ closing: null }),
    forceClose: (id) => {
      set({ closing: null });
      commit(closeTabIn(get().tabs, get().activeId, id));
    },

    activate: (id) => commit({ tabs: get().tabs, activeId: id }),
    close: (id) => {
      if (get().dirty[id] || get().edited[id]) return set({ closing: id, activeId: id });
      commit(closeTabIn(get().tabs, get().activeId, id));
    },
    // Bulk closes never discard work: tabs with unsaved edits stay open.
    closeOthers: (id) => {
      const keep = get().tabs.filter((t) => t.id === id || get().dirty[t.id] || get().edited[t.id]);
      if (keep.length) commit({ tabs: keep, activeId: id });
    },
    closeAll: () => {
      const keep = get().tabs.filter((t) => get().dirty[t.id] || get().edited[t.id]);
      if (keep.length) commit({ tabs: keep, activeId: keep.some((t) => t.id === get().activeId) ? get().activeId : keep[0]!.id });
      else commit({ tabs: [WELCOME], activeId: WELCOME.id });
    },
    closeForConnection: (connectionId) => {
      let next = { tabs: get().tabs, activeId: get().activeId };
      for (const t of get().tabs) {
        if (t.kind !== "welcome" && t.connectionId === connectionId) next = closeTabIn(next.tabs, next.activeId, t.id);
      }
      commit(next);
    },
    databaseGone: (connectionId, database, to) =>
      commit(databaseGoneIn(get().tabs, get().activeId, connectionId, database, to)),
  };
});
