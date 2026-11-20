import { Grid2x2Plus, Lock, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, Input } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ConnectionView, NewTable, TypeChoice } from "../../lib/types";
import { nodeKey, useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { ColumnsEditor } from "./ColumnsEditor";
import { type ColumnDraft, firstColumns, tableSpec } from "./newTable";
import { Failure, Footer, Header, Notice } from "./parts";

/**
 * "New table…": a name, its columns and its primary key. The form only chooses; the
 * statements are built by the backend, shown here as they will run, and run by the same code.
 */
export function NewTableDialog() {
  const target = useUi((s) => s.newTableIn);
  const close = () => useUi.getState().askNewTable(null);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={target !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-new-table
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).querySelector<HTMLElement>("input")?.focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(92vh,760px)] w-[min(96vw,1100px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {target && <Form key={`${target.connection.id}/${target.database}/${target.schema}`} {...target} busy={busy} onBusy={setBusy} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface FormProps {
  connection: ConnectionView;
  database: string;
  schema: string | null;
  busy: boolean;
  onBusy: (busy: boolean) => void;
  onDone: () => void;
}

type Plan = ({ sql: string[] } | { problem: string }) & { spec: NewTable };

function Form({ connection, database, schema, busy, onBusy, onDone }: FormProps) {
  const { t } = useTranslation();
  const mysql = connection.engine === "mysql";
  const production = connection.environment === "production";

  const [types, setTypes] = useState<TypeChoice[] | null>(null);
  const [name, setName] = useState("");
  const [comment, setComment] = useState("");
  const [columns, setColumns] = useState<ColumnDraft[]>([]);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    ipc
      .columnTypes(connection.id)
      .then((list) => {
        if (stale) return;
        setTypes(list);
        setColumns(firstColumns(connection.engine, list));
      })
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [connection.id, connection.engine]);

  const spec = useMemo(() => tableSpec(database, schema, name, comment, columns), [database, schema, name, comment, columns]);

  // The preview is the backend's own statements for exactly these choices.
  useEffect(() => {
    setError(null);
    if (!types || name.trim() === "") return setPlan(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .planCreateTable(connection.id, spec)
        .then((sql) => !stale && setPlan({ spec, sql }))
        .catch((e: unknown) => !stale && setPlan({ spec, problem: errorMessage(e) }));
    }, 150);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, spec, name, types]);

  const current = plan?.spec === spec;
  const ready = !!plan && current && "sql" in plan && !connection.readOnly && !busy;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    onBusy(true);
    setError(null);
    try {
      await ipc.createTable(connection.id, spec);
      const table = { database, schema, name: name.trim() };
      toast.success(t("schema.tableCreated", { name: table.name }));
      const store = useConnections.getState();
      const parent = schema === null ? nodeKey.database(connection.id, database) : nodeKey.schema(connection.id, database, schema);
      store.toggle(`${parent}#tables`, true);
      await store.loadObjects(connection.id, database, schema);
      useWorkspace.getState().openTable(connection.id, table, "table");
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      onBusy(false);
    }
  };

  const place = schema ? `${database} › ${schema}` : database;

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-1 flex-col">
      <Header
        icon={<Grid2x2Plus className="size-5 shrink-0 text-accent-text" />}
        title={t("schema.newTableTitle", { place })}
        connection={connection}
        busy={busy}
      />

      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <Notice tone="muted" icon={<Lock className="size-4" />}>
            {t("schema.readOnlyTable")}
          </Notice>
        )}
        <div className="flex flex-wrap gap-3">
          <label className="flex w-72 flex-col gap-1">
            <span className="text-xs font-medium text-fg-muted">{t("schema.tableName")}</span>
            <Input data-new-table-name value={name} onChange={(e) => setName(e.target.value)} placeholder={t("schema.tableNameHint")} />
          </label>
          <label className="flex min-w-60 flex-1 flex-col gap-1">
            <span className="text-xs font-medium text-fg-muted">
              {t("schema.tableComment")} <span className="font-normal text-fg-subtle">· {t("common.optional")}</span>
            </span>
            <Input value={comment} onChange={(e) => setComment(e.target.value)} />
          </label>
        </div>

        <ColumnsEditor columns={columns} setColumns={setColumns} types={types} mysql={mysql} withKey />

        <div className="flex flex-col gap-1">
          <span className="text-xs font-medium text-fg-muted">{t("schema.statement")}</span>
          {plan && "sql" in plan ? (
            <pre
              data-selectable
              data-ddl-preview
              aria-busy={!current}
              className={cn(
                "rounded-lg border border-border bg-sunken px-3 py-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap",
                !current && "opacity-50",
              )}
            >
              {plan.sql.join(";\n\n")}
            </pre>
          ) : (
            <div data-ddl-preview-empty className="rounded-lg border border-dashed border-border px-3 py-2 text-xs text-fg-subtle">
              {plan ? <span className="text-danger">{plan.problem}</span> : t("schema.tablePreviewEmpty")}
            </div>
          )}
        </div>
        {production && (
          <Notice tone="danger" icon={<TriangleAlert className="size-4" />}>
            {t("schema.prodNewTableWarning")}
          </Notice>
        )}
        <Failure message={error} />
      </div>

      <Footer busy={busy}>
        <Button type="submit" variant={production ? "danger" : "primary"} data-new-table-create disabled={!ready} loading={busy}>
          {t("schema.createTable")}
        </Button>
      </Footer>
    </form>
  );
}
