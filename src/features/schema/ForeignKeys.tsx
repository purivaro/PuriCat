import { ArrowRight, Plus, Trash2, Undo2 } from "lucide-react";
import { type Dispatch, type SetStateAction, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, IconButton, Input, inputClass } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { FkAction, ForeignKeyInfo, TableRef } from "../../lib/types";
import { type KeyDraft, newKey } from "./alterTable";

const ACTIONS: FkAction[] = ["noAction", "restrict", "cascade", "setNull"];
const actionSql = (a: FkAction | null) =>
  a === null ? "SET DEFAULT" : { noAction: "NO ACTION", restrict: "RESTRICT", cascade: "CASCADE", setNull: "SET NULL" }[a];

/**
 * The table's foreign keys: drop one, or make one from the columns the table will have to
 * columns of a table in the same database (and schema).
 */
export function ForeignKeys({
  connectionId,
  table,
  keys,
  dropped,
  setDropped,
  drafts,
  setDrafts,
  available,
}: {
  connectionId: string;
  table: TableRef;
  keys: ForeignKeyInfo[];
  dropped: string[];
  setDropped: Dispatch<SetStateAction<string[]>>;
  drafts: KeyDraft[];
  setDrafts: Dispatch<SetStateAction<KeyDraft[]>>;
  available: string[];
}) {
  const { t } = useTranslation();
  const [tables, setTables] = useState<string[] | null>(null);
  // The columns of each table a new key points to, as they are loaded.
  const [columnsOf, setColumnsOf] = useState<Record<string, string[] | "loading" | { error: string }>>({});

  const wanted = drafts.length > 0;
  useEffect(() => {
    if (!wanted || tables !== null) return;
    let stale = false;
    ipc
      .listObjects(connectionId, table.database, table.schema)
      .then((list) => !stale && setTables(list.filter((o) => o.kind === "table").map((o) => o.name)))
      .catch(() => !stale && setTables([]));
    return () => {
      stale = true;
    };
  }, [wanted, tables, connectionId, table.database, table.schema]);

  const load = (name: string) => {
    if (name === "" || name === table.name || columnsOf[name]) return;
    setColumnsOf((m) => ({ ...m, [name]: "loading" }));
    ipc
      .describeTable(connectionId, { database: table.database, schema: table.schema, name })
      .then((d) => setColumnsOf((m) => ({ ...m, [name]: d.columns.map((c) => c.name) })))
      .catch((e: unknown) => setColumnsOf((m) => ({ ...m, [name]: { error: errorMessage(e) } })));
  };

  const setDraft = (i: number, change: Partial<KeyDraft>) => setDrafts((l) => l.map((x, j) => (j === i ? { ...x, ...change } : x)));
  const toggle = (list: string[], name: string) => (list.includes(name) ? list.filter((n) => n !== name) : [...list, name]);
  const chip = (on: boolean) =>
    cn("h-6 rounded-md px-2 transition-colors", on ? "bg-accent font-semibold text-accent-fg" : "bg-sunken text-fg-muted hover:text-fg");
  const label = "flex flex-col gap-1 text-[11px] text-fg-muted";

  return (
    <>
      <div className="overflow-hidden rounded-xl border border-border">
        {keys.map((k) => {
          const gone = dropped.includes(k.name);
          return (
            <div
              key={k.name}
              data-foreign-key={k.name}
              className={cn("flex items-center gap-3 border-b border-border px-3 py-1.5 text-xs last:border-0", gone && "bg-danger-soft/50")}
            >
              <span className={cn("w-56 truncate font-medium", gone && "text-danger line-through")}>{k.name}</span>
              <span className="flex min-w-0 flex-1 items-center gap-1.5 truncate font-mono text-[11px] text-fg-muted">
                ({k.columns.join(", ")}) <ArrowRight className="size-3 shrink-0" /> {k.references.name} ({k.refColumns.join(", ")})
              </span>
              <span className="w-64 truncate text-[11px] text-fg-subtle">
                ON DELETE {actionSql(k.onDelete)} · ON UPDATE {actionSql(k.onUpdate)}
              </span>
              {gone ? (
                <Button size="sm" variant="ghost" icon={<Undo2 className="size-3.5" />} onClick={() => setDropped((l) => l.filter((n) => n !== k.name))}>
                  {t("schema.keepIndex")}
                </Button>
              ) : (
                <Button
                  size="sm"
                  variant="ghost"
                  data-drop-foreign-key={k.name}
                  icon={<Trash2 className="size-3.5" />}
                  onClick={() => setDropped((l) => [...l, k.name])}
                >
                  {t("schema.dropIndex")}
                </Button>
              )}
            </div>
          );
        })}
        {keys.length === 0 && <p className="px-3 py-2 text-xs text-fg-subtle">{t("schema.noForeignKeys")}</p>}
      </div>

      {drafts.map((k, i) => {
        const self = k.table === table.name;
        const target = self ? available : columnsOf[k.table];
        return (
          <div key={k.key} data-new-foreign-key={i} className="flex flex-col gap-2 rounded-xl border border-dashed border-border px-3 py-2.5 text-xs">
            <div className="flex flex-wrap items-end gap-3">
              <label className={label}>
                {t("schema.fkName")}
                <span className="w-48">
                  <Input placeholder={t("schema.indexNameHint")} value={k.name} onChange={(e) => setDraft(i, { name: e.target.value })} className="h-7" />
                </span>
              </label>
              <label className={label}>
                {t("schema.fkTable")}
                <span className="w-56">
                  <select
                    data-fk-table
                    value={k.table}
                    onChange={(e) => {
                      setDraft(i, { table: e.target.value, refColumns: [] });
                      load(e.target.value);
                    }}
                    className={cn(inputClass, "h-7 text-xs")}
                  >
                    <option value="">{tables === null ? t("common.loading") : t("schema.fkPickTable")}</option>
                    {tables?.map((name) => (
                      <option key={name} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                </span>
              </label>
              {(["onDelete", "onUpdate"] as const).map((field) => (
                <label key={field} className={label}>
                  {t(field === "onDelete" ? "schema.fkOnDelete" : "schema.fkOnUpdate")}
                  <span className="w-32">
                    <select
                      data-fk-action={field}
                      value={k[field]}
                      onChange={(e) => setDraft(i, { [field]: e.target.value as FkAction })}
                      className={cn(inputClass, "h-7 text-xs")}
                    >
                      {ACTIONS.map((a) => (
                        <option key={a} value={a}>
                          {actionSql(a)}
                        </option>
                      ))}
                    </select>
                  </span>
                </label>
              ))}
              <span className="flex-1" />
              <IconButton label={t("schema.removeForeignKey")} onClick={() => setDrafts((l) => l.filter((_, j) => j !== i))}>
                <Trash2 className="size-3.5" />
              </IconButton>
            </div>
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="w-28 shrink-0 text-[11px] text-fg-muted">{t("schema.fkColumns")}</span>
              {available.map((name) => {
                const at = k.columns.indexOf(name);
                return (
                  <button
                    key={name}
                    type="button"
                    aria-pressed={at >= 0}
                    data-fk-column={name}
                    onClick={() => setDraft(i, { columns: toggle(k.columns, name) })}
                    className={chip(at >= 0)}
                  >
                    {at >= 0 && <span className="mr-1 tabular-nums">{at + 1}</span>}
                    {name}
                  </button>
                );
              })}
            </div>
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="w-28 shrink-0 text-[11px] text-fg-muted">{t("schema.fkReferences")}</span>
              {k.table === "" ? (
                <span className="text-[11px] text-fg-subtle">{t("schema.fkPickTableFirst")}</span>
              ) : target === undefined || target === "loading" ? (
                <span className="text-[11px] text-fg-subtle">{t("common.loading")}</span>
              ) : "error" in target ? (
                <span className="text-[11px] text-danger">{target.error}</span>
              ) : (
                target.map((name) => {
                  const at = k.refColumns.indexOf(name);
                  return (
                    <button
                      key={name}
                      type="button"
                      aria-pressed={at >= 0}
                      data-fk-ref-column={name}
                      onClick={() => setDraft(i, { refColumns: toggle(k.refColumns, name) })}
                      className={chip(at >= 0)}
                    >
                      {at >= 0 && <span className="mr-1 tabular-nums">{at + 1}</span>}
                      {name}
                    </button>
                  );
                })
              )}
            </div>
          </div>
        );
      })}
      <div>
        <Button size="sm" variant="ghost" data-alter-add-foreign-key icon={<Plus className="size-3.5" />} onClick={() => setDrafts((l) => [...l, newKey()])}>
          {t("schema.addForeignKey")}
        </Button>
      </div>
    </>
  );
}
