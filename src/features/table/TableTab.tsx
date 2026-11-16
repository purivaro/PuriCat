import {
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Eye,
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
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, IconButton, Segmented, Spinner, StateMessage, Tooltip } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ApplyResult, RowPage, SortSpec } from "../../lib/types";
import { sessionOf, useConnections } from "../../stores/connections";
import { type TableTab, useWorkspace } from "../../stores/workspace";
import { DataGrid, rangeOf, type Selection } from "./DataGrid";
import {
  addRows,
  canRedo,
  canUndo,
  current,
  EMPTY,
  isInserted,
  NO_HISTORY,
  type Pending,
  record,
  redo,
  setDeleted,
  summarize,
  toChangeSet,
  undo,
} from "./pending";
import { ReviewDialog } from "./ReviewDialog";

const PAGE_SIZES = [100, 200, 500, 1000];

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
  const [page, setPage] = useState<RowPage | null>(null);
  const [load, setLoad] = useState<Load>({ status: "idle" });
  const [reloads, setReloads] = useState(0);
  const request = useRef(0);

  // Staged edits for the loaded page, with undo history. Nothing reaches the database until Save.
  const [history, setHistory] = useState(NO_HISTORY);
  const [selection, setSelection] = useState<Selection | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const pending = current(history);
  const summary = summarize(pending);
  const dirty = summary.rows > 0;

  // Load when the tab is first shown and whenever paging, sorting or the session changes.
  // Hidden tabs do not query: a restored workspace must not hit every server at once.
  const wanted = connected && (active || page !== null);
  useEffect(() => {
    if (!wanted) return;
    const id = ++request.current;
    setLoad({ status: "loading" });
    ipc
      .fetchRows(tab.connectionId, tab.table, { offset, limit, sort })
      .then((rows) => {
        if (id !== request.current) return; // a newer request superseded this one
        setPage(rows);
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
  }, [wanted, tab.connectionId, tab.table, offset, limit, sort, reloads]);

  // A closed session invalidates what is on screen.
  useEffect(() => {
    if (!connected) {
      request.current++;
      setPage(null);
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

  const refresh = useCallback(() => setReloads((n) => n + 1), []);
  const changeSort = (next: SortSpec | null) => {
    setSort(next);
    setOffset(0);
  };

  const commit = useCallback((next: Pending) => {
    setSaveError(null);
    setHistory((h) => record(h, next));
  }, []);

  const saved = useCallback(
    (result: ApplyResult) => {
      setReviewing(false);
      setSaving(false);
      toast.success(t("table.saved", { ...result }));
      refresh(); // shows what the server actually stored and clears the staged edits
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
      if (!page || !connection || !dirty || saving) return;
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
    [page, connection, dirty, saving, summary.deleted, summary.rows, tab.table, pending, saved],
  );

  // Tab-level shortcuts: ⌘S saves, ⌘R reloads (unless that would drop edits).
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
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, connected, dirty, refresh, save]);

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
      <div className="flex shrink-0 items-center gap-3 border-b border-border px-4 py-2.5">
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

        {editable && (
          <div className="flex shrink-0 items-center gap-1">
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
              <Button icon={<RefreshCw className="size-3.5" />} onClick={refresh}>
                {t("common.retry")}
              </Button>
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
              editable={editable && !saving}
              pending={pending}
              onChange={commit}
              selection={selection}
              onSelect={setSelection}
              onUndo={() => setHistory(undo)}
              onRedo={() => setHistory(redo)}
              onSave={() => save()}
            />
            {page.rows.length === 0 && pending.inserts.length === 0 && (
              <div className="pointer-events-none absolute inset-x-0 top-24 text-center text-[13px] text-fg-muted">
                {t("table.empty")}
              </div>
            )}
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
              {page.estimatedRows != null && (
                <span className="shrink-0">· {t("table.estimated", { total: number.format(page.estimatedRows) })}</span>
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
