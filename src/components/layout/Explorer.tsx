import {
  ChevronRight,
  CircleAlert,
  Copy,
  Database,
  Eye,
  FolderTree,
  Lock,
  Pencil,
  Plug,
  Plus,
  RefreshCw,
  Search,
  Star,
  StarOff,
  Table2,
  TerminalSquare,
  Trash2,
  Unplug,
  X,
} from "lucide-react";
import { ContextMenu } from "radix-ui";
import { type KeyboardEvent, type ReactNode, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { clipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage } from "../../lib/ipc";
import type { ConnectionView, DbObject, Namespace } from "../../lib/types";
import { type Loadable, nodeKey, sessionOf, useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";
import { tableTabId, useWorkspace } from "../../stores/workspace";
import { EngineTag, EnvBadge } from "../Brand";
import {
  Button,
  IconButton,
  menuContentClass,
  menuDangerClass,
  menuItemClass,
  menuSeparatorClass,
  Spinner,
} from "../ui/primitives";

const lower = (s: string) => s.toLocaleLowerCase();

// ---- Row ---------------------------------------------------------------------------

interface RowProps {
  depth: number;
  icon: ReactNode;
  label: ReactNode;
  title?: string;
  expanded?: boolean; // undefined: leaf
  active?: boolean;
  dim?: boolean;
  trailing?: ReactNode;
  onClick?: () => void;
  menu?: ReactNode;
}

function Row({ depth, icon, label, title, expanded, active, dim, trailing, onClick, menu }: RowProps) {
  const row = (
    <button
      type="button"
      role="treeitem"
      data-tree-row
      aria-expanded={expanded}
      aria-selected={active}
      title={title}
      onClick={onClick}
      style={{ paddingLeft: 6 + depth * 14 }}
      className={cn(
        "group flex h-7 w-full items-center gap-1.5 rounded-md pr-2 text-left text-[13px] outline-none",
        "focus-visible:ring-2 focus-visible:ring-(--ring)",
        active ? "bg-accent-soft font-medium text-accent-text" : "text-fg hover:bg-sunken",
        dim && !active && "text-fg-muted",
      )}
    >
      <ChevronRight
        aria-hidden
        className={cn(
          "size-3.5 shrink-0 text-fg-subtle transition-transform",
          expanded && "rotate-90",
          expanded === undefined && "invisible",
        )}
      />
      <span className="flex size-4 shrink-0 items-center justify-center">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {trailing}
    </button>
  );
  if (!menu) return row;
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{row}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className={menuContentClass}>{menu}</ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

function Note({ depth, children, tone }: { depth: number; children: ReactNode; tone?: "danger" }) {
  return (
    <div
      data-selectable
      style={{ paddingLeft: 28 + depth * 14 }}
      className={cn("py-1 pr-2 text-xs leading-snug break-words", tone === "danger" ? "text-danger" : "text-fg-subtle")}
    >
      {children}
    </div>
  );
}

/** Loading / error / empty placeholder for a lazily loaded tree level. */
function Pending<T>({
  state,
  depth,
  empty,
  onRetry,
}: {
  state: Loadable<T> | undefined;
  depth: number;
  empty: string;
  onRetry: () => void;
}) {
  const { t } = useTranslation();
  if (!state || state.status === "loading") {
    return (
      <Note depth={depth}>
        <span className="inline-flex items-center gap-1.5">
          <Spinner className="size-3" /> {t("common.loading")}
        </span>
      </Note>
    );
  }
  if (state.status === "error") {
    return (
      <Note depth={depth} tone="danger">
        {state.message}{" "}
        <button type="button" className="font-medium underline underline-offset-2" onClick={onRetry}>
          {t("common.retry")}
        </button>
      </Note>
    );
  }
  return state.items.length === 0 ? <Note depth={depth}>{empty}</Note> : null;
}

// ---- Objects -----------------------------------------------------------------------

interface ObjectsProps {
  connection: ConnectionView;
  database: string;
  schema: string | null;
  depth: number;
  query: string;
}

function Objects({ connection, database, schema, depth, query }: ObjectsProps) {
  const { t } = useTranslation();
  const key = schema === null ? nodeKey.database(connection.id, database) : nodeKey.schema(connection.id, database, schema);
  const state = useConnections((s) => s.objects[key]);
  const expanded = useConnections((s) => s.expanded);
  const { toggle, loadObjects } = useConnections.getState();
  const activeId = useWorkspace((s) => s.activeId);
  const openTable = useWorkspace((s) => s.openTable);

  if (state?.status !== "ready" || state.items.length === 0) {
    return (
      <Pending
        state={state}
        depth={depth}
        empty={t("explorer.noObjects")}
        onRetry={() => void loadObjects(connection.id, database, schema)}
      />
    );
  }

  const groups: { id: string; label: string; items: DbObject[]; openByDefault: boolean }[] = [
    { id: "tables", label: t("explorer.tables"), items: state.items.filter((o) => o.kind === "table"), openByDefault: true },
    { id: "views", label: t("explorer.views"), items: state.items.filter((o) => o.kind !== "table"), openByDefault: false },
  ];

  return (
    <>
      {groups.map((g) => {
        if (g.items.length === 0) return null;
        const shown = query ? g.items.filter((o) => lower(o.name).includes(query)) : g.items;
        if (query && shown.length === 0) return null;
        const groupKey = `${key}#${g.id}`;
        const open = query ? true : (expanded[groupKey] ?? g.openByDefault);
        return (
          <div key={g.id} role="group">
            <Row
              depth={depth}
              expanded={open}
              dim
              icon={<FolderTree className="size-3.5 text-fg-subtle" />}
              label={
                <>
                  {g.label} <span className="text-fg-subtle">({g.items.length})</span>
                </>
              }
              onClick={() => toggle(groupKey, !open)}
            />
            {open &&
              shown.map((o) => {
                const table = { database, schema, name: o.name };
                const active = activeId === tableTabId(connection.id, table);
                const Icon = o.kind === "table" ? Table2 : Eye;
                return (
                  <Row
                    key={o.name}
                    depth={depth + 1}
                    active={active}
                    title={o.name}
                    icon={<Icon className={cn("size-3.5", active ? "text-accent-text" : "text-fg-subtle")} />}
                    label={o.name}
                    onClick={() => openTable(connection.id, table, o.kind)}
                    menu={
                      <>
                        <ContextMenu.Item className={menuItemClass} onSelect={() => openTable(connection.id, table, o.kind)}>
                          <Table2 className="size-3.5" /> {t("explorer.openData")}
                        </ContextMenu.Item>
                        <ContextMenu.Item
                          className={menuItemClass}
                          onSelect={() => void clipboard.write(o.name).then(() => toast.success(t("common.copied")))}
                        >
                          <Copy className="size-3.5" /> {t("explorer.copyName")}
                        </ContextMenu.Item>
                      </>
                    }
                  />
                );
              })}
          </div>
        );
      })}
    </>
  );
}

// ---- Schemas (PostgreSQL) ----------------------------------------------------------

function Schemas({ connection, database, depth, query }: Omit<ObjectsProps, "schema">) {
  const { t } = useTranslation();
  const key = nodeKey.database(connection.id, database);
  const state = useConnections((s) => s.schemas[key]);
  const expanded = useConnections((s) => s.expanded);
  const objects = useConnections((s) => s.objects);
  const { toggle, loadSchemas, loadObjects } = useConnections.getState();

  if (state?.status !== "ready" || state.items.length === 0) {
    return (
      <Pending
        state={state}
        depth={depth}
        empty={t("explorer.noSchemas")}
        onRetry={() => void loadSchemas(connection.id, database)}
      />
    );
  }

  return (
    <>
      {state.items.map((schema: Namespace) => {
        const schemaKey = nodeKey.schema(connection.id, database, schema.name);
        const loaded = objects[schemaKey];
        const hit =
          !!query && loaded?.status === "ready" && loaded.items.some((o) => lower(o.name).includes(query));
        if (query && !hit && !lower(schema.name).includes(query)) return null;
        const open = hit || !!expanded[schemaKey];
        return (
          <div key={schema.name} role="group">
            <Row
              depth={depth}
              expanded={open}
              dim={schema.system}
              icon={<FolderTree className="size-3.5 text-fg-subtle" />}
              label={schema.name}
              trailing={schema.system && <span className="text-[10px] text-fg-subtle">{t("explorer.system")}</span>}
              onClick={() => {
                toggle(schemaKey, !open);
                if (!open && !loaded) void loadObjects(connection.id, database, schema.name);
              }}
              menu={
                <ContextMenu.Item
                  className={menuItemClass}
                  onSelect={() => void loadObjects(connection.id, database, schema.name)}
                >
                  <RefreshCw className="size-3.5" /> {t("common.refresh")}
                </ContextMenu.Item>
              }
            />
            {open && (
              <Objects connection={connection} database={database} schema={schema.name} depth={depth + 1} query={hit ? query : ""} />
            )}
          </div>
        );
      })}
    </>
  );
}

// ---- Databases ---------------------------------------------------------------------

function Databases({ connection, query }: { connection: ConnectionView; query: string }) {
  const { t } = useTranslation();
  const key = nodeKey.connection(connection.id);
  const state = useConnections((s) => s.databases[key]);
  const session = useConnections((s) => sessionOf(s.sessions, connection.id));
  const expanded = useConnections((s) => s.expanded);
  const objects = useConnections((s) => s.objects);
  const schemas = useConnections((s) => s.schemas);
  const { toggle, loadDatabases, loadSchemas, loadObjects } = useConnections.getState();
  const hasSchemas = session.status === "connected" && session.info.capabilities.schemas;

  if (state?.status !== "ready" || state.items.length === 0) {
    return (
      <Pending state={state} depth={1} empty={t("explorer.noDatabases")} onRetry={() => void loadDatabases(connection.id)} />
    );
  }

  /** Does anything already loaded below this database match the filter? */
  const hasHit = (database: string): boolean => {
    if (!query) return false;
    const dbKey = nodeKey.database(connection.id, database);
    const matches = (l: Loadable<DbObject> | undefined) =>
      l?.status === "ready" && l.items.some((o) => lower(o.name).includes(query));
    if (!hasSchemas) return matches(objects[dbKey]);
    const s = schemas[dbKey];
    return (
      s?.status === "ready" &&
      s.items.some(
        (x) => lower(x.name).includes(query) || matches(objects[nodeKey.schema(connection.id, database, x.name)]),
      )
    );
  };

  return (
    <>
      {state.items.map((db) => {
        const dbKey = nodeKey.database(connection.id, db.name);
        const hit = hasHit(db.name);
        if (query && !hit && !lower(db.name).includes(query)) return null;
        const open = hit || !!expanded[dbKey];
        const load = () =>
          hasSchemas ? loadSchemas(connection.id, db.name) : loadObjects(connection.id, db.name, null);
        const loaded = hasSchemas ? schemas[dbKey] : objects[dbKey];
        return (
          <div key={db.name} role="group">
            <Row
              depth={1}
              expanded={open}
              dim={db.system}
              icon={<Database className="size-3.5 text-fg-subtle" />}
              label={db.name}
              trailing={db.system && <span className="text-[10px] text-fg-subtle">{t("explorer.system")}</span>}
              onClick={() => {
                toggle(dbKey, !open);
                if (!open && !loaded) void load();
              }}
              menu={
                <>
                  <ContextMenu.Item className={menuItemClass} onSelect={() => useWorkspace.getState().openSql(connection.id, db.name)}>
                    <TerminalSquare className="size-3.5" /> {t("sql.newQuery")}
                  </ContextMenu.Item>
                  <ContextMenu.Item className={menuItemClass} onSelect={() => void load()}>
                    <RefreshCw className="size-3.5" /> {t("common.refresh")}
                  </ContextMenu.Item>
                </>
              }
            />
            {open &&
              (hasSchemas ? (
                <Schemas connection={connection} database={db.name} depth={2} query={hit ? query : ""} />
              ) : (
                <Objects connection={connection} database={db.name} schema={null} depth={2} query={hit ? query : ""} />
              ))}
          </div>
        );
      })}
    </>
  );
}

// ---- Connection --------------------------------------------------------------------

function ConnectionNode({ connection, query }: { connection: ConnectionView; query: string }) {
  const { t } = useTranslation();
  const key = nodeKey.connection(connection.id);
  const session = useConnections((s) => sessionOf(s.sessions, connection.id));
  const expandedFlag = useConnections((s) => !!s.expanded[key]);
  const { toggle, connect, disconnect, loadDatabases, duplicate, toggleFavorite } = useConnections.getState();
  const { openConnectionDialog, askDelete } = useUi.getState();
  const connected = session.status === "connected";
  const open = expandedFlag && session.status !== "idle";

  const openNode = async () => {
    toggle(key, true);
    if ((await connect(connection.id)) && !useConnections.getState().databases[key]) {
      await loadDatabases(connection.id);
    }
  };

  const run = (action: () => Promise<void>) => () =>
    void action().catch((e: unknown) => toast.error(errorMessage(e)));

  return (
    <div role="group">
      <Row
        depth={0}
        expanded={open}
        title={`${connection.username}@${connection.host}:${connection.port}`}
        icon={<EngineTag engine={connection.engine} className="size-4 rounded text-[9px]" />}
        label={<span className="font-medium">{connection.name}</span>}
        trailing={
          <>
            {connection.readOnly && <Lock aria-label={t("explorer.readOnly")} className="size-3 shrink-0 text-fg-subtle" />}
            <EnvBadge environment={connection.environment} />
            {session.status === "connecting" ? (
              <Spinner className="size-3 text-fg-subtle" />
            ) : session.status === "error" ? (
              <CircleAlert aria-label={t("explorer.failed")} className="size-3.5 shrink-0 text-danger" />
            ) : (
              <span
                aria-label={connected ? t("explorer.connected") : undefined}
                className={cn("size-2 shrink-0 rounded-full", connected ? "bg-ok" : "bg-border-strong")}
              />
            )}
          </>
        }
        onClick={() => (open ? toggle(key, false) : void openNode())}
        menu={
          <>
            {connected ? (
              <ContextMenu.Item className={menuItemClass} onSelect={run(() => disconnect(connection.id))}>
                <Unplug className="size-3.5" /> {t("explorer.disconnect")}
              </ContextMenu.Item>
            ) : (
              <ContextMenu.Item className={menuItemClass} onSelect={() => void openNode()}>
                <Plug className="size-3.5" /> {t("explorer.connect")}
              </ContextMenu.Item>
            )}
            {connected && (
              <ContextMenu.Item className={menuItemClass} onSelect={() => useWorkspace.getState().openSql(connection.id, null)}>
                <TerminalSquare className="size-3.5" /> {t("sql.newQuery")}
                <span className="ml-auto pl-4 text-xs text-fg-subtle">⌘T</span>
              </ContextMenu.Item>
            )}
            {connected && (
              <ContextMenu.Item className={menuItemClass} onSelect={() => void loadDatabases(connection.id)}>
                <RefreshCw className="size-3.5" /> {t("common.refresh")}
              </ContextMenu.Item>
            )}
            <ContextMenu.Separator className={menuSeparatorClass} />
            <ContextMenu.Item className={menuItemClass} onSelect={() => openConnectionDialog(connection)}>
              <Pencil className="size-3.5" /> {t("explorer.edit")}
            </ContextMenu.Item>
            <ContextMenu.Item
              className={menuItemClass}
              onSelect={run(() => duplicate(connection.id, t("explorer.duplicateName", { name: connection.name })))}
            >
              <Copy className="size-3.5" /> {t("explorer.duplicate")}
            </ContextMenu.Item>
            <ContextMenu.Item className={menuItemClass} onSelect={run(() => toggleFavorite(connection.id))}>
              {connection.favorite ? <StarOff className="size-3.5" /> : <Star className="size-3.5" />}
              {connection.favorite ? t("explorer.unfavorite") : t("explorer.favorite")}
            </ContextMenu.Item>
            <ContextMenu.Separator className={menuSeparatorClass} />
            <ContextMenu.Item className={cn(menuItemClass, menuDangerClass)} onSelect={() => askDelete(connection)}>
              <Trash2 className="size-3.5" /> {t("explorer.delete")}
            </ContextMenu.Item>
          </>
        }
      />
      {open && session.status === "error" && (
        <Note depth={0} tone="danger">
          {session.message}{" "}
          <button type="button" className="font-medium underline underline-offset-2" onClick={() => void openNode()}>
            {t("common.retry")}
          </button>
        </Note>
      )}
      {open && connected && <Databases connection={connection} query={query} />}
    </div>
  );
}

// ---- Panel -------------------------------------------------------------------------

/** Arrow keys move between visible rows; Right/Left expand and collapse. */
function onTreeKeyDown(e: KeyboardEvent<HTMLDivElement>) {
  const current = (e.target as HTMLElement).closest<HTMLElement>("[data-tree-row]");
  if (!current) return;
  const rows = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-tree-row]")];
  const index = rows.indexOf(current);
  const expanded = current.getAttribute("aria-expanded");
  if (e.key === "ArrowDown") rows[index + 1]?.focus();
  else if (e.key === "ArrowUp") rows[index - 1]?.focus();
  else if (e.key === "ArrowRight" && expanded === "false") current.click();
  else if (e.key === "ArrowLeft" && expanded === "true") current.click();
  else return;
  e.preventDefault();
}

export function Explorer() {
  const { t } = useTranslation();
  const connections = useConnections((s) => s.connections);
  const loaded = useConnections((s) => s.loaded);
  const objects = useConnections((s) => s.objects);
  const databases = useConnections((s) => s.databases);
  const schemas = useConnections((s) => s.schemas);
  const openConnectionDialog = useUi((s) => s.openConnectionDialog);
  const [filter, setFilter] = useState("");
  const query = lower(filter.trim());

  // A connection stays visible when it matches, or when something loaded below it does.
  const visible = useMemo(() => {
    if (!query) return connections.map((c) => ({ connection: c, childQuery: "" }));
    const out: { connection: ConnectionView; childQuery: string }[] = [];
    for (const c of connections) {
      const own = [c.name, c.host, ...c.tags].some((v) => lower(v).includes(query));
      const prefix = JSON.stringify([c.id]).slice(0, -1);
      const below = [objects, databases, schemas].some((record) =>
        Object.entries(record).some(
          ([k, v]) => k.startsWith(prefix) && v.status === "ready" && (v.items as { name: string }[]).some((i) => lower(i.name).includes(query)),
        ),
      );
      if (below) out.push({ connection: c, childQuery: query });
      else if (own) out.push({ connection: c, childQuery: "" });
    }
    return out;
  }, [connections, query, objects, databases, schemas]);

  const favorites = visible.filter((v) => v.connection.favorite);
  const others = visible.filter((v) => !v.connection.favorite);

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex h-11 shrink-0 items-center gap-1 pr-2 pl-3.5">
        <h2 className="flex-1 truncate text-[13px] font-semibold">{t("explorer.title")}</h2>
        <IconButton label={t("explorer.new")} onClick={() => openConnectionDialog()}>
          <Plus className="size-4" />
        </IconButton>
      </div>

      {connections.length > 0 && (
        <div className="relative shrink-0 px-2.5 pb-2">
          <Search aria-hidden className="pointer-events-none absolute top-2 left-5 size-3.5 text-fg-subtle" />
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setFilter("")}
            placeholder={t("explorer.filter")}
            aria-label={t("explorer.filter")}
            spellCheck={false}
            className="h-[30px] w-full rounded-lg border border-transparent bg-sunken pr-7 pl-8 text-[13px] placeholder:text-fg-subtle focus:border-accent focus:bg-surface focus:outline-none"
          />
          {filter && (
            <button
              type="button"
              aria-label={t("common.close")}
              onClick={() => setFilter("")}
              className="absolute top-1.5 right-4 flex size-[18px] items-center justify-center rounded text-fg-subtle hover:bg-border hover:text-fg"
            >
              <X className="size-3" />
            </button>
          )}
        </div>
      )}

      <div role="tree" aria-label={t("explorer.title")} onKeyDown={onTreeKeyDown} className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-3">
        {loaded && connections.length === 0 && (
          <div className="flex flex-col items-center gap-3 px-4 pt-10 text-center">
            <div className="flex size-11 items-center justify-center rounded-2xl bg-accent-soft text-accent-text">
              <Database className="size-5" />
            </div>
            <div>
              <div className="text-[13px] font-semibold">{t("explorer.emptyTitle")}</div>
              <div className="mt-1 text-xs leading-relaxed text-fg-muted">{t("explorer.emptyBody")}</div>
            </div>
            <Button variant="primary" size="sm" icon={<Plus className="size-3.5" />} onClick={() => openConnectionDialog()}>
              {t("explorer.new")}
            </Button>
          </div>
        )}

        {query && visible.length === 0 && connections.length > 0 && (
          <div className="px-4 pt-6 text-center text-xs text-fg-muted">{t("explorer.noMatch", { query: filter.trim() })}</div>
        )}

        {favorites.length > 0 && (
          <>
            <SectionLabel>{t("explorer.favorites")}</SectionLabel>
            {favorites.map((v) => (
              <ConnectionNode key={v.connection.id} connection={v.connection} query={v.childQuery} />
            ))}
            {others.length > 0 && <SectionLabel>{t("explorer.all")}</SectionLabel>}
          </>
        )}
        {others.map((v) => (
          <ConnectionNode key={v.connection.id} connection={v.connection} query={v.childQuery} />
        ))}
      </div>
    </div>
  );
}

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="px-2 pt-2.5 pb-1 text-[10.5px] font-semibold tracking-wider text-fg-subtle uppercase">{children}</div>
  );
}
