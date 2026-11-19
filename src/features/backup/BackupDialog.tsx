import { CircleAlert, CircleCheck, Clock, FolderOpen, HardDriveDownload, Square, TriangleAlert, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, IconButton, inputClass, Segmented, Spinner, StateMessage, Switch } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { BackupAgent, BackupFormat, BackupPlan, BackupRun, BackupSchedule, ConnectionView } from "../../lib/types";
import { useUi } from "../../stores/ui";
import { fileName, scheduleOf, sizeText, type ScheduleKind, toTimeInput } from "./schedule";

/**
 * "Back up…" from a database's menu: make a backup now, into a folder of the user's choice,
 * and say when backups should run by themselves. Everything about one database's backups is
 * here, with the history of what was made and what failed.
 */
export function BackupDialog() {
  const target = useUi((s) => s.backupOf);
  const close = () => useUi.getState().askBackup(null);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={target !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-backup-dialog
          // Focus on the dialog itself: the first control is the close button, and focusing
          // it would open its tooltip over the title.
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[min(92vh,760px)] w-[min(94vw,600px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {target && <Body key={`${target.connection.id}/${target.database}`} connection={target.connection} database={target.database} onBusy={setBusy} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

const sectionTitle = "text-[11px] font-semibold tracking-wider text-fg-subtle uppercase";

function Body({ connection, database, onBusy }: { connection: ConnectionView; database: string; onBusy: (busy: boolean) => void }) {
  const { t, i18n } = useTranslation();
  const postgres = connection.engine === "postgres";

  const [plan, setPlan] = useState<BackupPlan | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [destination, setDestination] = useState("");
  const [format, setFormat] = useState<BackupFormat>("sql");
  const [compress, setCompress] = useState(true);
  const [kind, setKind] = useState<ScheduleKind>("off");
  const [time, setTime] = useState("02:00");
  const [days, setDays] = useState<number[]>([1]);
  const [keep, setKeep] = useState(7);
  // What is saved, to tell whether the schedule on screen differs from it.
  const [saved, setSaved] = useState<{ schedule: BackupSchedule; keep: number } | null>(null);
  const [agent, setAgent] = useState<BackupAgent | null>(null);
  const [nextRun, setNextRun] = useState<string | null>(null);
  const [runs, setRuns] = useState<BackupRun[]>([]);

  // The run in progress: its cancel token, the bytes so far, and when it began.
  const [running, setRunning] = useState<{ job: string; bytes: number; since: number } | null>(null);
  const [, setTick] = useState(0);
  const [last, setLast] = useState<BackupRun | null>(null);
  const [saving, setSaving] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    // Set again on every mount: React's strict mode mounts, unmounts and mounts once more.
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    let stale = false;
    ipc
      .backupPlan(connection.id, database)
      .then((p) => {
        if (stale) return;
        setPlan(p);
        setDestination(p.job?.destination ?? p.defaultDestination);
        // PostgreSQL's own archive is the better default there: compressed, and restorable in parts.
        setFormat(p.job?.format ?? (postgres ? "custom" : "sql"));
        setCompress(p.job?.compress ?? true);
        const schedule = p.job?.schedule ?? { kind: "off" };
        setKind(schedule.kind);
        if (schedule.kind !== "off") setTime(toTimeInput(schedule.hour, schedule.minute));
        if (schedule.kind === "weekly") setDays(schedule.days);
        setKeep(p.job?.keep ?? 7);
        setSaved(p.job ? { schedule, keep: p.job.keep } : null);
        setAgent(p.agent);
        setNextRun(p.nextRun);
        setRuns(p.runs);
      })
      .catch((e: unknown) => !stale && setFailed(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [connection.id, database, postgres]);

  // A second hand for the elapsed time while a backup runs.
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setTick((n) => n + 1), 1000);
    return () => window.clearInterval(timer);
  }, [running]);

  const schedule = scheduleOf(kind, time, days);
  const scheduleValid = kind !== "weekly" || days.length > 0;
  const scheduleChanged = JSON.stringify({ schedule, keep }) !== JSON.stringify(saved ?? { schedule: { kind: "off" }, keep: 7 });

  const refreshRuns = () =>
    ipc.listBackupRuns(connection.id, database, 20).then((list) => {
      if (alive.current) setRuns(list);
    });

  const backUpNow = async () => {
    const job = crypto.randomUUID();
    setProblem(null);
    setLast(null);
    setRunning({ job, bytes: 0, since: Date.now() });
    onBusy(true);
    try {
      const run = await ipc.runBackup(connection.id, database, { destination, format, compress }, job, (bytes) =>
        setRunning((r) => (r ? { ...r, bytes } : r)),
      );
      if (!alive.current) return;
      setLast(run);
      await refreshRuns();
    } catch (e) {
      if (alive.current) setProblem(errorMessage(e));
    } finally {
      onBusy(false);
      if (alive.current) setRunning(null);
    }
  };

  const chooseFolder = async () => {
    try {
      const folder = await ipc.pickBackupFolder();
      if (folder && alive.current) setDestination(folder);
    } catch (e) {
      setProblem(errorMessage(e));
    }
  };

  const saveSchedule = async () => {
    setSaving(true);
    setProblem(null);
    try {
      const done = await ipc.saveBackupJob(connection.id, database, { destination, format, compress, schedule, keep });
      if (!alive.current) return;
      setSaved({ schedule: done.job.schedule, keep: done.job.keep });
      setAgent(done.agent);
      setNextRun(done.nextRun);
    } catch (e) {
      if (alive.current) setProblem(errorMessage(e));
    } finally {
      if (alive.current) setSaving(false);
    }
  };

  const reveal = (run: BackupRun) => void ipc.revealBackup(run.id).catch((e: unknown) => setProblem(errorMessage(e)));
  const when = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" });
  const weekday = new Intl.DateTimeFormat(i18n.language, { weekday: "short" });
  // 2024-01-01 was a Monday: day 1.
  const dayName = (day: number) => weekday.format(new Date(2024, 0, day));
  const elapsed = running ? Math.floor((Date.now() - running.since) / 1000) : 0;

  const header = (
    <div className="flex shrink-0 items-start gap-3 border-b border-border py-3.5 pr-3 pl-5">
      <div className="flex size-9 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent-text">
        <HardDriveDownload className="size-4.5" />
      </div>
      <div className="min-w-0 flex-1">
        <Dialog.Title className="truncate text-[15px] font-semibold">{t("backup.title", { database })}</Dialog.Title>
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
  );

  if (failed) {
    return (
      <>
        {header}
        <div className="h-56">
          <StateMessage tone="danger" icon={<CircleAlert className="size-5" />} title={t("backup.loadFailed")} body={failed} />
        </div>
      </>
    );
  }
  if (!plan) {
    return (
      <>
        {header}
        <div className="flex h-56 items-center justify-center gap-2 text-[13px] text-fg-muted">
          <Spinner className="size-3.5" /> {t("common.loading")}
        </div>
      </>
    );
  }

  const tool = plan.choice.tool;
  return (
    <>
      {header}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {/* ---- the tool: what makes the backup, and whether it fits the server ---- */}
        <div className="border-b border-border px-5 py-3 text-xs">
          {tool ? (
            <div data-backup-tool className="flex items-start gap-2 text-fg-muted">
              <CircleCheck className="mt-px size-3.5 shrink-0 text-ok" />
              <span className="min-w-0 break-words">
                {t("backup.tool", { tool: `${tool.path.split("/").pop()} ${tool.version}`, server: plan.serverVersion })}
                <span className="block font-mono text-[11px] text-fg-subtle">{tool.path}</span>
              </span>
            </div>
          ) : (
            <div data-backup-no-tool className="flex items-start gap-2 text-danger">
              <CircleAlert className="mt-px size-3.5 shrink-0" />
              <span className="min-w-0 break-words">{plan.choice.problem}</span>
            </div>
          )}
          {plan.choice.warning && (
            <div className="mt-2 flex items-start gap-2 rounded-lg bg-warn-soft px-2.5 py-2 text-warn">
              <TriangleAlert className="mt-px size-3.5 shrink-0" />
              <span className="min-w-0 break-words">{plan.choice.warning}</span>
            </div>
          )}
        </div>

        {/* ---- where to, and in which form ---- */}
        <div className="flex flex-col gap-3 border-b border-border px-5 py-4">
          <div>
            <div className={sectionTitle}>{t("backup.folder")}</div>
            <div className="mt-1.5 flex items-center gap-2">
              <div data-backup-folder data-selectable title={destination} className={cn(inputClass, "flex min-w-0 flex-1 items-center font-mono text-xs")}>
                <span className="truncate [direction:rtl]">
                  <bdi>{destination}</bdi>
                </span>
              </div>
              <Button data-backup-choose icon={<FolderOpen className="size-3.5" />} disabled={!!running} onClick={() => void chooseFolder()}>
                {t("backup.choose")}
              </Button>
            </div>
            <div className="mt-1.5 text-xs leading-snug break-all text-fg-subtle">
              {t("backup.fileName")}{" "}
              <span data-backup-file-name className="font-mono text-fg-muted">
                {fileName(connection.name, database, new Date(), format, compress)}
              </span>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
            {postgres && (
              <Segmented
                label={t("backup.format")}
                value={format}
                onChange={setFormat}
                options={[
                  { value: "custom", label: t("backup.formatCustom") },
                  { value: "sql", label: t("backup.formatSql") },
                ]}
              />
            )}
            {format === "sql" ? (
              <label className="flex items-center gap-2 text-[13px]">
                <Switch checked={compress} onChange={setCompress} label={t("backup.compress")} />
                {t("backup.compress")}
              </label>
            ) : (
              <span className="text-xs text-fg-subtle">{t("backup.customNote")}</span>
            )}
          </div>

          {running ? (
            <div data-backup-running className="flex items-center gap-3 rounded-xl bg-sunken px-3.5 py-2.5">
              <Spinner className="size-4 text-accent-text" />
              <div className="min-w-0 flex-1 text-[13px]">
                <div className="font-medium">{t("backup.running")}</div>
                <div className="text-xs text-fg-muted tabular-nums">
                  {t("backup.progress", { size: sizeText(running.bytes), minutes: Math.floor(elapsed / 60), seconds: String(elapsed % 60).padStart(2, "0") })}
                </div>
              </div>
              <Button size="sm" icon={<Square className="size-3" />} onClick={() => void ipc.cancelJob(running.job)}>
                {t("backup.stop")}
              </Button>
            </div>
          ) : (
            <div className="flex items-center gap-3">
              <Button variant="primary" data-backup-now icon={<HardDriveDownload className="size-3.5" />} disabled={!tool} onClick={() => void backUpNow()}>
                {t("backup.now")}
              </Button>
              <span className="text-xs leading-snug text-fg-subtle">{t("backup.contains")}</span>
            </div>
          )}

          {last && !running && (
            <div
              data-backup-result={last.status}
              className={cn(
                "flex items-start gap-2.5 rounded-xl px-3.5 py-2.5 text-[13px]",
                last.status === "ok" ? "bg-ok-soft text-ok" : last.status === "stopped" ? "bg-sunken text-fg-muted" : "bg-danger-soft text-danger",
              )}
            >
              {last.status === "ok" ? <CircleCheck className="mt-0.5 size-4 shrink-0" /> : <CircleAlert className="mt-0.5 size-4 shrink-0" />}
              <div data-selectable className="min-w-0 flex-1">
                {last.status === "ok" ? (
                  <>
                    <div className="font-medium">{t("backup.done", { size: sizeText(last.bytes ?? 0) })}</div>
                    <div className="font-mono text-xs break-all opacity-90">{last.file}</div>
                  </>
                ) : (
                  <>
                    <div className="font-medium">{t(last.status === "stopped" ? "backup.stopped" : "backup.failed")}</div>
                    <div className="text-xs break-words opacity-90">{last.message}</div>
                  </>
                )}
              </div>
              {last.status === "ok" && (
                <Button size="sm" variant="secondary" onClick={() => reveal(last)}>
                  {t("backup.reveal")}
                </Button>
              )}
            </div>
          )}
          {problem && (
            <div data-backup-problem className="flex items-start gap-2 rounded-xl bg-danger-soft px-3.5 py-2.5 text-[13px] text-danger">
              <CircleAlert className="mt-0.5 size-4 shrink-0" />
              <span data-selectable className="min-w-0 break-words">
                {problem}
              </span>
            </div>
          )}
        </div>

        {/* ---- the schedule ---- */}
        <div className="flex flex-col gap-3 border-b border-border px-5 py-4">
          <div className="flex items-center gap-3">
            <div className={cn(sectionTitle, "flex-1")}>{t("backup.schedule")}</div>
            <Segmented
              label={t("backup.schedule")}
              value={kind}
              onChange={setKind}
              options={[
                { value: "off", label: t("backup.off") },
                { value: "daily", label: t("backup.daily") },
                { value: "weekly", label: t("backup.weekly") },
              ]}
            />
          </div>
          {kind !== "off" && (
            <>
              <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[13px]">
                <label className="flex items-center gap-2 whitespace-nowrap">
                  {t("backup.at")}
                  {/* The width is on a box: the input's own class says full width. */}
                  <span className="w-24">
                    <input
                      type="time"
                      data-backup-time
                      value={time}
                      onChange={(e) => e.target.value && setTime(e.target.value)}
                      className={cn(inputClass, "tabular-nums")}
                    />
                  </span>
                </label>
                {kind === "weekly" && (
                  <div role="group" aria-label={t("backup.days")} className="flex gap-1">
                    {[1, 2, 3, 4, 5, 6, 7].map((day) => {
                      const on = days.includes(day);
                      return (
                        <button
                          key={day}
                          type="button"
                          aria-pressed={on}
                          onClick={() => setDays(on ? days.filter((d) => d !== day) : [...days, day].sort())}
                          className={cn(
                            "h-7 min-w-9 rounded-md px-1.5 text-xs transition-colors outline-none focus-visible:ring-2 focus-visible:ring-(--ring)",
                            on ? "bg-accent font-semibold text-accent-fg" : "bg-sunken text-fg-muted hover:text-fg",
                          )}
                        >
                          {dayName(day)}
                        </button>
                      );
                    })}
                  </div>
                )}
                <label className="flex items-center gap-2 whitespace-nowrap">
                  {t("backup.keep")}
                  <span className="w-[4.5rem]">
                    <input
                      type="number"
                      min={0}
                      max={1000}
                      data-backup-keep
                      value={keep}
                      onChange={(e) => setKeep(Math.max(0, Math.min(1000, Math.floor(Number(e.target.value) || 0))))}
                      className={cn(inputClass, "tabular-nums")}
                    />
                  </span>
                  {t("backup.keepUnit")}
                </label>
              </div>
              <p className="text-xs leading-relaxed text-fg-subtle">
                {keep === 0 ? t("backup.keepAll") : t("backup.keepHint", { count: keep })} {t("backup.runnerNote")}
              </p>
            </>
          )}
          <div className="flex items-center gap-3">
            <Button data-backup-save disabled={!scheduleChanged || !scheduleValid || saving || !!running} loading={saving} onClick={() => void saveSchedule()}>
              {t(kind === "off" && saved?.schedule.kind !== "off" && saved ? "backup.removeSchedule" : "backup.saveSchedule")}
            </Button>
            <div data-backup-schedule-status className="min-w-0 flex-1 text-xs leading-snug">
              {!scheduleValid ? (
                <span className="text-danger">{t("backup.needDay")}</span>
              ) : scheduleChanged ? (
                <span className="text-fg-subtle">{t("backup.notSaved")}</span>
              ) : saved && saved.schedule.kind !== "off" ? (
                nextRun ? (
                  <span className="flex items-center gap-1.5 text-fg-muted">
                    <Clock className="size-3.5 shrink-0" />
                    {t("backup.next", { when: when.format(new Date(nextRun)) })}
                  </span>
                ) : (
                  <span className="flex items-start gap-1.5 text-warn">
                    <TriangleAlert className="mt-px size-3.5 shrink-0" />
                    {agent?.problem ?? t("backup.inactive")}
                  </span>
                )
              ) : (
                <span className="text-fg-subtle">{t("backup.manualOnly")}</span>
              )}
            </div>
          </div>
        </div>

        {/* ---- the history of this database's backups ---- */}
        <div className="px-5 py-4">
          <div className={sectionTitle}>{t("backup.history")}</div>
          {runs.length === 0 ? (
            <p className="mt-2 text-xs text-fg-subtle">{t("backup.noHistory")}</p>
          ) : (
            <ul className="mt-2 flex flex-col">
              {runs.map((run) => (
                <li key={run.id} data-backup-run={run.status} className="flex items-start gap-2.5 border-b border-border py-2 text-xs last:border-0">
                  <span
                    className={cn(
                      "mt-px flex h-[18px] shrink-0 items-center rounded px-1.5 text-[10px] leading-none font-bold",
                      run.status === "ok" ? "bg-ok-soft text-ok" : run.status === "stopped" ? "bg-sunken text-fg-muted" : "bg-danger-soft text-danger",
                    )}
                  >
                    {t(`backup.status.${run.status}`)}
                  </span>
                  <div data-selectable className="min-w-0 flex-1">
                    <div className="text-fg">
                      {when.format(run.startedAt)}
                      <span className="text-fg-subtle"> · {t(run.scheduled ? "backup.byschedule" : "backup.byhand")}</span>
                      {run.bytes !== null && <span className="text-fg-subtle"> · {sizeText(run.bytes)}</span>}
                    </div>
                    <div className={cn("font-mono text-[11px] break-all", run.removedAt ? "text-fg-subtle line-through" : "text-fg-muted")}>
                      {run.status === "ok" ? run.file?.split("/").pop() : <span className="font-sans">{run.message}</span>}
                    </div>
                    {run.removedAt !== null && <div className="text-fg-subtle">{t("backup.removed")}</div>}
                  </div>
                  {run.status === "ok" && run.removedAt === null && (
                    <Button size="sm" variant="ghost" onClick={() => reveal(run)}>
                      {t("backup.reveal")}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </>
  );
}
