import { ChevronUp, Plus, X } from "lucide-react";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, IconButton } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import type { ColumnMeta, RowFilter } from "../../lib/types";
import { type Draft, newDraft, opsFor, sameFilters, takesNoValue, toDrafts, toFilters, withColumn } from "./filter";

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
  onApply: (filters: RowFilter[]) => void;
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
export function FilterBar({ columns, applied, onApply, onClose, locked, suggested, focusSignal }: Props) {
  const { t } = useTranslation();
  const root = useRef<HTMLDivElement>(null);
  const blank = () => newDraft(columns.find((c) => c.name === suggested) ?? columns[0]!);
  const [drafts, setDrafts] = useState<Draft[]>(() => (applied.length > 0 ? toDrafts(applied) : [blank()]));

  // Conditions set from elsewhere (a cell's menu, "Clear filter") replace what is being written.
  useEffect(() => {
    if (!sameFilters(applied, toFilters(drafts))) setDrafts(applied.length > 0 ? toDrafts(applied) : [blank()]);
    // Only a change of the applied conditions matters here, not every keystroke in a draft.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [applied]);

  useEffect(() => {
    const first = root.current?.querySelector<HTMLElement>("input:not(:disabled)") ?? root.current?.querySelector<HTMLElement>("select:not(:disabled)");
    first?.focus();
  }, [focusSignal]);

  const next = toFilters(drafts);
  const changed = !sameFilters(next, applied);
  const update = (id: number, change: (d: Draft) => Draft) => setDrafts((list) => list.map((d) => (d.id === id ? change(d) : d)));
  const apply = () => {
    if (!locked && changed) onApply(next);
  };
  const clear = () => {
    setDrafts([blank()]);
    if (applied.length > 0) onApply([]);
  };
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
        return (
          <div key={d.id} data-filter-row className="flex items-center gap-1.5">
            <span className="w-14 shrink-0 text-right text-xs text-fg-subtle">{t(i === 0 ? "filter.where" : "filter.and")}</span>
            <select
              aria-label={t("filter.column")}
              value={d.column}
              disabled={locked}
              onChange={(e) => {
                const picked = columns.find((c) => c.name === e.target.value);
                if (picked) update(d.id, (x) => withColumn(x, picked));
              }}
              className={cn(fieldClass, "w-48 font-medium")}
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
              className={cn(fieldClass, "w-40")}
            >
              {ops.map((op) => (
                <option key={op} value={op}>
                  {t(`filter.op.${op}`)}
                </option>
              ))}
            </select>
            {takesNoValue(d.op) ? (
              <span className="max-w-96 flex-1" />
            ) : (
              <input
                autoComplete="off"
                autoCorrect="off"
                autoCapitalize="off"
                spellCheck={false}
                aria-label={t("filter.value")}
                value={d.value}
                disabled={locked}
                placeholder={d.emptyText ? t("filter.emptyText") : t(column?.kind === "temporal" ? "filter.valueDate" : "filter.value")}
                onChange={(e) => update(d.id, (x) => ({ ...x, value: e.target.value, emptyText: false }))}
                onKeyDown={(e) => {
                  // Not while an input method is still composing the text (Thai, Japanese, ...).
                  if (e.key === "Enter" && !e.nativeEvent.isComposing) apply();
                }}
                className={cn(fieldClass, "max-w-96 flex-1")}
              />
            )}
            <IconButton label={t("filter.remove")} disabled={locked} onClick={() => setDrafts((list) => list.filter((x) => x.id !== d.id))}>
              <X className="size-3.5" />
            </IconButton>
          </div>
        );
      })}

      <div className="flex items-center gap-1.5 pl-[62px]">
        <Button
          size="sm"
          variant="ghost"
          disabled={locked || drafts.length >= MAX_CONDITIONS}
          icon={<Plus className="size-3.5" />}
          onClick={() => setDrafts((list) => [...list, blank()])}
        >
          {t("filter.add")}
        </Button>
        <span data-filter-note className={cn("min-w-0 flex-1 truncate text-xs", changed && !locked ? "text-accent-text" : "text-fg-subtle")}>
          {locked ? t("table.dirtyLocked") : changed ? t("filter.notApplied") : applied.length > 0 ? t("filter.applied", { count: applied.length }) : ""}
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
