import { ArrowDown, ArrowUp, KeyRound, Plus, Trash2 } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";

import { Button, IconButton, Input, inputClass } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import type { TypeChoice } from "../../lib/types";
import { blankColumn, type ColumnDraft, move, withType } from "./newTable";

/**
 * Rows of columns to make: the new-table form's columns, and the columns the change form
 * adds. `withKey`: the primary key and self-numbering can be chosen (a new table only).
 */
export function ColumnsEditor({
  columns,
  setColumns,
  types,
  mysql,
  withKey = false,
  keepOne = true,
}: {
  columns: ColumnDraft[];
  setColumns: Dispatch<SetStateAction<ColumnDraft[]>>;
  types: TypeChoice[] | null;
  mysql: boolean;
  withKey?: boolean;
  /** A new table keeps at least one column; the change form may add none. */
  keepOne?: boolean;
}) {
  const { t } = useTranslation();
  const update = (index: number, change: Partial<ColumnDraft>) => setColumns((list) => list.map((c, i) => (i === index ? { ...c, ...change } : c)));
  const choiceOf = (c: ColumnDraft) => types?.find((x) => x.name === c.type);
  const cell = "border-b border-border px-1.5 py-1 align-middle";
  return (
    <div className="overflow-x-auto rounded-xl border border-border">
      <table className="w-full min-w-[1040px] table-fixed border-separate border-spacing-0 text-xs">
        <thead className="bg-sunken text-left text-[11px] font-semibold tracking-wide text-fg-subtle uppercase">
          <tr>
            {withKey && (
              <th className="w-10 px-1.5 py-1.5" title={t("schema.columnKey")}>
                <KeyRound className="mx-auto size-3.5" />
              </th>
            )}
            <th className="px-1.5 py-1.5">{t("schema.columnName")}</th>
            <th className="w-36 px-1.5 py-1.5">{t("schema.columnType")}</th>
            <th className="w-32 px-1.5 py-1.5">{t("schema.columnSize")}</th>
            <th className="w-12 px-1.5 py-1.5 text-center">{t("schema.columnNull")}</th>
            {withKey && <th className="w-14 px-1.5 py-1.5 text-center">{t(mysql ? "schema.columnAutoMysql" : "schema.columnAutoPostgres")}</th>}
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
              <tr key={c.key} data-table-column={i}>
                {withKey && (
                  <td className={cn(cell, "text-center")}>
                    <input
                      type="checkbox"
                      aria-label={t("schema.columnKey")}
                      checked={c.primary}
                      onChange={(e) => update(i, { primary: e.target.checked })}
                      className="size-3.5 accent-(--accent)"
                    />
                  </td>
                )}
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
                {withKey && (
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
                )}
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
                  <Input aria-label={t("schema.columnComment")} value={c.comment} onChange={(e) => update(i, { comment: e.target.value })} className="h-7" />
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
                    disabled={keepOne && columns.length === 1}
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
  );
}
