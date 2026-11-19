import { Eye, Lock, Play, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, Field, Input, Spinner } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { QueryPreview, TableRef, ViewSpec } from "../../lib/types";
import { nodeKey, useConnections } from "../../stores/connections";
import { useUi, type ViewTask } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { Failure, Footer, Header, Notice, type Plan, Statement } from "./parts";

type Monaco = typeof import("../sql/monaco");
type Editor = ReturnType<Monaco["createEditor"]>;

/**
 * "New view…" and "Edit view…". The form holds the view's query and nothing else of the
 * statement: `CREATE VIEW … AS` is written around it by the backend, shown as it will run,
 * and run by the same code. The query can be tried first; trying only reads.
 */
export function ViewDialog() {
  const task = useUi((s) => s.viewTask);
  const close = () => useUi.getState().askViewTask(null);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={task !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-view-dialog={task?.kind}
          // The form's own controls close it: a click beside it, or Escape pressed to put
          // away the editor's suggestions, must not throw a query away.
          onInteractOutside={(e) => e.preventDefault()}
          onEscapeKeyDown={(e) => (e.target as HTMLElement | null)?.closest?.(".monaco-editor") && e.preventDefault()}
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            const root = e.currentTarget as HTMLElement;
            (root.querySelector<HTMLElement>("input:not([disabled])") ?? root).focus();
          }}
          tabIndex={-1}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[min(94vh,820px)] w-[min(94vw,760px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {/* Mounted per opening, so the form reads the server afresh. */}
          {task && <Form key={task.kind} task={task} onBusy={setBusy} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface FormProps {
  task: ViewTask;
  onBusy: (busy: boolean) => void;
  onDone: () => void;
}

type Tried = { for: string; rows: QueryPreview } | { for: string; problem: string };

function Form({ task, onBusy, onDone }: FormProps) {
  const { t } = useTranslation();
  const { connection } = task;
  const editing = task.kind === "edit";
  const database = editing ? task.table.database : task.database;
  const schema = editing ? (task.table.schema ?? null) : task.schema;
  const production = connection.environment === "production";

  const [name, setName] = useState(editing ? task.table.name : "");
  /** The query the editor starts with: `null` while the view's own is being read. */
  const [initial, setInitial] = useState<string | null>(editing ? null : "");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [select, setSelect] = useState("");
  const [plan, setPlan] = useState<(NonNullable<Plan> & { spec: ViewSpec }) | null>(null);
  const [tried, setTried] = useState<Tried | null>(null);
  const [trying, setTrying] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (task.kind !== "edit") return;
    let stale = false;
    ipc
      .viewSelect(connection.id, task.table)
      .then((query) => {
        if (stale) return;
        setSelect(query);
        setInitial(query);
      })
      .catch((e: unknown) => !stale && setLoadError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [connection.id, task]);

  // ---- the editor: the app's SQL editor, loaded as the same lazy chunk a query tab uses --
  const root = useRef<HTMLFormElement>(null);
  const host = useRef<HTMLDivElement>(null);
  const tryRef = useRef<() => void>(() => undefined);
  const tables = useMemo(
    () => ({
      tables: () => {
        const prefix = JSON.stringify([connection.id]).slice(0, -1);
        const names = new Set<string>();
        for (const [key, state] of Object.entries(useConnections.getState().objects)) {
          if (key.startsWith(prefix) && state.status === "ready") for (const o of state.items) names.add(o.name);
        }
        return [...names];
      },
      columns: (table: string) =>
        ipc
          .describeTable(connection.id, { database, schema, name: table })
          .then((meta) => meta.columns.map((c) => c.name))
          .catch(() => []),
    }),
    [connection.id, database, schema],
  );
  useEffect(() => {
    if (initial === null || !host.current) return;
    let disposed = false;
    let editor: Editor | undefined;
    void import("../sql/monaco").then((monaco) => {
      if (disposed || !host.current) return;
      editor = monaco.createEditor({
        container: host.current,
        engine: connection.engine,
        value: initial,
        schema: tables,
        onChange: setSelect,
        onRun: () => tryRef.current(),
        onRunCurrent: () => tryRef.current(),
        onSave: () => undefined,
        onCursor: () => undefined,
      });
      // A view that is being edited opens on its query; a new one on its name.
      if (editing) editor.focus();
      // Lets the end-to-end test type into the real editor.
      (root.current as unknown as { __editor?: Editor }).__editor = editor;
    });
    return () => {
      disposed = true;
      editor?.dispose();
    };
    // Created once per opening; `tables` is stable for it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initial]);

  // ---- the statement, from the backend ------------------------------------------------
  // `null`: not enough to make a statement of yet.
  const spec = useMemo<ViewSpec | null>(() => {
    if (!name.trim() || !select.trim()) return null;
    const table: TableRef = { database, schema, name: editing ? name : name.trim() };
    return { table, select, replace: editing };
  }, [name, select, database, schema, editing]);

  useEffect(() => {
    setError(null);
    if (!spec) return setPlan(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .planView(connection.id, spec)
        .then((statements) => !stale && setPlan({ spec, sql: statements.map((s) => `${s};`).join("\n\n") }))
        .catch((e: unknown) => !stale && setPlan({ spec, problem: errorMessage(e) }));
    }, 200);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, spec]);

  // ---- trying the query ---------------------------------------------------------------
  const tryIt = () => {
    if (trying || !select.trim()) return;
    const asked = select;
    setTrying(true);
    ipc
      .previewViewSelect(connection.id, database, asked)
      .then((rows) => setTried({ for: asked, rows }))
      .catch((e: unknown) => setTried({ for: asked, problem: errorMessage(e) }))
      .finally(() => setTrying(false));
  };
  tryRef.current = tryIt;

  const current = plan?.spec === spec;
  const ready = !!plan && current && "sql" in plan && !connection.readOnly && !saving;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready || !spec) return;
    setSaving(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.saveView(connection.id, spec);
      toast.success(t(editing ? "schema.viewSaved" : "schema.viewCreated", { name: spec.table.name }));
      const store = useConnections.getState();
      // The new view is shown where it now is: under "Views", opened.
      const parent = schema === null ? nodeKey.database(connection.id, database) : nodeKey.schema(connection.id, database, schema);
      store.toggle(`${parent}#views`, true);
      await store.loadObjects(connection.id, database, schema);
      // An open tab of the view shows the rows (and the columns) of its new query.
      if (editing) useWorkspace.getState().reloadTable(connection.id, spec.table);
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setSaving(false);
      onBusy(false);
    }
  };

  const place = schema ? `${database} › ${schema}` : database;
  const outdated = !!tried && tried.for !== select;

  return (
    <form ref={root} onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-col">
      <Header
        icon={<Eye className="size-5 shrink-0 text-accent-text" />}
        title={editing ? t("schema.editViewTitle", { name: task.table.name }) : t("schema.newViewTitle", { place })}
        connection={connection}
        busy={saving}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <Notice tone="muted" icon={<Lock className="size-4" />}>
            {t("schema.readOnlyChange")}
          </Notice>
        )}
        <Field label={t("schema.name")} hint={editing ? t("schema.viewNameFixed") : undefined}>
          <Input
            value={name}
            disabled={saving || editing}
            readOnly={editing}
            placeholder={t("schema.viewNamePlaceholder")}
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            onChange={(e) => setName(e.target.value)}
          />
        </Field>

        <div className="flex flex-col gap-1">
          <div className="flex items-baseline justify-between gap-3">
            <span className="text-xs font-medium text-fg-muted">{t("schema.viewQuery")}</span>
            <span className="truncate text-xs text-fg-subtle">{t("schema.viewQueryHint")}</span>
          </div>
          {loadError ? (
            <Failure message={loadError} />
          ) : (
            // Not clipped: the editor's suggestions may hang below it. The padding keeps its
            // square corners inside the rounded frame.
            <div className="relative h-48 rounded-lg border border-border-strong bg-surface p-1 focus-within:border-accent focus-within:ring-3 focus-within:ring-accent-soft">
              <div ref={host} data-view-editor className="h-full" />
              {initial === null && (
                <div className="absolute inset-0 flex items-center gap-2 px-3 text-[13px] text-fg-muted">
                  <Spinner className="size-4" /> {t("common.loading")}
                </div>
              )}
            </div>
          )}
          {editing && connection.engine === "mysql" && <p className="text-xs leading-snug text-fg-subtle">{t("schema.viewQueryMysql")}</p>}
          {connection.engine === "postgres" && <p className="text-xs leading-snug text-fg-subtle">{t("schema.viewQueryPostgres")}</p>}
        </div>

        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-3">
            <Button
              size="sm"
              icon={<Play className="size-3.5" />}
              loading={trying}
              disabled={trying || saving || !select.trim()}
              onClick={tryIt}
            >
              {t("schema.viewTry")}
              <kbd className="ml-0.5 font-sans text-[11px] opacity-70">⌘↵</kbd>
            </Button>
            <span data-view-tried className={cn("min-w-0 flex-1 truncate text-xs", outdated ? "text-accent-text" : "text-fg-subtle")}>
              {outdated
                ? t("schema.viewTriedOutdated")
                : tried && "rows" in tried
                  ? `${
                      tried.rows.rows.length === 0
                        ? t("schema.viewTriedNone")
                        : tried.rows.capped
                          ? t("schema.viewTriedFirst", { count: tried.rows.rows.length })
                          : t("schema.viewTriedRows", { count: tried.rows.rows.length })
                    } · ${tried.rows.elapsedMs} ms`
                  : !tried
                    ? t("schema.viewTryHint")
                    : ""}
            </span>
          </div>
          {tried && "problem" in tried && (
            <div role="alert" data-view-try-error className={cn("rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger", outdated && "opacity-50")}>
              <span data-selectable className="break-words">
                {tried.problem}
              </span>
            </div>
          )}
          {tried && "rows" in tried && tried.rows.columns.length > 0 && <Rows preview={tried.rows} dim={outdated} />}
        </div>

        <Statement plan={plan} empty={t("schema.viewStatementEmpty")} stale={!current} />

        {editing && (
          <p className="text-xs leading-relaxed text-fg-subtle">
            {t("schema.viewEditNote")} {connection.engine !== "mysql" && t(`schema.viewEditNoteOn.${connection.engine === "postgres" ? "postgres" : "sqlite"}`)}
          </p>
        )}
        {production && !connection.readOnly && (
          <Notice tone="danger" icon={<TriangleAlert className="size-4" />}>
            {t("schema.prodTableWarning")}
          </Notice>
        )}
        <Failure message={error} />
      </div>
      <Footer busy={saving}>
        <Button type="submit" variant={production ? "danger" : "primary"} loading={saving} disabled={!ready}>
          {saving ? t(editing ? "schema.saving" : "schema.creating") : t(editing ? "schema.viewSave" : "schema.viewCreate")}
        </Button>
      </Footer>
    </form>
  );
}

/** The rows a tried query gave: a small grid, enough to see that it is the right query. */
function Rows({ preview, dim }: { preview: QueryPreview; dim: boolean }) {
  return (
    <div data-view-rows className={cn("max-h-44 overflow-auto rounded-lg border border-border transition-opacity", dim && "opacity-50")}>
      <table className="w-max min-w-full border-separate border-spacing-0 text-xs">
        <thead>
          <tr>
            {preview.columns.map((c, i) => (
              <th
                key={i}
                title={c.typeName}
                className={cn(
                  "sticky top-0 border-b border-border bg-sunken px-2.5 py-1.5 font-medium whitespace-nowrap text-fg-muted",
                  c.kind === "number" ? "text-right" : "text-left",
                )}
              >
                {c.name}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {preview.rows.map((row, r) => (
            <tr key={r}>
              {row.map((cell, c) => (
                <td
                  key={c}
                  data-selectable
                  className={cn(
                    "max-w-72 truncate border-b border-border/60 px-2.5 py-1 font-mono whitespace-nowrap",
                    preview.columns[c]?.kind === "number" && "text-right tabular-nums",
                    r === preview.rows.length - 1 && "border-b-0",
                  )}
                >
                  {cell === null ? <span className="font-sans text-fg-subtle italic">NULL</span> : cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
