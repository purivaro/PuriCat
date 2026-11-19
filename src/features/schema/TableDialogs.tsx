import { Copy, Eraser, FileCode2, Lock, PencilLine, TerminalSquare, Trash2, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, Field, Input, Spinner } from "../../components/ui/primitives";
import { clipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { TableChange, TableDefinition } from "../../lib/types";
import { useConnections } from "../../stores/connections";
import { type TableTask, useUi } from "../../stores/ui";
import { tableTabId, useWorkspace } from "../../stores/workspace";
import { Failure, Footer, Header, Notice, type Plan, Statement } from "./parts";

/**
 * What the menu of a table or a view does: show its definition, rename it, empty it, drop
 * it. As in the database forms, a form only chooses; the statement is built by the backend,
 * shown here as it will run, and run by the same code.
 */
export function TableDialogs() {
  const task = useUi((s) => s.tableTask);
  const close = () => useUi.getState().askTableTask(null);
  const [busy, setBusy] = useState(false);
  const wide = task?.kind === "definition";
  return (
    <Dialog.Root open={task !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-table-dialog={task?.kind}
          // Start in the first field, not on the close button (which would pop its tooltip).
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            const root = e.currentTarget as HTMLElement;
            (root.querySelector<HTMLElement>("input:not([disabled])") ?? root).focus();
          }}
          tabIndex={-1}
          className={cn(
            "animate-pop fixed top-1/2 left-1/2 z-50 flex -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none",
            wide ? "max-h-[min(92vh,760px)] w-[min(94vw,780px)]" : "max-h-[min(92vh,640px)] w-[min(94vw,540px)]",
          )}
        >
          {/* Mounted per opening, so each form reads the server afresh. */}
          {task?.kind === "definition" && <DefinitionView task={task} onDone={close} />}
          {task?.kind === "rename" && <RenameForm task={task} onBusy={setBusy} onDone={close} />}
          {(task?.kind === "empty" || task?.kind === "drop") && <DestroyForm task={task} onBusy={setBusy} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface FormProps {
  task: TableTask;
  onBusy: (busy: boolean) => void;
  onDone: () => void;
}

/** Does a tab of this table hold edits that were not saved? */
const useUnsaved = (task: TableTask) => useWorkspace((s) => !!s.dirty[tableTabId(task.connection.id, task.table)]);

// ---- Definition --------------------------------------------------------------------

function DefinitionView({ task, onDone }: Pick<FormProps, "task" | "onDone">) {
  const { t } = useTranslation();
  const { connection, table, objectKind } = task;
  const [definition, setDefinition] = useState<TableDefinition | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    ipc
      .tableDefinition(connection.id, table, objectKind)
      .then((d) => !stale && setDefinition(d))
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [connection.id, table, objectKind]);

  const copy = () =>
    definition &&
    void clipboard
      .write(definition.sql)
      .then(() => toast.success(t("common.copied")))
      .catch((e: unknown) => toast.error(errorMessage(e)));
  const openInQuery = () => {
    if (!definition) return;
    useWorkspace.getState().openSql(connection.id, table.database, definition.sql);
    onDone();
  };

  return (
    <div className="flex min-h-0 flex-col">
      <Header
        icon={<FileCode2 className="size-5 shrink-0 text-accent-text" />}
        title={t("schema.definitionTitle", { name: table.name })}
        connection={connection}
        busy={false}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-3 px-5 py-4">
        {error ? (
          <Failure message={error} />
        ) : !definition ? (
          <div className="flex items-center gap-2 py-8 text-[13px] text-fg-muted">
            <Spinner className="size-4" /> {t("common.loading")}
          </div>
        ) : (
          <>
            <pre
              data-selectable
              data-definition
              tabIndex={0}
              className="min-h-24 flex-1 overflow-auto rounded-lg border border-border bg-sunken px-3 py-2.5 font-mono text-xs leading-relaxed whitespace-pre outline-none focus-visible:ring-2 focus-visible:ring-(--ring)"
            >
              {definition.sql}
            </pre>
            <p data-definition-source className="text-xs leading-relaxed text-fg-subtle">
              {t(`schema.definitionFrom.${definition.source}`)}
            </p>
          </>
        )}
      </div>
      <div className="flex shrink-0 items-center justify-end gap-2 border-t border-border px-5 py-3">
        <Dialog.Close asChild>
          <Button variant="ghost">{t("common.close")}</Button>
        </Dialog.Close>
        <Button icon={<Copy className="size-4" />} disabled={!definition} onClick={copy}>
          {t("common.copy")}
        </Button>
        <Button variant="primary" icon={<TerminalSquare className="size-4" />} disabled={!definition} onClick={openInQuery}>
          {t("schema.openInQueryTab")}
        </Button>
      </div>
    </div>
  );
}

// ---- Rename ------------------------------------------------------------------------

function RenameForm({ task, onBusy, onDone }: FormProps) {
  const { t } = useTranslation();
  const { connection, table, objectKind } = task;
  const production = connection.environment === "production";
  const [newName, setNewName] = useState(table.name);
  const [plan, setPlan] = useState<(NonNullable<Plan> & { change: TableChange }) | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const blocked = useUnsaved(task);
  const field = useRef<HTMLInputElement>(null);

  // The old name is there to be typed over.
  useEffect(() => field.current?.select(), []);

  // `null`: nothing to rename to yet.
  const change = useMemo<TableChange | null>(() => {
    const name = newName.trim();
    return name && name !== table.name ? { action: "rename", table, kind: objectKind, newName: name } : null;
  }, [newName, table, objectKind]);

  useEffect(() => {
    setError(null);
    if (!change) return setPlan(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .planTableChange(connection.id, change)
        .then((sql) => !stale && setPlan({ change, sql }))
        .catch((e: unknown) => !stale && setPlan({ change, problem: errorMessage(e) }));
    }, 120);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, change]);

  // Between a change and the backend's answer the statement shown is the previous one:
  // it is dimmed and cannot be run.
  const current = plan?.change === change;
  const ready = !!plan && current && "sql" in plan && !connection.readOnly && !blocked && !saving;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready || !change || change.action !== "rename") return;
    setSaving(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.applyTableChange(connection.id, change, null);
      toast.success(t("schema.tableRenamed", { from: table.name, to: change.newName }));
      useWorkspace.getState().tableGone(connection.id, table);
      await useConnections.getState().loadObjects(connection.id, table.database, table.schema ?? null);
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setSaving(false);
      onBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-col">
      <Header
        icon={<PencilLine className="size-5 shrink-0 text-accent-text" />}
        title={t("schema.renameTableTitle", { name: table.name })}
        connection={connection}
        busy={saving}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <Notice tone="muted" icon={<Lock className="size-4" />}>
            {t("schema.readOnlyChange")}
          </Notice>
        )}
        <Field label={t("schema.newName")}>
          <Input
            ref={field}
            value={newName}
            disabled={saving}
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            onChange={(e) => setNewName(e.target.value)}
          />
        </Field>
        <Statement plan={plan} empty={t("schema.renameEmpty")} stale={!current} />
        {blocked && (
          <Notice tone="warn" icon={<TriangleAlert className="size-4" />}>
            {t("schema.unsavedTable")}
          </Notice>
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
          {saving ? t("schema.renaming") : t("schema.renameAction")}
        </Button>
      </Footer>
    </form>
  );
}

// ---- Empty, drop -------------------------------------------------------------------

/** Both destroy data: the same form, which runs only after the name was typed. */
function DestroyForm({ task, onBusy, onDone }: FormProps) {
  const { t } = useTranslation();
  const { connection, table, objectKind } = task;
  const emptying = task.kind === "empty";
  const production = connection.environment === "production";
  const [plan, setPlan] = useState<Plan>(null);
  const [rows, setRows] = useState<number | null>(null);
  const [typed, setTyped] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const blocked = useUnsaved(task);
  const open = useWorkspace((s) => s.tabs.some((x) => x.id === tableTabId(connection.id, table)));

  const change = useMemo<TableChange>(
    () => (emptying ? { action: "empty", table } : { action: "drop", table, kind: objectKind }),
    [emptying, table, objectKind],
  );

  useEffect(() => {
    let stale = false;
    ipc
      .planTableChange(connection.id, change)
      .then((sql) => !stale && setPlan({ sql }))
      .catch((e: unknown) => !stale && setPlan({ problem: errorMessage(e) }));
    // How much is about to be lost. The form works without it.
    if (objectKind === "table") {
      ipc
        .describeTable(connection.id, table)
        .then((meta) => !stale && setRows(meta.estimatedRows))
        .catch(() => undefined);
    }
    return () => {
      stale = true;
    };
  }, [connection.id, change, table, objectKind]);

  const ready = !!plan && "sql" in plan && typed === table.name && !connection.readOnly && !blocked && !running;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setRunning(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.applyTableChange(connection.id, change, typed);
      if (emptying) {
        toast.success(t("schema.tableEmptied", { name: table.name }));
        useWorkspace.getState().reloadTable(connection.id, table);
      } else {
        toast.success(t("schema.tableDropped", { name: table.name }));
        useWorkspace.getState().tableGone(connection.id, table);
        await useConnections.getState().loadObjects(connection.id, table.database, table.schema ?? null);
      }
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setRunning(false);
      onBusy(false);
    }
  };

  const isTable = objectKind === "table";
  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-col">
      <Header
        icon={emptying ? <Eraser className="size-5 shrink-0 text-danger" /> : <Trash2 className="size-5 shrink-0 text-danger" />}
        title={t(emptying ? "schema.emptyTableTitle" : "schema.dropTableTitle", { name: table.name })}
        connection={connection}
        busy={running}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <Notice tone="muted" icon={<Lock className="size-4" />}>
            {t("schema.readOnlyChange")}
          </Notice>
        )}
        <div className="flex flex-col gap-1 text-[13px] leading-relaxed">
          <p>{emptying ? t(`schema.emptyAbout.${connection.engine}`) : t(`schema.dropAbout.${objectKind}`)}</p>
          <p data-table-contents className="text-fg-muted">
            {rows == null ? null : rows === 0 ? t("schema.holdsNoRows") : t("schema.holdsRows", { count: rows })}
            {!emptying && open && ` ${t("schema.closesTab")}`}
          </p>
        </div>

        <Statement plan={plan} empty="…" />

        <Field label={t("schema.dropType", { name: table.name })}>
          <Input
            value={typed}
            disabled={running || connection.readOnly || !plan || "problem" in plan}
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            placeholder={table.name}
            onChange={(e) => setTyped(e.target.value)}
          />
        </Field>

        {blocked && (
          <Notice tone="warn" icon={<TriangleAlert className="size-4" />}>
            {t("schema.unsavedTable")}
          </Notice>
        )}
        {production && !connection.readOnly && (
          <Notice tone="danger" icon={<TriangleAlert className="size-4" />}>
            {t("schema.prodTableWarning")}
          </Notice>
        )}
        <Failure message={error} />
      </div>
      <Footer busy={running}>
        <Button type="submit" variant="danger" loading={running} disabled={!ready}>
          {emptying
            ? running
              ? t("schema.emptying")
              : t("schema.emptyAction")
            : running
              ? t("schema.dropping")
              : t(isTable ? "schema.dropTableAction" : "schema.dropViewAction")}
        </Button>
      </Footer>
    </form>
  );
}
