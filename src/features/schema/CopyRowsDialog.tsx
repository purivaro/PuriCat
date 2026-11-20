import { ArrowRight, CircleAlert, CircleCheck, Copy, CopyPlus, Lock, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, inputClass, Segmented, Spinner } from "../../components/ui/primitives";
import { clipboard, toClipboardTable } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ColumnMeta, ConnectionView, CopyPreview, OnProblem, PasteMode, PasteRowStatus, TableRef, UniqueKey } from "../../lib/types";
import { sessionOf, useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { addUp, nothingWritten, type Written } from "../table/largeImport";
import { type Mapping, updatableColumns, usableKeys } from "../table/smartPaste";
import { COPY_PAGE, copyable, copyRequest, pairByName, rowLabel } from "./copyRows";
import { Failure, Header, Notice } from "./parts";

/** A row of the source that was not written. */
interface FailedRow {
  /** Its place in the source, in key order, from 1. */
  at: number;
  label: string;
  status: PasteRowStatus | null;
  message: string | null;
  values: (string | null)[];
}

interface Run {
  done: number;
  total: number | null;
  written: Written;
  failed: FailedRow[];
  end: null | { how: "done" | "stopped" | "problem" | "error"; ms: number; at?: number; error?: string };
}

/**
 * "Copy rows to…": the rows of a table, into a table that exists on any connection (another
 * server, database or engine). Columns are paired by name and can be changed; the rows are
 * matched and written as a large import's batches are (ADR 0027), a page at a time.
 */
export function CopyRowsDialog() {
  const source = useUi((s) => s.copyOf);
  const close = () => useUi.getState().askCopy(null);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={source !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-copy-rows
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).focus();
          }}
          onInteractOutside={(e) => e.preventDefault()}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(90vh,800px)] w-[min(95vw,940px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {source && <Copier key={`${source.connection.id}/${JSON.stringify(source.table)}`} {...source} onBusy={setBusy} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Copier({ connection, table, onBusy, onDone }: { connection: ConnectionView; table: TableRef; onBusy: (b: boolean) => void; onDone: () => void }) {
  const { t, i18n } = useTranslation();
  const number = new Intl.NumberFormat(i18n.language);
  const connections = useConnections((s) => s.connections);
  const sessions = useConnections((s) => s.sessions);

  const [sourceColumns, setSourceColumns] = useState<ColumnMeta[] | null>(null);
  const [targetId, setTargetId] = useState(connection.id);
  const [database, setDatabase] = useState(table.database);
  const [schema, setSchema] = useState<string | null>(table.schema);
  const [targetName, setTargetName] = useState("");
  const [lists, setLists] = useState<{ databases: string[] | null; schemas: string[] | null; tables: string[] | null }>({
    databases: null,
    schemas: null,
    tables: null,
  });
  const [target, setTarget] = useState<{ columns: ColumnMeta[]; keys: UniqueKey[] } | null>(null);
  const [mapping, setMapping] = useState<Mapping>([]);
  const [mode, setMode] = useState<PasteMode>("insert");
  const [keyName, setKeyName] = useState<string | null>(null);
  const [onProblem, setOnProblem] = useState<OnProblem>("stop");
  const [preview, setPreview] = useState<{ of: string; result: CopyPreview } | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [run, setRun] = useState<Run | null>(null);
  const stopping = useRef(false);
  const job = useRef<string | null>(null);

  const into = connections.find((c) => c.id === targetId);
  const targetEngine = into?.engine;
  const targetSession = sessionOf(sessions, targetId);
  const status = targetSession.status;
  const hasSchemas = targetEngine === "postgres";
  const toTable: TableRef | null = targetName ? { database, schema: hasSchemas ? schema : null, name: targetName } : null;
  const sameTable = targetId === connection.id && !!toTable && JSON.stringify(toTable) === JSON.stringify(table);
  const running = !!run && run.end === null;
  useEffect(() => {
    onBusy(running);
    return () => onBusy(false);
  }, [running, onBusy]);

  useEffect(() => {
    ipc
      .describeTable(connection.id, table)
      .then((m) => setSourceColumns(m.columns))
      .catch((e: unknown) => setError(errorMessage(e)));
  }, [connection.id, table]);

  // The target: connected when chosen, then its databases, schemas and tables.
  useEffect(() => {
    if (status === "idle" || status === "error") void useConnections.getState().connect(targetId);
  }, [targetId, status]);
  useEffect(() => {
    if (status !== "connected") return;
    let stale = false;
    ipc
      .listDatabases(targetId)
      .then((l) => !stale && setLists((x) => ({ ...x, databases: l.filter((d) => !d.system).map((d) => d.name) })))
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [targetId, status]);
  useEffect(() => {
    if (status !== "connected" || !hasSchemas || !database) return;
    let stale = false;
    ipc
      .listSchemas(targetId, database)
      .then((l) => !stale && setLists((x) => ({ ...x, schemas: l.filter((s) => !s.system).map((s) => s.name) })))
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [targetId, status, hasSchemas, database]);
  useEffect(() => {
    if (status !== "connected" || !database || (hasSchemas && !schema)) return;
    let stale = false;
    setLists((x) => ({ ...x, tables: null }));
    ipc
      .listObjects(targetId, database, hasSchemas ? schema : null)
      .then((l) => !stale && setLists((x) => ({ ...x, tables: l.filter((o) => o.kind === "table").map((o) => o.name) })))
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [targetId, status, hasSchemas, database, schema]);
  const toKey = toTable ? JSON.stringify([targetId, toTable]) : "";
  useEffect(() => {
    setTarget(null);
    setPreview(null);
    if (!toTable || sameTable) return;
    let stale = false;
    Promise.all([ipc.describeTable(targetId, toTable), ipc.uniqueKeys(targetId, toTable)])
      .then(([m, keys]) => {
        if (stale) return;
        setTarget({ columns: m.columns, keys });
        if (sourceColumns && targetEngine) setMapping(pairByName(connection.engine, sourceColumns, m.columns));
      })
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
    // `toKey` names the target; the rest follows from it.
  }, [toKey, sourceColumns]);

  // ---- what to do -----------------------------------------------------------------------
  const available = useMemo(() => (target ? usableKeys(target.keys, mapping) : []), [target, mapping]);
  const picked = available.find((k) => k.name === keyName);
  const chosen = mode === "insert" ? picked : (picked ?? available[0]);
  const key = useMemo(() => chosen?.columns ?? [], [chosen]);
  const built = useMemo(
    () => (sourceColumns && target ? copyRequest({ mode, source: sourceColumns, mapping, key, update: updatableColumns(mapping, key, target.columns) }) : null),
    [sourceColumns, target, mode, mapping, key],
  );
  const signature = built && toTable ? JSON.stringify([targetId, toTable, built]) : "";
  const current = preview?.of === signature ? preview.result : null;
  const keyAt = built ? key.map((k) => built.request.columns.indexOf(k)) : [];
  const readOnly = !!into?.readOnly;
  const production = into?.environment === "production";
  const setupProblems = current?.analysis.problems ?? [];
  const canPreview = !!built && built.columns.length > 0 && !checking && !run && (mode === "insert" || key.length > 0);
  const canRun = !!current && setupProblems.length === 0 && !readOnly && !run;

  const check = async () => {
    if (!built || !toTable) return;
    setChecking(true);
    setError(null);
    try {
      const result = await ipc.copyPreview(connection.id, table, built.columns, targetId, toTable, built.request, COPY_PAGE);
      setPreview({ of: signature, result });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setChecking(false);
    }
  };

  const start = async () => {
    if (!built || !toTable || !canRun) return;
    const started = Date.now();
    stopping.current = false;
    let written = nothingWritten;
    const failed: FailedRow[] = [];
    const end = (how: NonNullable<Run["end"]>["how"], more: Partial<NonNullable<Run["end"]>> = {}) =>
      setRun((r) => r && { ...r, end: { how, ms: Date.now() - started, ...more } });
    setRun({ done: 0, total: null, written, failed: [], end: null });
    setError(null);
    try {
      const counted = await ipc.countRows(connection.id, table, [], false, `copy-count-${crypto.randomUUID()}`);
      setRun((r) => r && { ...r, total: counted.count });
    } catch {
      /* the count only draws the progress bar */
    }
    for (let offset = 0; ; offset += COPY_PAGE) {
      if (stopping.current) return end("stopped", { at: offset });
      const id = crypto.randomUUID();
      job.current = id;
      try {
        const page = await ipc.copyRows(connection.id, table, built.columns, targetId, toTable, built.request, onProblem, offset, COPY_PAGE, id);
        written = addUp(written, page.outcome);
        const here = page.outcome.failed.map((f, i) => {
          const values = page.failedRows[i] ?? [];
          return { at: offset + f.row + 1, label: rowLabel(values, keyAt), status: f.status, message: f.message, values };
        });
        failed.push(...here);
        setRun((r) => r && { ...r, done: offset + page.read, written, failed: [...failed] });
        if (page.outcome.stopped) return end("problem", { at: here[0]?.at });
        if (page.outcome.cancelled) return end("stopped", { at: offset + page.read });
        if (page.read < COPY_PAGE) return end("done");
      } catch (e) {
        return stopping.current ? end("stopped", { at: offset }) : end("error", { at: offset + 1, error: errorMessage(e) });
      } finally {
        job.current = null;
      }
    }
  };

  const stop = () => {
    stopping.current = true;
    if (job.current) void ipc.cancelJob(job.current);
  };

  const reasonOf = (f: FailedRow) => [f.status ? t(`smart.status.${f.status}`) : t("smart.refused"), f.message].filter(Boolean).join(": ");

  const copyFailed = async () => {
    if (!run || !built) return;
    const rows = [[...built.columns, t("smart.problemColumn")], ...run.failed.map((f) => [...f.values.map((v) => v ?? "NULL"), reasonOf(f)])];
    try {
      await clipboard.write(toClipboardTable(rows));
      toast.success(t("copy.failedCopied", { count: run.failed.length }), { id: "copy-failed", duration: 2500 });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const finish = () => {
    if (toTable) useWorkspace.getState().reloadTable(targetId, toTable);
    if (run?.end && run.end.how !== "error") {
      toast.success(t("copy.done", { inserted: number.format(run.written.inserted), updated: number.format(run.written.updated) }));
    }
    onDone();
  };

  const section = "text-[11px] font-semibold tracking-wider text-fg-subtle uppercase";
  const place = [table.database, table.schema, table.name].filter(Boolean).join(" › ");
  const select = cn(inputClass, "h-8 text-[13px]");
  const locked = !!run;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Header icon={<CopyPlus className="size-5 shrink-0 text-accent-text" />} title={t("copy.title", { place })} connection={connection} busy={running} />

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {/* Where to */}
        <section className="flex flex-col gap-2">
          <h3 className={section}>{t("copy.into")}</h3>
          <div className="grid grid-cols-[repeat(auto-fit,minmax(180px,1fr))] gap-2">
            <label className="flex flex-col gap-1 text-[11px] text-fg-muted">
              {t("copy.connection")}
              <select
                data-copy-connection
                value={targetId}
                disabled={locked}
                onChange={(e) => {
                  setTargetId(e.target.value);
                  setDatabase("");
                  setSchema(null);
                  setTargetName("");
                  setLists({ databases: null, schemas: null, tables: null });
                }}
                className={select}
              >
                {connections.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name} · {c.engine}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1 text-[11px] text-fg-muted">
              {t("copy.database")}
              <select
                data-copy-database
                value={database}
                disabled={locked || !lists.databases}
                onChange={(e) => {
                  setDatabase(e.target.value);
                  setSchema(null);
                  setTargetName("");
                }}
                className={select}
              >
                <option value="">{lists.databases ? t("copy.pick") : t("common.loading")}</option>
                {lists.databases?.map((d) => (
                  <option key={d} value={d}>
                    {d}
                  </option>
                ))}
              </select>
            </label>
            {hasSchemas && (
              <label className="flex flex-col gap-1 text-[11px] text-fg-muted">
                {t("copy.schema")}
                <select
                  data-copy-schema
                  value={schema ?? ""}
                  disabled={locked || !lists.schemas}
                  onChange={(e) => {
                    setSchema(e.target.value || null);
                    setTargetName("");
                  }}
                  className={select}
                >
                  <option value="">{lists.schemas ? t("copy.pick") : t("common.loading")}</option>
                  {lists.schemas?.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="flex flex-col gap-1 text-[11px] text-fg-muted">
              {t("copy.table")}
              <select data-copy-table value={targetName} disabled={locked || !lists.tables} onChange={(e) => setTargetName(e.target.value)} className={select}>
                <option value="">{lists.tables ? t("copy.pick") : t("common.loading")}</option>
                {lists.tables?.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {status === "connecting" && (
            <p className="flex items-center gap-2 text-xs text-fg-muted">
              <Spinner className="size-3" /> {t("copy.connecting")}
            </p>
          )}
          {targetSession.status === "error" && <Failure message={targetSession.message || t("copy.cannotConnect")} />}
          {sameTable && <p className="text-xs text-danger">{t("copy.sameTable")}</p>}
          {readOnly && (
            <Notice tone="muted" icon={<Lock className="size-4" />}>
              {t("copy.readOnly")}
            </Notice>
          )}
        </section>

        {target && sourceColumns && built && (
          <>
            {/* What to do with the rows */}
            <section className="flex flex-col gap-2">
              <div className="flex flex-wrap items-center gap-3">
                <Segmented<PasteMode>
                  label={t("smart.mode.insert")}
                  value={mode}
                  onChange={(m) => !locked && setMode(m)}
                  options={(["insert", "update", "upsert"] as const).map((m) => ({ value: m, label: t(`smart.mode.${m}`) }))}
                />
                <span className="min-w-0 flex-1 text-[13px] text-fg-muted">{t(`smart.modeHint.${mode}`)}</span>
              </div>
              <label className="flex flex-wrap items-center gap-2 text-[13px]">
                <span className="shrink-0 font-medium">{t("smart.key")}</span>
                <span className="min-w-56">
                  <select
                    data-copy-key
                    value={chosen?.name ?? ""}
                    disabled={locked || (mode !== "insert" && available.length === 0)}
                    onChange={(e) => setKeyName(e.target.value)}
                    className={select}
                  >
                    {(mode === "insert" || available.length === 0) && <option value="">{t("smart.keyNone")}</option>}
                    {available.map((k) => (
                      <option key={k.name} value={k.name}>
                        {t(k.primary ? "smart.keyPrimary" : "smart.keyUnique", { columns: k.columns.join(", ") })}
                      </option>
                    ))}
                  </select>
                </span>
                {mode !== "insert" && available.length === 0 && <span className="text-xs text-danger">{t("smart.noKey")}</span>}
              </label>
              <div className="flex flex-wrap items-center gap-2 text-[13px]">
                <span className="text-fg-muted">{t("smart.onProblem")}</span>
                <Segmented<OnProblem>
                  label={t("smart.onProblem")}
                  value={onProblem}
                  onChange={(v) => !locked && setOnProblem(v)}
                  options={[
                    { value: "stop", label: t("smart.onProblemStop") },
                    { value: "skip", label: t("smart.onProblemSkip") },
                  ]}
                />
              </div>
            </section>

            {/* Which column goes where */}
            <section className="flex flex-col gap-1.5">
              <h3 className={section}>{t("copy.columns")}</h3>
              <div className="overflow-hidden rounded-xl border border-border">
                {sourceColumns.map((c, i) => {
                  const can = copyable(connection.engine, c);
                  const to = mapping[i] ?? null;
                  return (
                    <div key={c.name} data-copy-column={c.name} className="flex items-center gap-2.5 border-b border-border px-3 py-1 last:border-b-0">
                      <span className="w-48 shrink-0 truncate text-[13px] font-medium" title={c.name}>
                        {c.name}
                      </span>
                      <span className="w-36 shrink-0 truncate font-mono text-[11px] text-fg-muted">{c.dataType}</span>
                      <ArrowRight aria-hidden className="size-3.5 shrink-0 text-fg-subtle" />
                      {can ? (
                        <span className="w-64 shrink-0">
                          <select
                            aria-label={`${t("smart.tableColumn")}: ${c.name}`}
                            value={to ?? ""}
                            disabled={locked}
                            onChange={(e) =>
                              setMapping((m) => {
                                const name = e.target.value || null;
                                return sourceColumns.map((_, j) => (j === i ? name : name && m[j] === name ? null : (m[j] ?? null)));
                              })
                            }
                            className={cn(inputClass, "h-7 text-xs", !to && "text-fg-subtle")}
                          >
                            <option value="">{t("copy.dontCopy")}</option>
                            {target.columns.map((tc) => (
                              <option key={tc.name} value={tc.name}>
                                {tc.name} · {tc.dataType}
                                {tc.editable ? "" : ` (${t("smart.matchOnly")})`}
                              </option>
                            ))}
                          </select>
                        </span>
                      ) : (
                        <span className="text-xs text-fg-subtle">{t("copy.binary")}</span>
                      )}
                      {to && key.includes(to) && (
                        <span className="rounded bg-accent-soft px-1.5 py-0.5 text-[10px] font-bold text-accent-text uppercase">key</span>
                      )}
                    </div>
                  );
                })}
              </div>
              <p className="text-xs leading-relaxed text-fg-subtle">{t("copy.semantics", { page: number.format(COPY_PAGE) })}</p>
            </section>

            {/* The first page, analysed */}
            {current && !run && (
              <section data-copy-preview className="flex flex-col gap-2">
                {setupProblems.length > 0 ? (
                  <ul className="flex flex-col gap-1 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
                    {setupProblems.map((p) => (
                      <li key={p} className="flex gap-2">
                        <CircleAlert className="mt-0.5 size-4 shrink-0" /> {p}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <>
                    <p className="text-[13px] text-fg-muted">{t("copy.previewOf", { count: number.format(current.rows.length) })}</p>
                    <div className="flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
                      {(["insert", "update", "unchanged", "unmatched", "problems"] as const).map((k) => (
                        <span key={k} data-copy-count={k} className={cn(current.analysis.counts[k] === 0 && "opacity-45")}>
                          <b className="text-[15px] tabular-nums">{number.format(current.analysis.counts[k])}</b>{" "}
                          <span className="text-fg-muted">{t(`smart.count.${k}`)}</span>
                        </span>
                      ))}
                    </div>
                    {current.analysis.counts.problems > 0 && (
                      <ul className="flex flex-col gap-0.5 rounded-lg bg-danger-soft px-3 py-2 text-xs text-danger">
                        {current.analysis.rows
                          .map((r, i) => ({ r, i }))
                          .filter(({ r }) => ["missing_key", "duplicate", "exists", "invalid"].includes(r.status))
                          .slice(0, 8)
                          .map(({ r, i }) => (
                            <li key={i}>
                              <span className="font-mono">{rowLabel(current.rows[i] ?? [], keyAt)}</span> · {t(`smart.status.${r.status}`)}
                              {r.message ? `: ${r.message}` : ""}
                            </li>
                          ))}
                      </ul>
                    )}
                  </>
                )}
              </section>
            )}

            {run && <Report run={run} number={number} reasonOf={reasonOf} onCopy={() => void copyFailed()} />}
          </>
        )}
        {!target && toTable && !sameTable && !error && (
          <div className="flex items-center gap-2 text-[13px] text-fg-muted">
            <Spinner className="size-3.5" /> {t("common.loading")}
          </div>
        )}
        <Failure message={error} />
      </div>

      <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
        {run ? (
          run.end === null ? (
            <>
              <div className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-sunken">
                <div
                  className="h-full rounded-full bg-accent transition-[width]"
                  style={{ width: run.total ? `${Math.min(100, (run.done / Math.max(1, run.total)) * 100)}%` : "5%" }}
                />
              </div>
              <span data-copy-progress className="shrink-0 text-xs text-fg-muted tabular-nums">
                {run.total === null ? t("copy.counting") : t("copy.progress", { done: number.format(run.done), total: number.format(run.total) })}
              </span>
              <Button variant="danger" size="sm" data-copy-stop onClick={stop}>
                {t("smart.importStop")}
              </Button>
            </>
          ) : (
            <>
              <span className="flex-1" />
              <Button variant="primary" data-copy-close onClick={finish}>
                {t("common.close")}
              </Button>
            </>
          )
        ) : (
          <>
            {production && (
              <span className="flex items-center gap-1.5 text-xs font-medium text-danger">
                <TriangleAlert className="size-3.5" /> {t("copy.prodWarning")}
              </span>
            )}
            <span className="flex-1" />
            <Dialog.Close asChild>
              <Button variant="ghost">{t("common.cancel")}</Button>
            </Dialog.Close>
            <Button data-copy-check loading={checking} disabled={!canPreview} onClick={() => void check()}>
              {t("copy.check")}
            </Button>
            <Button
              variant={production ? "danger" : "primary"}
              data-copy-run
              disabled={!canRun}
              title={current ? undefined : t("copy.checkFirst")}
              onClick={() => void start()}
            >
              {t(production ? "copy.runProd" : "copy.run")}
            </Button>
          </>
        )}
      </div>
    </div>
  );
}

/** How a copy ended, what it wrote, and the rows it did not. */
function Report({ run, number, reasonOf, onCopy }: { run: Run; number: Intl.NumberFormat; reasonOf: (f: FailedRow) => string; onCopy: () => void }) {
  const { t } = useTranslation();
  const end = run.end;
  const counts: [keyof Written | "failed", number, string][] = [
    ["inserted", run.written.inserted, "text-ok"],
    ["updated", run.written.updated, "text-warn"],
    ["unchanged", run.written.unchanged, "text-fg-muted"],
    ["unmatched", run.written.unmatched, "text-info"],
    ["failed", run.failed.length, "text-danger"],
  ];
  return (
    <section data-copy-report={end?.how ?? "running"} className="flex flex-col gap-2">
      {end && (
        <p
          className={cn(
            "flex items-start gap-2 rounded-lg px-3 py-2 text-[13px] leading-relaxed",
            end.how === "done" ? "bg-ok-soft text-ok" : end.how === "stopped" ? "bg-warn-soft text-warn" : "bg-danger-soft text-danger",
          )}
        >
          {end.how === "done" ? <CircleCheck className="mt-0.5 size-4 shrink-0" /> : <CircleAlert className="mt-0.5 size-4 shrink-0" />}
          <span data-selectable>
            {end.how === "done" && t("copy.ended.done", { read: number.format(run.done), seconds: Math.max(1, Math.round(end.ms / 1000)) })}
            {end.how === "stopped" && t("copy.ended.stopped", { at: number.format(end.at ?? run.done) })}
            {end.how === "problem" && t("copy.ended.problem", { at: number.format(end.at ?? 0) })}
            {end.how === "error" && t("copy.ended.error", { at: number.format(end.at ?? 0), error: end.error ?? "" })}
          </span>
        </p>
      )}
      <div className="flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
        {counts.map(([k, n, tone]) => (
          <span key={k} data-copy-written={k} className={cn(n === 0 && "opacity-45")}>
            <b className={cn("text-[15px] tabular-nums", tone)}>{number.format(n)}</b> <span className="text-fg-muted">{t(`smart.imported.${k}`)}</span>
          </span>
        ))}
      </div>
      {run.failed.length > 0 && (
        <>
          <div className="flex items-center gap-2">
            <h3 className="text-xs font-semibold tracking-wide text-fg-subtle uppercase">{t("copy.failedTitle")}</h3>
            <span className="flex-1" />
            <Button size="sm" data-copy-copy-failed icon={<Copy className="size-3.5" />} disabled={!end} onClick={onCopy}>
              {t("copy.copyFailed")}
            </Button>
          </div>
          <div className="max-h-56 overflow-auto rounded-xl border border-border">
            <table className="w-full border-separate border-spacing-0 text-xs">
              <thead>
                <tr className="[&>th]:sticky [&>th]:top-0 [&>th]:border-b [&>th]:border-border [&>th]:bg-sunken [&>th]:px-2.5 [&>th]:py-1.5 [&>th]:text-left [&>th]:font-semibold">
                  <th className="w-20">{t("copy.rowAt")}</th>
                  <th className="w-40">{t("copy.rowKey")}</th>
                  <th>{t("smart.detail")}</th>
                </tr>
              </thead>
              <tbody data-selectable>
                {run.failed.slice(0, 200).map((f) => (
                  <tr key={f.at} data-copy-failed={f.label} className="[&>td]:border-b [&>td]:border-border [&>td]:px-2.5 [&>td]:py-1 [&>td]:align-top">
                    <td className="font-mono text-fg-subtle tabular-nums">{number.format(f.at)}</td>
                    <td className="max-w-40 truncate font-mono">{f.label}</td>
                    <td className="break-words text-fg-muted">{reasonOf(f)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {run.failed.length > 200 && <p className="text-xs text-fg-subtle">{t("smart.rowsShown", { shown: 200, total: number.format(run.failed.length) })}</p>}
        </>
      )}
    </section>
  );
}
