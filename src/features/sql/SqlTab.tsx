import { save } from "@tauri-apps/plugin-dialog";
import {
  AlignLeft,
  CircleAlert,
  CircleCheck,
  CircleSlash,
  Download,
  History,
  OctagonX,
  Play,
  Plug,
  ShieldAlert,
  Square,
  TerminalSquare,
  Trash2,
  TriangleAlert,
  Unplug,
} from "lucide-react";
import { AlertDialog, DropdownMenu } from "radix-ui";
import { type PointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { EngineTag, EnvBadge } from "../../components/Brand";
import {
  Button,
  IconButton,
  menuContentClass,
  menuDangerClass,
  menuItemClass,
  menuSeparatorClass,
  Spinner,
  StateMessage,
} from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type {
  ExportFormat,
  HistoryEntry,
  ResultPreview,
  RowPage,
  RunOutcome,
  SqlStatement,
  SqlStatus,
  StatementOutcome,
} from "../../lib/types";
import { nodeKey, sessionOf, useConnections } from "../../stores/connections";
import { useSettings } from "../../stores/settings";
import { type SqlTab, sqlTextKey, useWorkspace } from "../../stores/workspace";
import { DataGrid, type Selection } from "../table/DataGrid";
import { EMPTY } from "../table/pending";

type Monaco = typeof import("./monaco");
type Editor = ReturnType<Monaco["createEditor"]>;

const MAX_ROWS = [100, 1000, 10000, 100000];
const STATUS_TONE: Record<SqlStatus, string> = {
  ok: "text-ok",
  error: "text-danger",
  blocked: "text-warn",
  cancelled: "text-fg-muted",
};
const STATUS_ICON = { ok: CircleCheck, error: CircleAlert, blocked: ShieldAlert, cancelled: CircleSlash };
const selectClass =
  "h-7 max-w-44 rounded-md border border-border-strong bg-surface px-2 text-xs text-fg hover:border-fg-subtle focus:border-accent focus:outline-none disabled:opacity-50";

/** A query result in the shape the shared grid displays. */
function toPage(result: ResultPreview): RowPage {
  return {
    columns: result.columns.map((c) => ({
      name: c.name,
      dataType: c.typeName.toLowerCase(),
      kind: c.kind,
      nullable: true,
      default: null,
      primaryKey: false,
      generated: false,
      editable: false,
    })),
    rows: result.rows,
    truncated: result.truncated,
    primaryKey: [],
    estimatedRows: null,
    offset: 0,
    hasMore: false,
    orderedBy: [],
    elapsedMs: 0,
  };
}

const firstLine = (sql: string) => sql.replace(/\s+/g, " ").trim();

export default function SqlTabView({ tab, active }: { tab: SqlTab; active: boolean }) {
  const { t, i18n } = useTranslation();
  const connection = useConnections((s) => s.connections.find((c) => c.id === tab.connectionId));
  const session = useConnections((s) => sessionOf(s.sessions, tab.connectionId));
  const databases = useConnections((s) => s.databases[nodeKey.connection(tab.connectionId)]);
  const theme = useSettings((s) => s.theme);
  const connected = session.status === "connected";
  const hasSchemas = connected && session.info.capabilities.schemas;
  const database = tab.database ?? connection?.database ?? (hasSchemas ? "postgres" : null);

  const rootRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<Editor | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const [initial, setInitial] = useState<string | null>(null); // null until the saved text is read
  const [empty, setEmpty] = useState(true);
  const [editorHeight, setEditorHeight] = useState(240);
  const [maxRows, setMaxRows] = useState(1000);
  const [running, setRunning] = useState(false);
  const [outcome, setOutcome] = useState<RunOutcome | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [view, setView] = useState<"messages" | number>("messages");
  const [selection, setSelection] = useState<Selection | null>(null);
  const [confirm, setConfirm] = useState<{ sql: string; limit: number | null; risky: SqlStatement[] } | null>(null);
  const [history, setHistory] = useState<HistoryEntry[] | null>(null);

  // ---- saved text ------------------------------------------------------------------
  useEffect(() => {
    let stale = false;
    ipc
      .getSettings()
      .then((all) => !stale && setInitial(all[sqlTextKey(tab.id)] ?? ""))
      .catch(() => !stale && setInitial(""));
    return () => {
      stale = true;
    };
  }, [tab.id]);

  const saveTimer = useRef<number | undefined>(undefined);
  const onChange = useCallback(
    (value: string) => {
      setEmpty(value.trim() === "");
      window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => void ipc.setSetting(sqlTextKey(tab.id), value), 600);
    },
    [tab.id],
  );

  // ---- completion sources ----------------------------------------------------------
  const columnCache = useRef(new Map<string, Promise<string[]>>());
  const schema = useMemo(
    () => ({
      tables: () => {
        const prefix = JSON.stringify([tab.connectionId]).slice(0, -1);
        const names = new Set<string>();
        for (const [key, state] of Object.entries(useConnections.getState().objects)) {
          if (key.startsWith(prefix) && state.status === "ready") for (const o of state.items) names.add(o.name);
        }
        return [...names];
      },
      columns: (table: string) => {
        const cached = columnCache.current.get(table);
        if (cached) return cached;
        // Prefer the place the explorer found the table in; otherwise the tab's database.
        let ref = { database: database ?? "", schema: hasSchemas ? "public" : null, name: table } as {
          database: string;
          schema: string | null;
          name: string;
        };
        for (const [key, state] of Object.entries(useConnections.getState().objects)) {
          const [id, db, sch] = JSON.parse(key) as [string, string, string | undefined];
          if (id === tab.connectionId && state.status === "ready" && state.items.some((o) => o.name === table)) {
            ref = { database: db, schema: sch ?? null, name: table };
            if (db === database) break;
          }
        }
        const request = ipc
          .describeTable(tab.connectionId, ref)
          .then((meta) => meta.columns.map((c) => c.name))
          .catch(() => []);
        columnCache.current.set(table, request);
        return request;
      },
    }),
    [tab.connectionId, database, hasSchemas],
  );

  // Make the current database's tables known to the completion (and the explorer).
  useEffect(() => {
    if (!connected || !database) return;
    const store = useConnections.getState();
    const key = hasSchemas ? nodeKey.schema(tab.connectionId, database, "public") : nodeKey.database(tab.connectionId, database);
    if (!store.objects[key]) void store.loadObjects(tab.connectionId, database, hasSchemas ? "public" : null);
    if (!hasSchemas && !store.databases[nodeKey.connection(tab.connectionId)]) void store.loadDatabases(tab.connectionId);
  }, [connected, database, hasSchemas, tab.connectionId]);

  // ---- running ---------------------------------------------------------------------
  const limit = connection?.bulkReviewRows ?? 10;
  const production = connection?.environment === "production";

  const execute = useCallback(
    async (sql: string, changeLimit: number | null) => {
      setRunning(true);
      setFailure(null);
      try {
        const result = await ipc.runSql(tab.connectionId, tab.id, tab.database, sql, { maxRows, changeLimit });
        setOutcome(result);
        setSelection(null);
        const first = result.statements.flatMap((s) => s.results)[0];
        setView(first ? first.id : "messages");
        setHistory(null);
      } catch (e) {
        setFailure(errorMessage(e));
        setView("messages"); // the failure is shown there, not over an older result
      } finally {
        setRunning(false);
        editorRef.current?.focus();
      }
    },
    [tab.connectionId, tab.id, tab.database, maxRows],
  );

  /** Ask first when the script contains something destructive (or anything that writes, on production). */
  const run = useCallback(
    async (sql: string, changeLimit: number | null) => {
      if (running || !connected || !sql.trim()) return;
      try {
        const statements = await ipc.analyzeSql(tab.connectionId, sql);
        const risky = statements.filter((s) => s.danger !== null || (production && s.kind !== "read"));
        if (risky.length > 0) return setConfirm({ sql, limit: changeLimit, risky });
      } catch (e) {
        setView("messages");
        return setFailure(errorMessage(e));
      }
      await execute(sql, changeLimit);
    },
    [running, connected, tab.connectionId, production, execute],
  );

  const runEditor = useCallback(() => {
    const editor = editorRef.current;
    if (editor && monacoRef.current) void run(monacoRef.current.textToRun(editor), limit);
  }, [run, limit]);
  const runRef = useRef(runEditor);
  runRef.current = runEditor;

  // An open transaction is uncommitted work: the tab asks before closing, and the
  // connection refuses to disconnect, exactly as for staged edits in a table tab.
  const inTransaction = connected && !!outcome?.inTransaction;
  useEffect(() => {
    useWorkspace.getState().setDirty(tab.id, inTransaction);
  }, [tab.id, inTransaction]);
  useEffect(() => () => useWorkspace.getState().setDirty(tab.id, false), [tab.id]);
  // A closed session took the tab's connection, and any transaction, with it.
  useEffect(() => {
    if (!connected) setOutcome((o) => (o?.inTransaction ? { ...o, inTransaction: false } : o));
  }, [connected]);

  // ---- editor ----------------------------------------------------------------------
  const engine = connection?.engine;
  useEffect(() => {
    if (initial === null || !hostRef.current || !engine) return;
    let disposed = false;
    let editor: Editor | undefined;
    void import("./monaco").then((monaco) => {
      if (disposed || !hostRef.current) return;
      monacoRef.current = monaco;
      editor = monaco.createEditor({
        container: hostRef.current,
        engine,
        value: initial,
        schema,
        onChange,
        onRun: () => runRef.current(),
      });
      editorRef.current = editor;
      setEmpty(initial.trim() === "");
      // Lets the end-to-end test type into the real editor.
      (rootRef.current as unknown as { __editor?: Editor }).__editor = editor;
    });
    return () => {
      disposed = true;
      editor?.dispose();
      editorRef.current = null;
    };
    // The editor is created once per tab; `schema` and callbacks are read through refs/closures.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initial, engine]);

  useEffect(() => {
    monacoRef.current?.applyTheme();
  }, [theme]);
  useEffect(() => {
    if (active) editorRef.current?.focus();
  }, [active]);

  const format = async () => {
    const editor = editorRef.current;
    const monaco = monacoRef.current;
    if (!editor || !monaco || !engine) return;
    try {
      const { format: formatSql } = await import("sql-formatter");
      const selection = editor.getSelection();
      const whole = !selection || selection.isEmpty();
      const formatted = formatSql(monaco.textToRun(editor), {
        language: engine === "postgres" ? "postgresql" : engine === "mysql" ? "mysql" : "sql",
        keywordCase: "upper",
        tabWidth: 2,
      });
      monaco.replaceText(editor, formatted, whole);
    } catch {
      toast.error(t("sql.formatFailed"));
    }
  };

  const startResize = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const startY = e.clientY;
    const start = editorHeight;
    const max = (rootRef.current?.clientHeight ?? 600) - 160;
    const move = (ev: globalThis.PointerEvent) => setEditorHeight(Math.min(max, Math.max(90, start + ev.clientY - startY)));
    const stop = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", stop);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", stop);
  };

  // ---- results ---------------------------------------------------------------------
  const results = useMemo(() => outcome?.statements.flatMap((s) => s.results) ?? [], [outcome]);
  const shown = typeof view === "number" ? results.find((r) => r.id === view) : undefined;
  const page = useMemo(() => (shown ? toPage(shown) : null), [shown]);
  const number = new Intl.NumberFormat(i18n.language);

  const exportResult = async (format: ExportFormat) => {
    if (!shown) return;
    const path = await save({
      defaultPath: `${tab.title.replace(/\s+/g, "-").toLowerCase()}-result-${results.indexOf(shown) + 1}.${format}`,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (!path) return;
    try {
      const rows = await ipc.exportResult(tab.id, shown.id, format, path);
      toast.success(t("sql.exported", { count: rows }), { description: shown.capped ? t("sql.exportedCapped") : path });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  if (!connection) {
    return (
      <StateMessage tone="danger" icon={<CircleAlert className="size-5" />} title={t("table.missingConnection")} body={t("table.missingConnectionBody")} />
    );
  }

  return (
    <div ref={rootRef} data-sql-tab className="flex h-full flex-col">
      {/* Identity and controls */}
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-3 py-2">
        <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent-text">
          <TerminalSquare className="size-4" />
        </div>
        <span className="shrink-0 text-[14px] font-semibold">{tab.title}</span>
        <div className="flex h-7 max-w-52 shrink-0 items-center gap-1.5 rounded-lg border border-border px-1.5 text-xs">
          <EngineTag engine={connection.engine} />
          <span className="truncate font-medium">{connection.name}</span>
          <EnvBadge environment={connection.environment} />
        </div>
        {hasSchemas ? (
          <span className="shrink-0 rounded-md bg-sunken px-2 py-1 font-mono text-xs text-fg-muted" title={t("sql.database")}>
            {database}
          </span>
        ) : (
          <select
            aria-label={t("sql.database")}
            title={inTransaction ? t("sql.databaseLocked") : t("sql.database")}
            value={tab.database ?? ""}
            // Switching database opens a new connection, which would drop the transaction.
            disabled={!connected || running || inTransaction}
            onChange={(e) => useWorkspace.getState().setSqlDatabase(tab.id, e.target.value || null)}
            className={cn(selectClass, "font-mono")}
          >
            <option value="">{connection.database ?? t("sql.defaultDatabase")}</option>
            {databases?.status === "ready" && databases.items.filter((d) => d.name !== connection.database).map((d) => <option key={d.name}>{d.name}</option>)}
          </select>
        )}
        <span className="flex-1" />

        <select
          aria-label={t("sql.maxRows")}
          title={t("sql.maxRows")}
          value={maxRows}
          disabled={running}
          onChange={(e) => setMaxRows(Number(e.target.value))}
          className={cn(selectClass, "tabular-nums")}
        >
          {MAX_ROWS.map((n) => (
            <option key={n} value={n}>
              {number.format(n)}
            </option>
          ))}
        </select>
        <IconButton label={t("sql.format")} disabled={empty || running} onClick={() => void format()}>
          <AlignLeft className="size-4" />
        </IconButton>
        <DropdownMenu.Root onOpenChange={(open) => open && !history && void ipc.listHistory(connection.id, 100).then(setHistory)}>
          <DropdownMenu.Trigger asChild>
            <IconButton label={t("sql.history")}>
              <History className="size-4" />
            </IconButton>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content align="end" sideOffset={6} data-sql-history className={cn(menuContentClass, "max-h-[60vh] w-[min(90vw,520px)] overflow-y-auto")}>
              {!history ? (
                <div className="flex items-center gap-2 px-2 py-3 text-xs text-fg-muted">
                  <Spinner className="size-3.5" /> {t("common.loading")}
                </div>
              ) : history.length === 0 ? (
                <div className="px-3 py-4 text-center text-xs text-fg-muted">{t("sql.historyEmpty")}</div>
              ) : (
                <>
                  {history.map((h) => (
                    <DropdownMenu.Item
                      key={h.id}
                      className={cn(menuItemClass, "h-auto items-start py-1.5")}
                      onSelect={() => {
                        if (editorRef.current && monacoRef.current) monacoRef.current.replaceText(editorRef.current, h.sql, true);
                        toast.success(t("sql.historyLoaded"), { duration: 1500 });
                      }}
                    >
                      <span className={cn("mt-1.5 size-1.5 shrink-0 rounded-full bg-current", STATUS_TONE[h.status] ?? "text-fg-muted")} />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-mono text-xs text-fg">{firstLine(h.sql)}</span>
                        <span className="block truncate text-[11px] text-fg-subtle">
                          {new Date(h.startedAt).toLocaleString(i18n.language)} · {h.summary} · {number.format(h.elapsedMs)} ms
                        </span>
                      </span>
                    </DropdownMenu.Item>
                  ))}
                  <DropdownMenu.Separator className={menuSeparatorClass} />
                  <DropdownMenu.Item
                    className={cn(menuItemClass, menuDangerClass)}
                    onSelect={() => void ipc.clearHistory(connection.id).then(() => setHistory([]))}
                  >
                    <Trash2 className="size-3.5" /> {t("sql.historyClear")}
                  </DropdownMenu.Item>
                </>
              )}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
        {running ? (
          <Button size="sm" variant="danger" icon={<Square className="size-3 fill-current" />} onClick={() => void ipc.cancelSql(tab.id)}>
            {t("sql.stop")}
          </Button>
        ) : (
          <Button size="sm" variant="primary" disabled={!connected || empty} title={t("sql.runHint")} icon={<Play className="size-3 fill-current" />} onClick={runEditor}>
            {t("sql.run")} <kbd className="font-sans text-[11px] opacity-80">⌘↵</kbd>
          </Button>
        )}
      </div>

      {/* Editor */}
      <div style={{ height: editorHeight }} className="relative shrink-0">
        <div ref={hostRef} className="absolute inset-0" />
        {empty && initial !== null && (
          <div className="pointer-events-none absolute top-[10px] left-[52px] font-mono text-[13px] leading-[21px] text-fg-subtle">{t("sql.placeholder")}</div>
        )}
      </div>
      <div
        role="separator"
        aria-orientation="horizontal"
        onPointerDown={startResize}
        className="group relative z-10 -my-1 h-2 shrink-0 cursor-row-resize"
      >
        <div className="absolute inset-x-0 top-[3px] h-px bg-border transition-colors group-hover:h-0.5 group-hover:bg-accent group-active:bg-accent" />
      </div>

      {/* Results */}
      <div className="flex min-h-0 flex-1 flex-col">
        {!connected ? (
          <StateMessage
            tone={session.status === "error" ? "danger" : "neutral"}
            icon={session.status === "error" ? <CircleAlert className="size-5" /> : <Unplug className="size-5" />}
            title={session.status === "error" ? t("explorer.failed") : t("sql.notConnected")}
            body={session.status === "error" ? session.message : t("sql.notConnectedBody", { name: connection.name })}
            action={
              <Button
                variant="primary"
                loading={session.status === "connecting"}
                icon={<Plug className="size-3.5" />}
                onClick={() => void useConnections.getState().connect(connection.id)}
              >
                {t("explorer.connect")}
              </Button>
            }
          />
        ) : (
          <>
            <div className="flex h-9 shrink-0 items-center gap-1 border-b border-border bg-bg/60 px-2">
              <ResultTab active={view === "messages"} onClick={() => setView("messages")} label={t("sql.messages")} />
              {results.map((r, i) => (
                <ResultTab
                  key={r.id}
                  active={view === r.id}
                  onClick={() => {
                    setView(r.id);
                    setSelection(null);
                  }}
                  label={t("sql.result", { n: i + 1 })}
                  detail={`${number.format(r.rows.length)}${r.capped ? "+" : ""}`}
                />
              ))}
              <span className="flex-1" />
              {inTransaction && (
                <span data-sql-transaction className="flex items-center gap-1 rounded-md bg-warn-soft px-2 py-0.5 text-[11px] font-medium text-warn">
                  <TriangleAlert className="size-3" /> {t("sql.inTransaction")}
                </span>
              )}
              {outcome && (
                <span data-sql-summary className="px-1 text-[11px] text-fg-subtle tabular-nums">
                  {t("sql.summary", { count: outcome.statements.length, ms: number.format(outcome.elapsedMs) })}
                </span>
              )}
              {shown && (
                <DropdownMenu.Root>
                  <DropdownMenu.Trigger asChild>
                    <Button size="sm" icon={<Download className="size-3.5" />}>
                      {t("sql.export")}
                    </Button>
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Portal>
                    <DropdownMenu.Content align="end" sideOffset={6} className={menuContentClass}>
                      {(["csv", "xlsx", "json"] as const).map((f) => (
                        <DropdownMenu.Item key={f} className={menuItemClass} onSelect={() => void exportResult(f)}>
                          {f.toUpperCase()}
                        </DropdownMenu.Item>
                      ))}
                    </DropdownMenu.Content>
                  </DropdownMenu.Portal>
                </DropdownMenu.Root>
              )}
            </div>

            <div className="relative min-h-0 flex-1">
              {running && <div aria-hidden className="absolute inset-x-0 top-0 z-10 h-0.5 animate-pulse bg-accent" />}
              {shown && page ? (
                <div className="flex h-full flex-col">
                  {shown.capped && (
                    <div className="flex shrink-0 items-center gap-2 border-b border-border bg-warn-soft px-3 py-1.5 text-xs text-warn">
                      <TriangleAlert className="size-3.5 shrink-0" /> {t("sql.capped", { rows: number.format(shown.rows.length) })}
                    </div>
                  )}
                  <div className="min-h-0 flex-1">
                    <DataGrid
                      page={page}
                      sort={null}
                      sortDisabled
                      editable={false}
                      pending={EMPTY}
                      onChange={() => {}}
                      selection={selection}
                      onSelect={setSelection}
                      onUndo={() => {}}
                      onRedo={() => {}}
                      onSave={() => {}}
                    />
                  </div>
                </div>
              ) : (
                <Messages
                  outcome={outcome}
                  failure={failure}
                  limit={limit}
                  onRunAnyway={(sql) => void execute(sql, null)}
                  running={running}
                />
              )}
            </div>
          </>
        )}
      </div>

      {/* Destructive statements (and every write on production) are confirmed before they run. */}
      <AlertDialog.Root open={confirm !== null} onOpenChange={(open) => !open && setConfirm(null)}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
          <AlertDialog.Content
            data-sql-confirm
            className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[80vh] w-[min(94vw,620px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised p-5 shadow-pop outline-none"
          >
            <div className="flex gap-3.5">
              <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-danger-soft text-danger">
                <OctagonX className="size-5" />
              </div>
              <div className="min-w-0 flex-1">
                <AlertDialog.Title className="text-[15px] font-semibold">{t("sql.confirmTitle")}</AlertDialog.Title>
                <AlertDialog.Description className="mt-1 text-[13px] leading-relaxed text-fg-muted">
                  {production ? t("sql.confirmProd") : t("sql.confirmBody", { name: connection.name })}
                </AlertDialog.Description>
              </div>
            </div>
            <ul data-selectable className="mt-4 flex min-h-0 flex-col gap-2 overflow-y-auto">
              {confirm?.risky.map((s, i) => (
                <li key={i} className="rounded-lg border border-border px-3 py-2">
                  <div className="truncate font-mono text-xs" title={s.text}>
                    {firstLine(s.text)}
                  </div>
                  <div className={cn("mt-1 text-xs", s.danger ? "font-medium text-danger" : "text-fg-muted")}>{s.danger ?? t("sql.changesData")}</div>
                </li>
              ))}
            </ul>
            <div className="mt-5 flex justify-end gap-2">
              <AlertDialog.Cancel asChild>
                <Button variant="ghost">{t("common.cancel")}</Button>
              </AlertDialog.Cancel>
              <Button
                variant="danger"
                onClick={() => {
                  const pending = confirm;
                  setConfirm(null);
                  if (pending) void execute(pending.sql, pending.limit);
                }}
              >
                {t("sql.confirmRun")}
              </Button>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </div>
  );
}

function ResultTab({ active, onClick, label, detail }: { active: boolean; onClick: () => void; label: string; detail?: string }) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={cn(
        "flex h-6 shrink-0 items-center gap-1.5 rounded-md px-2 text-xs transition-colors",
        active ? "bg-surface font-semibold text-fg shadow-card ring-1 ring-border" : "text-fg-muted hover:text-fg",
      )}
    >
      {label}
      {detail && <span className="font-normal text-fg-subtle tabular-nums">{detail}</span>}
    </button>
  );
}

interface MessagesProps {
  outcome: RunOutcome | null;
  failure: string | null;
  limit: number;
  running: boolean;
  onRunAnyway: (sql: string) => void;
}

/** What each statement did: status, rows, timing, the server's message. */
function Messages({ outcome, failure, limit, running, onRunAnyway }: MessagesProps) {
  const { t, i18n } = useTranslation();
  const number = new Intl.NumberFormat(i18n.language);
  if (failure) {
    return (
      <div role="alert" data-selectable className="m-3 flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
        <CircleAlert className="mt-0.5 size-4 shrink-0" /> <span className="min-w-0 break-words">{failure}</span>
      </div>
    );
  }
  if (!outcome) {
    return <div className="flex h-full items-center justify-center text-[13px] text-fg-subtle">{running ? <Spinner /> : t("sql.noResults")}</div>;
  }
  const detail = (s: StatementOutcome) => {
    if (s.status === "blocked") return t("sql.blocked", { rows: number.format(s.rowsAffected ?? 0), limit: number.format(limit) });
    if (s.message) return s.message;
    const parts = s.results.map((r) => t("sql.rows", { count: r.rows.length }) + (r.capped ? "+" : ""));
    if (s.rowsAffected !== null) parts.push(t("sql.affected", { count: s.rowsAffected }));
    return parts.join(" · ");
  };
  return (
    <div data-sql-messages className="h-full overflow-y-auto">
      <ol data-selectable className="flex flex-col">
        {outcome.statements.map((s, i) => {
          const Icon = STATUS_ICON[s.status];
          return (
            <li key={i} data-status={s.status} className="flex items-start gap-2.5 border-b border-border px-3 py-2">
              <Icon aria-label={t(`sql.status.${s.status}`)} className={cn("mt-0.5 size-4 shrink-0", STATUS_TONE[s.status])} />
              <div className="min-w-0 flex-1">
                <div className="truncate font-mono text-xs text-fg" title={s.sql}>
                  {firstLine(s.sql)}
                </div>
                <div className={cn("mt-0.5 text-xs break-words", s.status === "ok" ? "text-fg-muted" : STATUS_TONE[s.status])}>{detail(s)}</div>
                {s.status === "blocked" && outcome.resume && (
                  <div className="mt-2 flex flex-wrap items-center gap-2">
                    <Button size="sm" variant="danger" disabled={running} onClick={() => onRunAnyway(outcome.resume!)}>
                      {t("sql.runAnyway")}
                    </Button>
                    <span className="text-[11px] text-fg-subtle">{t("sql.limitHint")}</span>
                  </div>
                )}
              </div>
              <span className="shrink-0 rounded bg-sunken px-1.5 py-px text-[10px] text-fg-muted uppercase">{t(`sql.kind.${s.kind}`)}</span>
              <span className="w-16 shrink-0 text-right text-[11px] text-fg-subtle tabular-nums">{number.format(s.elapsedMs)} ms</span>
            </li>
          );
        })}
      </ol>
      {outcome.skipped > 0 && <p className="px-3 py-2 text-xs text-fg-muted">{t("sql.skipped", { count: outcome.skipped })}</p>}
    </div>
  );
}
