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

export type Tab = WelcomeTab | TableTab;

const WELCOME: WelcomeTab = { id: "welcome", kind: "welcome" };

export function tableTabId(connectionId: string, t: TableRef): string {
  return JSON.stringify([connectionId, t.database, t.schema, t.name]);
}

/** Remove a tab and decide which tab becomes active: the right neighbour, else the left. */
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
  /** A dirty tab the user asked to close; the workspace asks before discarding. */
  closing: string | null;
  setDirty: (id: string, dirty: boolean) => void;
  hasUnsavedFor: (connectionId: string) => boolean;
  /** Close even if dirty (after the user confirmed). */
  forceClose: (id: string) => void;
  cancelClose: () => void;
  restore: () => Promise<void>;
  openTable: (connectionId: string, table: TableRef, objectKind: ObjectKind) => void;
  openWelcome: () => void;
  activate: (id: string) => void;
  close: (id: string) => void;
  closeOthers: (id: string) => void;
  closeAll: () => void;
  closeForConnection: (connectionId: string) => void;
}

function persist(state: Pick<WorkspaceState, "tabs" | "activeId">) {
  void ipc.setSetting("workspace", JSON.stringify({ tabs: state.tabs, activeId: state.activeId }));
}

function isTab(v: unknown): v is Tab {
  if (!v || typeof v !== "object") return false;
  const t = v as Record<string, unknown>;
  if (t.kind === "welcome") return t.id === "welcome";
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

    openWelcome: () => {
      const { tabs } = get();
      commit({ tabs: tabs.some((t) => t.id === WELCOME.id) ? tabs : [WELCOME, ...tabs], activeId: WELCOME.id });
    },

    dirty: {},
    closing: null,
    setDirty: (id, dirty) =>
      set((s) => {
        if (!!s.dirty[id] === dirty) return s;
        const next = { ...s.dirty };
        if (dirty) next[id] = true;
        else delete next[id];
        return { dirty: next };
      }),
    hasUnsavedFor: (connectionId) =>
      get().tabs.some((t) => t.kind === "table" && t.connectionId === connectionId && get().dirty[t.id]),
    cancelClose: () => set({ closing: null }),
    forceClose: (id) => {
      set({ closing: null });
      commit(closeTabIn(get().tabs, get().activeId, id));
    },

    activate: (id) => commit({ tabs: get().tabs, activeId: id }),
    close: (id) => {
      if (get().dirty[id]) return set({ closing: id, activeId: id });
      commit(closeTabIn(get().tabs, get().activeId, id));
    },
    // Bulk closes never discard work: tabs with unsaved edits stay open.
    closeOthers: (id) => {
      const keep = get().tabs.filter((t) => t.id === id || get().dirty[t.id]);
      if (keep.length) commit({ tabs: keep, activeId: id });
    },
    closeAll: () => {
      const keep = get().tabs.filter((t) => get().dirty[t.id]);
      if (keep.length) commit({ tabs: keep, activeId: keep.some((t) => t.id === get().activeId) ? get().activeId : keep[0]!.id });
      else commit({ tabs: [WELCOME], activeId: WELCOME.id });
    },
    closeForConnection: (connectionId) => {
      let next = { tabs: get().tabs, activeId: get().activeId };
      for (const t of get().tabs) {
        if (t.kind === "table" && t.connectionId === connectionId) next = closeTabIn(next.tabs, next.activeId, t.id);
      }
      commit(next);
    },
  };
});
