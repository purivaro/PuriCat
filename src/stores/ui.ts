import { create } from "zustand";

import type { ConnectionView } from "../lib/types";

interface UiState {
  /** `null` closed, `"new"` creating, otherwise the connection being edited. */
  connectionDialog: null | "new" | ConnectionView;
  deleteTarget: ConnectionView | null;
  /** The connection a new database is being created on. */
  newDatabaseOn: ConnectionView | null;
  paletteOpen: boolean;
  openConnectionDialog: (target?: ConnectionView) => void;
  closeConnectionDialog: () => void;
  askDelete: (target: ConnectionView | null) => void;
  askNewDatabase: (connection: ConnectionView | null) => void;
  setPaletteOpen: (open: boolean) => void;
}

export const useUi = create<UiState>((set) => ({
  connectionDialog: null,
  deleteTarget: null,
  newDatabaseOn: null,
  paletteOpen: false,
  openConnectionDialog: (target) => set({ connectionDialog: target ?? "new" }),
  closeConnectionDialog: () => set({ connectionDialog: null }),
  askDelete: (deleteTarget) => set({ deleteTarget }),
  askNewDatabase: (newDatabaseOn) => set({ newDatabaseOn }),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
}));
