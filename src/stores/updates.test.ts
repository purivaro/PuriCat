import { beforeEach, describe, expect, it, vi } from "vitest";

import type { UpdateInfo, UpdateProgress } from "../lib/types";

const checkUpdate = vi.fn<() => Promise<UpdateInfo | null>>();
const installUpdate = vi.fn<(onProgress: (p: UpdateProgress) => void) => Promise<void>>();
const restartApp = vi.fn<() => Promise<void>>(() => Promise.resolve());
vi.mock("../lib/ipc", () => ({
  ipc: {
    checkUpdate: () => checkUpdate(),
    installUpdate: (onProgress: (p: UpdateProgress) => void) => installUpdate(onProgress),
    restartApp: () => restartApp(),
  },
  errorMessage: (e: unknown) => (e as { message: string }).message,
}));

const { useUpdates } = await import("./updates");

const found: UpdateInfo = { version: "0.4.0", currentVersion: "0.3.1", date: "2026-12-01T00:00:00Z", notes: null };
const state = () => useUpdates.getState();

beforeEach(() => {
  checkUpdate.mockReset();
  installUpdate.mockReset();
  restartApp.mockClear();
  useUpdates.setState({ status: "idle", info: null, progress: null, error: null, failedStep: null, open: false });
});

describe("updates store", () => {
  it("a check that was asked for opens the dialog and shows what it found", async () => {
    checkUpdate.mockResolvedValue(found);
    const done = state().check();
    expect(state()).toMatchObject({ status: "checking", open: true });
    await done;
    expect(state()).toMatchObject({ status: "available", info: found, open: true });

    checkUpdate.mockResolvedValue(null);
    await state().check();
    expect(state()).toMatchObject({ status: "current", info: null });
  });

  it("the check at start-up stays out of the way: no dialog, and a failure is not shown", async () => {
    checkUpdate.mockRejectedValue({ kind: "io", message: "offline" });
    await state().check(true);
    expect(state()).toMatchObject({ status: "idle", open: false, error: null });

    checkUpdate.mockResolvedValue(found);
    await state().check(true);
    expect(state()).toMatchObject({ status: "available", info: found, open: false });

    // A later quiet check that fails keeps the update that was found on offer.
    checkUpdate.mockRejectedValue({ kind: "io", message: "offline" });
    await state().check(true);
    expect(state()).toMatchObject({ status: "available", info: found, error: null });
  });

  it("a failed check that was asked for says why, and can be repeated", async () => {
    checkUpdate.mockRejectedValue({ kind: "io", message: "Could not check for updates: offline" });
    await state().check();
    expect(state()).toMatchObject({ status: "failed", failedStep: "check", error: "Could not check for updates: offline", open: true });

    checkUpdate.mockResolvedValue(null);
    await state().check();
    expect(state()).toMatchObject({ status: "current", error: null, failedStep: null });
  });

  it("installs with progress, then waits for a restart instead of checking again", async () => {
    checkUpdate.mockResolvedValue(found);
    await state().check();
    installUpdate.mockImplementation(async (onProgress) => {
      expect(state().status).toBe("installing");
      onProgress({ downloaded: 512, total: 2048 });
      expect(state().progress).toEqual({ downloaded: 512, total: 2048 });
    });
    await state().install();
    expect(state()).toMatchObject({ status: "ready", info: found, progress: null });

    state().setOpen(false);
    await state().check();
    expect(checkUpdate).toHaveBeenCalledTimes(1);
    expect(state()).toMatchObject({ status: "ready", open: true });

    state().restart();
    expect(restartApp).toHaveBeenCalledTimes(1);
  });

  it("a failed install keeps the update, so it can be tried again", async () => {
    checkUpdate.mockResolvedValue(found);
    await state().check();
    installUpdate.mockRejectedValueOnce({ kind: "io", message: "The download failed: reset" });
    await state().install();
    expect(state()).toMatchObject({ status: "failed", failedStep: "install", info: found, error: "The download failed: reset" });

    installUpdate.mockResolvedValueOnce();
    await state().install();
    expect(state().status).toBe("ready");
  });

  it("does not install when no update was found", async () => {
    await state().install();
    expect(installUpdate).not.toHaveBeenCalled();
    expect(state().status).toBe("idle");
  });
});
