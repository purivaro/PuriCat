import { ArrowDown, ArrowUp, Grid2x2Plus, KeyRound, Lock, Plus, Trash2, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, IconButton, Input, inputClass } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ConnectionView, NewTable, TypeChoice } from "../../lib/types";
import { nodeKey, useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { blankColumn, type ColumnDraft, firstColumns, move, tableSpec, withType } from "./newTable";
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

  const update = (index: number, change: Partial<ColumnDraft>) => setColumns((list) => list.map((c, i) => (i === index ? { ...c, ...change } : c)));
  const choiceOf = (c: ColumnDraft) => types?.find((x) => x.name === c.type);

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
  const cell = "border-b border-border px-1.5 py-1 align-middle";

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

        <div className="overflow-x-auto rounded-xl border border-border">
          <table className="w-full min-w-[1040px] table-fixed border-separate border-spacing-0 text-xs">
            <thead className="bg-sunken text-left text-[11px] font-semibold tracking-wide text-fg-subtle uppercase">
              <tr>
                <th className="w-10 px-1.5 py-1.5" title={t("schema.columnKey")}>
                  <KeyRound className="mx-auto size-3.5" />
                </th>
                <th className="px-1.5 py-1.5">{t("schema.columnName")}</th>
                <th className="w-36 px-1.5 py-1.5">{t("schema.columnType")}</th>
                <th className="w-32 px-1.5 py-1.5">{t("schema.columnSize")}</th>
                <th className="w-12 px-1.5 py-1.5 text-center">{t("schema.columnNull")}</th>
                <th className="w-14 px-1.5 py-1.5 text-center">{t(mysql ? "schema.columnAutoMysql" : "schema.columnAutoPostgres")}</th>
                {mysql && <th className="w-20 px-1.5 py-1.5 text-center">{t("schema.columnUnsigned")}</th>}
                <th className="w-48 px-1.5 py-1.5">{t("schema.columnDefault")}</th>
                <th className="w-32 px-1.5 py-1.5">{t("schema.columnComment")}</th>
                <th className="w-[88px] px-1.5 py-1.5" />
              </tr>
            </thead>
            <tbody>
              {columns.map((c, i) => {
                const choice = choiceOf(c);
                const integer = choice?.family === "integer";
                const numeric = integer || choice?.family === "number";
                return (
                  <tr key={c.key} data-new-table-column={i}>
                    <td className={cn(cell, "text-center")}>
                      <input
                        type="checkbox"
                        aria-label={t("schema.columnKey")}
                        checked={c.primary}
                        onChange={(e) => update(i, { primary: e.target.checked })}
                        className="size-3.5 accent-(--accent)"
                      />
                    </td>
                    <td className={cell}>
                      <Input aria-label={t("schema.columnName")} value={c.name} onChange={(e) => update(i, { name: e.target.value })} className="h-7" />
                    </td>
                    <td className={cell}>
                      <select
                        aria-label={t("schema.columnType")}
                        value={c.type}
                        onChange={(e) => {
                          const next = types?.find((x) => x.name === e.target.value);
                          if (next) setColumns((list) => list.map((x, j) => (j === i ? withType(x, next) : x)));
                        }}
                        className={cn(inputClass, "h-7 font-mono text-xs")}
                      >
                        {types?.map((x) => (
                          <option key={x.name} value={x.name}>
                            {x.name}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td className={cell}>
                      {choice?.params.kind === "length" && (
                        <Input
                          aria-label={t("schema.columnLength")}
                          inputMode="numeric"
                          value={c.length}
                          placeholder={choice.params.required ? "" : t("schema.noLimit")}
                          onChange={(e) => update(i, { length: e.target.value })}
                          className="h-7 tabular-nums"
                        />
                      )}
                      {choice?.params.kind === "precision" && (
                        <div className="flex items-center gap-1">
                          <Input
                            aria-label={t("schema.columnPrecision")}
                            inputMode="numeric"
                            value={c.length}
                            onChange={(e) => update(i, { length: e.target.value })}
                            className="h-7 tabular-nums"
                          />
                          <span className="text-fg-subtle">,</span>
                          <Input
                            aria-label={t("schema.columnScale")}
                            inputMode="numeric"
                            value={c.scale}
                            onChange={(e) => update(i, { scale: e.target.value })}
                            className="h-7 tabular-nums"
                          />
                        </div>
                      )}
                    </td>
                    <td className={cn(cell, "text-center")}>
                      <input
                        type="checkbox"
                        aria-label={t("schema.columnNull")}
                        checked={c.nullable && !c.primary && !c.auto}
                        disabled={c.primary || c.auto}
                        onChange={(e) =>
                          update(i, {
                            nullable: e.target.checked,
                            defaultKind: !e.target.checked && c.defaultKind === "null" ? "none" : c.defaultKind,
                          })
                        }
                        className="size-3.5 accent-(--accent)"
                      />
                    </td>
                    <td className={cn(cell, "text-center")}>
                      <input
                        type="checkbox"
                        aria-label={t(mysql ? "schema.columnAutoMysql" : "schema.columnAutoPostgres")}
                        checked={c.auto}
                        disabled={!integer}
                        onChange={(e) =>
                          update(i, {
                            auto: e.target.checked,
                            defaultKind: e.target.checked ? "none" : c.defaultKind,
                          })
                        }
                        className="size-3.5 accent-(--accent)"
                      />
                    </td>
                    {mysql && (
                      <td className={cn(cell, "text-center")}>
                        <input
                          type="checkbox"
                          aria-label={t("schema.columnUnsigned")}
                          checked={c.unsigned}
                          disabled={!numeric}
                          onChange={(e) => update(i, { unsigned: e.target.checked })}
                          className="size-3.5 accent-(--accent)"
                        />
                      </td>
                    )}
                    <td className={cell}>
                      <div className="flex items-center gap-1">
                        <span className={c.defaultKind === "value" ? "w-24 shrink-0" : "w-full"}>
                          <select
                            aria-label={t("schema.columnDefault")}
                            value={c.defaultKind}
                            disabled={c.auto}
                            onChange={(e) =>
                              update(i, {
                                defaultKind: e.target.value as ColumnDraft["defaultKind"],
                              })
                            }
                            className={cn(inputClass, "h-7 text-xs")}
                          >
                            <option value="none">{t("schema.defaultNone")}</option>
                            <option value="value">{t("schema.defaultValue")}</option>
                            {c.nullable && !c.primary && <option value="null">NULL</option>}
                            {choice?.timestamp && <option value="currentTimestamp">CURRENT_TIMESTAMP</option>}
                          </select>
                        </span>
                        {c.defaultKind === "value" && (
                          <span className="min-w-0 flex-1">
                            <Input
                              aria-label={t("schema.defaultValue")}
                              value={c.defaultValue}
                              onChange={(e) => update(i, { defaultValue: e.target.value })}
                              className="h-7"
                            />
                          </span>
                        )}
                      </div>
                    </td>
                    <td className={cell}>
                      <Input
                        aria-label={t("schema.columnComment")}
                        value={c.comment}
                        onChange={(e) => update(i, { comment: e.target.value })}
                        className="h-7"
                      />
                    </td>
                    <td className={cn(cell, "whitespace-nowrap")}>
                      <IconButton label={t("schema.moveUp")} disabled={i === 0} onClick={() => setColumns((l) => move(l, i, -1))}>
                        <ArrowUp className="size-3.5" />
                      </IconButton>
                      <IconButton label={t("schema.moveDown")} disabled={i === columns.length - 1} onClick={() => setColumns((l) => move(l, i, 1))}>
                        <ArrowDown className="size-3.5" />
                      </IconButton>
                      <IconButton
                        label={t("schema.removeColumn")}
                        disabled={columns.length === 1}
                        onClick={() => setColumns((l) => l.filter((_, j) => j !== i))}
                      >
                        <Trash2 className="size-3.5" />
                      </IconButton>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          <div className="px-2 py-1.5">
            <Button
              size="sm"
              variant="ghost"
              data-new-table-add
              icon={<Plus className="size-3.5" />}
              disabled={!types}
              onClick={() => types && setColumns((l) => [...l, blankColumn(types)])}
            >
              {t("schema.addColumn")}
            </Button>
          </div>
        </div>

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
