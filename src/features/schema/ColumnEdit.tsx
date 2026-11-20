import { TriangleAlert, Undo2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button, Input, inputClass } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import type { ColumnDefinition, TypeChoice } from "../../lib/types";
import type { ColumnEditDraft } from "./alterTable";

/** A kept type that may default to CURRENT_TIMESTAMP. */
const timeType = (columnType: string) => /^(timestamp|datetime)/i.test(columnType.trim());

/**
 * The fields of a change to a column that is there: its type, NULL, default and comment.
 * What is left as it is stays as it is; the server's statement says the rest again.
 */
export function ColumnEdit({
  def,
  draft,
  set,
  types,
  mysql,
  primary,
  onCancel,
}: {
  def: ColumnDefinition;
  draft: ColumnEditDraft;
  set: (change: Partial<ColumnEditDraft>) => void;
  types: TypeChoice[];
  mysql: boolean;
  primary: boolean;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const choice = draft.type === "" ? null : types.find((x) => x.name === draft.type);
  const numeric = choice ? choice.family === "integer" || choice.family === "number" : false;
  const timestamp = choice ? choice.timestamp : timeType(def.columnType);
  const label = "flex flex-col gap-1 text-[11px] text-fg-muted";
  // PostgreSQL cannot carry a default across a change of type: the form must say what it becomes.
  const sayDefault = !mysql && draft.type !== "" && def.default !== null && draft.defaultKind === "keep";

  const pickType = (name: string) => {
    const next = types.find((x) => x.name === name);
    const change: Partial<ColumnEditDraft> = { type: name, length: "", scale: "" };
    if (next?.params.kind === "length" && next.params.required) change.length = String(next.params.default);
    if (next?.params.kind === "precision") {
      change.length = String(next.params.defaultPrecision);
      change.scale = String(next.params.defaultScale);
    }
    if (!next || (next.family !== "integer" && next.family !== "number")) change.unsigned = false;
    if (draft.defaultKind === "currentTimestamp" && !(next ? next.timestamp : timeType(def.columnType))) change.defaultKind = "keep";
    set(change);
  };

  return (
    <div data-column-edit={def.name} className="flex flex-col gap-2 border-t border-dashed border-border bg-sunken/60 px-3 py-2.5 pl-10">
      <div className="flex flex-wrap items-end gap-3">
        <label className={label}>
          {t("schema.columnType")}
          <span className="w-52">
            <select
              aria-label={t("schema.columnType")}
              data-column-edit-type
              value={draft.type}
              onChange={(e) => pickType(e.target.value)}
              className={cn(inputClass, "h-7 font-mono text-xs")}
            >
              <option value="">{t("schema.keepType", { type: def.columnType })}</option>
              {types.map((x) => (
                <option key={x.name} value={x.name}>
                  {x.name}
                </option>
              ))}
            </select>
          </span>
        </label>
        {choice?.params.kind === "length" && (
          <label className={label}>
            {t("schema.columnLength")}
            <span className="w-24">
              <Input
                inputMode="numeric"
                value={draft.length}
                placeholder={choice.params.required ? "" : t("schema.noLimit")}
                onChange={(e) => set({ length: e.target.value })}
                className="h-7 tabular-nums"
              />
            </span>
          </label>
        )}
        {choice?.params.kind === "precision" && (
          <label className={label}>
            {t("schema.columnPrecision")}, {t("schema.columnScale").toLowerCase()}
            <span className="flex w-32 items-center gap-1">
              <Input
                aria-label={t("schema.columnPrecision")}
                inputMode="numeric"
                value={draft.length}
                onChange={(e) => set({ length: e.target.value })}
                className="h-7 tabular-nums"
              />
              <span className="text-fg-subtle">,</span>
              <Input
                aria-label={t("schema.columnScale")}
                inputMode="numeric"
                value={draft.scale}
                onChange={(e) => set({ scale: e.target.value })}
                className="h-7 tabular-nums"
              />
            </span>
          </label>
        )}
        {mysql && numeric && (
          <label className="flex h-7 items-center gap-1.5 text-xs">
            <input type="checkbox" checked={draft.unsigned} onChange={(e) => set({ unsigned: e.target.checked })} className="size-3.5 accent-(--accent)" />
            {t("schema.columnUnsigned")}
          </label>
        )}
        <label className="flex h-7 items-center gap-1.5 text-xs" title={primary ? t("schema.keyNotNull") : undefined}>
          <input
            type="checkbox"
            data-column-edit-null
            checked={draft.nullable}
            disabled={primary}
            onChange={(e) =>
              set({
                nullable: e.target.checked,
                defaultKind: !e.target.checked && draft.defaultKind === "null" ? "keep" : draft.defaultKind,
              })
            }
            className="size-3.5 accent-(--accent)"
          />
          {t("schema.allowNull")}
        </label>
        <label className={label}>
          {t("schema.columnDefault")}
          <span className="flex items-center gap-1">
            <span className="w-48">
              <select
                aria-label={t("schema.columnDefault")}
                data-column-edit-default
                value={draft.defaultKind}
                onChange={(e) => set({ defaultKind: e.target.value as ColumnEditDraft["defaultKind"] })}
                className={cn(inputClass, "h-7 text-xs", sayDefault && "border-warn")}
              >
                <option value="keep">{def.default === null ? t("schema.keepNoDefault") : t("schema.keepDefault", { value: def.default })}</option>
                <option value="none">{t("schema.defaultNone")}</option>
                <option value="value">{t("schema.defaultValue")}</option>
                {draft.nullable && <option value="null">NULL</option>}
                {timestamp && <option value="currentTimestamp">CURRENT_TIMESTAMP</option>}
              </select>
            </span>
            {draft.defaultKind === "value" && (
              <span className="w-40">
                <Input
                  aria-label={t("schema.defaultValue")}
                  data-column-edit-default-value
                  value={draft.defaultValue}
                  onChange={(e) => set({ defaultValue: e.target.value })}
                  className="h-7"
                />
              </span>
            )}
          </span>
        </label>
        <label className={cn(label, "min-w-48 flex-1")}>
          {t("schema.columnComment")}
          <Input data-column-edit-comment value={draft.comment} onChange={(e) => set({ comment: e.target.value })} className="h-7" />
        </label>
        <Button size="sm" variant="ghost" icon={<Undo2 className="size-3.5" />} onClick={onCancel}>
          {t("schema.cancelColumnChange")}
        </Button>
      </div>
      {sayDefault && (
        <p className="flex items-center gap-1.5 text-[11px] text-warn">
          <TriangleAlert className="size-3.5 shrink-0" />
          {t("schema.sayDefaultOnTypeChange")}
        </p>
      )}
    </div>
  );
}
