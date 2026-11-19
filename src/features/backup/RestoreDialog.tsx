import { ArchiveRestore, CircleAlert, CircleCheck, Lock, Square, TriangleAlert, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, Field, IconButton, Input, Spinner } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { BackupRun, ConnectionView, RestorePlan } from "../../lib/types";
import { nodeKey, useConnections } from "../../stores/connections";
import { restoredName, sizeText } from "./schedule";

/**
 * "Restore…" on a backup in the history: load it into a database that is created for it.
 * Nothing that exists is replaced, so the only thing to decide is the new database's name.
 */
export function RestoreDialog({ connection, run, onClose }: { connection: ConnectionView; run: BackupRun | null; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={run !== null} onOpenChange={(open) => !open && !busy && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-[60] bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-restore-dialog
          // Focus on the name, selected: the first control is the close button, and focusing
          // it would open its tooltip over the title.
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            const name = (e.currentTarget as HTMLElement).querySelector<HTMLInputElement>("[data-restore-name]");
            name?.focus();
            name?.select();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-[60] flex max-h-[min(92vh,640px)] w-[min(94vw,520px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {run && <Body key={run.id} connection={connection} run={run} onBusy={setBusy} onClose={onClose} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

type Plan = ({ plan: RestorePlan } | { problem: string }) & { name: string };

function Body({ connection, run, onBusy, onClose }: { connection: ConnectionView; run: BackupRun; onBusy: (busy: boolean) => void; onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const [name, setName] = useState(() => restoredName(run.database, new Date(run.startedAt)));
  // The backend's answer for one name: the statement may only run while it is the one on screen.
  const [plan, setPlan] = useState<Plan | null>(null);
  const [running, setRunning] = useState<{ job: string; bytes: number; since: number } | null>(null);
  const [, setTick] = useState(0);
  const [done, setDone] = useState<{ database: string; seconds: number } | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    // Set again on every mount: React's strict mode mounts, unmounts and mounts once more.
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    if (name.trim() === "") return setPlan(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .restorePlan(connection.id, run.id, name)
        .then((p) => !stale && setPlan({ name, plan: p }))
        .catch((e: unknown) => !stale && setPlan({ name, problem: errorMessage(e) }));
    }, 150);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, run.id, name]);

  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setTick((n) => n + 1), 1000);
    return () => window.clearInterval(timer);
  }, [running]);

  const current = plan?.name === name;
  const ready = !!plan && current && "plan" in plan && !connection.readOnly && !running && !done;
  const production = connection.environment === "production";
  const total = run.bytes ?? 0;
  const elapsed = running ? Math.floor((Date.now() - running.since) / 1000) : 0;
  const when = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" });

  const restore = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    const job = crypto.randomUUID();
    const since = Date.now();
    setFailure(null);
    setRunning({ job, bytes: 0, since });
    onBusy(true);
    try {
      const restored = await ipc.restoreBackup(connection.id, run.id, name, job, (bytes) => setRunning((r) => (r ? { ...r, bytes } : r)));
      if (alive.current) setDone({ database: restored.database, seconds: Math.max(1, Math.round(restored.durationMs / 1000)) });
    } catch (err) {
      if (alive.current) setFailure(errorMessage(err));
    } finally {
      onBusy(false);
      if (alive.current) setRunning(null);
      // The new database is there after a success, and after most failures too.
      const store = useConnections.getState();
      store.toggle(nodeKey.connection(connection.id), true);
      void store.loadDatabases(connection.id).catch(() => undefined);
    }
  };

  return (
    <form onSubmit={(e) => void restore(e)} className="flex min-h-0 flex-col">
      <div className="flex shrink-0 items-start gap-3 border-b border-border py-3.5 pr-3 pl-5">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent-text">
          <ArchiveRestore className="size-4.5" />
        </div>
        <div className="min-w-0 flex-1">
          <Dialog.Title className="truncate text-[15px] font-semibold">{t("restore.title")}</Dialog.Title>
          <div className="mt-1 flex items-center gap-1.5 text-xs text-fg-muted">
            <EngineTag engine={connection.engine} className="size-4 rounded text-[9px]" />
            <span className="truncate">{connection.name}</span>
            <EnvBadge environment={connection.environment} />
          </div>
        </div>
        <Dialog.Close asChild>
          <IconButton label={t("common.close")} disabled={!!running}>
            <X className="size-4" />
          </IconButton>
        </Dialog.Close>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {/* ---- what is restored ---- */}
        <div data-restore-source className="rounded-xl bg-sunken px-3.5 py-2.5 text-xs">
          <div className="text-fg">
            {t("restore.source", { database: run.database, when: when.format(run.startedAt) })}
            {run.bytes !== null && <span className="text-fg-subtle"> · {sizeText(run.bytes)}</span>}
          </div>
          <div data-selectable className="mt-0.5 font-mono text-[11px] break-all text-fg-muted">
            {run.file?.split("/").pop()}
          </div>
        </div>

        {connection.readOnly && (
          <div className="flex items-start gap-2 rounded-lg bg-sunken px-3 py-2 text-[13px] text-fg-muted">
            <Lock className="mt-0.5 size-4 shrink-0" /> {t("restore.readOnly")}
          </div>
        )}

        <Field label={t("restore.name")} hint={t("restore.nameHint")}>
          <Input data-restore-name value={name} disabled={!!running || !!done} onChange={(e) => setName(e.target.value)} />
        </Field>

        {/* ---- what will run: the backend's own statement, then the vendor's client ---- */}
        <div className="flex flex-col gap-1">
          <span className="text-xs font-medium text-fg-muted">{t(done ? "restore.stepsDone" : "restore.steps")}</span>
          {plan && "plan" in plan ? (
            <div data-restore-plan aria-busy={!current} className={cn("flex flex-col gap-1.5 text-xs transition-opacity", !current && "opacity-50")}>
              <pre data-selectable className="rounded-lg border border-border bg-sunken px-3 py-2 font-mono leading-relaxed break-words whitespace-pre-wrap">
                {plan.plan.statement}
              </pre>
              <div className="leading-snug text-fg-muted">
                {t("restore.load", { client: plan.plan.client })}
                <span className="block font-mono text-[11px] text-fg-subtle">{plan.plan.clientPath}</span>
              </div>
              {!done && <div className="leading-snug text-fg-subtle">{t(plan.plan.allOrNothing ? "restore.atomic" : "restore.notAtomic")}</div>}
            </div>
          ) : (
            <div data-restore-no-plan className="rounded-lg border border-dashed border-border px-3 py-2 text-xs leading-snug text-fg-subtle">
              {plan ? <span className="text-danger">{plan.problem}</span> : t("restore.nameEmpty")}
            </div>
          )}
        </div>

        {production && !connection.readOnly && !done && (
          <div className="flex items-center gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] font-medium text-danger">
            <TriangleAlert className="size-4 shrink-0" /> {t("restore.prodWarning")}
          </div>
        )}

        {running && (
          <div data-restore-running className="flex items-center gap-3 rounded-xl bg-sunken px-3.5 py-2.5">
            <Spinner className="size-4 shrink-0 text-accent-text" />
            <div className="min-w-0 flex-1 text-[13px]">
              <div className="font-medium">{t("restore.running")}</div>
              <div className="text-xs text-fg-muted tabular-nums">
                {running.bytes > 0 && total > 0
                  ? t("restore.progress", {
                      percent: Math.min(100, Math.floor((running.bytes / total) * 100)),
                      minutes: Math.floor(elapsed / 60),
                      seconds: String(elapsed % 60).padStart(2, "0"),
                    })
                  : t("restore.elapsed", { minutes: Math.floor(elapsed / 60), seconds: String(elapsed % 60).padStart(2, "0") })}
              </div>
              {running.bytes > 0 && total > 0 && (
                <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-border">
                  <div className="h-full rounded-full bg-accent transition-[width]" style={{ width: `${Math.min(100, (running.bytes / total) * 100)}%` }} />
                </div>
              )}
            </div>
            <Button size="sm" icon={<Square className="size-3" />} onClick={() => void ipc.cancelJob(running.job)}>
              {t("backup.stop")}
            </Button>
          </div>
        )}

        {done && (
          <div data-restore-result="ok" className="flex items-start gap-2.5 rounded-xl bg-ok-soft px-3.5 py-2.5 text-[13px] text-ok">
            <CircleCheck className="mt-0.5 size-4 shrink-0" />
            <div data-selectable className="min-w-0 flex-1">
              <div className="font-medium break-words">{t("restore.done", { database: done.database })}</div>
              <div className="text-xs opacity-90">{t("restore.doneHint", { count: done.seconds })}</div>
            </div>
          </div>
        )}
        {failure && !running && (
          <div data-restore-result="failed" className="flex items-start gap-2.5 rounded-xl bg-danger-soft px-3.5 py-2.5 text-[13px] text-danger">
            <CircleAlert className="mt-0.5 size-4 shrink-0" />
            <div data-selectable className="min-w-0 flex-1">
              <div className="font-medium">{t("restore.failed")}</div>
              <div className="text-xs break-words whitespace-pre-wrap opacity-90">{failure}</div>
            </div>
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center justify-end gap-2 border-t border-border px-5 py-3">
        {done ? (
          <Button variant="primary" data-restore-close onClick={onClose}>
            {t("common.close")}
          </Button>
        ) : (
          <>
            <Button disabled={!!running} onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" variant={production ? "danger" : "primary"} data-restore-run icon={<ArchiveRestore className="size-3.5" />} disabled={!ready}>
              {t("restore.run")}
            </Button>
          </>
        )}
      </div>
    </form>
  );
}
