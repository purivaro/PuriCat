import {
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  ClipboardPaste,
  Eye,
  Funnel,
  KeyRound,
  Lock,
  Pencil,
  Plug,
  Plus,
  Redo2,
  RefreshCw,
  RotateCcw,
  Table2,
  Trash2,
  TriangleAlert,
  Undo2,
  Unplug,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, IconButton, Segmented, Spinner, StateMessage, Tooltip } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc, toAppError } from "../../lib/ipc";
import type { ApplyResult, RowFilter, RowPage, SortSpec } from "../../lib/types";
import { sessionOf, useConnections } from "../../stores/connections";
import { type TableTab, useWorkspace } from "../../stores/workspace";
import { CellDialog } from "./CellDialog";
import { DataGrid, rangeOf, type Selection } from "./DataGrid";
import { sameFilters } from "./filter";
import { FilterBar } from "./FilterBar";
import {
  addRows,
  type Cell,
  canRedo,
  cellIndex,
  canUndo,
  current,
  EMPTY,
  isInserted,
  NO_HISTORY,
  type Pending,
  record,
  redo,
  setCells,
  setDeleted,
  summarize,
  toChangeSet,
  undo,
  withFull,
} from "./pending";
import { ReviewDialog } from "./ReviewDialog";
import { SmartPasteDialog } from "./SmartPasteDialog";

const PAGE_SIZES = [100, 200, 500, 1000];

/** The exact number of rows, which is only counted when asked for. */
type Count =
  | { status: "idle" }
  | { status: "counting" }
  | { status: "done"; total: number }
  | { status: "error"; message: string };

type Load =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready" };

export function TableTabView({ tab, active }: { tab: TableTab; active: boolean }) {
  const { t, i18n } = useTranslation();
  const connection = useConnections((s) => s.connections.find((c) => c.id === tab.connectionId));
  const session = useConnections((s) => sessionOf(s.sessions, tab.connectionId));
  const connected = session.status === "connected";

  const [view, setView] = useState<"data" | "structure">("data");
  const [limit, setLimit] = useState(200);
  const [offset, setOffset] = useState(0);
  const [sort, setSort] = useState<SortSpec | null>(null);
  const [filters, setFilters] = useState<RowFilter[]>([]);
  const [filterOpen, setFilterOpen] = useState(false);
  const [filterFocus, setFilterFocus] = useState(0);
  const [loaded, setPage] = useState<RowPage | null>(null);
  // Whole values learned for cells the page shows shortened, by `cellIndex`. With them such
  // a cell can be edited: its original is complete.
  const [full, setFull] = useState<Record<number, string | null>>({});
  const page = useMemo(() => loaded && withFull(loaded, full), [loaded, full]);
  const [loadId, setLoadId] = useState(0);
  const [inspect, setInspect] = useState<Cell | null>(null);
  const [load, setLoad] = useState<Load>({ status: "idle" });
  const [reloads, setReloads] = useState(0);
  // The table was changed outside this tab (emptied from the explorer's menu).
  const outside = useWorkspace((s) => s.reloads[tab.id] ?? 0);
  const request = useRef(0);

  // Staged edits for the loaded page, with undo history. Nothing reaches the database until Save.
  const [history, setHistory] = useState(NO_HISTORY);
  const [selection, setSelection] = useState<Selection | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [smartPaste, setSmartPaste] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const pending = current(history);
  const summary = summarize(pending);
  const dirty = summary.rows > 0;

  // Load when the tab is first shown and whenever paging, sorting, the filter or the session changes.
  // Hidden tabs do not query: a restored workspace must not hit every server at once.
  const wanted = connected && (active || page !== null);
  useEffect(() => {
    if (!wanted) return;
    const id = ++request.current;
    setLoad({ status: "loading" });
    ipc
      .fetchRows(tab.connectionId, tab.table, { offset, limit, sort, filters })
      .then((rows) => {
        if (id !== request.current) return; // a newer request superseded this one
        setPage(rows);
        setFull({});
        setLoadId((n) => n + 1);
        setInspect(null);
        // Row indexes belong to the old page: staged edits and the selection end with it.
        setHistory(NO_HISTORY);
        setSelection(null);
        setSaveError(null);
        setLoad({ status: "ready" });
      })
      .catch((e: unknown) => {
        if (id === request.current) setLoad({ status: "error", message: errorMessage(e) });
      });
    // `page` is intentionally not a dependency: it only gates the first load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wanted, tab.connectionId, tab.table, offset, limit, sort, filters, reloads, outside]);

  // A closed session invalidates what is on screen.
  useEffect(() => {
    if (!connected) {
      request.current++;
      setPage(null);
      setFull({});
      setInspect(null);
      setHistory(NO_HISTORY);
      setSelection(null);
      setLoad({ status: "idle" });
    }
  }, [connected]);

  // The tab strip shows a dot, and closing asks before discarding.
  useEffect(() => {
    useWorkspace.getState().setDirty(tab.id, dirty);
  }, [tab.id, dirty]);
  useEffect(() => () => useWorkspace.getState().setDirty(tab.id, false), [tab.id]);

  // The exact count belongs to the table and the filter it was asked for. Paging and sorting
  // show the same rows, so they keep it; a new filter, a reload or a save does not.
  const [count, setCount] = useState<Count>({ status: "idle" });
  const countJob = useRef<string | null>(null);
  useEffect(() => {
    setCount({ status: "idle" });
    return () => {
      // Whatever is still being counted is for rows that are no longer shown.
      if (countJob.current) void ipc.cancelJob(countJob.current);
      countJob.current = null;
    };
  }, [tab.connectionId, tab.table, filters, reloads, outside, connected]);
  const countRows = () => {
    const job = crypto.randomUUID();
    countJob.current = job;
    setCount({ status: "counting" });
    ipc
      .countRows(tab.connectionId, tab.table, filters, job)
      .then((result) => {
        if (countJob.current !== job) return;
        countJob.current = null;
        setCount({ status: "done", total: result.count });
      })
      .catch((e: unknown) => {
        if (countJob.current !== job) return;
        countJob.current = null;
        // Stopped by the user: back to where it was, without an error.
        setCount(toAppError(e).kind === "conflict" ? { status: "idle" } : { status: "error", message: errorMessage(e) });
      });
  };

  const refresh = useCallback(() => setReloads((n) => n + 1), []);
  const changeSort = (next: SortSpec | null) => {
    setSort(next);
    setOffset(0);
  };
  const changeFilters = useCallback(
    (next: RowFilter[]) => {
      if (sameFilters(next, filters)) return;
      setFilters(next);
      setOffset(0);
    },
    [filters],
  );
  const openFilter = useCallback(() => {
    setView("data");
    setFilterOpen(true);
    setFilterFocus((n) => n + 1);
  }, []);

  const commit = useCallback((next: Pending) => {
    setSaveError(null);
    setHistory((h) => record(h, next));
  }, []);

  const saved = useCallback(
    (result: ApplyResult) => {
      setReviewing(false);
      setSaving(false);
      // The edits are in the database now: drop them at once, so a second Save (or ⌘S) before
      // the reload returns cannot apply the same inserts again.
      setHistory(NO_HISTORY);
      setSelection(null);
      toast.success(t("table.saved", { ...result }));
      refresh(); // shows what the server actually stored
    },
    [refresh, t],
  );

  const isTable = tab.objectKind === "table";
  const serverReadOnly = connected && session.info.readOnly;
  const readOnlyReason = !isTable
    ? "view"
    : connection?.readOnly || serverReadOnly
      ? "session"
      : page && page.primaryKey.length === 0
        ? "noKey"
        : null;
  const editable = !!page && readOnlyReason === null;

  const save = useCallback(
    (forceReview = false) => {
      // While the review dialog is open, it owns the save.
      if (!page || !connection || !dirty || saving || reviewing) return;
      // Bulk, destructive and production saves always pass through the review step.
      const needsReview =
        forceReview ||
        connection.environment === "production" ||
        summary.deleted > 0 ||
        summary.rows > connection.bulkReviewRows;
      if (needsReview) return setReviewing(true);
      setSaving(true);
      setSaveError(null);
      ipc
        .applyChanges(connection.id, tab.table, toChangeSet(page, pending))
        .then(saved)
        .catch((e: unknown) => {
          setSaving(false);
          setSaveError(errorMessage(e));
        });
    },
    [page, connection, dirty, saving, reviewing, summary.deleted, summary.rows, tab.table, pending, saved],
  );

  // Tab-level shortcuts: ⌘S saves, ⌘R reloads (unless that would drop edits), ⌘F filters.
  useEffect(() => {
    if (!active || !connected) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.defaultPrevented) return;
      const key = e.key.toLowerCase();
      if (key === "r") {
        e.preventDefault();
        if (!dirty) refresh();
      } else if (key === "s") {
        e.preventDefault();
        save();
      } else if (key === "f" && !e.shiftKey && !e.altKey) {
        e.preventDefault();
        openFilter();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, connected, dirty, refresh, save, openFilter]);

  if (!connection) {
    return (
      <StateMessage
        tone="danger"
        icon={<CircleAlert className="size-5" />}
        title={t("table.missingConnection")}
        body={t("table.missingConnectionBody")}
      />
    );
  }

  const number = new Intl.NumberFormat(i18n.language);
  const Icon = isTable ? Table2 : Eye;
  const kindLabel = t(isTable ? "table.tableKind" : `table.${tab.objectKind}`);
  const path = [tab.table.database, tab.table.schema, tab.table.name].filter(Boolean);
  const busy = load.status === "loading";
  const locked = dirty || saving;
  const lockTitle = (label: string) => (dirty ? t("table.dirtyLocked") : label);

  const range = selection ? rangeOf(selection) : null;
  const selectedRows = range ? Array.from({ length: range.bottom - range.top + 1 }, (_, i) => range.top + i) : [];
  const allDeleted = selectedRows.length > 0 && selectedRows.every((r) => pending.deletes[r]);

  const addRow = () => {
    if (!page) return;
    commit(addRows(page, pending));
    const first = Math.max(
      0,
      page.columns.findIndex((c) => c.editable),
    );
    const r = page.rows.length + pending.inserts.length;
    setSelection({ anchor: { r, c: first }, focus: { r, c: first } });
    setView("data");
  };
  const toggleDelete = () => {
    if (!page || selectedRows.length === 0) return;
    commit(setDeleted(page, pending, selectedRows, !allDeleted));
    if (selectedRows.some((r) => isInserted(page, r))) setSelection(null);
  };

  return (
    <div className="flex h-full flex-col">
      {/* Identity: what this is, and exactly which server it comes from. */}
      {/* A container: when the tab is narrow (the AI panel open), the buttons keep their icons and give the name its room. */}
      <div className="@container flex shrink-0 items-center gap-3 border-b border-border px-4 py-2.5">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent-text">
          <Icon className="size-[18px]" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h1 data-selectable className="truncate text-[15px] leading-tight font-semibold">
              {tab.table.name}
            </h1>
            <span className="shrink-0 text-xs text-fg-subtle">{kindLabel}</span>
          </div>
          <div className="flex items-center gap-1 truncate text-xs text-fg-muted">
            {path.map((part, i) => (
              <span key={i} className="flex min-w-0 items-center gap-1">
                {i > 0 && <ChevronRight aria-hidden className="size-3 shrink-0 text-fg-subtle" />}
                <span className="truncate">{part}</span>
              </span>
            ))}
          </div>
        </div>

        {page && (
          <Tooltip label={`${t("filter.open")} (⌘F)`}>
            <Button
              size="sm"
              data-filter-toggle
              aria-label={t("filter.open")}
              aria-pressed={filterOpen}
              icon={<Funnel className="size-3.5" />}
              onClick={() => (filterOpen && view === "data" ? setFilterOpen(false) : openFilter())}
            >
              <span className="hidden @3xl:inline">{t("filter.title")}</span>
              {filters.length > 0 && (
                <span data-filter-count className="flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-[10px] font-semibold text-accent-fg tabular-nums">
                  {filters.length}
                </span>
              )}
            </Button>
          </Tooltip>
        )}
        {editable && (
          <div className="flex shrink-0 items-center gap-1">
            <Tooltip label={dirty ? t("table.dirtyLocked") : t("smart.openHint")}>
              <span>
                <Button
                  size="sm"
                  aria-label={t("smart.open")}
                  disabled={dirty || saving}
                  icon={<ClipboardPaste className="size-3.5" />}
                  onClick={() => setSmartPaste(true)}
                >
                  <span className="hidden @3xl:inline">{t("smart.open")}</span>
                </Button>
              </span>
            </Tooltip>
            <Button size="sm" variant="primary" disabled={saving} icon={<Plus className="size-3.5" />} onClick={addRow}>
              {t("table.addRow")}
            </Button>
            <IconButton
              label={allDeleted ? t("table.restoreSelected") : t("table.deleteSelected")}
              disabled={selectedRows.length === 0 || view !== "data" || saving}
              onClick={toggleDelete}
            >
              {allDeleted ? <RotateCcw className="size-3.5" /> : <Trash2 className="size-3.5" />}
            </IconButton>
          </div>
        )}

        {page &&
          (editable ? (
            <span data-edit-state="editable" className="flex h-6 shrink-0 items-center gap-1 rounded-md bg-ok-soft px-2 text-[11px] font-medium text-ok">
              <Pencil className="size-3" /> {t("table.editableBadge")}
            </span>
          ) : (
            <Tooltip label={t(`table.readOnlyReason.${readOnlyReason ?? "view"}`)}>
              <span data-edit-state="read-only" className="flex h-6 shrink-0 items-center gap-1 rounded-md bg-sunken px-2 text-[11px] font-medium text-fg-muted">
                <Lock className="size-3" /> {t("table.readOnlyBadge")}
              </span>
            </Tooltip>
          ))}
        <div className="flex h-7 max-w-56 shrink-0 items-center gap-1.5 rounded-lg border border-border px-1.5 text-xs">
          <EngineTag engine={connection.engine} />
          <span className="truncate font-medium">{connection.name}</span>
          <EnvBadge environment={connection.environment} />
        </div>
      </div>

      {filterOpen && connected && page && view === "data" && (
        <FilterBar
          columns={page.columns}
          applied={filters}
          onApply={changeFilters}
          onClose={() => setFilterOpen(false)}
          locked={locked}
          suggested={selection ? (page.columns[selection.focus.c]?.name ?? null) : null}
          focusSignal={filterFocus}
        />
      )}

      <div className="relative min-h-0 flex-1">
        {!connected ? (
          <StateMessage
            tone={session.status === "error" ? "danger" : "neutral"}
            icon={session.status === "error" ? <CircleAlert className="size-5" /> : <Unplug className="size-5" />}
            title={session.status === "error" ? t("explorer.failed") : t("table.notConnected")}
            body={session.status === "error" ? session.message : t("table.notConnectedBody", { name: connection.name })}
            action={
              <Button
                variant="primary"
                loading={session.status === "connecting"}
                icon={<Plug className="size-3.5" />}
                onClick={() => void useConnections.getState().connect(connection.id)}
              >
                {session.status === "connecting" ? t("explorer.connecting") : t("explorer.connect")}
              </Button>
            }
          />
        ) : load.status === "error" ? (
          <StateMessage
            tone="danger"
            icon={<CircleAlert className="size-5" />}
            title={t("table.loadFailed")}
            body={load.message}
            action={
              <div className="flex items-center gap-2">
                <Button icon={<RefreshCw className="size-3.5" />} onClick={refresh}>
                  {t("common.retry")}
                </Button>
                {filters.length > 0 && <Button onClick={() => changeFilters([])}>{t("filter.clear")}</Button>}
              </div>
            }
          />
        ) : !page ? (
          <div className="flex h-full items-center justify-center gap-2 text-fg-muted">
            <Spinner /> {t("common.loading")}
          </div>
        ) : view === "structure" ? (
          <Structure page={page} />
        ) : (
          <>
            <DataGrid
              page={page}
              sort={sort}
              onSort={changeSort}
              sortDisabled={locked}
              loadId={loadId}
              onInspect={setInspect}
              filtered={filters.map((f) => f.column)}
              onFilter={(f) => {
                changeFilters([...filters.filter((x) => !sameFilters([x], [f])), f]);
                setFilterOpen(true);
              }}
              editable={editable && !saving}
              pending={pending}
              onChange={commit}
              selection={selection}
              onSelect={setSelection}
              onUndo={() => setHistory(undo)}
              onRedo={() => setHistory(redo)}
              onSave={() => save()}
            />
            {page.rows.length === 0 &&
              pending.inserts.length === 0 &&
              (filters.length > 0 ? (
                <div data-filter-empty className="absolute inset-x-0 top-24 flex flex-col items-center gap-3 text-center">
                  <div>
                    <div className="text-[13px] font-medium">{t("filter.noMatch")}</div>
                    <div className="mt-0.5 text-xs text-fg-muted">{t("filter.noMatchBody")}</div>
                  </div>
                  <Button size="sm" disabled={busy} onClick={() => changeFilters([])}>
                    {t("filter.clear")}
                  </Button>
                </div>
              ) : (
                <div className="pointer-events-none absolute inset-x-0 top-24 text-center text-[13px] text-fg-muted">{t("table.empty")}</div>
              ))}
          </>
        )}
        {busy && page && <div aria-hidden className="animate-fade absolute inset-x-0 top-0 h-0.5 animate-pulse bg-accent" />}
      </div>

      {/* Unsaved changes: always visible, with the way out in both directions. */}
      {dirty && (
        <div data-dirty-bar className="shrink-0 border-t border-accent/30 bg-accent-soft">
          <div className="flex min-h-11 items-center gap-2 px-3 py-1.5">
            <span className="size-2 shrink-0 rounded-full bg-accent" />
            <span className="shrink-0 text-[13px] font-semibold">{t("table.unsaved")}</span>
            <span data-dirty-summary className="min-w-0 flex-1 truncate text-xs text-fg-muted">
              {[
                summary.editedCells > 0 && t("table.changedCells", { count: summary.editedCells }),
                summary.inserted > 0 && t("table.changedInserted", { count: summary.inserted }),
                summary.deleted > 0 && t("table.changedDeleted", { count: summary.deleted }),
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
            <IconButton label={`${t("table.undo")} (⌘Z)`} disabled={!canUndo(history) || saving} onClick={() => setHistory(undo)}>
              <Undo2 className="size-3.5" />
            </IconButton>
            <IconButton label={`${t("table.redo")} (⇧⌘Z)`} disabled={!canRedo(history) || saving} onClick={() => setHistory(redo)}>
              <Redo2 className="size-3.5" />
            </IconButton>
            <Button size="sm" variant="ghost" disabled={saving} onClick={() => commit(EMPTY)}>
              {t("table.discard")}
            </Button>
            <Button size="sm" disabled={saving} onClick={() => save(true)}>
              {t("table.review")}
            </Button>
            <Button size="sm" variant="primary" loading={saving} onClick={() => save()}>
              {saving ? t("table.saving") : t("table.save")}
              {!saving && <kbd className="ml-0.5 font-sans text-[11px] opacity-80">⌘S</kbd>}
            </Button>
          </div>
          {saveError && (
            <div role="alert" data-save-error className="flex items-start gap-2 border-t border-danger/20 bg-danger-soft px-3 py-2 text-[13px] text-danger">
              <CircleAlert className="mt-0.5 size-4 shrink-0" />
              <span data-selectable className="min-w-0 flex-1 break-words">
                {saveError}
              </span>
            </div>
          )}
        </div>
      )}

      {/* Footer: view switch, what is on screen, paging. */}
      <div className="flex h-11 shrink-0 items-center gap-3 border-t border-border bg-bg/60 px-3">
        <Segmented
          label={t("table.data")}
          value={view}
          onChange={setView}
          options={[
            { value: "data", label: t("table.data") },
            { value: "structure", label: t("table.structure") },
          ]}
        />

        <div data-table-status className="flex min-w-0 flex-1 items-center gap-2 truncate text-xs text-fg-muted">
          {page && (
            <>
              <span className="shrink-0 font-medium text-fg tabular-nums">
                {page.rows.length === 0
                  ? t("table.noRows")
                  : t("table.rows", {
                      from: number.format(page.offset + 1),
                      to: number.format(page.offset + page.rows.length),
                    })}
              </span>
              {filters.length > 0 && (
                <span data-filter-status className="shrink-0 font-medium text-accent-text">
                  · {t("filter.applied", { count: filters.length })}
                </span>
              )}
              {count.status === "done" && (
                <span data-row-count className="shrink-0 font-medium text-fg tabular-nums">
                  · {t(filters.length > 0 ? "filter.exact" : "table.exact", { total: number.format(count.total) })}
                </span>
              )}
              {/* An exact total replaces the estimate; an exact number of matches goes beside it. */}
              {page.estimatedRows != null && !(count.status === "done" && filters.length === 0) && (
                <span className="shrink-0">
                  · {t(filters.length > 0 ? "filter.estimatedTable" : "table.estimated", { total: number.format(page.estimatedRows) })}
                </span>
              )}
              {/* Everything on one page: the row range already is the count. */}
              {(page.hasMore || page.offset > 0) && count.status !== "done" && (
                <span className="flex shrink-0 items-center gap-1.5">
                  ·
                  {count.status === "counting" ? (
                    <>
                      <Spinner className="size-3" /> {t("table.counting")}
                      <button
                        type="button"
                        data-count-stop
                        className="rounded-md px-1.5 py-0.5 font-medium text-fg hover:bg-sunken"
                        onClick={() => countJob.current && void ipc.cancelJob(countJob.current)}
                      >
                        {t("table.countStop")}
                      </button>
                    </>
                  ) : (
                    <Tooltip label={t("table.countHint")}>
                      <button
                        type="button"
                        data-count-rows
                        disabled={!connected}
                        className="rounded-md px-1.5 py-0.5 font-medium text-accent-text hover:bg-accent-soft disabled:opacity-50"
                        onClick={countRows}
                      >
                        {t(filters.length > 0 ? "filter.count" : "table.count")}
                      </button>
                    </Tooltip>
                  )}
                  {count.status === "error" && (
                    <span role="alert" data-count-error title={count.message} className="max-w-56 truncate text-danger">
                      {t("table.countFailed", { message: count.message })}
                    </span>
                  )}
                </span>
              )}
              <span className="shrink-0 tabular-nums">· {t("table.elapsed", { ms: page.elapsedMs })}</span>
              {page.orderedBy.length > 0 ? (
                <span className="truncate">· {t("table.orderedBy", { columns: page.orderedBy.join(", ") })}</span>
              ) : (
                <span className="flex min-w-0 items-center gap-1 text-warn">
                  <TriangleAlert className="size-3.5 shrink-0" />
                  <span className="truncate">{t("table.unordered")}</span>
                </span>
              )}
            </>
          )}
        </div>

        <select
          aria-label={t("table.pageSize")}
          title={lockTitle(t("table.pageSize"))}
          value={limit}
          disabled={!connected || locked}
          onChange={(e) => {
            setLimit(Number(e.target.value));
            setOffset(0);
          }}
          className="h-7 rounded-lg border border-border-strong bg-surface px-2 text-xs text-fg tabular-nums disabled:opacity-50"
        >
          {PAGE_SIZES.map((n) => (
            <option key={n} value={n}>
              {number.format(n)}
            </option>
          ))}
        </select>
        <div className="flex items-center">
          <IconButton
            label={lockTitle(t("table.prev"))}
            disabled={!page || offset === 0 || busy || locked}
            onClick={() => setOffset(Math.max(0, offset - limit))}
          >
            <ChevronLeft className="size-4" />
          </IconButton>
          <IconButton label={lockTitle(t("table.next"))} disabled={!page?.hasMore || busy || locked} onClick={() => setOffset(offset + limit)}>
            <ChevronRight className="size-4" />
          </IconButton>
        </div>
        <IconButton label={lockTitle(`${t("common.refresh")} (⌘R)`)} disabled={!connected || busy || locked} onClick={refresh}>
          <RefreshCw className={cn("size-3.5", busy && "animate-spin")} />
        </IconButton>
      </div>

      {page && (
        <SmartPasteDialog
          open={smartPaste}
          onClose={() => setSmartPaste(false)}
          connection={connection}
          table={tab.table}
          columns={page.columns}
          onDone={(result, skipped) => {
            setSmartPaste(false);
            toast.success(t("smart.done", { ...result }), { description: skipped > 0 ? t("smart.doneSkipped", { count: skipped }) : undefined });
            refresh();
          }}
        />
      )}
      {page && loaded && view === "data" && (
        <CellDialog
          cell={inspect}
          onClose={() => setInspect(null)}
          connectionId={connection.id}
          table={tab.table}
          page={page}
          pending={pending}
          editable={editable && !saving}
          onApply={(cell, original, value) => {
            // The whole original makes a shortened cell editable; the edit is staged as usual.
            const known = { ...full, [cellIndex(loaded, cell.r, cell.c)]: original };
            const next = cell.r < loaded.rows.length ? known : full;
            setFull(next);
            commit(setCells(withFull(loaded, next), pending, [{ ...cell, value }]).pending);
            setInspect(null);
          }}
        />
      )}
      {page && (
        <ReviewDialog
          open={reviewing}
          onClose={() => setReviewing(false)}
          connection={connection}
          table={tab.table}
          changes={reviewing ? toChangeSet(page, pending) : { updates: [], inserts: [], deletes: [] }}
          summary={summary}
          onSaved={saved}
        />
      )}
    </div>
  );
}

function Structure({ page }: { page: RowPage }) {
  const { t } = useTranslation();
  const th = "sticky top-0 z-10 border-b border-border bg-sunken px-3 py-2 text-left text-xs font-semibold";
  const td = "border-b border-border px-3 py-1.5 align-top";
  return (
    <div className="h-full overflow-auto">
      <table className="w-full border-separate border-spacing-0 text-[13px]">
        <thead>
          <tr>
            <th className={cn(th, "w-10 text-right text-fg-subtle")}>#</th>
            <th className={th}>{t("table.column")}</th>
            <th className={th}>{t("table.type")}</th>
            <th className={th}>{t("table.nullable")}</th>
            <th className={th}>{t("table.default")}</th>
            <th className={th}>{t("table.key")}</th>
          </tr>
        </thead>
        <tbody data-selectable>
          {page.columns.map((c, i) => (
            <tr key={c.name} className="hover:bg-sunken/70">
              <td className={cn(td, "text-right font-mono text-[11px] text-fg-subtle")}>{i + 1}</td>
              <td className={cn(td, "font-medium")}>{c.name}</td>
              <td className={cn(td, "font-mono text-xs text-fg-muted")}>{c.dataType}</td>
              <td className={cn(td, c.nullable ? "text-fg-muted" : "font-medium")}>{c.nullable ? t("table.yes") : t("table.no")}</td>
              <td className={cn(td, "max-w-80 font-mono text-xs break-words text-fg-muted")}>
                {c.default ?? <span className="text-fg-subtle italic">{c.generated ? t("table.generated") : "—"}</span>}
              </td>
              <td className={td}>
                {c.primaryKey && (
                  <span className="inline-flex items-center gap-1 rounded bg-accent-soft px-1.5 py-0.5 text-[11px] font-semibold text-accent-text">
                    <KeyRound className="size-3" /> {t("table.primaryKey")}
                  </span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
