import { create } from "zustand";

import type { ConnectionView } from "../lib/types";

interface UiState {
  /** `null` closed, `"new"` creating, otherwise the connection being edited. */
  connectionDialog: null | "new" | ConnectionView;
  deleteTarget: ConnectionView | null;
  paletteOpen: boolean;
  openConnectionDialog: (target?: ConnectionView) => void;
  closeConnectionDialog: () => void;
  askDelete: (target: ConnectionView | null) => void;
  setPaletteOpen: (open: boolean) => void;
}

export const useUi = create<UiState>((set) => ({
  connectionDialog: null,
  deleteTarget: null,
  paletteOpen: false,
  openConnectionDialog: (target) => set({ connectionDialog: target ?? "new" }),
  closeConnectionDialog: () => set({ connectionDialog: null }),
  askDelete: (deleteTarget) => set({ deleteTarget }),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
}));
