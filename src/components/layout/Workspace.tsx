import { Eye, Home, Plus, Table2, TerminalSquare, X } from "lucide-react";
import { AlertDialog, ContextMenu, DropdownMenu } from "radix-ui";
import { type ButtonHTMLAttributes, lazy, type Ref, Suspense, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { TableTabView } from "../../features/table/TableTab";
import { WelcomeTab } from "../../features/welcome/WelcomeTab";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import { newQueryTarget } from "../../lib/newQuery";
import { useConnections } from "../../stores/connections";
import { sqlEditors, type Tab, useWorkspace } from "../../stores/workspace";
import { ENV_TONE, EngineTag } from "../Brand";
import { Button, menuContentClass, menuItemClass, menuSeparatorClass, Tooltip } from "../ui/primitives";

function TabButton({ tab, active }: { tab: Tab; active: boolean }) {
  const { t } = useTranslation();
  const { activate, close, closeOthers, closeAll } = useWorkspace.getState();
  const connection = useConnections((s) =>
    tab.kind === "welcome" ? undefined : s.connections.find((c) => c.id === tab.connectionId),
  );
  const dirty = useWorkspace((s) => !!s.dirty[tab.id]);
  const edited = useWorkspace((s) => !!s.edited[tab.id]);
  // An open transaction is the weightier of the two things a query tab can hold.
  const pending = dirty ? (tab.kind === "sql" ? "sql.transactionOpen" : "table.unsaved") : edited ? "sql.scriptEditedHint" : null;
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [active]);

  const Icon = tab.kind === "welcome" ? Home : tab.kind === "sql" ? TerminalSquare : tab.objectKind === "table" ? Table2 : Eye;
  const title = tab.kind === "welcome" ? t("tabs.welcome") : tab.kind === "sql" ? tab.title : tab.table.name;
  const where =
    tab.kind === "table"
      ? [connection?.name, tab.table.database, tab.table.schema].filter(Boolean).join(" › ")
      : tab.kind === "sql"
        ? [connection?.name, tab.database].filter(Boolean).join(" › ")
        : undefined;

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
        <div
          ref={ref}
          role="tab"
          aria-selected={active}
          tabIndex={active ? 0 : -1}
          title={where ? `${title} — ${where}` : title}
          onClick={() => activate(tab.id)}
          onAuxClick={(e) => e.button === 1 && close(tab.id)}
          onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && activate(tab.id)}
          className={cn(
            "group relative flex h-9 max-w-60 min-w-28 shrink-0 items-center gap-1.5 rounded-t-lg border border-b-0 pr-1.5 pl-2.5 text-[13px] outline-none",
            "focus-visible:ring-2 focus-visible:ring-(--ring)",
            active
              ? "border-border bg-surface font-medium text-fg"
              : "border-transparent text-fg-muted hover:bg-sunken hover:text-fg",
          )}
        >
          {active && <span aria-hidden className="absolute inset-x-2 top-0 h-0.5 rounded-b bg-accent" />}
          <Icon className={cn("size-3.5 shrink-0", active ? "text-accent-text" : "text-fg-subtle")} />
          <span className="min-w-0 flex-1 truncate">{title}</span>
          {pending && <span role="img" aria-label={t(pending)} title={t(pending)} className="size-2 shrink-0 rounded-full bg-accent" />}
          {connection && (
            // The colour ties the tab to its environment even when the name is cut off.
            <span className={cn("max-w-20 shrink truncate rounded px-1 text-[10px] font-semibold", ENV_TONE[connection.environment])}>
              {connection.name}
            </span>
          )}
          <button
            type="button"
            aria-label={t("tabs.close")}
            onClick={(e) => {
              e.stopPropagation();
              close(tab.id);
            }}
            className={cn(
              "flex size-5 shrink-0 items-center justify-center rounded text-fg-subtle hover:bg-border hover:text-fg",
              !active && "opacity-0 group-hover:opacity-100 focus-visible:opacity-100",
            )}
          >
            <X className="size-3" />
          </button>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className={menuContentClass}>
          <ContextMenu.Item className={menuItemClass} onSelect={() => close(tab.id)}>
            {t("tabs.close")}
            <span className="ml-auto pl-4 text-xs text-fg-subtle">⌘W</span>
          </ContextMenu.Item>
          <ContextMenu.Item className={menuItemClass} onSelect={() => closeOthers(tab.id)}>
            {t("tabs.closeOthers")}
          </ContextMenu.Item>
          <ContextMenu.Separator className={menuSeparatorClass} />
          <ContextMenu.Item className={menuItemClass} onSelect={closeAll}>
            {t("tabs.closeAll")}
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

const SqlTabView = lazy(() => import("../../features/sql/SqlTab"));

function QueryButton({ ref, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { ref?: Ref<HTMLButtonElement> }) {
  const { t } = useTranslation();
  return (
    <Tooltip label={`${t("sql.newQuery")} (⌘T)`}>
      <button
        ref={ref}
        type="button"
        data-new-query
        aria-label={t("sql.newQuery")}
        {...rest}
        className="mb-1 ml-1 inline-flex size-7 shrink-0 items-center justify-center rounded-md text-fg-muted transition-colors outline-none hover:bg-sunken hover:text-fg focus-visible:ring-2 focus-visible:ring-(--ring) data-[state=open]:bg-sunken data-[state=open]:text-fg"
      >
        <Plus className="size-4" />
      </button>
    </Tooltip>
  );
}

/**
 * "New query": a plus after the last tab, as in a browser, always in view. It opens on the
 * connection of the tab in front (as ⌘T does); with several connections open and none in
 * front, it asks which.
 */
function NewQueryButton() {
  const { t } = useTranslation();
  const tabs = useWorkspace((s) => s.tabs);
  const activeId = useWorkspace((s) => s.activeId);
  const sessions = useConnections((s) => s.sessions);
  const connected = useConnections((s) => s.connections).filter((c) => sessions[c.id]?.status === "connected");
  const target = newQueryTarget(
    tabs,
    activeId,
    connected.map((c) => c.id),
  );
  const { openSql } = useWorkspace.getState();

  if (target || connected.length === 0) {
    return (
      <QueryButton onClick={() => (target ? openSql(target.connectionId, target.database) : toast.info(t("sql.needConnection")))} />
    );
  }
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <QueryButton />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="start" sideOffset={4} collisionPadding={12} className={menuContentClass}>
          {connected.map((c) => (
            <DropdownMenu.Item key={c.id} className={menuItemClass} onSelect={() => openSql(c.id, null)}>
              <EngineTag engine={c.engine} className="size-4 rounded text-[9px]" />
              {t("sql.newQueryHere", { name: c.name })}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/**
 * Closing a tab with staged edits, a query tab with an open transaction, or a saved script
 * with changes that were not saved, asks first.
 */
function DiscardDialog() {
  const { t } = useTranslation();
  const closing = useWorkspace((s) => s.tabs.find((x) => x.id === s.closing));
  const dirty = useWorkspace((s) => !!closing && !!s.dirty[closing.id]);
  const edited = useWorkspace((s) => !!closing && !!s.edited[closing.id]);
  const { cancelClose, forceClose } = useWorkspace.getState();
  const [saving, setSaving] = useState(false);
  const sql = closing?.kind === "sql";
  // Only the script is at stake: it can be saved from here.
  const script = sql && !dirty && closing.scriptId !== undefined ? closing.scriptId : null;

  const saveAndClose = async () => {
    const text = closing && sqlEditors.get(closing.id)?.();
    if (!closing || script === null || text === undefined) return;
    setSaving(true);
    try {
      await ipc.updateScript(script, text);
      forceClose(closing.id);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <AlertDialog.Root open={!!closing} onOpenChange={(open) => !open && cancelClose()}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <AlertDialog.Content
          data-close-dialog
          className="animate-pop fixed top-1/2 left-1/2 z-50 w-[min(92vw,440px)] -translate-x-1/2 -translate-y-1/2 rounded-2xl border border-border bg-raised p-5 shadow-pop outline-none"
        >
          <AlertDialog.Title className="text-[15px] leading-snug font-semibold break-words">
            {script !== null
              ? t("closeDirty.titleScript", { name: closing?.kind === "sql" ? closing.title : "" })
              : sql
                ? t("closeDirty.titleSql", { name: closing.title })
                : t("closeDirty.title", { name: closing?.kind === "table" ? closing.table.name : "" })}
          </AlertDialog.Title>
          <AlertDialog.Description className="mt-1.5 text-[13px] leading-relaxed text-fg-muted">
            {t(script !== null ? "closeDirty.bodyScript" : sql ? "closeDirty.bodySql" : "closeDirty.body")}
            {/* A transaction and unsaved script text: closing loses both. */}
            {sql && dirty && edited && ` ${t("closeDirty.alsoScript")}`}
          </AlertDialog.Description>
          {script !== null ? (
            <div className="mt-5 flex justify-end gap-2">
              <AlertDialog.Cancel asChild>
                <Button variant="ghost">{t("common.cancel")}</Button>
              </AlertDialog.Cancel>
              <Button data-close-discard onClick={() => closing && forceClose(closing.id)}>
                {t("closeDirty.discardScript")}
              </Button>
              <Button variant="primary" data-close-save loading={saving} onClick={() => void saveAndClose()}>
                {t("common.save")}
              </Button>
            </div>
          ) : (
            <div className="mt-5 flex justify-end gap-2">
              <AlertDialog.Cancel asChild>
                <Button variant="primary">{t(sql ? "closeDirty.keepSql" : "closeDirty.keep")}</Button>
              </AlertDialog.Cancel>
              <Button variant="danger" onClick={() => closing && forceClose(closing.id)}>
                {t(sql ? "closeDirty.discardSql" : "closeDirty.discard")}
              </Button>
            </div>
          )}
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}

export function Workspace() {
  const tabs = useWorkspace((s) => s.tabs);
  const activeId = useWorkspace((s) => s.activeId);

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex h-9 shrink-0 items-end">
        <div role="tablist" className="flex h-9 min-w-0 items-end gap-0.5 overflow-x-auto px-1 [scrollbar-width:none]">
          {tabs.map((tab) => (
            <TabButton key={tab.id} tab={tab} active={tab.id === activeId} />
          ))}
        </div>
        {/* Outside the scrolling strip, so it stays in view however many tabs are open. */}
        <NewQueryButton />
      </div>
      {/* Panels stay mounted so scroll position, paging and sorting survive a tab switch. */}
      <div className="relative min-h-0 flex-1 overflow-hidden rounded-xl rounded-tl-none border border-border bg-surface shadow-card">
        {tabs.map((tab) => (
          <div key={tab.id} role="tabpanel" hidden={tab.id !== activeId} className="h-full">
            {tab.kind === "welcome" ? (
              <WelcomeTab />
            ) : tab.kind === "sql" ? (
              // The editor is a large dependency: it loads when the first query tab opens.
              <Suspense fallback={null}>
                <SqlTabView tab={tab} active={tab.id === activeId} />
              </Suspense>
            ) : (
              <TableTabView tab={tab} active={tab.id === activeId} />
            )}
          </div>
        ))}
      </div>
      <DiscardDialog />
    </div>
  );
}
