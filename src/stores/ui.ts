import { create } from "zustand";

import type { ConnectionView, FlushKind, ObjectKind, TableRef } from "../lib/types";

/** Something a database's or a connection's menu asked to do to the server. */
export type DatabaseTask =
  | { kind: "edit"; connection: ConnectionView; database: string }
  | { kind: "drop"; connection: ConnectionView; database: string }
  | { kind: "flush"; connection: ConnectionView; what: FlushKind };

/** Something the menu of a table or a view asked for. */
export interface TableTask {
  kind: "definition" | "rename" | "empty" | "drop";
  connection: ConnectionView;
  table: TableRef;
  objectKind: ObjectKind;
}

/** A view to make in a database (or a schema of it), or one whose query is to be changed. */
export type ViewTask =
  | { kind: "new"; connection: ConnectionView; database: string; schema: string | null }
  | { kind: "edit"; connection: ConnectionView; table: TableRef };

interface UiState {
  /** `null` closed, `"new"` creating, otherwise the connection being edited. */
  connectionDialog: null | "new" | ConnectionView;
  deleteTarget: ConnectionView | null;
  /** The connection a new database is being created on. */
  newDatabaseOn: ConnectionView | null;
  databaseTask: DatabaseTask | null;
  tableTask: TableTask | null;
  viewTask: ViewTask | null;
  /** The connection whose users and privileges are open. */
  usersOf: ConnectionView | null;
  /** The table whose columns and indexes the change form is changing. */
  alterOf: { connection: ConnectionView; table: TableRef } | null;
  /** Where the table form makes a new table. */
  newTableIn: { connection: ConnectionView; database: string; schema: string | null } | null;
  /** The connection whose server sessions are open. */
  sessionsOf: ConnectionView | null;
  /** The database whose backup dialog is open. */
  backupOf: { connection: ConnectionView; database: string } | null;
  paletteOpen: boolean;
  openConnectionDialog: (target?: ConnectionView) => void;
  closeConnectionDialog: () => void;
  askDelete: (target: ConnectionView | null) => void;
  askNewDatabase: (connection: ConnectionView | null) => void;
  askDatabaseTask: (task: DatabaseTask | null) => void;
  askTableTask: (task: TableTask | null) => void;
  askViewTask: (task: ViewTask | null) => void;
  askUsers: (connection: ConnectionView | null) => void;
  askSessions: (connection: ConnectionView | null) => void;
  askAlterTable: (target: { connection: ConnectionView; table: TableRef } | null) => void;
  askNewTable: (target: { connection: ConnectionView; database: string; schema: string | null } | null) => void;
  askBackup: (target: { connection: ConnectionView; database: string } | null) => void;
  setPaletteOpen: (open: boolean) => void;
}

export const useUi = create<UiState>((set) => ({
  connectionDialog: null,
  deleteTarget: null,
  newDatabaseOn: null,
  databaseTask: null,
  tableTask: null,
  viewTask: null,
  usersOf: null,
  sessionsOf: null,
  newTableIn: null,
  alterOf: null,
  backupOf: null,
  paletteOpen: false,
  openConnectionDialog: (target) => set({ connectionDialog: target ?? "new" }),
  closeConnectionDialog: () => set({ connectionDialog: null }),
  askDelete: (deleteTarget) => set({ deleteTarget }),
  askNewDatabase: (newDatabaseOn) => set({ newDatabaseOn }),
  askDatabaseTask: (databaseTask) => set({ databaseTask }),
  askTableTask: (tableTask) => set({ tableTask }),
  askViewTask: (viewTask) => set({ viewTask }),
  askUsers: (usersOf) => set({ usersOf }),
  askSessions: (sessionsOf) => set({ sessionsOf }),
  askNewTable: (newTableIn) => set({ newTableIn }),
  askAlterTable: (alterOf) => set({ alterOf }),
  askBackup: (backupOf) => set({ backupOf }),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
}));
