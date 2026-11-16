import { getVersion } from "@tauri-apps/api/app";
import { CircleAlert, Trash2 } from "lucide-react";
import { AlertDialog, Tooltip } from "radix-ui";
import { type PointerEvent, useEffect, useState } from "react";
import type React from "react";
import { useTranslation } from "react-i18next";
import { toast, Toaster } from "sonner";

import { AiPanel } from "./components/layout/AiPanel";
import { CommandPalette } from "./components/layout/CommandPalette";
import { Explorer } from "./components/layout/Explorer";
import { TitleBar } from "./components/layout/TitleBar";
import { Workspace } from "./components/layout/Workspace";
import { Button, StateMessage } from "./components/ui/primitives";
import { ConnectionDialog } from "./features/connections/ConnectionDialog";
import i18n from "./i18n";
import { errorMessage } from "./lib/ipc";
import { useConnections } from "./stores/connections";
import { resolveTheme, useSettings } from "./stores/settings";
import { useUi } from "./stores/ui";
import { useWorkspace } from "./stores/workspace";

const inTauri = "__TAURI_INTERNALS__" in window;

/** Vertical drag handle between two panels. `direction` is +1 when dragging right grows the panel. */
function Resizer({ width, onResize, direction, label }: { width: number; onResize: (w: number) => void; direction: 1 | -1; label: string }) {
  const start = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const move = (ev: globalThis.PointerEvent) => onResize(width + (ev.clientX - startX) * direction);
    const stop = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", stop);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", stop);
  };
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      onPointerDown={start}
      className="group relative z-10 -mx-1 w-2 shrink-0 cursor-col-resize"
    >
      <div className="absolute inset-y-3 left-[3px] w-0.5 rounded bg-transparent transition-colors group-hover:bg-accent group-active:bg-accent" />
    </div>
  );
}

function DeleteConnectionDialog() {
  const { t } = useTranslation();
  const target = useUi((s) => s.deleteTarget);
  const askDelete = useUi((s) => s.askDelete);
  const [busy, setBusy] = useState(false);

  const confirm = async () => {
    if (!target) return;
    setBusy(true);
    try {
      await useConnections.getState().remove(target.id);
      toast.success(t("connection.deleted"));
      askDelete(null);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <AlertDialog.Root open={target !== null} onOpenChange={(open) => !open && askDelete(null)}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <AlertDialog.Content className="animate-pop fixed top-1/2 left-1/2 z-50 w-[min(92vw,420px)] -translate-x-1/2 -translate-y-1/2 rounded-2xl border border-border bg-raised p-5 shadow-pop outline-none">
          <div className="flex gap-3.5">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-danger-soft text-danger">
              <Trash2 className="size-5" />
            </div>
            <div className="min-w-0 flex-1">
              <AlertDialog.Title className="text-[15px] leading-snug font-semibold break-words">
                {t("deleteConnection.title", { name: target?.name ?? "" })}
              </AlertDialog.Title>
              <AlertDialog.Description className="mt-1.5 text-[13px] leading-relaxed text-fg-muted">
                {t("deleteConnection.body")}
              </AlertDialog.Description>
            </div>
          </div>
          <div className="mt-5 flex justify-end gap-2">
            <AlertDialog.Cancel asChild>
              <Button variant="ghost">{t("common.cancel")}</Button>
            </AlertDialog.Cancel>
            <Button variant="danger" loading={busy} onClick={() => void confirm()}>
              {t("deleteConnection.confirm")}
            </Button>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}

function StatusBar() {
  const { t } = useTranslation();
  const total = useConnections((s) => s.connections.length);
  const open = useConnections((s) => Object.values(s.sessions).filter((x) => x.status === "connected").length);
  const [version, setVersion] = useState("");
  useEffect(() => {
    void getVersion().then(setVersion);
  }, []);
  return (
    <footer className="flex h-7 shrink-0 items-center gap-3 px-3.5 text-[11px] text-fg-subtle">
      <span className="flex items-center gap-1.5">
        <span className={`size-1.5 rounded-full ${open > 0 ? "bg-ok" : "bg-border-strong"}`} />
        {total === 0 ? t("status.ready") : t("status.connections", { open, total })}
      </span>
      <span className="flex-1" />
      {version && <span className="tabular-nums">PuriCat {t("status.version", { version })}</span>}
    </footer>
  );
}

export default function App() {
  const { t } = useTranslation();
  const { loaded, theme, explorerOpen, aiOpen, explorerWidth, aiWidth, update } = useSettings();
  const [startupError, setStartupError] = useState<string | null>(null);

  useEffect(() => {
    if (!inTauri) return;
    Promise.all([useSettings.getState().load(), useWorkspace.getState().restore(), useConnections.getState().load()]).catch(
      (e: unknown) => setStartupError(errorMessage(e)),
    );
  }, []);

  // Global shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const ui = useUi.getState();
      const settings = useSettings.getState();
      const actions: Record<string, () => void> = {
        k: () => ui.setPaletteOpen(!ui.paletteOpen),
        n: () => ui.openConnectionDialog(),
        b: () => settings.update({ explorerOpen: !settings.explorerOpen }),
        j: () => settings.update({ aiOpen: !settings.aiOpen }),
        w: () => useWorkspace.getState().close(useWorkspace.getState().activeId),
        // New query on the connection of the tab in front, or the only one that is connected.
        t: () => {
          const workspace = useWorkspace.getState();
          const current = workspace.tabs.find((x) => x.id === workspace.activeId);
          const sessions = useConnections.getState().sessions;
          const open = Object.keys(sessions).filter((id) => sessions[id]?.status === "connected");
          const target =
            current && current.kind !== "welcome" && sessions[current.connectionId]?.status === "connected"
              ? current.connectionId
              : open.length === 1
                ? open[0]
                : undefined;
          if (target) workspace.openSql(target, current?.kind === "table" ? current.table.database : current?.kind === "sql" ? current.database : null);
          else toast.info(i18n.t("sql.needConnection"));
        },
      };
      const action = !e.shiftKey && actions[e.key.toLowerCase()];
      if (!action) return;
      e.preventDefault();
      action();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!inTauri || startupError) {
    return (
      <div className="h-full bg-surface">
        <StateMessage
          tone="danger"
          icon={<CircleAlert className="size-5" />}
          title={startupError ? t("errors.startup") : "PuriCat"}
          body={startupError ? `${t("errors.startupBody")} ${startupError}` : t("errors.notInApp")}
        />
      </div>
    );
  }
  if (!loaded) return null; // a few milliseconds; avoids flashing the wrong layout

  const card = "min-h-0 overflow-hidden rounded-xl border border-border bg-surface shadow-card";

  return (
    <Tooltip.Provider delayDuration={350} skipDelayDuration={200}>
      <div className="flex h-full flex-col">
        <TitleBar />
        <main className="flex min-h-0 flex-1 gap-2 px-2">
          {explorerOpen && (
            <>
              <aside style={{ width: explorerWidth }} className={`${card} shrink-0`}>
                <Explorer />
              </aside>
              <Resizer
                label={t("titlebar.toggleExplorer")}
                width={explorerWidth}
                direction={1}
                onResize={(w) => update({ explorerWidth: Math.min(480, Math.max(200, w)) })}
              />
            </>
          )}
          <section className="min-w-0 flex-1">
            <Workspace />
          </section>
          {aiOpen && (
            <>
              <Resizer
                label={t("titlebar.toggleAi")}
                width={aiWidth}
                direction={-1}
                onResize={(w) => update({ aiWidth: Math.min(560, Math.max(280, w)) })}
              />
              <aside style={{ width: aiWidth }} className={`${card} shrink-0`}>
                <AiPanel />
              </aside>
            </>
          )}
        </main>
        <StatusBar />
      </div>

      <ConnectionDialog />
      <DeleteConnectionDialog />
      <CommandPalette />
      <Toaster
        theme={resolveTheme(theme)}
        position="bottom-left"
        offset={{ left: 16, bottom: 40 }}
        style={{ "--width": "252px" } as React.CSSProperties}
        duration={2600}
        toastOptions={{
          style: {
            background: "var(--raised)",
            color: "var(--fg)",
            border: "1px solid var(--border)",
            boxShadow: "var(--shadow-pop)",
            fontFamily: "var(--font-sans)",
            fontSize: 13,
          },
        }}
      />
    </Tooltip.Provider>
  );
}
