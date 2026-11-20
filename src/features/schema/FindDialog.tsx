import { Eye, Search, SearchX, Square, Table2, TextSearch, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, Input, inputClass, Segmented, Spinner } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ConnectionView, FindMode, ObjectKind, TableRef } from "../../lib/types";
import { useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { findFilters, groupNames, type NameGroup, type Searched, tableLabel, tally } from "./find";
import { Header, Notice } from "./parts";

type Scope = "data" | "names";
type DataRun = {
  kind: "data";
  text: string;
  mode: FindMode;
  total: number | null;
  /** The table being read now. */
  current: string | null;
  list: Searched[];
  done: boolean;
  stopped: boolean;
  /** The tables could not be listed. */
  error: string | null;
};
type Run = DataRun | { kind: "names"; text: string; groups: NameGroup[] | null; error: string | null };

/** The last search of each place, kept while the app runs: opening a result closes the dialog. */
const kept = new Map<string, { text: string; mode: FindMode; scope: Scope; run: Run | null }>();

/**
 * Find in database: the tables whose rows hold a value (each table read once on the server,
 * one after another, and stoppable), or the tables and columns whose name contains it.
 * A result opens the table filtered to the rows that were counted.
 */
export function FindDialog() {
  const target = useUi((s) => s.findIn);
  const close = () => useUi.getState().askFind(null);
  return (
    <Dialog.Root open={target !== null} onOpenChange={(open) => !open && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-find-dialog
          // The text box, not the header's close button: the first thing to do is type.
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).querySelector<HTMLInputElement>("[data-find-text]")?.focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(88vh,760px)] w-[min(94vw,900px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {target && <Finder key={placeKey(target)} {...target} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

const placeKey = (t: { connection: ConnectionView; database: string; schema: string | null }) => `${t.connection.id}/${t.database}/${t.schema ?? ""}`;

function Finder({ connection, database, schema, onDone }: { connection: ConnectionView; database: string; schema: string | null; onDone: () => void }) {
  const { t } = useTranslation();
  const key = placeKey({ connection, database, schema });
  const last = kept.get(key);
  const [text, setText] = useState(last?.text ?? "");
  const [mode, setMode] = useState<FindMode>(last?.mode ?? "contains");
  const [scope, setScope] = useState<Scope>(last?.scope ?? "data");
  const [run, setRun] = useState<Run | null>(last?.run ?? null);
  const job = useRef<string | null>(null);
  const stopping = useRef(false);
  const state = useRef({ text, mode, scope, run });
  state.current = { text, mode, scope, run };
  // Several schemas are searched: a table is named with its schema.
  const wide = connection.engine === "postgres" && schema === null;
  const place = [database, schema].filter(Boolean).join(" › ");
  const running = run?.kind === "data" ? !run.done : run?.kind === "names" && run.groups === null && run.error === null;

  // Closing stops a search that is still going; what it found so far is kept.
  useEffect(
    () => () => {
      stopping.current = true;
      if (job.current) void ipc.cancelJob(job.current);
      const s = state.current;
      const run = s.run?.kind === "data" && !s.run.done ? { ...s.run, done: true, stopped: true, current: null } : s.run;
      kept.set(key, { ...s, run });
    },
    [key],
  );

  const tables = async (): Promise<TableRef[]> => {
    const of = async (inSchema: string | null) =>
      (await ipc.listObjects(connection.id, database, inSchema)).filter((o) => o.kind === "table").map((o) => ({ database, schema: inSchema, name: o.name }));
    if (!wide) return of(schema);
    const schemas = (await ipc.listSchemas(connection.id, database)).filter((s) => !s.system);
    return (await Promise.all(schemas.map((s) => of(s.name)))).flat();
  };

  const patchData = (change: (r: DataRun) => Partial<DataRun>) => setRun((r) => (r?.kind === "data" ? { ...r, ...change(r) } : r));

  const start = async (e: FormEvent) => {
    e.preventDefault();
    if (text === "" || running) return;
    stopping.current = false;
    if (scope === "names") {
      setRun({ kind: "names", text, groups: null, error: null });
      try {
        const hits = await ipc.findNames(connection.id, database, schema, text);
        setRun({ kind: "names", text, groups: groupNames(hits), error: null });
      } catch (err) {
        setRun({ kind: "names", text, groups: null, error: errorMessage(err) });
      }
      return;
    }
    const looking = { text, mode };
    setRun({ kind: "data", ...looking, total: null, current: null, list: [], done: false, stopped: false, error: null });
    let list: TableRef[];
    try {
      list = await tables();
    } catch (err) {
      patchData(() => ({ done: true, error: errorMessage(err) }));
      return;
    }
    patchData(() => ({ total: list.length }));
    for (const table of list) {
      if (stopping.current) break;
      patchData(() => ({ current: tableLabel(table, wide) }));
      const id = `find-${crypto.randomUUID()}`;
      job.current = id;
      let found: Searched;
      try {
        found = { table, result: await ipc.findInTable(connection.id, table, looking.text, looking.mode, id), error: null };
      } catch (err) {
        if (stopping.current) break;
        found = { table, result: null, error: errorMessage(err) };
      } finally {
        job.current = null;
      }
      patchData((r) => ({ list: [...r.list, found] }));
    }
    patchData(() => ({ current: null, done: true, stopped: stopping.current }));
  };

  const stop = () => {
    stopping.current = true;
    if (job.current) void ipc.cancelJob(job.current);
  };

  const open = (table: TableRef, kind: ObjectKind, columns?: string[]) => {
    const ws = useWorkspace.getState();
    if (columns && run?.kind === "data") ws.openTableFiltered(connection.id, table, kind, findFilters(columns, run.mode, run.text), columns.length > 1);
    else ws.openTable(connection.id, table, kind);
    onDone();
  };

  return (
    <form onSubmit={(e) => void start(e)} className="flex min-h-0 flex-1 flex-col">
      <Header icon={<TextSearch className="size-5 shrink-0 text-accent-text" />} title={t("find.title", { place })} connection={connection} busy={false} />

      <div className="flex shrink-0 flex-col gap-3 border-b border-border px-5 py-4">
        <div className="flex flex-wrap items-center gap-2">
          <Segmented<Scope>
            label={t("find.lookIn")}
            value={scope}
            onChange={setScope}
            options={[
              { value: "data", label: t("find.data") },
              { value: "names", label: t("find.names") },
            ]}
          />
          <span className="min-w-56 flex-1">
            <Input
              data-find-text
              aria-label={t("find.textLabel")}
              placeholder={scope === "data" ? t("find.dataPlaceholder") : t("find.namesPlaceholder")}
              value={text}
              onChange={(e) => setText(e.target.value)}
              className="h-8"
            />
          </span>
          {scope === "data" && (
            <span className="w-36">
              <select
                aria-label={t("find.mode")}
                data-find-mode
                value={mode}
                onChange={(e) => setMode(e.target.value as FindMode)}
                className={cn(inputClass, "h-8 text-xs")}
              >
                <option value="contains">{t("find.contains")}</option>
                <option value="equals">{t("find.equals")}</option>
                <option value="startsWith">{t("find.startsWith")}</option>
              </select>
            </span>
          )}
          {running && run?.kind === "data" ? (
            <Button type="button" variant="secondary" data-find-stop icon={<Square className="size-3.5" />} onClick={stop}>
              {t("find.stop")}
            </Button>
          ) : (
            <Button type="submit" variant="primary" data-find-start icon={<Search className="size-3.5" />} disabled={text === "" || running}>
              {t("find.find")}
            </Button>
          )}
        </div>
        {scope === "data" &&
          (connection.environment === "production" ? (
            <Notice tone="warn" icon={<TriangleAlert className="size-4" />}>
              {t("find.prodWarning")}
            </Notice>
          ) : (
            <p className="text-[11px] text-fg-subtle">{mode === "equals" ? t("find.equalsHint") : t("find.dataHint")}</p>
          ))}
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-5 py-4">
        {!run && <p className="py-10 text-center text-[13px] text-fg-subtle">{t("find.nothingYet")}</p>}
        {run?.kind === "data" && <DataResults run={run} wide={wide} onOpen={open} />}
        {run?.kind === "names" && <NameResults run={run} database={database} onOpen={open} />}
      </div>

      <div className="flex shrink-0 items-center justify-end gap-2 border-t border-border px-5 py-3">
        <Dialog.Close asChild>
          <Button variant="ghost">{t("common.close")}</Button>
        </Dialog.Close>
      </div>
    </form>
  );
}

function DataResults({ run, wide, onOpen }: { run: DataRun; wide: boolean; onOpen: (table: TableRef, kind: ObjectKind, columns?: string[]) => void }) {
  const { t } = useTranslation();
  const { found, failed, skipped } = tally(run.list);
  const done = run.list.length;
  return (
    <>
      <div data-find-status={run.done ? (run.stopped ? "stopped" : "done") : "running"} className="flex flex-col gap-1.5">
        {!run.done ? (
          <>
            <div className="flex items-center gap-2 text-[13px] text-fg-muted">
              <Spinner className="size-3.5" />
              {run.total === null ? t("find.listing") : t("find.progress", { table: run.current ?? "", done, total: run.total })}
            </div>
            {run.total !== null && run.total > 0 && (
              <div className="h-1 overflow-hidden rounded-full bg-sunken">
                <div className="h-full bg-accent transition-[width]" style={{ width: `${(done / run.total) * 100}%` }} />
              </div>
            )}
          </>
        ) : run.error ? (
          <p className="text-[13px] text-danger">{run.error}</p>
        ) : (
          <p className="text-[13px] text-fg-muted">
            {run.stopped
              ? t("find.stopped", { done, total: run.total ?? done })
              : found.length > 0
                ? t("find.foundIn", { count: found.length, total: run.total ?? done })
                : ""}
          </p>
        )}
      </div>

      {run.done && !run.error && found.length === 0 && failed.length === 0 && (
        <div data-find-empty className="flex flex-col items-center gap-2 py-10 text-center">
          <SearchX className="size-8 text-fg-subtle" />
          <p className="text-[13px] text-fg-muted">{t(run.stopped ? "find.notFoundYet" : "find.notFound", { text: run.text, total: run.total ?? done })}</p>
        </div>
      )}

      {found.length > 0 && (
        <div className="overflow-hidden rounded-xl border border-border">
          {found.map(({ table, result }) => (
            <div
              key={tableLabel(table, true)}
              data-find-table={tableLabel(table, wide)}
              className="flex flex-col gap-1.5 border-b border-border px-3 py-2 last:border-0"
            >
              <button
                type="button"
                data-find-open
                title={t("find.openRows")}
                onClick={() =>
                  onOpen(
                    table,
                    "table",
                    result!.hits.map((h) => h.column),
                  )
                }
                className="flex items-center gap-2 rounded-md text-left text-[13px] font-medium hover:text-accent-text"
              >
                <Table2 className="size-3.5 shrink-0 text-fg-subtle" />
                <span className="truncate">{tableLabel(table, wide)}</span>
                <span className="flex-1" />
                <span className="shrink-0 text-xs font-normal text-fg-muted tabular-nums">{t("find.rows", { count: result!.rows })}</span>
              </button>
              <div className="flex flex-wrap gap-1.5 pl-5">
                {result!.hits.map((h) => (
                  <button
                    key={h.column}
                    type="button"
                    data-find-column={h.column}
                    title={t("find.openColumn", { column: h.column })}
                    onClick={() => onOpen(table, "table", [h.column])}
                    className="h-6 rounded-md bg-sunken px-2 text-[11px] text-fg-muted transition-colors hover:bg-accent-soft hover:text-accent-text"
                  >
                    {h.column} <span className="tabular-nums opacity-70">· {h.rows.toLocaleString()}</span>
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {failed.length > 0 && (
        <div className="flex flex-col gap-1">
          <h3 className="text-[11px] font-semibold tracking-wider text-fg-subtle uppercase">{t("find.failed")}</h3>
          {failed.map(({ table, error }) => (
            <p key={tableLabel(table, true)} data-find-failed={tableLabel(table, wide)} className="text-xs">
              <span className="font-medium">{tableLabel(table, wide)}</span> <span className="text-danger">{error}</span>
            </p>
          ))}
        </div>
      )}

      {run.done && skipped > 0 && <p className="text-[11px] text-fg-subtle">{t("find.skipped", { count: skipped })}</p>}
    </>
  );
}

function NameResults({
  run,
  database,
  onOpen,
}: {
  run: Extract<Run, { kind: "names" }>;
  database: string;
  onOpen: (table: TableRef, kind: ObjectKind) => void;
}) {
  const { t } = useTranslation();
  if (run.error) return <p className="text-[13px] text-danger">{run.error}</p>;
  if (!run.groups) {
    return (
      <div className="flex items-center gap-2 text-[13px] text-fg-muted">
        <Spinner className="size-3.5" /> {t("common.loading")}
      </div>
    );
  }
  if (run.groups.length === 0) {
    return (
      <div data-find-empty className="flex flex-col items-center gap-2 py-10 text-center">
        <SearchX className="size-8 text-fg-subtle" />
        <p className="text-[13px] text-fg-muted">{t("find.noNames", { text: run.text })}</p>
      </div>
    );
  }
  const several = new Set(run.groups.map((g) => g.schema)).size > 1;
  return (
    <>
      <p className="text-[13px] text-fg-muted">{t("find.namesFound", { count: run.groups.length })}</p>
      <div className="overflow-hidden rounded-xl border border-border">
        {run.groups.map((g) => {
          const Icon = g.kind === "table" ? Table2 : Eye;
          const label = several && g.schema ? `${g.schema}.${g.table}` : g.table;
          return (
            <button
              key={`${g.schema}.${g.table}`}
              type="button"
              data-find-name={label}
              onClick={() => onOpen({ database, schema: g.schema, name: g.table }, g.kind)}
              className="flex w-full flex-col gap-1 border-b border-border px-3 py-2 text-left last:border-0 hover:bg-sunken/60"
            >
              <span className="flex items-center gap-2 text-[13px]">
                <Icon className="size-3.5 shrink-0 text-fg-subtle" />
                <span className={cn("truncate", g.named ? "font-semibold text-accent-text" : "font-medium")}>{label}</span>
                {g.kind !== "table" && <span className="text-[11px] text-fg-subtle">{t(g.kind === "view" ? "find.view" : "find.matview")}</span>}
              </span>
              {g.columns.length > 0 && (
                <span className="flex flex-wrap gap-1.5 pl-5">
                  {g.columns.map((c) => (
                    <span key={c.column} className="rounded-md bg-sunken px-2 py-0.5 font-mono text-[11px] text-fg-muted">
                      <span className="font-semibold text-fg">{c.column}</span> {c.dataType}
                    </span>
                  ))}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </>
  );
}
