import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ArrowDown,
  ArrowUp,
  Binary,
  Braces,
  Calendar,
  CircleDot,
  Copy,
  Hash,
  KeyRound,
  Rows3,
  ToggleLeft,
  Type,
} from "lucide-react";
import { ContextMenu } from "radix-ui";
import { type KeyboardEvent, type PointerEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { menuContentClass, menuItemClass } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import type { ColumnMeta, RowPage, SortSpec, ValueKind } from "../../lib/types";
import { nextSort } from "./sort";

const ROW_HEIGHT = 30;
const HEADER_HEIGHT = 34;
const GUTTER = 56;
const MIN_WIDTH = 64;

const KIND_ICON: Record<ValueKind, typeof Hash> = {
  number: Hash,
  text: Type,
  bool: ToggleLeft,
  temporal: Calendar,
  binary: Binary,
  json: Braces,
  other: CircleDot,
};

/** Width from the header and a sample of values, so most columns fit without dragging. */
function initialWidths(page: RowPage): number[] {
  return page.columns.map((col, c) => {
    let chars = col.name.length + 5;
    for (const row of page.rows.slice(0, 60)) chars = Math.max(chars, Math.min(row[c]?.length ?? 4, 44));
    return Math.round(Math.min(380, Math.max(84, chars * 7.4 + 26)));
  });
}

interface Props {
  page: RowPage;
  sort: SortSpec | null;
  onSort: (sort: SortSpec | null) => void;
}

export function DataGrid({ page, sort, onSort }: Props) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const signature = page.columns.map((c) => c.name).join("\u0000");
  const [widths, setWidths] = useState(() => initialWidths(page));
  const [active, setActive] = useState<{ r: number; c: number } | null>(null);

  // New table shape: recompute widths. Paging or sorting the same table keeps the user's widths.
  useEffect(() => {
    setWidths(initialWidths(page));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  // A new page of rows: the old selection no longer points at the same data.
  useEffect(() => {
    setActive(null);
    scrollRef.current?.scrollTo({ top: 0 });
  }, [page]);

  const truncated = useMemo(
    () => new Set(page.truncated.map(([r, c]) => r * page.columns.length + c)),
    [page],
  );
  const lefts = useMemo(() => {
    let x = GUTTER;
    return widths.map((w) => {
      const left = x;
      x += w;
      return left;
    });
  }, [widths]);
  const totalWidth = GUTTER + widths.reduce((a, b) => a + b, 0);

  const virtualizer = useVirtualizer({
    count: page.rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
    scrollPaddingStart: HEADER_HEIGHT,
  });

  const reveal = (r: number, c: number) => {
    virtualizer.scrollToIndex(r);
    const el = scrollRef.current;
    if (!el) return;
    const left = lefts[c] ?? 0;
    const right = left + (widths[c] ?? 0);
    if (left - GUTTER < el.scrollLeft) el.scrollLeft = left - GUTTER;
    else if (right > el.scrollLeft + el.clientWidth) el.scrollLeft = right - el.clientWidth;
  };

  const copy = (text: string) =>
    void navigator.clipboard.writeText(text).then(() => toast.success(t("common.copied"), { duration: 1200 }));
  const copyCell = () => active && copy(page.rows[active.r]?.[active.c] ?? "");
  const copyRow = () => active && copy((page.rows[active.r] ?? []).map((v) => v ?? "").join("\t"));

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const rows = page.rows.length;
    const cols = page.columns.length;
    if (!rows || !cols) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "c") {
      if (!active) return;
      e.preventDefault();
      return e.shiftKey ? copyRow() : copyCell();
    }
    const cur = active ?? { r: 0, c: 0 };
    const pageRows = Math.max(1, Math.floor(((scrollRef.current?.clientHeight ?? 300) - HEADER_HEIGHT) / ROW_HEIGHT) - 1);
    const moves: Record<string, [number, number]> = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
      PageUp: [-pageRows, 0],
      PageDown: [pageRows, 0],
      Home: [0, -cols],
      End: [0, cols],
    };
    const move = moves[e.key];
    if (!move) return;
    e.preventDefault();
    const next = active
      ? { r: Math.min(rows - 1, Math.max(0, cur.r + move[0])), c: Math.min(cols - 1, Math.max(0, cur.c + move[1])) }
      : cur;
    setActive(next);
    reveal(next.r, next.c);
  };

  const startResize = (e: PointerEvent<HTMLSpanElement>, c: number) => {
    e.preventDefault();
    e.stopPropagation();
    const handle = e.currentTarget;
    handle.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startWidth = widths[c] ?? MIN_WIDTH;
    const onMove = (ev: globalThis.PointerEvent) =>
      setWidths((w) => w.map((x, i) => (i === c ? Math.max(MIN_WIDTH, startWidth + ev.clientX - startX) : x)));
    const onUp = () => {
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onUp);
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onUp);
  };

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
        <div
          ref={scrollRef}
          role="grid"
          tabIndex={0}
          aria-rowcount={page.rows.length}
          aria-colcount={page.columns.length}
          onKeyDown={onKeyDown}
          className="h-full overflow-auto outline-none"
        >
          <div style={{ width: totalWidth, minWidth: "100%" }}>
            <div role="row" style={{ height: HEADER_HEIGHT }} className="sticky top-0 z-20 flex border-b border-border bg-sunken">
              <div style={{ width: GUTTER }} className="sticky left-0 z-10 shrink-0 border-r border-border bg-sunken" />
              {page.columns.map((col, c) => (
                <HeaderCell
                  key={col.name}
                  column={col}
                  width={widths[c] ?? MIN_WIDTH}
                  sort={sort?.column === col.name ? sort : null}
                  onSort={() => onSort(nextSort(sort, col.name))}
                  onResizeStart={(e) => startResize(e, c)}
                />
              ))}
              <div className="flex-1" />
            </div>

            <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
              {virtualizer.getVirtualItems().map((v) => {
                const row = page.rows[v.index]!;
                const selectedRow = active?.r === v.index;
                return (
                  <div
                    key={v.index}
                    role="row"
                    aria-rowindex={v.index + 1}
                    style={{ height: ROW_HEIGHT, transform: `translateY(${v.start}px)` }}
                    className={cn(
                      "group absolute top-0 left-0 flex w-full border-b border-border",
                      selectedRow ? "bg-accent-soft/60" : "hover:bg-sunken/70",
                    )}
                  >
                    <div
                      style={{ width: GUTTER }}
                      className={cn(
                        "sticky left-0 z-10 flex shrink-0 items-center justify-end border-r border-border pr-2 font-mono text-[11px] text-fg-subtle tabular-nums",
                        selectedRow ? "bg-accent-soft font-semibold text-accent-text" : "bg-surface group-hover:bg-sunken",
                      )}
                    >
                      {page.offset + v.index + 1}
                    </div>
                    {row.map((value, c) => {
                      const col = page.columns[c]!;
                      const isActive = selectedRow && active?.c === c;
                      const cut = truncated.has(v.index * page.columns.length + c);
                      return (
                        <div
                          key={c}
                          role="gridcell"
                          aria-selected={isActive}
                          onMouseDown={() => setActive({ r: v.index, c })}
                          onContextMenu={() => setActive({ r: v.index, c })}
                          style={{ width: widths[c] ?? MIN_WIDTH }}
                          className={cn(
                            "flex shrink-0 items-center overflow-hidden border-r border-border px-2 text-[12.5px]",
                            col.kind === "number" && "justify-end font-mono tabular-nums",
                            (col.kind === "binary" || col.kind === "json") && "font-mono text-fg-muted",
                            isActive && "bg-surface ring-2 ring-accent ring-inset",
                          )}
                        >
                          {value === null ? (
                            <span className="text-[11px] tracking-wide text-fg-subtle italic">{t("table.null")}</span>
                          ) : (
                            <span className="truncate whitespace-pre">{value}</span>
                          )}
                          {cut && (
                            <span title={t("table.truncated")} className="ml-1 shrink-0 rounded bg-sunken px-1 text-[10px] text-fg-subtle">
                              …
                            </span>
                          )}
                        </div>
                      );
                    })}
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className={menuContentClass}>
          <ContextMenu.Item className={menuItemClass} disabled={!active} onSelect={copyCell}>
            <Copy className="size-3.5" /> {t("table.copyCell")}
            <span className="ml-auto pl-4 text-xs text-fg-subtle">⌘C</span>
          </ContextMenu.Item>
          <ContextMenu.Item className={menuItemClass} disabled={!active} onSelect={copyRow}>
            <Rows3 className="size-3.5" /> {t("table.copyRow")}
            <span className="ml-auto pl-4 text-xs text-fg-subtle">⇧⌘C</span>
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

interface HeaderCellProps {
  column: ColumnMeta;
  width: number;
  sort: SortSpec | null;
  onSort: () => void;
  onResizeStart: (e: PointerEvent<HTMLSpanElement>) => void;
}

function HeaderCell({ column, width, sort, onSort, onResizeStart }: HeaderCellProps) {
  const Icon = column.primaryKey ? KeyRound : KIND_ICON[column.kind];
  return (
    <div
      role="columnheader"
      aria-sort={sort ? (sort.descending ? "descending" : "ascending") : "none"}
      style={{ width }}
      className="relative flex shrink-0 items-center border-r border-border"
    >
      <button
        type="button"
        onClick={onSort}
        title={`${column.name} · ${column.dataType}`}
        className="flex h-full min-w-0 flex-1 items-center gap-1.5 px-2 text-left text-xs font-semibold text-fg hover:bg-border/50 focus-visible:outline-offset-[-2px]"
      >
        <Icon className={cn("size-3.5 shrink-0", column.primaryKey ? "text-accent-text" : "text-fg-subtle")} />
        <span className="truncate">{column.name}</span>
        {sort && (sort.descending ? <ArrowDown className="size-3 shrink-0 text-accent-text" /> : <ArrowUp className="size-3 shrink-0 text-accent-text" />)}
      </button>
      <span
        aria-hidden
        onPointerDown={onResizeStart}
        className="absolute top-0 -right-1 z-10 h-full w-2 cursor-col-resize after:absolute after:inset-y-1.5 after:left-[3px] after:w-0.5 after:rounded hover:after:bg-accent active:after:bg-accent"
      />
    </div>
  );
}
