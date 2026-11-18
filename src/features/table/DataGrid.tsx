import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ArrowDown,
  ArrowUp,
  Binary,
  Braces,
  Calendar,
  CircleDot,
  CircleSlash,
  ClipboardPaste,
  Copy,
  Funnel,
  Hash,
  KeyRound,
  Lock,
  Maximize2,
  Plus,
  RotateCcw,
  ToggleLeft,
  Trash2,
  Type,
} from "lucide-react";
import { ContextMenu } from "radix-ui";
import { type KeyboardEvent, type PointerEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { menuContentClass, menuDangerClass, menuItemClass, menuSeparatorClass } from "../../components/ui/primitives";
import { clipboard, parseClipboardTable, toClipboardTable } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage } from "../../lib/ipc";
import type { ColumnMeta, RowFilter, RowPage, SortSpec, ValueKind } from "../../lib/types";
import { cellFilter } from "./filter";
import {
  addRows,
  blocker,
  type Cell,
  cellValue,
  clearedValue,
  isEdited,
  isInserted,
  isTruncated,
  pasteMatrix,
  type Pending,
  rowCount,
  setCells,
  setDeleted,
  typedValue,
} from "./pending";
import { nextSort } from "./sort";

const ROW_HEIGHT = 30;
const HEADER_HEIGHT = 34;
const GUTTER = 56;
const MIN_WIDTH = 64;
/** Values longer than this are edited in the cell dialog, not in the cell. */
const LONG_VALUE = 1000;

const KIND_ICON: Record<ValueKind, typeof Hash> = {
  number: Hash,
  text: Type,
  bool: ToggleLeft,
  temporal: Calendar,
  binary: Binary,
  json: Braces,
  other: CircleDot,
};

export interface Selection {
  anchor: Cell;
  focus: Cell;
}

export interface Range {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

export function rangeOf(s: Selection): Range {
  return {
    top: Math.min(s.anchor.r, s.focus.r),
    bottom: Math.max(s.anchor.r, s.focus.r),
    left: Math.min(s.anchor.c, s.focus.c),
    right: Math.max(s.anchor.c, s.focus.c),
  };
}

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
  /** Omit for grids that cannot be re-sorted on the server (query results). */
  onSort?: (sort: SortSpec | null) => void;
  /** Sorting reloads the page, which is not allowed while edits are pending. */
  sortDisabled: boolean;
  /** Names of the columns the page is filtered on; their headers say so. */
  filtered?: string[];
  /** "Filter by this value" on a cell. Follows `sortDisabled`: it reloads the page too. */
  onFilter?: (filter: RowFilter) => void;
  /** Changes with every page loaded from the server (not when a loaded page is amended). */
  loadId?: number;
  /** Show one cell whole, in a dialog: a long or multi-line value, or one cut for preview. */
  onInspect?: (cell: Cell) => void;
  /** `false` renders a plain read-only grid. */
  editable: boolean;
  pending: Pending;
  onChange: (next: Pending) => void;
  selection: Selection | null;
  onSelect: (selection: Selection | null) => void;
  onUndo: () => void;
  onRedo: () => void;
  onSave: () => void;
}

interface Editing extends Cell {
  text: string;
  /** What the editor opened with; an untouched editor changes nothing. */
  initial: string;
}

export function DataGrid(props: Props) {
  const { page, sort, onSort, sortDisabled, filtered, onFilter, loadId, onInspect, editable, pending, onChange, selection, onSelect } = props;
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const signature = page.columns.map((c) => c.name).join("\u0000");
  const [widths, setWidths] = useState(() => initialWidths(page));
  const [editing, setEditing] = useState<Editing | null>(null);

  const rows = rowCount(page, pending);
  const cols = page.columns.length;

  // New table shape: recompute widths. Paging or sorting the same table keeps the user's widths.
  useEffect(() => {
    setWidths(initialWidths(page));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  // A new page of rows: scroll back to the top (the owner resets selection and edits).
  useEffect(() => {
    setEditing(null);
    scrollRef.current?.scrollTo({ top: 0 });
    // With a `loadId`, a page that only learned a cell's whole value keeps its scroll position.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadId ?? page]);

  useEffect(() => {
    const stop = () => (dragging.current = false);
    window.addEventListener("mouseup", stop);
    return () => window.removeEventListener("mouseup", stop);
  }, []);

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
    count: rows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
    scrollPaddingStart: HEADER_HEIGHT,
  });

  // Keep the focused cell in view, however the selection moved.
  const focus = selection?.focus;
  useEffect(() => {
    if (!focus || dragging.current) return;
    virtualizer.scrollToIndex(focus.r);
    const el = scrollRef.current;
    if (!el) return;
    const left = lefts[focus.c] ?? 0;
    const right = left + (widths[focus.c] ?? 0);
    if (left - GUTTER < el.scrollLeft) el.scrollLeft = left - GUTTER;
    else if (right > el.scrollLeft + el.clientWidth) el.scrollLeft = right - el.clientWidth;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus?.r, focus?.c]);

  const range = selection ? rangeOf(selection) : null;
  const selectedRows = range ? Array.from({ length: range.bottom - range.top + 1 }, (_, i) => range.top + i) : [];
  const allSelectedDeleted = selectedRows.length > 0 && selectedRows.every((r) => pending.deletes[r]);
  const oneCellAny = range && range.top === range.bottom && range.left === range.right ? range : null;
  // One stored cell, as the server sent it: the only thing "filter by this value" can mean.
  const oneCell = oneCellAny && !isInserted(page, oneCellAny.top) ? oneCellAny : null;
  const filterForCell =
    oneCell && page.columns[oneCell.left]
      ? cellFilter(page.columns[oneCell.left]!, page.rows[oneCell.top]?.[oneCell.left] ?? null, isTruncated(page, oneCell.top, oneCell.left))
      : null;

  const clampCell = (r: number, c: number): Cell => ({
    r: Math.min(rows - 1, Math.max(0, r)),
    c: Math.min(cols - 1, Math.max(0, c)),
  });
  const moveTo = (cell: Cell, extend = false) =>
    onSelect({ anchor: extend && selection ? selection.anchor : cell, focus: cell });

  // ---- editing ---------------------------------------------------------------------

  const startEdit = (cell: Cell, typed?: string) => {
    // A read-only grid has nothing to edit: a double-click shows the cell whole instead.
    if (!editable) return typed === undefined ? onInspect?.(cell) : undefined;
    const reason = blocker(page, pending, cell.r, cell.c);
    const long = (cellValue(page, pending, cell.r, cell.c)?.length ?? 0) > LONG_VALUE;
    // A value cut for preview, or too long for a cell, is read and edited in the dialog.
    if (onInspect && (reason === "truncated" || (reason === null && long))) return onInspect(cell);
    if (reason) {
      toast.info(t(`table.notEditable.${reason}`), { id: "not-editable", duration: 2200 });
      return;
    }
    const initial = cellValue(page, pending, cell.r, cell.c) ?? "";
    setEditing({ ...cell, text: typed ?? initial, initial: typed === undefined ? initial : "\u0000" });
  };

  const finishEdit = (commit: boolean, move?: [number, number]) => {
    const e = editing;
    if (!e) return;
    setEditing(null);
    if (commit && e.text !== e.initial) {
      onChange(setCells(page, pending, [{ r: e.r, c: e.c, value: typedValue(page.columns[e.c]!, e.text) }]).pending);
    }
    if (move) moveTo(clampCell(e.r + move[0], e.c + move[1]));
    scrollRef.current?.focus({ preventScroll: true });
  };

  const clearSelection = () => {
    if (!range || !editable) return;
    const cells = [];
    for (let r = range.top; r <= range.bottom; r++) {
      for (let c = range.left; c <= range.right; c++) cells.push({ r, c, value: clearedValue(page.columns[c]!) });
    }
    onChange(setCells(page, pending, cells).pending);
  };

  const setNull = () => {
    if (!range || !editable) return;
    const cells = [];
    for (let r = range.top; r <= range.bottom; r++) {
      for (let c = range.left; c <= range.right; c++) if (page.columns[c]!.nullable) cells.push({ r, c, value: null });
    }
    onChange(setCells(page, pending, cells).pending);
  };

  const toggleDeleteRows = () => {
    if (!editable || selectedRows.length === 0) return;
    onChange(setDeleted(page, pending, selectedRows, !allSelectedDeleted));
    // New rows vanish when deleted, so the old selection may point past the end.
    if (selectedRows.some((r) => isInserted(page, r))) onSelect(null);
  };

  const appendRow = () => {
    if (!editable) return;
    onChange(addRows(page, pending));
    const first = page.columns.findIndex((c) => c.editable);
    moveTo({ r: rows, c: Math.max(0, first) });
  };

  // ---- clipboard -------------------------------------------------------------------

  const copy = () => {
    if (!range) return;
    const matrix = [];
    for (let r = range.top; r <= range.bottom; r++) {
      const line = [];
      for (let c = range.left; c <= range.right; c++) line.push(cellValue(page, pending, r, c) ?? "");
      matrix.push(line);
    }
    clipboard
      .write(toClipboardTable(matrix))
      .then(() => toast.success(t("common.copied"), { id: "copied", duration: 1200 }))
      .catch((e: unknown) => toast.error(errorMessage(e)));
  };

  const paste = async () => {
    if (!range || !editable) return;
    let text: string;
    try {
      text = await clipboard.read();
    } catch {
      return; // an empty or non-text clipboard is not an error worth a message
    }
    const matrix = parseClipboardTable(text);
    if (matrix.length === 0) return;
    const result = pasteMatrix(page, pending, matrix, { r: range.top, c: range.left }, { r: range.bottom, c: range.right });
    onChange(result.pending);
    const single = matrix.length === 1 && matrix[0]!.length === 1;
    onSelect({
      anchor: { r: range.top, c: range.left },
      focus: {
        r: single ? range.bottom : range.top + matrix.length - 1,
        c: Math.min(cols - 1, single ? range.right : range.left + matrix[0]!.length - 1),
      },
    });
    const parts = [t("table.pasted", { count: result.applied })];
    if (result.addedRows) parts.push(t("table.pastedRows", { count: result.addedRows }));
    if (result.skipped) parts.push(t("table.pastedSkipped", { count: result.skipped }));
    if (result.droppedColumns) parts.push(t("table.pastedDropped", { count: result.droppedColumns }));
    (result.skipped || result.droppedColumns ? toast.warning : toast.success)(parts.join(" · "), { id: "paste" });
  };

  // ---- keyboard --------------------------------------------------------------------

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (editing || !rows || !cols) return;
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();

    if (mod) {
      const commands: Record<string, () => void> = {
        c: copy,
        v: () => void paste(),
        z: e.shiftKey ? props.onRedo : props.onUndo,
        y: props.onRedo,
        s: props.onSave,
        a: () => onSelect({ anchor: { r: 0, c: 0 }, focus: { r: rows - 1, c: cols - 1 } }),
        backspace: toggleDeleteRows,
      };
      const command = commands[key];
      if (!command) return;
      e.preventDefault();
      command();
      return;
    }

    const cur = selection?.focus ?? { r: 0, c: 0 };
    const visible = Math.max(1, Math.floor(((scrollRef.current?.clientHeight ?? 300) - HEADER_HEIGHT) / ROW_HEIGHT) - 1);
    const moves: Record<string, [number, number]> = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
      PageUp: [-visible, 0],
      PageDown: [visible, 0],
      Home: [0, -cols],
      End: [0, cols],
      Tab: [0, e.shiftKey ? -1 : 1],
    };
    const move = moves[e.key];
    if (move) {
      e.preventDefault();
      moveTo(selection ? clampCell(cur.r + move[0], cur.c + move[1]) : cur, e.shiftKey && e.key !== "Tab");
    } else if (e.key === "Enter" || e.key === "F2") {
      e.preventDefault();
      if (selection && editable) startEdit(cur);
      else if (selection) moveTo(clampCell(cur.r + 1, cur.c));
    } else if (e.key === "Backspace" || e.key === "Delete") {
      e.preventDefault();
      clearSelection();
    } else if (e.key === "Escape") {
      if (selection) onSelect({ anchor: selection.focus, focus: selection.focus });
    } else if (e.key.length === 1 && !e.altKey && selection && editable) {
      // Typing replaces the cell, as in a spreadsheet.
      e.preventDefault();
      startEdit(cur, e.key);
    }
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

  const inRange = (r: number, c: number) =>
    !!range && r >= range.top && r <= range.bottom && c >= range.left && c <= range.right;

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
        <div
          ref={scrollRef}
          role="grid"
          tabIndex={0}
          aria-rowcount={rows}
          aria-colcount={cols}
          aria-readonly={!editable}
          onKeyDown={onKeyDown}
          className="h-full overflow-auto outline-none"
        >
          <div style={{ width: totalWidth, minWidth: "100%" }}>
            <div role="row" style={{ height: HEADER_HEIGHT }} className="sticky top-0 z-30 flex border-b border-border bg-sunken">
              <div style={{ width: GUTTER }} className="sticky left-0 z-10 shrink-0 border-r border-border bg-sunken" />
              {page.columns.map((col, c) => (
                <HeaderCell
                  key={c}
                  column={col}
                  width={widths[c] ?? MIN_WIDTH}
                  sort={sort?.column === col.name ? sort : null}
                  filtered={!!filtered?.includes(col.name)}
                  sortDisabled={sortDisabled || !onSort}
                  sortHint={onSort ? t("table.dirtyLocked") : undefined}
                  showLock={editable && !col.editable}
                  onSort={() => onSort?.(nextSort(sort, col.name))}
                  onResizeStart={(e) => startResize(e, c)}
                />
              ))}
              <div className="flex-1" />
            </div>

            <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
              {virtualizer.getVirtualItems().map((v) => {
                const r = v.index;
                const inserted = isInserted(page, r);
                const deleted = !!pending.deletes[r];
                const rowSelected = !!range && r >= range.top && r <= range.bottom;
                const editingHere = editing?.r === r;
                return (
                  <div
                    key={r}
                    role="row"
                    aria-rowindex={r + 1}
                    data-row-state={deleted ? "deleted" : inserted ? "inserted" : undefined}
                    style={{ height: ROW_HEIGHT, transform: `translateY(${v.start}px)` }}
                    className={cn(
                      "group absolute top-0 left-0 flex w-full border-b border-border",
                      deleted && "bg-danger-soft/60 text-fg-muted line-through decoration-danger/50",
                      inserted && "bg-ok-soft/50",
                      !deleted && !inserted && "hover:bg-sunken/70",
                      editingHere && "z-20",
                    )}
                  >
                    <div
                      style={{ width: GUTTER }}
                      onMouseDown={(e) => {
                        const anchor = e.shiftKey && selection ? { r: selection.anchor.r, c: 0 } : { r, c: 0 };
                        onSelect({ anchor, focus: { r, c: cols - 1 } });
                      }}
                      className={cn(
                        "sticky left-0 z-10 flex shrink-0 items-center justify-end border-r border-border pr-2 font-mono text-[11px] tabular-nums no-underline",
                        rowSelected ? "bg-accent-soft font-semibold text-accent-text" : "bg-surface text-fg-subtle group-hover:bg-sunken",
                        inserted && !rowSelected && "bg-ok-soft text-ok",
                        deleted && !rowSelected && "bg-danger-soft text-danger",
                      )}
                    >
                      {inserted ? <Plus aria-label={t("table.newRow")} className="size-3" /> : page.offset + r + 1}
                    </div>
                    {page.columns.map((col, c) => {
                      const value = cellValue(page, pending, r, c);
                      const isActive = focus?.r === r && focus.c === c;
                      const selected = inRange(r, c);
                      const edited = !inserted && !deleted && isEdited(pending, r, c);
                      const cut = !inserted && !edited && isTruncated(page, r, c);
                      const isEditing = editingHere && editing.c === c;
                      return (
                        <div
                          key={c}
                          role="gridcell"
                          aria-selected={selected}
                          data-edited={edited || undefined}
                          title={edited ? t("table.was", { value: page.rows[r]![c] ?? t("table.null") }) : undefined}
                          onMouseDown={(e) => {
                            if (e.button !== 0) return;
                            dragging.current = true;
                            moveTo({ r, c }, e.shiftKey);
                          }}
                          onMouseEnter={() => dragging.current && moveTo({ r, c }, true)}
                          onDoubleClick={() => startEdit({ r, c })}
                          onContextMenu={() => !selected && moveTo({ r, c })}
                          style={{ width: widths[c] ?? MIN_WIDTH }}
                          className={cn(
                            "relative flex shrink-0 items-center border-r border-border px-2 text-[12.5px]",
                            !isEditing && "overflow-hidden",
                            col.kind === "number" && "justify-end font-mono tabular-nums",
                            (col.kind === "binary" || col.kind === "json") && "font-mono text-fg-muted",
                            selected && !edited && "bg-accent-soft/70",
                            edited && "bg-warn-soft",
                            edited && selected && "ring-1 ring-accent/50 ring-inset",
                            isActive && "ring-2 ring-accent ring-inset",
                          )}
                        >
                          {isEditing ? (
                            <CellEditor
                              editing={editing}
                              numeric={col.kind === "number"}
                              onText={(text) => setEditing({ ...editing, text })}
                              onDone={finishEdit}
                            />
                          ) : value === undefined ? (
                            <span className="truncate text-[11px] text-fg-subtle italic">{col.default ?? t("table.defaultValue")}</span>
                          ) : value === null ? (
                            <span className="text-[11px] tracking-wide text-fg-subtle italic">{t("table.null")}</span>
                          ) : (
                            <span className="truncate whitespace-pre">{value}</span>
                          )}
                          {cut && (
                            <span title={t(onInspect ? "cell.truncatedHint" : "table.truncated")} className="ml-1 shrink-0 rounded bg-sunken px-1 text-[10px] text-fg-subtle">
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
          <ContextMenu.Item className={menuItemClass} disabled={!range} onSelect={copy}>
            <Copy className="size-3.5" /> {t("common.copy")}
            <Shortcut>⌘C</Shortcut>
          </ContextMenu.Item>
          {onInspect && (
            <ContextMenu.Item className={menuItemClass} disabled={!oneCellAny} onSelect={() => oneCellAny && onInspect({ r: oneCellAny.top, c: oneCellAny.left })}>
              <Maximize2 className="size-3.5" /> {t("cell.view")}
            </ContextMenu.Item>
          )}
          {onFilter && (
            <ContextMenu.Item
              className={menuItemClass}
              disabled={!filterForCell || sortDisabled}
              onSelect={() => filterForCell && onFilter(filterForCell)}
            >
              <Funnel className="size-3.5" /> {t("filter.byValue")}
            </ContextMenu.Item>
          )}
          {editable && (
            <>
              <ContextMenu.Item className={menuItemClass} disabled={!range} onSelect={() => void paste()}>
                <ClipboardPaste className="size-3.5" /> {t("table.paste")}
                <Shortcut>⌘V</Shortcut>
              </ContextMenu.Item>
              <ContextMenu.Item className={menuItemClass} disabled={!range} onSelect={setNull}>
                <CircleSlash className="size-3.5" /> {t("table.setNull")}
              </ContextMenu.Item>
              <ContextMenu.Separator className={menuSeparatorClass} />
              <ContextMenu.Item className={menuItemClass} onSelect={appendRow}>
                <Plus className="size-3.5" /> {t("table.addRow")}
              </ContextMenu.Item>
              {allSelectedDeleted ? (
                <ContextMenu.Item className={menuItemClass} onSelect={toggleDeleteRows}>
                  <RotateCcw className="size-3.5" /> {t("table.restoreRows", { count: selectedRows.length })}
                </ContextMenu.Item>
              ) : (
                <ContextMenu.Item className={cn(menuItemClass, menuDangerClass)} disabled={!range} onSelect={toggleDeleteRows}>
                  <Trash2 className="size-3.5" /> {t("table.deleteRows", { count: selectedRows.length })}
                  <Shortcut>⌘⌫</Shortcut>
                </ContextMenu.Item>
              )}
            </>
          )}
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

function Shortcut({ children }: { children: string }) {
  return <span className="ml-auto pl-4 text-xs text-fg-subtle">{children}</span>;
}

interface CellEditorProps {
  editing: Editing;
  numeric: boolean;
  onText: (text: string) => void;
  onDone: (commit: boolean, move?: [number, number]) => void;
}

/** In-cell editor. Values with line breaks get a taller box; Alt+Enter adds a line. */
function CellEditor({ editing, numeric, onText, onDone }: CellEditorProps) {
  const multiline = editing.text.includes("\n");
  const ref = useRef<HTMLTextAreaElement & HTMLInputElement>(null);

  // Put the caret at the end once, instead of selecting everything (typing has already begun).
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, [multiline]);

  // An edit ends once. Ending it hands the focus back to the grid, which blurs this input:
  // without the guard that blur would commit what Escape had just cancelled, and record
  // an edit ended with Enter or Tab a second time (so the first undo seemed to do nothing).
  const ended = useRef(false);
  const end = (commit: boolean, move?: [number, number]) => {
    if (ended.current) return;
    ended.current = true;
    onDone(commit, move);
  };

  const shared = {
    ref,
    value: editing.text,
    spellCheck: false,
    "aria-label": "Edit cell",
    onChange: (e: { target: { value: string } }) => onText(e.target.value),
    onBlur: () => end(true),
    onMouseDown: (e: { stopPropagation: () => void }) => e.stopPropagation(),
    onDoubleClick: (e: { stopPropagation: () => void }) => e.stopPropagation(),
    onKeyDown: (e: KeyboardEvent<HTMLElement>) => {
      e.stopPropagation(); // the grid's own shortcuts must not fire while typing
      if (e.key === "Escape") end(false);
      else if (e.key === "Tab") {
        e.preventDefault();
        end(true, [0, e.shiftKey ? -1 : 1]);
      } else if (e.key === "Enter" && e.altKey) {
        e.preventDefault();
        const el = e.currentTarget as HTMLTextAreaElement;
        onText(`${el.value.slice(0, el.selectionStart)}\n${el.value.slice(el.selectionEnd)}`);
      } else if (e.key === "Enter") {
        e.preventDefault();
        end(true, [e.shiftKey ? -1 : 1, 0]);
      }
    },
  };
  const look = "absolute top-0 left-0 z-30 bg-surface px-2 text-[12.5px] text-fg outline-none ring-2 ring-accent ring-inset";

  return multiline ? (
    <textarea {...shared} className={cn(look, "h-28 w-full min-w-64 resize-none py-1.5 leading-snug shadow-pop")} />
  ) : (
    <input {...shared} className={cn(look, "h-full w-full", numeric && "text-right font-mono tabular-nums")} />
  );
}

interface HeaderCellProps {
  column: ColumnMeta;
  width: number;
  sort: SortSpec | null;
  filtered: boolean;
  sortDisabled: boolean;
  sortHint?: string;
  showLock: boolean;
  onSort: () => void;
  onResizeStart: (e: PointerEvent<HTMLSpanElement>) => void;
}

function HeaderCell({ column, width, sort, filtered, sortDisabled, sortHint, showLock, onSort, onResizeStart }: HeaderCellProps) {
  const { t } = useTranslation();
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
        disabled={sortDisabled}
        title={sortDisabled && sortHint ? sortHint : `${column.name} · ${column.dataType}`}
        className="flex h-full min-w-0 flex-1 items-center gap-1.5 px-2 text-left text-xs font-semibold text-fg enabled:hover:bg-border/50 focus-visible:outline-offset-[-2px]"
      >
        <Icon className={cn("size-3.5 shrink-0", column.primaryKey ? "text-accent-text" : "text-fg-subtle")} />
        <span className="truncate">{column.name}</span>
        {showLock && <Lock aria-label={t("table.notEditable.column")} className="size-3 shrink-0 text-fg-subtle" />}
        {filtered && <Funnel aria-label={t("filter.filteredColumn")} className="size-3 shrink-0 text-accent-text" />}
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
