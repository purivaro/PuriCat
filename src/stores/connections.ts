import { create } from "zustand";

import i18n from "../i18n";
import { errorMessage, ipc } from "../lib/ipc";
import type { ConnectionProfile, ConnectionView, DbObject, Namespace, PasswordChange, ServerInfo } from "../lib/types";
import { useWorkspace } from "./workspace";

export type SessionState =
  | { status: "idle" }
  | { status: "connecting" }
  | { status: "connected"; info: ServerInfo }
  | { status: "error"; message: string };

/** One lazily loaded level of the explorer tree. */
export type Loadable<T> =
  | { status: "loading" }
  | { status: "ready"; items: T[] }
  | { status: "error"; message: string };

/** Tree keys. JSON keeps names with slashes or dots unambiguous. */
export const nodeKey = {
  connection: (id: string) => JSON.stringify([id]),
  database: (id: string, database: string) => JSON.stringify([id, database]),
  schema: (id: string, database: string, schema: string) => JSON.stringify([id, database, schema]),
};

/** Profile fields that make the backend close an open session when they change. */
const SESSION_FIELDS = [
  "engine",
  "host",
  "port",
  "username",
  "database",
  "tls",
  "tlsCaPath",
  "readOnly",
  "connectTimeoutSecs",
] as const satisfies readonly (keyof ConnectionProfile)[];

interface ConnectionsState {
  connections: ConnectionView[];
  loaded: boolean;
  sessions: Record<string, SessionState>;
  databases: Record<string, Loadable<Namespace>>;
  schemas: Record<string, Loadable<Namespace>>;
  objects: Record<string, Loadable<DbObject>>;
  expanded: Record<string, boolean>;

  load: () => Promise<void>;
  save: (profile: ConnectionProfile, password: PasswordChange) => Promise<ConnectionView>;
  duplicate: (id: string, name: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  toggleFavorite: (id: string) => Promise<void>;

  connect: (id: string) => Promise<boolean>;
  disconnect: (id: string) => Promise<void>;

  toggle: (key: string, open?: boolean) => void;
  loadDatabases: (id: string) => Promise<void>;
  loadSchemas: (id: string, database: string) => Promise<void>;
  loadObjects: (id: string, database: string, schema: string | null) => Promise<void>;
}

function forget<T>(record: Record<string, T>, connectionId: string): Record<string, T> {
  const prefix = JSON.stringify([connectionId]).slice(0, -1); // `["id"`
  return Object.fromEntries(Object.entries(record).filter(([k]) => !k.startsWith(prefix)));
}

export const useConnections = create<ConnectionsState>((set, get) => {
  const replace = (view: ConnectionView) =>
    set((s) => ({
      connections: [...s.connections.filter((c) => c.id !== view.id), view].sort((a, b) =>
        a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
      ),
    }));

  /** Drop the session and everything cached from it. */
  const reset = (id: string, session: SessionState = { status: "idle" }) =>
    set((s) => ({
      sessions: { ...s.sessions, [id]: session },
      databases: forget(s.databases, id),
      schemas: forget(s.schemas, id),
      objects: forget(s.objects, id),
    }));

  return {
    connections: [],
    loaded: false,
    sessions: {},
    databases: {},
    schemas: {},
    objects: {},
    expanded: {},

    load: async () => set({ connections: await ipc.listConnections(), loaded: true }),

    save: async (profile, password) => {
      // Changing how to connect closes the session, which would drop staged edits.
      // Cosmetic changes (name, label, favorite, tags) leave the session alone and are fine.
      const before = get().connections.find((c) => c.id === profile.id);
      const reconnects =
        password.action !== "keep" || !before || SESSION_FIELDS.some((k) => before[k] !== profile[k]);
      if (profile.id && reconnects && useWorkspace.getState().hasUnsavedFor(profile.id)) {
        throw new Error(i18n.t("table.unsavedBlocks"));
      }
      const { connection, sessionClosed } = await ipc.saveConnection(profile, password);
      replace(connection);
      if (sessionClosed) reset(connection.id);
      return connection;
    },

    duplicate: async (id, name) => replace(await ipc.duplicateConnection(id, name)),

    remove: async (id) => {
      if (useWorkspace.getState().hasUnsavedFor(id)) throw new Error(i18n.t("table.unsavedBlocks"));
      await ipc.deleteConnection(id);
      useWorkspace.getState().closeForConnection(id);
      reset(id);
      set((s) => ({ connections: s.connections.filter((c) => c.id !== id) }));
    },

    toggleFavorite: async (id) => {
      const current = get().connections.find((c) => c.id === id);
      if (!current) return;
      const { hasPassword: _ignored, ...profile } = current;
      await get().save({ ...profile, favorite: !profile.favorite }, { action: "keep" });
    },

    connect: async (id) => {
      const state = get().sessions[id]?.status;
      if (state === "connecting") return false;
      if (state === "connected") return true;
      set((s) => ({ sessions: { ...s.sessions, [id]: { status: "connecting" } } }));
      try {
        const info = await ipc.openConnection(id);
        set((s) => ({
          sessions: { ...s.sessions, [id]: { status: "connected", info } },
          // The backend stamped the profile; mirror it so "recent connections" is right at once.
          connections: s.connections.map((c) => (c.id === id ? { ...c, lastUsedAt: Date.now() } : c)),
        }));
        return true;
      } catch (e) {
        set((s) => ({ sessions: { ...s.sessions, [id]: { status: "error", message: errorMessage(e) } } }));
        return false;
      }
    },

    disconnect: async (id) => {
      if (useWorkspace.getState().hasUnsavedFor(id)) throw new Error(i18n.t("table.unsavedBlocks"));
      await ipc.closeConnection(id);
      reset(id);
      set((s) => ({ expanded: { ...s.expanded, [nodeKey.connection(id)]: false } }));
    },

    toggle: (key, open) => set((s) => ({ expanded: { ...s.expanded, [key]: open ?? !s.expanded[key] } })),

    loadDatabases: async (id) => {
      const key = nodeKey.connection(id);
      set((s) => ({ databases: { ...s.databases, [key]: { status: "loading" } } }));
      try {
        const items = await ipc.listDatabases(id);
        set((s) => ({ databases: { ...s.databases, [key]: { status: "ready", items } } }));
      } catch (e) {
        set((s) => ({ databases: { ...s.databases, [key]: { status: "error", message: errorMessage(e) } } }));
      }
    },

    loadSchemas: async (id, database) => {
      const key = nodeKey.database(id, database);
      set((s) => ({ schemas: { ...s.schemas, [key]: { status: "loading" } } }));
      try {
        const items = await ipc.listSchemas(id, database);
        set((s) => ({ schemas: { ...s.schemas, [key]: { status: "ready", items } } }));
      } catch (e) {
        set((s) => ({ schemas: { ...s.schemas, [key]: { status: "error", message: errorMessage(e) } } }));
      }
    },

    loadObjects: async (id, database, schema) => {
      const key = schema === null ? nodeKey.database(id, database) : nodeKey.schema(id, database, schema);
      set((s) => ({ objects: { ...s.objects, [key]: { status: "loading" } } }));
      try {
        const items = await ipc.listObjects(id, database, schema);
        set((s) => ({ objects: { ...s.objects, [key]: { status: "ready", items } } }));
      } catch (e) {
        set((s) => ({ objects: { ...s.objects, [key]: { status: "error", message: errorMessage(e) } } }));
      }
    },
  };
});

const IDLE: SessionState = { status: "idle" };

/** Stable fallback object: selectors must return the same reference for unchanged state. */
export function sessionOf(sessions: Record<string, SessionState>, id: string): SessionState {
  return sessions[id] ?? IDLE;
}
