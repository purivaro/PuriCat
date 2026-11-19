import { create } from "zustand";

import { errorMessage, ipc } from "../lib/ipc";
import type { UpdateInfo, UpdateProgress } from "../lib/types";

/**
 * `idle`: nothing is known. `current`: the last check found nothing newer.
 * `ready`: the new version is on disk and starts with the next launch.
 */
export type UpdateStatus = "idle" | "checking" | "current" | "available" | "installing" | "ready" | "failed";

interface UpdatesState {
  status: UpdateStatus;
  info: UpdateInfo | null;
  progress: UpdateProgress | null;
  error: string | null;
  /** What "Try again" repeats. */
  failedStep: "check" | "install" | null;
  /** The dialog. */
  open: boolean;
  setOpen: (open: boolean) => void;
  /**
   * Look for a newer version. `quiet` is the look PuriCat takes by itself at start-up: the
   * dialog stays closed and a failure is not shown, since nobody asked.
   */
  check: (quiet?: boolean) => Promise<void>;
  install: () => Promise<void>;
  restart: () => void;
}

export const useUpdates = create<UpdatesState>((set, get) => ({
  status: "idle",
  info: null,
  progress: null,
  error: null,
  failedStep: null,
  open: false,
  setOpen: (open) => set({ open }),

  check: async (quiet = false) => {
    const { status } = get();
    // Something is under way or waiting for the user: show it, do not start over.
    if (status === "checking" || status === "installing" || status === "ready") {
      if (!quiet) set({ open: true });
      return;
    }
    const before = { status, info: get().info };
    set(quiet ? { status: "checking" } : { status: "checking", open: true, error: null, failedStep: null });
    try {
      const info = await ipc.checkUpdate();
      set({ status: info ? "available" : "current", info, error: null, failedStep: null });
    } catch (e) {
      // A quiet check that fails leaves things as they were (an update found earlier stays on offer).
      set(quiet ? before : { status: "failed", error: errorMessage(e), failedStep: "check" });
    }
  },

  install: async () => {
    if (get().status === "installing" || !get().info) return;
    set({ status: "installing", progress: null, error: null, failedStep: null });
    try {
      await ipc.installUpdate((progress) => set({ progress }));
      set({ status: "ready", progress: null });
    } catch (e) {
      set({ status: "failed", error: errorMessage(e), failedStep: "install", progress: null });
    }
  },

  restart: () => void ipc.restartApp(),
}));
