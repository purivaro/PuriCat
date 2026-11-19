import { Check, ChevronUp, Plus, X } from "lucide-react";
import { type ClipboardEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, IconButton, Segmented } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import type { ColumnMeta, RowFilter } from "../../lib/types";
import {
  type Draft,
  formatValues,
  MAX_VALUES,
  newDraft,
  opsFor,
  parseValues,
  sameFilters,
  takesList,
  takesNoValue,
  toDrafts,
  toFilters,
  withColumn,
} from "./filter";

/** The backend's limit (`db::filter::MAX_FILTERS`). */
const MAX_CONDITIONS = 20;

// One look for the selects and the value box of a condition, so a row reads as one line.
const fieldClass = cn(
  "h-7 min-w-0 rounded-lg border border-border-strong bg-surface px-2 text-xs text-fg placeholder:text-fg-subtle",
  "hover:border-fg-subtle focus:border-accent focus:ring-3 focus:ring-accent-soft focus:outline-none disabled:opacity-50",
);

interface Props {
  columns: ColumnMeta[];
  /** The conditions the rows on screen were loaded with. */
  applied: RowFilter[];
  /** Whether those rows had to meet any one of them, instead of all. */
  appliedAny: boolean;
  onApply: (filters: RowFilter[], matchAny: boolean) => void;
  onClose: () => void;
  /** Staged edits lock the page: a new filter would reload it and drop them. */
  locked: boolean;
  /** The column a new condition starts on: the one the grid's current cell is in. */
  suggested: string | null;
  /** Changes whenever the bar is asked for again (⌘F), to bring the keyboard back to it. */
  focusSignal: number;
}

/**
 * Conditions on the rows of the table in front. Nothing is queried while typing: the
 * conditions are sent on Enter or "Apply", and the bar says when what it shows is not
 * what the grid was loaded with.
 */
export function FilterBar({ columns, applied, appliedAny, onApply, onClose, locked, suggested, focusSignal }: Props) {
  const { t } = useTranslation();
  const root = useRef<HTMLDivElement>(null);
  const blank = () => newDraft(columns.find((c) => c.name === suggested) ?? columns[0]!);
  const [drafts, setDrafts] = useState<Draft[]>(() => (applied.length > 0 ? toDrafts(applied) : [blank()]));
  const [any, setAny] = useState(appliedAny);

  // Conditions set from elsewhere (a cell's menu, "Clear filter") replace what is being
  // written. The ones that are switched off were never applied: they stay.
  useEffect(() => {
    if (sameFilters(applied, toFilters(drafts))) return;
    const off = drafts.filter((d) => d.off);
    setDrafts(applied.length > 0 || off.length > 0 ? [...toDrafts(applied), ...off] : [blank()]);
    // Only a change of the applied conditions matters here, not every keystroke in a draft.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [applied]);
  useEffect(() => setAny(appliedAny), [appliedAny]);

  useEffect(() => {
    const first = root.current?.querySelector<HTMLElement>("input:not(:disabled)") ?? root.current?.querySelector<HTMLElement>("select:not(:disabled)");
    first?.focus();
  }, [focusSignal]);

  const next = toFilters(drafts);
  // "Any" is a matter of two conditions or more: one condition is the same either way.
  const nextAny = any && next.length > 1;
  const changed = !sameFilters(next, applied) || nextAny !== appliedAny;
  const update = (id: number, change: (d: Draft) => Draft) => setDrafts((list) => list.map((d) => (d.id === id ? change(d) : d)));
  const apply = () => {
    if (!locked && changed) onApply(next, nextAny);
  };
  const clear = () => {
    setDrafts([blank()]);
    setAny(false);
    if (applied.length > 0) onApply([], false);
  };
  const onEnter = (e: KeyboardEvent<HTMLInputElement>) => {
    // Not while an input method is still composing the text (Thai, Japanese, ...).
    if (e.key === "Enter" && !e.nativeEvent.isComposing) apply();
  };
  // A column copied from a spreadsheet is lines; a one-line box would run them together.
  const pasteList = (e: ClipboardEvent<HTMLInputElement>, d: Draft) => {
    const text = e.clipboardData.getData("text/plain");
    if (!/[\n\r]/.test(text)) return;
    e.preventDefault();
    const box = e.currentTarget;
    const before = d.value.slice(0, box.selectionStart ?? d.value.length);
    const after = d.value.slice(box.selectionEnd ?? d.value.length);
    const joined = [before.trim().replace(/,$/, ""), formatValues(parseValues(text)), after.trim().replace(/^,/, "")].filter((x) => x.trim() !== "");
    update(d.id, (x) => ({ ...x, value: joined.join(", ") }));
  };
  const textProps = { autoComplete: "off", autoCorrect: "off", autoCapitalize: "off", spellCheck: false, disabled: locked } as const;
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
    }
  };

  return (
    <div ref={root} data-filter-bar onKeyDown={onKeyDown} className="flex shrink-0 flex-col gap-1.5 border-b border-border bg-sunken/50 px-3 py-2">
      {drafts.map((d, i) => {
        const column = columns.find((c) => c.name === d.column);
        const ops = column ? opsFor(column.kind) : [d.op];
        const listed = takesList(d.op) ? parseValues(d.value).length : 0;
        const dim = d.off && "opacity-45";
        return (
          <div key={d.id} data-filter-row data-off={d.off || undefined} className="flex items-center gap-1.5">
            <button
              type="button"
              role="checkbox"
              aria-checked={!d.off}
              aria-label={t("filter.use")}
              title={t("filter.use")}
              disabled={locked}
              onClick={() => update(d.id, (x) => ({ ...x, off: !x.off }))}
              className={cn(
                "flex size-4 shrink-0 items-center justify-center rounded border transition-colors outline-none",
                "focus-visible:ring-3 focus-visible:ring-accent-soft disabled:opacity-50",
                d.off ? "border-border-strong bg-surface hover:border-fg-subtle" : "border-accent bg-accent text-accent-fg",
              )}
            >
              {!d.off && <Check className="size-3" strokeWidth={3} />}
            </button>
            <span className={cn("w-11 shrink-0 text-right text-xs text-fg-subtle", dim)}>{t(i === 0 ? "filter.where" : any ? "filter.or" : "filter.and")}</span>
            <select
              aria-label={t("filter.column")}
              value={d.column}
              disabled={locked}
              onChange={(e) => {
                const picked = columns.find((c) => c.name === e.target.value);
                if (picked) update(d.id, (x) => withColumn(x, picked));
              }}
              className={cn(fieldClass, "w-48 font-medium", dim)}
            >
              {/* A condition on a column the table no longer has stays visible until removed. */}
              {!column && <option value={d.column}>{d.column}</option>}
              {columns.map((c) => (
                <option key={c.name} value={c.name}>
                  {c.name}
                </option>
              ))}
            </select>
            <select
              aria-label={t("filter.operator")}
              value={d.op}
              disabled={locked}
              onChange={(e) => update(d.id, (x) => ({ ...x, op: e.target.value as Draft["op"] }))}
              className={cn(fieldClass, "w-40", dim)}
            >
              {ops.map((op) => (
                <option key={op} value={op}>
                  {t(`filter.op.${op}`)}
                </option>
              ))}
            </select>
            {takesNoValue(d.op) ? (
              <span className={cn("max-w-96 flex-1 truncate text-xs text-fg-subtle", dim)}>{d.op === "isNotEmpty" ? t("filter.notEmptyHint") : ""}</span>
            ) : d.op === "between" ? (
              <div className={cn("flex max-w-96 min-w-0 flex-1 items-center gap-1.5", dim)}>
                <input
                  {...textProps}
                  aria-label={t("filter.from")}
                  value={d.value}
                  placeholder={t(column?.kind === "temporal" ? "filter.valueDate" : "filter.from")}
                  onChange={(e) => update(d.id, (x) => ({ ...x, value: e.target.value }))}
                  onKeyDown={onEnter}
                  className={cn(fieldClass, "flex-1")}
                />
                <span className="shrink-0 text-xs text-fg-subtle">{t("filter.rangeTo")}</span>
                <input
                  {...textProps}
                  data-filter-to
                  aria-label={t("filter.to")}
                  value={d.to ?? ""}
                  placeholder={t(column?.kind === "temporal" ? "filter.valueDate" : "filter.to")}
                  onChange={(e) => update(d.id, (x) => ({ ...x, to: e.target.value }))}
                  onKeyDown={onEnter}
                  className={cn(fieldClass, "flex-1")}
                />
              </div>
            ) : takesList(d.op) ? (
              <div className={cn("relative flex max-w-96 min-w-0 flex-1", dim)}>
                <input
                  {...textProps}
                  aria-label={t("filter.values")}
                  title={t("filter.valuesHelp")}
                  value={d.value}
                  placeholder={t("filter.valuesHint")}
                  onChange={(e) => update(d.id, (x) => ({ ...x, value: e.target.value }))}
                  onPaste={(e) => pasteList(e, d)}
                  onKeyDown={onEnter}
                  className={cn(fieldClass, "w-full", listed > 0 && "pr-20")}
                />
                {listed > 0 && (
                  <span
                    data-filter-values
                    className={cn(
                      "pointer-events-none absolute inset-y-0 right-2 flex items-center text-[11px] tabular-nums",
                      listed > MAX_VALUES ? "font-medium text-danger" : "text-fg-subtle",
                    )}
                  >
                    {listed > MAX_VALUES ? t("filter.tooManyValues", { max: MAX_VALUES }) : t("filter.valueCount", { count: listed })}
                  </span>
                )}
              </div>
            ) : (
              <input
                {...textProps}
                aria-label={t("filter.value")}
                value={d.value}
                placeholder={d.emptyText ? t("filter.emptyText") : t(column?.kind === "temporal" ? "filter.valueDate" : "filter.value")}
                onChange={(e) => update(d.id, (x) => ({ ...x, value: e.target.value, emptyText: false }))}
                onKeyDown={onEnter}
                className={cn(fieldClass, "max-w-96 flex-1", dim)}
              />
            )}
            <IconButton label={t("filter.remove")} disabled={locked} onClick={() => setDrafts((list) => list.filter((x) => x.id !== d.id))}>
              <X className="size-3.5" />
            </IconButton>
          </div>
        );
      })}

      <div className="flex items-center gap-1.5 pl-[72px]">
        <Button
          size="sm"
          variant="ghost"
          disabled={locked || drafts.length >= MAX_CONDITIONS}
          icon={<Plus className="size-3.5" />}
          onClick={() => setDrafts((list) => [...list, blank()])}
        >
          {t("filter.add")}
        </Button>
        {drafts.length > 1 && (
          <div data-filter-match className="flex shrink-0 items-center gap-1.5 pl-1.5">
            <span className="text-xs text-fg-subtle">{t("filter.match")}</span>
            <Segmented
              label={t("filter.match")}
              value={any ? "any" : "all"}
              onChange={(v) => !locked && setAny(v === "any")}
              options={[
                { value: "all", label: t("filter.matchAll") },
                { value: "any", label: t("filter.matchAny") },
              ]}
              className="[&>button]:h-6 [&>button]:px-2.5"
            />
          </div>
        )}
        <span data-filter-note className={cn("min-w-0 flex-1 truncate text-xs", changed && !locked ? "text-accent-text" : "text-fg-subtle")}>
          {locked
            ? t("table.dirtyLocked")
            : changed
              ? t("filter.notApplied")
              : applied.length > 0
                ? t(appliedAny ? "filter.appliedAny" : "filter.applied", { count: applied.length })
                : ""}
        </span>
        <Button size="sm" variant="ghost" disabled={locked || (applied.length === 0 && next.length === 0)} onClick={clear}>
          {t("filter.clear")}
        </Button>
        <Button size="sm" variant={changed ? "primary" : "secondary"} disabled={locked || !changed} onClick={apply}>
          {t("filter.apply")}
          <kbd className="ml-0.5 font-sans text-[11px] opacity-80">↵</kbd>
        </Button>
        <IconButton label={t("filter.hide")} onClick={onClose}>
          <ChevronUp className="size-4" />
        </IconButton>
      </div>
    </div>
  );
}
