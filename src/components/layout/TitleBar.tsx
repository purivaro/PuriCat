import { Check, Languages, Lock, Monitor, Moon, PanelLeft, Search, Sparkles, Sun } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { useTranslation } from "react-i18next";

import { LANGUAGES } from "../../i18n";
import { sessionOf, useConnections } from "../../stores/connections";
import { type ThemeChoice, useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { BrandMark, EngineTag, EnvBadge } from "../Brand";
import { IconButton, menuContentClass, menuItemClass } from "../ui/primitives";

const THEMES: { value: ThemeChoice; icon: typeof Sun; key: string }[] = [
  { value: "light", icon: Sun, key: "titlebar.themeLight" },
  { value: "dark", icon: Moon, key: "titlebar.themeDark" },
  { value: "system", icon: Monitor, key: "titlebar.themeSystem" },
];

/** Connection of the active tab, shown so it is always clear which server you are looking at. */
function ActiveConnection() {
  const { t } = useTranslation();
  const tab = useWorkspace((s) => s.tabs.find((x) => x.id === s.activeId));
  const connectionId = tab?.kind === "table" ? tab.connectionId : null;
  const connection = useConnections((s) => s.connections.find((c) => c.id === connectionId));
  const session = useConnections((s) => (connectionId ? sessionOf(s.sessions, connectionId) : null));

  if (!connection) {
    return <span className="hidden px-2 text-xs text-fg-subtle lg:inline">{t("titlebar.noConnection")}</span>;
  }
  return (
    <div className="flex h-7 max-w-64 items-center gap-1.5 rounded-lg border border-border bg-surface pr-2 pl-1 text-xs">
      <EngineTag engine={connection.engine} />
      <span className="truncate font-medium">{connection.name}</span>
      <EnvBadge environment={connection.environment} />
      {/* Shown only when the server itself confirms that the session rejects writes. */}
      {session?.status === "connected" && session.info.readOnly && (
        <Lock aria-label={t("explorer.readOnly")} className="size-3 shrink-0 text-fg-muted" />
      )}
      <span
        className={`size-1.5 shrink-0 rounded-full ${session?.status === "connected" ? "bg-ok" : "bg-border-strong"}`}
      />
    </div>
  );
}

export function TitleBar() {
  const { t } = useTranslation();
  const { theme, language, explorerOpen, aiOpen, update } = useSettings();
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const ThemeIcon = THEMES.find((x) => x.value === theme)?.icon ?? Monitor;

  return (
    // The whole bar drags the window; interactive children opt out by not carrying the attribute.
    <header data-tauri-drag-region className="flex h-12 shrink-0 items-center gap-2 pr-3 pl-[88px]">
      <IconButton label={t("titlebar.toggleExplorer")} active={explorerOpen} onClick={() => update({ explorerOpen: !explorerOpen })}>
        <PanelLeft className="size-4" />
      </IconButton>

      <div data-tauri-drag-region className="flex items-center gap-1.5 pr-2 pl-1">
        <BrandMark className="size-6" />
        <span className="pointer-events-none text-[15px] font-bold tracking-tight">
          Puri<span className="text-accent-text">Cat</span>
        </span>
      </div>

      <div data-tauri-drag-region className="flex min-w-0 flex-1 justify-center">
        <button
          type="button"
          onClick={() => setPaletteOpen(true)}
          className="flex h-8 w-full max-w-md items-center gap-2 rounded-lg border border-border bg-surface px-2.5 text-[13px] text-fg-subtle transition-colors hover:border-border-strong hover:text-fg-muted"
        >
          <Search className="size-3.5 shrink-0" />
          <span className="flex-1 truncate text-left">{t("titlebar.search")}</span>
          <kbd className="rounded border border-border bg-bg px-1.5 font-sans text-[11px] text-fg-subtle">⌘K</kbd>
        </button>
      </div>

      <ActiveConnection />

      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <IconButton label={t("titlebar.language")}>
            <Languages className="size-4" />
          </IconButton>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content align="end" sideOffset={6} className={menuContentClass}>
            {LANGUAGES.map((l) => (
              <DropdownMenu.Item key={l.code} className={menuItemClass} onSelect={() => update({ language: l.code })}>
                <span className="flex-1">{l.label}</span>
                {language === l.code && <Check className="size-3.5" />}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>

      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <IconButton label={t("titlebar.theme")}>
            <ThemeIcon className="size-4" />
          </IconButton>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content align="end" sideOffset={6} className={menuContentClass}>
            {THEMES.map(({ value, icon: Icon, key }) => (
              <DropdownMenu.Item key={value} className={menuItemClass} onSelect={() => update({ theme: value })}>
                <Icon className="size-3.5" />
                <span className="flex-1">{t(key)}</span>
                {theme === value && <Check className="size-3.5" />}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>

      <IconButton label={t("titlebar.toggleAi")} active={aiOpen} onClick={() => update({ aiOpen: !aiOpen })}>
        <Sparkles className="size-4" />
      </IconButton>
    </header>
  );
}
