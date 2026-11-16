import { Eye, Home, Table2, X } from "lucide-react";
import { ContextMenu } from "radix-ui";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import { TableTabView } from "../../features/table/TableTab";
import { WelcomeTab } from "../../features/welcome/WelcomeTab";
import { cn } from "../../lib/cn";
import { useConnections } from "../../stores/connections";
import { type Tab, useWorkspace } from "../../stores/workspace";
import { ENV_TONE } from "../Brand";
import { menuContentClass, menuItemClass, menuSeparatorClass } from "../ui/primitives";

function TabButton({ tab, active }: { tab: Tab; active: boolean }) {
  const { t } = useTranslation();
  const { activate, close, closeOthers, closeAll } = useWorkspace.getState();
  const connection = useConnections((s) =>
    tab.kind === "table" ? s.connections.find((c) => c.id === tab.connectionId) : undefined,
  );
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [active]);

  const Icon = tab.kind === "welcome" ? Home : tab.objectKind === "table" ? Table2 : Eye;
  const title = tab.kind === "welcome" ? t("tabs.welcome") : tab.table.name;
  const where =
    tab.kind === "table"
      ? [connection?.name, tab.table.database, tab.table.schema].filter(Boolean).join(" › ")
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

export function Workspace() {
  const tabs = useWorkspace((s) => s.tabs);
  const activeId = useWorkspace((s) => s.activeId);

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div role="tablist" className="flex h-9 shrink-0 items-end gap-0.5 overflow-x-auto px-1 [scrollbar-width:none]">
        {tabs.map((tab) => (
          <TabButton key={tab.id} tab={tab} active={tab.id === activeId} />
        ))}
      </div>
      {/* Panels stay mounted so scroll position, paging and sorting survive a tab switch. */}
      <div className="relative min-h-0 flex-1 overflow-hidden rounded-xl rounded-tl-none border border-border bg-surface shadow-card">
        {tabs.map((tab) => (
          <div key={tab.id} role="tabpanel" hidden={tab.id !== activeId} className="h-full">
            {tab.kind === "welcome" ? <WelcomeTab /> : <TableTabView tab={tab} active={tab.id === activeId} />}
          </div>
        ))}
      </div>
    </div>
  );
}
