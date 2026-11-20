import { Activity, Copy, OctagonX, RefreshCw, Search, Square, TerminalSquare, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, IconButton, Input, inputClass, Spinner, Switch } from "../../components/ui/primitives";
import { clipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ConnectionView, EndProcess, ServerProcess } from "../../lib/types";
import { useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { Failure, Header, Notice } from "./parts";
import { duration, oneLine, visibleSessions } from "./sessions";

const REFRESH_CHOICES = [0, 2, 5, 10] as const;

/**
 * The sessions on a connection's server: who is connected, what each one runs and for how
 * long; and, for one session, stopping its query or ending it. The statement is the
 * backend's, shown before it runs.
 */
export function SessionsDialog() {
  const connection = useUi((s) => s.sessionsOf);
  const close = () => useUi.getState().askSessions(null);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={connection !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-sessions-dialog
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).querySelector<HTMLElement>("input")?.focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(92vh,680px)] w-[min(96vw,1040px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {/* Mounted per opening, so the list is read from the server afresh. */}
          {connection && <Sessions key={connection.id} connection={connection} busy={busy} onBusy={setBusy} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

type Confirm = { process: ServerProcess; what: EndProcess; sql: string | null; problem: string | null };

function Sessions({ connection, busy, onBusy }: { connection: ConnectionView; busy: boolean; onBusy: (busy: boolean) => void }) {
  const { t } = useTranslation();
  const postgres = connection.engine === "postgres";
  const production = connection.environment === "production";

  const [list, setList] = useState<ServerProcess[] | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [readAt, setReadAt] = useState<Date | null>(null);
  const [text, setText] = useState("");
  const [showIdle, setShowIdle] = useState(false);
  const [every, setEvery] = useState<(typeof REFRESH_CHOICES)[number]>(5);
  const [selected, setSelected] = useState<number | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const load = useCallback(() => {
    setLoading(true);
    return ipc
      .listProcesses(connection.id)
      .then((l) => {
        if (!alive.current) return;
        setList(l);
        setFailed(null);
        setReadAt(new Date());
      })
      .catch((e: unknown) => alive.current && setFailed(errorMessage(e)))
      .finally(() => alive.current && setLoading(false));
  }, [connection.id]);

  useEffect(() => {
    void load();
  }, [load]);

  // Read again on a timer, except while a decision is on screen: the row must not move away.
  useEffect(() => {
    if (every === 0 || confirm) return;
    const timer = window.setInterval(() => void load(), every * 1000);
    return () => window.clearInterval(timer);
  }, [every, confirm, load]);

  const shown = useMemo(() => (list ? visibleSessions(list, text, showIdle) : []), [list, text, showIdle]);
  const current = list?.find((p) => p.id === selected) ?? null;
  const running = list?.filter((p) => !p.idle).length ?? 0;
  const idle = list?.filter((p) => p.idle && !p.own).length ?? 0;

  const ask = (process: ServerProcess, what: EndProcess) => {
    setProblem(null);
    setConfirm({ process, what, sql: null, problem: null });
    ipc
      .planEndProcess(connection.id, process.id, what)
      .then((sql) => alive.current && setConfirm((c) => (c && c.process.id === process.id && c.what === what ? { ...c, sql } : c)))
      .catch((e: unknown) => alive.current && setConfirm((c) => (c ? { ...c, problem: errorMessage(e) } : c)));
  };

  const end = async () => {
    if (!confirm?.sql) return;
    onBusy(true);
    try {
      const sql = await ipc.endProcess(connection.id, confirm.process.id, confirm.what);
      toast.success(t("sessions.done", { statement: sql }));
      setConfirm(null);
      if (confirm.what === "connection") setSelected(null);
      await load();
    } catch (e) {
      setProblem(errorMessage(e));
    } finally {
      onBusy(false);
    }
  };

  const openInTab = (query: string) => {
    useWorkspace.getState().openSql(connection.id, current?.database ?? null, query);
    useUi.getState().askSessions(null);
  };

  return (
    <>
      <Header
        icon={
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent-text">
            <Activity className="size-4" />
          </div>
        }
        title={t("sessions.title")}
        connection={connection}
        busy={busy}
      />

      <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b border-border px-5 py-2.5">
        <div className="relative w-64">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-fg-subtle" />
          <Input data-sessions-search value={text} onChange={(e) => setText(e.target.value)} placeholder={t("sessions.search")} className="pl-8" />
        </div>
        <label className="flex items-center gap-2 text-[13px]">
          <Switch checked={showIdle} onChange={setShowIdle} label={t("sessions.showIdle")} />
          {t("sessions.showIdle")}
          {idle > 0 && <span className="text-xs text-fg-subtle">({idle})</span>}
        </label>
        <label className="flex items-center gap-2 text-[13px] whitespace-nowrap">
          {t("sessions.refresh")}
          <span className="w-28">
            <select
              aria-label={t("sessions.refresh")}
              value={every}
              onChange={(e) => setEvery(Number(e.target.value) as (typeof REFRESH_CHOICES)[number])}
              className={inputClass}
            >
              {REFRESH_CHOICES.map((s) => (
                <option key={s} value={s}>
                  {s === 0 ? t("sessions.refreshOff") : t("sessions.refreshEvery", { seconds: s })}
                </option>
              ))}
            </select>
          </span>
        </label>
        <span className="flex-1" />
        <span data-sessions-count className="text-xs text-fg-subtle tabular-nums">
          {list && t("sessions.count", { count: shown.length, running })}
        </span>
        <IconButton label={t("common.refresh")} onClick={() => void load()} disabled={loading}>
          {loading ? <Spinner className="size-3.5" /> : <RefreshCw className="size-3.5" />}
        </IconButton>
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {failed && !list ? (
          <div className="p-5">
            <Failure message={failed} />
          </div>
        ) : !list ? (
          <div className="flex h-full items-center justify-center gap-2 text-[13px] text-fg-muted">
            <Spinner className="size-3.5" /> {t("common.loading")}
          </div>
        ) : (
          <table className="w-full table-fixed border-separate border-spacing-0 text-xs">
            <thead className="sticky top-0 z-10 bg-raised text-left text-[11px] font-semibold tracking-wide text-fg-subtle uppercase">
              <tr>
                {[
                  ["w-20", t("sessions.id")],
                  ["w-28", t("sessions.user")],
                  ["w-36", t("sessions.host")],
                  ["w-28", t("sessions.database")],
                  ["w-28", postgres ? t("sessions.state") : t("sessions.command")],
                  ["w-20 text-right", t("sessions.time")],
                  ["", t("sessions.query")],
                ].map(([cls, label]) => (
                  <th key={label} className={cn("border-b border-border px-3 py-2", cls)}>
                    {label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {shown.map((p) => (
                <tr
                  key={p.id}
                  data-session-row={p.id}
                  aria-selected={p.id === selected}
                  onClick={() => {
                    setSelected(p.id);
                    if (confirm && confirm.process.id !== p.id) setConfirm(null);
                  }}
                  className={cn("cursor-default", p.id === selected ? "bg-accent-soft" : "hover:bg-sunken", p.idle && "text-fg-muted")}
                >
                  <td className="border-b border-border px-3 py-1.5 font-mono tabular-nums">
                    {p.id}
                    {p.own && <span className="ml-1.5 rounded bg-sunken px-1 font-sans text-[10px] text-fg-subtle">{t("sessions.own")}</span>}
                  </td>
                  <td className="truncate border-b border-border px-3 py-1.5" title={p.user ?? ""}>
                    {p.user}
                  </td>
                  <td className="truncate border-b border-border px-3 py-1.5" title={[p.host, p.client].filter(Boolean).join(" · ")}>
                    {p.host}
                    {p.client && <span className="text-fg-subtle"> · {p.client}</span>}
                  </td>
                  <td className="truncate border-b border-border px-3 py-1.5">{p.database}</td>
                  <td className="truncate border-b border-border px-3 py-1.5" title={p.state ?? ""}>
                    {p.command}
                  </td>
                  <td className="border-b border-border px-3 py-1.5 text-right tabular-nums">{duration(p.seconds)}</td>
                  <td className="truncate border-b border-border px-3 py-1.5 font-mono text-[11px]" title={p.query ?? ""}>
                    {oneLine(p.query)}
                  </td>
                </tr>
              ))}
              {shown.length === 0 && (
                <tr>
                  <td colSpan={7} className="px-3 py-8 text-center text-[13px] text-fg-muted">
                    {text ? t("sessions.noMatch") : t("sessions.noneRunning")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>

      {/* The session picked in the list: its whole query, and what can be done to it. */}
      <div className="flex max-h-[45%] shrink-0 flex-col gap-2.5 overflow-y-auto border-t border-border px-5 py-3">
        {failed && list && <Failure message={failed} />}
        {!current ? (
          <p className="text-xs text-fg-subtle">{selected === null ? t("sessions.pick") : t("sessions.gone")}</p>
        ) : (
          <>
            <div className="flex items-center gap-2 text-[13px]">
              <span className="font-semibold">{t("sessions.session", { id: current.id })}</span>
              <span className="min-w-0 flex-1 truncate text-xs text-fg-muted">
                {[current.user, current.host, current.database, current.state].filter(Boolean).join(" · ")}
              </span>
              {current.query && (
                <>
                  <Button size="sm" variant="ghost" icon={<Copy className="size-3.5" />} onClick={() => void clipboard.write(current.query ?? "").then(() => toast.success(t("common.copied")))}>
                    {t("common.copy")}
                  </Button>
                  <Button size="sm" variant="ghost" icon={<TerminalSquare className="size-3.5" />} onClick={() => openInTab(current.query ?? "")}>
                    {t("sessions.openInTab")}
                  </Button>
                </>
              )}
            </div>
            {current.query && (
              <pre data-selectable data-session-query className="max-h-28 overflow-auto rounded-lg border border-border bg-sunken px-3 py-2 font-mono text-[11px] leading-relaxed break-words whitespace-pre-wrap">
                {current.query}
              </pre>
            )}
            {current.own ? (
              <p className="text-xs text-fg-subtle">{t("sessions.ownHint")}</p>
            ) : connection.readOnly ? (
              <Notice tone="muted" icon={<TriangleAlert className="size-3.5" />}>
                {t("sessions.readOnly")}
              </Notice>
            ) : confirm && confirm.process.id === current.id ? (
              <div data-session-confirm className="flex flex-col gap-2 rounded-xl border border-danger/40 bg-danger-soft/40 p-3">
                <p className="text-[13px]">{t(confirm.what === "query" ? "sessions.confirmQuery" : "sessions.confirmConnection", { id: current.id })}</p>
                {production && (
                  <Notice tone="danger" icon={<TriangleAlert className="size-3.5" />}>
                    {t("sessions.production")}
                  </Notice>
                )}
                {confirm.sql ? (
                  <pre data-selectable data-session-statement className="rounded-lg border border-border bg-surface px-3 py-1.5 font-mono text-xs">
                    {confirm.sql}
                  </pre>
                ) : (
                  confirm.problem && <Failure message={confirm.problem} />
                )}
                <Failure message={problem} />
                <div className="flex justify-end gap-2">
                  <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm(null)}>
                    {t("common.cancel")}
                  </Button>
                  <Button size="sm" variant="danger" data-session-run disabled={!confirm.sql} loading={busy} onClick={() => void end()}>
                    {t(confirm.what === "query" ? "sessions.stopQuery" : "sessions.endConnection")}
                  </Button>
                </div>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-2">
                <Button size="sm" data-session-stop icon={<Square className="size-3 fill-current" />} disabled={current.idle} onClick={() => ask(current, "query")}>
                  {t("sessions.stopQuery")}
                </Button>
                <Button size="sm" data-session-end icon={<OctagonX className="size-3.5" />} onClick={() => ask(current, "connection")}>
                  {t("sessions.endConnection")}
                </Button>
                <span className="text-xs text-fg-subtle">{t(current.idle ? "sessions.idleHint" : "sessions.endHint")}</span>
              </div>
            )}
          </>
        )}
        <p className="text-[11px] leading-snug text-fg-subtle">
          {t(postgres ? "sessions.privilegePostgres" : "sessions.privilegeMysql")}
          {readAt && ` ${t("sessions.readAt", { time: readAt.toLocaleTimeString() })}`}
        </p>
      </div>
    </>
  );
}
