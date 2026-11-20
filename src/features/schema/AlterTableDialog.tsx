import { Columns3, KeyRound, Lock, Pencil, Plus, Trash2, TriangleAlert, Undo2 } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, IconButton, Input, Spinner } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { AlterTable, ColumnDefinition, ColumnMeta, ConnectionView, ForeignKeyInfo, IndexInfo, TableEdit, TableRef, TypeChoice } from "../../lib/types";
import { nodeKey, useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import {
  type ColumnChange,
  columnEdit,
  type ColumnEditDraft,
  columnsAfter,
  type IndexDraft,
  indexesLosing,
  type KeyDraft,
  keyEdit,
  keyReady,
  newIndex,
  startColumns,
  startEdit,
  tableEdits,
} from "./alterTable";
import { ColumnEdit } from "./ColumnEdit";
import { ColumnsEditor } from "./ColumnsEditor";
import { ForeignKeys } from "./ForeignKeys";
import type { ColumnDraft } from "./newTable";
import { Failure, Footer, Header, Notice } from "./parts";

/**
 * "Change table…": rename, change or drop the columns that are there, add columns, drop and
 * make indexes and foreign keys. The changes go out as one: one statement on MySQL, one transaction on PostgreSQL.
 * As in the other forms, the statements are the backend's, shown before they run.
 */
export function AlterTableDialog() {
  const target = useUi((s) => s.alterOf);
  const close = () => useUi.getState().askAlterTable(null);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={target !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-alter-table
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(92vh,820px)] w-[min(96vw,1100px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {target && <Form key={`${target.connection.id}/${JSON.stringify(target.table)}`} {...target} busy={busy} onBusy={setBusy} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

type Plan = ({ sql: string[] } | { problem: string }) & { spec: AlterTable };

function Form({
  connection,
  table,
  busy,
  onBusy,
  onDone,
}: {
  connection: ConnectionView;
  table: TableRef;
  busy: boolean;
  onBusy: (b: boolean) => void;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const mysql = connection.engine === "mysql";
  const production = connection.environment === "production";

  const [loaded, setLoaded] = useState<{
    columns: ColumnMeta[];
    definitions: ColumnDefinition[];
    indexes: IndexInfo[];
    keys: ForeignKeyInfo[];
    types: TypeChoice[];
  } | null>(null);
  const [existing, setExisting] = useState<ColumnChange[]>([]);
  const [added, setAdded] = useState<ColumnDraft[]>([]);
  const [droppedIndexes, setDroppedIndexes] = useState<string[]>([]);
  const [indexes, setIndexes] = useState<IndexDraft[]>([]);
  const [changes, setChanges] = useState<Record<string, ColumnEditDraft>>({});
  const [droppedKeys, setDroppedKeys] = useState<string[]>([]);
  const [keys, setKeys] = useState<KeyDraft[]>([]);
  const [confirm, setConfirm] = useState("");
  const [plan, setPlan] = useState<Plan | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    Promise.all([
      ipc.describeTable(connection.id, table),
      ipc.columnDefinitions(connection.id, table),
      ipc.tableIndexes(connection.id, table),
      ipc.foreignKeys(connection.id, table),
      ipc.columnTypes(connection.id),
    ])
      .then(([meta, definitions, idx, fks, types]) => {
        if (stale) return;
        setLoaded({ columns: meta.columns, definitions, indexes: idx, keys: fks, types });
        setExisting(startColumns(meta.columns));
      })
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [connection.id, table]);

  const edits = useMemo(() => {
    const changed = Object.entries(changes)
      .map(([name, d]) => {
        const def = loaded?.definitions.find((c) => c.name === name);
        return def ? columnEdit(def, d, loaded!.types) : null;
      })
      .filter((e): e is TableEdit => e !== null);
    const made = keys.filter(keyReady).map((k) => keyEdit(table, k));
    return tableEdits(table.database, table.schema, table.name, existing, added, droppedIndexes, indexes, changed, droppedKeys, made);
  }, [table, existing, added, droppedIndexes, indexes, changes, droppedKeys, keys, loaded]);
  const spec = useMemo<AlterTable>(() => ({ table, edits }), [table, edits]);
  const drops = existing.filter((c) => c.drop).map((c) => c.name);
  const available = columnsAfter(existing, added);
  const losing = loaded ? indexesLosing(loaded.indexes, existing).filter((n) => !droppedIndexes.includes(n)) : [];

  useEffect(() => {
    setError(null);
    if (!loaded || edits.length === 0) return setPlan(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .planAlterTable(connection.id, spec)
        .then((sql) => !stale && setPlan({ spec, sql }))
        .catch((e: unknown) => !stale && setPlan({ spec, problem: errorMessage(e) }));
    }, 150);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, spec, edits.length, loaded]);

  const current = plan?.spec === spec;
  const confirmed = drops.length === 0 || confirm === table.name;
  const unfinishedKeys = keys.some((k) => !keyReady(k));
  const ready = !!plan && current && "sql" in plan && confirmed && !unfinishedKeys && !connection.readOnly && !busy;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    onBusy(true);
    setError(null);
    try {
      await ipc.alterTable(connection.id, spec, drops.length > 0 ? confirm : null);
      toast.success(t("schema.tableChanged", { name: table.name }));
      const store = useConnections.getState();
      const parent = table.schema === null ? nodeKey.database(connection.id, table.database) : nodeKey.schema(connection.id, table.database, table.schema);
      store.toggle(`${parent}#tables`, true);
      await store.loadObjects(connection.id, table.database, table.schema);
      useWorkspace.getState().reloadTable(connection.id, table);
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      onBusy(false);
    }
  };

  const setColumn = (i: number, change: Partial<ColumnChange>) => setExisting((l) => l.map((c, j) => (j === i ? { ...c, ...change } : c)));
  const setChange = (name: string, change: Partial<ColumnEditDraft> | null) =>
    setChanges((m) => {
      const next = { ...m };
      if (change === null) delete next[name];
      else next[name] = { ...m[name]!, ...change };
      return next;
    });
  const setIndex = (i: number, change: Partial<IndexDraft>) => setIndexes((l) => l.map((x, j) => (j === i ? { ...x, ...change } : x)));
  const section = "text-[11px] font-semibold tracking-wider text-fg-subtle uppercase";
  const place = [table.database, table.schema, table.name].filter(Boolean).join(" › ");

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-1 flex-col">
      <Header
        icon={<Columns3 className="size-5 shrink-0 text-accent-text" />}
        title={t("schema.alterTableTitle", { place })}
        connection={connection}
        busy={busy}
      />

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <Notice tone="muted" icon={<Lock className="size-4" />}>
            {t("schema.readOnlyAlter")}
          </Notice>
        )}
        {!loaded ? (
          error ? (
            <Failure message={error} />
          ) : (
            <div className="flex items-center gap-2 py-8 text-[13px] text-fg-muted">
              <Spinner className="size-3.5" /> {t("common.loading")}
            </div>
          )
        ) : (
          <>
            {/* The columns that are there: keep, rename or drop. */}
            <section className="flex flex-col gap-2">
              <h3 className={section}>{t("schema.existingColumns")}</h3>
              <div className="overflow-hidden rounded-xl border border-border">
                {existing.map((c, i) => {
                  const meta = loaded.columns[i]!;
                  const def = loaded.definitions.find((d) => d.name === c.name);
                  const editing = changes[c.name];
                  const renaming = c.rename.trim() !== "" && c.rename.trim() !== c.name;
                  return (
                    <div key={c.name} className="border-b border-border last:border-0">
                      <div data-existing-column={c.name} className={cn("flex items-center gap-3 px-3 py-1.5 text-xs", c.drop && "bg-danger-soft/50")}>
                        <span className="w-4 shrink-0">{meta.primaryKey && <KeyRound className="size-3.5 text-fg-subtle" />}</span>
                        <span className={cn("w-48 shrink-0 truncate font-medium", c.drop && "text-danger line-through")}>{c.name}</span>
                        <span className="w-40 shrink-0 truncate font-mono text-[11px] text-fg-muted" title={def?.columnType ?? meta.dataType}>
                          {def?.columnType ?? meta.dataType}
                        </span>
                        <span className="w-20 shrink-0 text-fg-subtle">{meta.nullable ? "NULL" : "NOT NULL"}</span>
                        <span className="w-40 shrink-0 truncate text-[11px] text-fg-subtle" title={def?.comment ?? undefined}>
                          {def?.default !== null && def?.default !== undefined ? `= ${def.default}` : ""}
                        </span>
                        <span className="w-48 shrink-0">
                          {!c.drop && (
                            <Input
                              aria-label={t("schema.renameTo", { name: c.name })}
                              placeholder={t("schema.renamePlaceholder")}
                              value={c.rename}
                              disabled={!!editing}
                              title={editing ? t("schema.renameOrChange") : undefined}
                              onChange={(e) => setColumn(i, { rename: e.target.value })}
                              className="h-7"
                            />
                          )}
                        </span>
                        <span className="flex-1" />
                        {!c.drop && !editing && def && (
                          <Button
                            size="sm"
                            variant="ghost"
                            data-change-column={c.name}
                            icon={<Pencil className="size-3.5" />}
                            disabled={renaming || def.generated}
                            title={def.generated ? t("schema.generatedStays") : renaming ? t("schema.renameOrChange") : undefined}
                            onClick={() => setChanges((m) => ({ ...m, [c.name]: startEdit(def) }))}
                          >
                            {t("schema.changeColumn")}
                          </Button>
                        )}
                        {c.drop ? (
                          <Button size="sm" variant="ghost" icon={<Undo2 className="size-3.5" />} onClick={() => setColumn(i, { drop: false })}>
                            {t("schema.keepColumn")}
                          </Button>
                        ) : (
                          <Button
                            size="sm"
                            variant="ghost"
                            data-drop-column={c.name}
                            icon={<Trash2 className="size-3.5" />}
                            disabled={meta.primaryKey}
                            title={meta.primaryKey ? t("schema.keyColumnStays") : undefined}
                            onClick={() => {
                              setColumn(i, { drop: true, rename: "" });
                              setChange(c.name, null);
                            }}
                          >
                            {t("schema.dropColumn")}
                          </Button>
                        )}
                      </div>
                      {editing && def && (
                        <ColumnEdit
                          def={def}
                          draft={editing}
                          set={(change) => setChange(c.name, change)}
                          types={loaded.types}
                          mysql={mysql}
                          primary={meta.primaryKey}
                          onCancel={() => setChange(c.name, null)}
                        />
                      )}
                    </div>
                  );
                })}
              </div>
            </section>

            <section className="flex flex-col gap-2">
              <h3 className={section}>{t("schema.addColumns")}</h3>
              {added.length > 0 ? (
                <ColumnsEditor columns={added} setColumns={setAdded} types={loaded.types} mysql={mysql} keepOne={false} />
              ) : (
                <div>
                  <Button
                    size="sm"
                    variant="ghost"
                    data-alter-add-column
                    icon={<Plus className="size-3.5" />}
                    onClick={() => setAdded([{ ...emptyColumn(loaded.types), nullable: true }])}
                  >
                    {t("schema.addColumn")}
                  </Button>
                </div>
              )}
            </section>

            {/* Indexes: drop one, make one from the columns the table will have. */}
            <section className="flex flex-col gap-2">
              <h3 className={section}>{t("schema.indexes")}</h3>
              <div className="overflow-hidden rounded-xl border border-border">
                {loaded.indexes.map((x) => {
                  const dropped = droppedIndexes.includes(x.name);
                  return (
                    <div
                      key={x.name}
                      data-index={x.name}
                      className={cn("flex items-center gap-3 border-b border-border px-3 py-1.5 text-xs last:border-0", dropped && "bg-danger-soft/50")}
                    >
                      <span className={cn("w-56 truncate font-medium", dropped && "text-danger line-through")}>{x.name}</span>
                      <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-fg-muted">({x.columns.join(", ")})</span>
                      <span className="w-24 text-fg-subtle">{x.primary ? t("schema.primaryKey") : x.unique ? t("schema.unique") : ""}</span>
                      {!x.primary &&
                        (dropped ? (
                          <Button
                            size="sm"
                            variant="ghost"
                            icon={<Undo2 className="size-3.5" />}
                            onClick={() => setDroppedIndexes((l) => l.filter((n) => n !== x.name))}
                          >
                            {t("schema.keepIndex")}
                          </Button>
                        ) : (
                          <Button
                            size="sm"
                            variant="ghost"
                            data-drop-index={x.name}
                            icon={<Trash2 className="size-3.5" />}
                            onClick={() => setDroppedIndexes((l) => [...l, x.name])}
                          >
                            {t("schema.dropIndex")}
                          </Button>
                        ))}
                    </div>
                  );
                })}
                {loaded.indexes.length === 0 && <p className="px-3 py-2 text-xs text-fg-subtle">{t("schema.noIndexes")}</p>}
              </div>
              {losing.length > 0 && (
                <Notice tone="warn" icon={<TriangleAlert className="size-4" />}>
                  {t("schema.indexLosesColumn", { names: losing.join(", ") })}
                </Notice>
              )}
              {indexes.map((x, i) => (
                <div
                  key={x.key}
                  data-new-index={i}
                  className="flex flex-wrap items-center gap-2 rounded-xl border border-dashed border-border px-3 py-2 text-xs"
                >
                  <span className="w-48">
                    <Input
                      aria-label={t("schema.indexName")}
                      placeholder={t("schema.indexNameHint")}
                      value={x.name}
                      onChange={(e) => setIndex(i, { name: e.target.value })}
                      className="h-7"
                    />
                  </span>
                  <label className="flex items-center gap-1.5">
                    <input
                      type="checkbox"
                      checked={x.unique}
                      onChange={(e) => setIndex(i, { unique: e.target.checked })}
                      className="size-3.5 accent-(--accent)"
                    />
                    {t("schema.unique")}
                  </label>
                  <span className="text-fg-subtle">·</span>
                  {available.map((name) => {
                    const at = x.columns.indexOf(name);
                    return (
                      <button
                        key={name}
                        type="button"
                        aria-pressed={at >= 0}
                        data-index-column={name}
                        onClick={() => setIndex(i, { columns: at >= 0 ? x.columns.filter((n) => n !== name) : [...x.columns, name] })}
                        className={cn(
                          "h-6 rounded-md px-2 transition-colors",
                          at >= 0 ? "bg-accent font-semibold text-accent-fg" : "bg-sunken text-fg-muted hover:text-fg",
                        )}
                      >
                        {at >= 0 && <span className="mr-1 tabular-nums">{at + 1}</span>}
                        {name}
                      </button>
                    );
                  })}
                  <span className="flex-1" />
                  <IconButton label={t("schema.removeIndex")} onClick={() => setIndexes((l) => l.filter((_, j) => j !== i))}>
                    <Trash2 className="size-3.5" />
                  </IconButton>
                </div>
              ))}
              <div>
                <Button
                  size="sm"
                  variant="ghost"
                  data-alter-add-index
                  icon={<Plus className="size-3.5" />}
                  onClick={() => setIndexes((l) => [...l, newIndex()])}
                >
                  {t("schema.addIndex")}
                </Button>
              </div>
            </section>

            <section className="flex flex-col gap-2">
              <h3 className={section}>{t("schema.foreignKeys")}</h3>
              <ForeignKeys
                connectionId={connection.id}
                table={table}
                keys={loaded.keys}
                dropped={droppedKeys}
                setDropped={setDroppedKeys}
                drafts={keys}
                setDrafts={setKeys}
                available={available}
              />
              {unfinishedKeys && <p className="text-[11px] text-fg-subtle">{t("schema.fkUnfinished")}</p>}
            </section>

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
                  {plan ? <span className="text-danger">{plan.problem}</span> : t("schema.alterPreviewEmpty")}
                </div>
              )}
            </div>

            {drops.length > 0 && (
              <div className="flex flex-col gap-2 rounded-xl border border-danger/40 bg-danger-soft/40 p-3">
                <p className="text-[13px] text-danger">{t("schema.dropColumnsWarning", { count: drops.length, names: drops.join(", ") })}</p>
                <label className="flex flex-col gap-1 text-xs">
                  <span className="text-fg-muted">{t("schema.typeTableName", { name: table.name })}</span>
                  <span className="w-72">
                    <Input data-alter-confirm value={confirm} onChange={(e) => setConfirm(e.target.value)} className="h-8" />
                  </span>
                </label>
              </div>
            )}
            {production && (
              <Notice tone="danger" icon={<TriangleAlert className="size-4" />}>
                {t("schema.prodAlterWarning")}
              </Notice>
            )}
            <Failure message={error} />
          </>
        )}
      </div>

      <Footer busy={busy}>
        <Button type="submit" variant={production || drops.length > 0 ? "danger" : "primary"} data-alter-apply disabled={!ready} loading={busy}>
          {t("schema.applyChanges")}
        </Button>
      </Footer>
    </form>
  );
}

function emptyColumn(types: TypeChoice[]): ColumnDraft {
  const text = types.find((t) => t.family === "text" && t.params.kind === "length") ?? types[0]!;
  return {
    key: `a${Date.now()}`,
    name: "",
    type: text.name,
    length: text.params.kind === "length" && text.params.required ? String(text.params.default) : "",
    scale: "",
    unsigned: false,
    nullable: true,
    defaultKind: "none",
    defaultValue: "",
    auto: false,
    primary: false,
    comment: "",
  };
}
