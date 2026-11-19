import { create } from "zustand";

import type { ConnectionView, FlushKind } from "../lib/types";

/** Something a database's or a connection's menu asked to do to the server. */
export type DatabaseTask =
  | { kind: "edit"; connection: ConnectionView; database: string }
  | { kind: "drop"; connection: ConnectionView; database: string }
  | { kind: "flush"; connection: ConnectionView; what: FlushKind };

interface UiState {
  /** `null` closed, `"new"` creating, otherwise the connection being edited. */
  connectionDialog: null | "new" | ConnectionView;
  deleteTarget: ConnectionView | null;
  /** The connection a new database is being created on. */
  newDatabaseOn: ConnectionView | null;
  databaseTask: DatabaseTask | null;
  /** The database whose backup dialog is open. */
  backupOf: { connection: ConnectionView; database: string } | null;
  paletteOpen: boolean;
  openConnectionDialog: (target?: ConnectionView) => void;
  closeConnectionDialog: () => void;
  askDelete: (target: ConnectionView | null) => void;
  askNewDatabase: (connection: ConnectionView | null) => void;
  askDatabaseTask: (task: DatabaseTask | null) => void;
  askBackup: (target: { connection: ConnectionView; database: string } | null) => void;
  setPaletteOpen: (open: boolean) => void;
}

export const useUi = create<UiState>((set) => ({
  connectionDialog: null,
  deleteTarget: null,
  newDatabaseOn: null,
  databaseTask: null,
  backupOf: null,
  paletteOpen: false,
  openConnectionDialog: (target) => set({ connectionDialog: target ?? "new" }),
  closeConnectionDialog: () => set({ connectionDialog: null }),
  askDelete: (deleteTarget) => set({ deleteTarget }),
  askNewDatabase: (newDatabaseOn) => set({ newDatabaseOn }),
  askDatabaseTask: (databaseTask) => set({ databaseTask }),
  askBackup: (backupOf) => set({ backupOf }),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
}));
