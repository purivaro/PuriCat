import {
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Eye,
  KeyRound,
  Plug,
  RefreshCw,
  ShieldCheck,
  Table2,
  TriangleAlert,
  Unplug,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, IconButton, Segmented, Spinner, StateMessage, Tooltip } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { RowPage, SortSpec, TableMeta } from "../../lib/types";
import { sessionOf, useConnections } from "../../stores/connections";
import type { TableTab } from "../../stores/workspace";
import { DataGrid } from "./DataGrid";

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
  const [meta, setMeta] = useState<TableMeta | null>(null);
  const [load, setLoad] = useState<Load>({ status: "idle" });
  const [reloads, setReloads] = useState(0);
  const request = useRef(0);

  // Load when the tab is first shown and whenever paging, sorting or the session changes.
  // Hidden tabs do not query: a restored workspace must not hit every server at once.
  const wanted = connected && (active || page !== null);
  useEffect(() => {
    if (!wanted) return;
    const id = ++request.current;
    setLoad({ status: "loading" });
    Promise.all([
      ipc.fetchRows(tab.connectionId, tab.table, { offset, limit, sort }),
      ipc.describeTable(tab.connectionId, tab.table),
    ])
      .then(([rows, described]) => {
        if (id !== request.current) return; // a newer request superseded this one
        setPage(rows);
        setMeta(described);
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
      setMeta(null);
      setLoad({ status: "idle" });
    }
  }, [connected]);

  const refresh = useCallback(() => setReloads((n) => n + 1), []);
  const changeSort = (next: SortSpec | null) => {
    setSort(next);
    setOffset(0);
  };

  // ⌘R reloads the visible table.
  useEffect(() => {
    if (!active || !connected) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        refresh();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, connected, refresh]);

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
  const Icon = tab.objectKind === "table" ? Table2 : Eye;
  const kindLabel = t(tab.objectKind === "table" ? "table.tableKind" : `table.${tab.objectKind}`);
  const path = [tab.table.database, tab.table.schema, tab.table.name].filter(Boolean);
  const busy = load.status === "loading";

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

        <Tooltip label={t("table.readOnlyPreviewHint")}>
          <span className="flex h-6 shrink-0 items-center gap-1 rounded-md bg-sunken px-2 text-[11px] font-medium text-fg-muted">
            <ShieldCheck className="size-3.5" /> {t("table.readOnlyPreview")}
          </span>
        </Tooltip>
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
          <Structure page={page} meta={meta} />
        ) : (
          <>
            <DataGrid page={page} sort={sort} onSort={changeSort} />
            {page.rows.length === 0 && (
              <div className="pointer-events-none absolute inset-x-0 top-24 text-center text-[13px] text-fg-muted">
                {t("table.empty")}
              </div>
            )}
          </>
        )}
        {busy && page && <div aria-hidden className="animate-fade absolute inset-x-0 top-0 h-0.5 animate-pulse bg-accent" />}
      </div>

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
              {meta?.estimatedRows != null && (
                <span className="shrink-0">· {t("table.estimated", { total: number.format(meta.estimatedRows) })}</span>
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
          title={t("table.pageSize")}
          value={limit}
          disabled={!connected}
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
          <IconButton label={t("table.prev")} disabled={!page || offset === 0 || busy} onClick={() => setOffset(Math.max(0, offset - limit))}>
            <ChevronLeft className="size-4" />
          </IconButton>
          <IconButton label={t("table.next")} disabled={!page?.hasMore || busy} onClick={() => setOffset(offset + limit)}>
            <ChevronRight className="size-4" />
          </IconButton>
        </div>
        <IconButton label={`${t("common.refresh")} (⌘R)`} disabled={!connected || busy} onClick={refresh}>
          <RefreshCw className={cn("size-3.5", busy && "animate-spin")} />
        </IconButton>
      </div>
    </div>
  );
}

function Structure({ page, meta }: { page: RowPage; meta: TableMeta | null }) {
  const { t } = useTranslation();
  const columns = meta?.columns ?? page.columns;
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
          {columns.map((c, i) => (
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
