import { Command } from "cmdk";
import { Eye, Home, Languages, Moon, PanelLeft, Plug, Plus, Search, Sparkles, Sun, Table2, X } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { LANGUAGES } from "../../i18n";
import type { ObjectKind, TableRef } from "../../lib/types";
import { nodeKey, sessionOf, useConnections } from "../../stores/connections";
import { resolveTheme, useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { EngineTag, EnvBadge } from "../Brand";

const itemClass =
  "flex h-9 cursor-default items-center gap-2.5 rounded-lg px-2.5 text-[13px] data-[selected=true]:bg-accent-soft data-[selected=true]:text-accent-text";
const groupClass =
  "[&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:text-[10.5px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-fg-subtle [&_[cmdk-group-heading]]:uppercase";

interface LoadedTable {
  connectionId: string;
  connectionName: string;
  table: TableRef;
  kind: ObjectKind;
}

export function CommandPalette() {
  const { t } = useTranslation();
  const open = useUi((s) => s.paletteOpen);
  const setOpen = useUi((s) => s.setPaletteOpen);
  const settings = useSettings();
  const connections = useConnections((s) => s.connections);
  const sessions = useConnections((s) => s.sessions);
  const objects = useConnections((s) => s.objects);
  const tabs = useWorkspace((s) => s.tabs);
  const activeId = useWorkspace((s) => s.activeId);

  const run = (action: () => void) => () => {
    setOpen(false);
    action();
  };

  // Everything the explorer has already loaded is searchable without another round trip.
  const tables: LoadedTable[] = [];
  if (open) {
    for (const [key, state] of Object.entries(objects)) {
      if (state.status !== "ready") continue;
      const [connectionId, database, schema] = JSON.parse(key) as [string, string, string | undefined];
      const connection = connections.find((c) => c.id === connectionId);
      if (!connection) continue;
      for (const o of state.items) {
        tables.push({ connectionId, connectionName: connection.name, table: { database, schema: schema ?? null, name: o.name }, kind: o.kind });
      }
    }
  }

  const openConnection = async (id: string) => {
    settings.update({ explorerOpen: true });
    const store = useConnections.getState();
    store.toggle(nodeKey.connection(id), true);
    if ((await store.connect(id)) && !useConnections.getState().databases[nodeKey.connection(id)]) await store.loadDatabases(id);
  };

  const dark = resolveTheme(settings.theme) === "dark";
  const otherLanguage = LANGUAGES.find((l) => l.code !== settings.language)!;

  return (
    <Command.Dialog
      open={open}
      onOpenChange={setOpen}
      label={t("titlebar.search")}
      overlayClassName="animate-fade fixed inset-0 z-50 bg-black/25 backdrop-blur-[2px] dark:bg-black/50"
      contentClassName="animate-pop fixed top-[14vh] left-1/2 z-50 w-[min(92vw,580px)] -translate-x-1/2 overflow-hidden rounded-2xl border border-border bg-raised shadow-pop outline-none"
    >
      <div className="flex items-center gap-2.5 border-b border-border px-3.5">
        <Search className="size-4 shrink-0 text-fg-subtle" />
        <Command.Input
          placeholder={t("palette.placeholder")}
          className="h-12 flex-1 bg-transparent text-[14px] placeholder:text-fg-subtle focus:outline-none"
        />
        <kbd className="rounded border border-border px-1.5 text-[11px] text-fg-subtle">esc</kbd>
      </div>
      <Command.List className="max-h-[52vh] overflow-y-auto p-1.5">
        <Command.Empty className="py-8 text-center text-[13px] text-fg-muted">{t("palette.empty")}</Command.Empty>

        <Command.Group heading={t("palette.actions")} className={groupClass}>
          <Item icon={<Plus className="size-4" />} shortcut="⌘N" onSelect={run(() => useUi.getState().openConnectionDialog())}>
            {t("palette.newConnection")}
          </Item>
          <Item
            icon={dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
            onSelect={run(() => settings.update({ theme: dark ? "light" : "dark" }))}
          >
            {t("palette.toggleTheme", { theme: t(dark ? "titlebar.themeLight" : "titlebar.themeDark") })}
          </Item>
          <Item icon={<Languages className="size-4" />} onSelect={run(() => settings.update({ language: otherLanguage.code }))}>
            {t("palette.switchLanguage", { language: otherLanguage.label })}
          </Item>
          <Item icon={<PanelLeft className="size-4" />} shortcut="⌘B" onSelect={run(() => settings.update({ explorerOpen: !settings.explorerOpen }))}>
            {t("palette.toggleExplorer")}
          </Item>
          <Item icon={<Sparkles className="size-4" />} shortcut="⌘J" onSelect={run(() => settings.update({ aiOpen: !settings.aiOpen }))}>
            {t("palette.toggleAi")}
          </Item>
          <Item icon={<Home className="size-4" />} onSelect={run(() => useWorkspace.getState().openWelcome())}>
            {t("tabs.welcome")}
          </Item>
          <Item icon={<X className="size-4" />} shortcut="⌘W" onSelect={run(() => useWorkspace.getState().close(activeId))}>
            {t("palette.closeTab")}
          </Item>
        </Command.Group>

        {connections.length > 0 && (
          <Command.Group heading={t("palette.connections")} className={groupClass}>
            {connections.map((c) => (
              <Item
                key={c.id}
                value={`connection ${c.name} ${c.host} ${c.tags.join(" ")} ${c.id}`}
                icon={<EngineTag engine={c.engine} />}
                trailing={
                  <>
                    <EnvBadge environment={c.environment} />
                    {sessionOf(sessions, c.id).status === "connected" ? (
                      <span className="size-2 rounded-full bg-ok" />
                    ) : (
                      <Plug className="size-3.5 text-fg-subtle" />
                    )}
                  </>
                }
                onSelect={run(() => void openConnection(c.id))}
              >
                {c.name}
                <span className="ml-2 text-xs text-fg-subtle">{c.host}</span>
              </Item>
            ))}
          </Command.Group>
        )}

        {tabs.length > 1 && (
          <Command.Group heading={t("palette.tabs")} className={groupClass}>
            {tabs.map((tab) => (
              <Item
                key={tab.id}
                value={`tab ${tab.kind === "table" ? tab.table.name : "welcome"} ${tab.id}`}
                icon={tab.kind === "welcome" ? <Home className="size-4" /> : <Table2 className="size-4" />}
                onSelect={run(() => useWorkspace.getState().activate(tab.id))}
              >
                {tab.kind === "welcome" ? t("tabs.welcome") : tab.table.name}
              </Item>
            ))}
          </Command.Group>
        )}

        {tables.length > 0 && (
          <Command.Group heading={t("palette.tables")} className={groupClass}>
            {tables.map((x) => {
              const where = [x.connectionName, x.table.database, x.table.schema].filter(Boolean).join(" › ");
              return (
                <Item
                  key={`${x.connectionId}/${where}/${x.table.name}`}
                  value={`table ${x.table.name} ${where} ${x.connectionId}`}
                  icon={x.kind === "table" ? <Table2 className="size-4" /> : <Eye className="size-4" />}
                  onSelect={run(() => useWorkspace.getState().openTable(x.connectionId, x.table, x.kind))}
                >
                  {x.table.name}
                  <span className="ml-2 truncate text-xs text-fg-subtle">{where}</span>
                </Item>
              );
            })}
          </Command.Group>
        )}
      </Command.List>
    </Command.Dialog>
  );
}

interface ItemProps {
  icon: ReactNode;
  children: ReactNode;
  value?: string;
  shortcut?: string;
  trailing?: ReactNode;
  onSelect: () => void;
}

function Item({ icon, children, value, shortcut, trailing, onSelect }: ItemProps) {
  return (
    <Command.Item value={value} onSelect={onSelect} className={itemClass}>
      <span className="flex size-5 shrink-0 items-center justify-center text-fg-muted">{icon}</span>
      <span className="flex min-w-0 flex-1 items-baseline truncate">{children}</span>
      {trailing}
      {shortcut && <kbd className="font-sans text-xs text-fg-subtle">{shortcut}</kbd>}
    </Command.Item>
  );
}
