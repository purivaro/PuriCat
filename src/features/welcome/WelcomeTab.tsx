import { ArrowRight, CircleCheck, CircleDashed, Command, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import { BrandMark, EngineTag, EnvBadge } from "../../components/Brand";
import { Button } from "../../components/ui/primitives";
import { nodeKey, useConnections } from "../../stores/connections";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";

const READY = ["connections", "engines", "browse", "grid", "editing", "themes"] as const;
const NEXT = ["smartPaste", "sql", "sqlserver", "ai", "backup"] as const;

export function WelcomeTab() {
  const { t, i18n } = useTranslation();
  const connections = useConnections((s) => s.connections);
  const { openConnectionDialog, setPaletteOpen } = useUi.getState();

  const recent = [...connections]
    .sort((a, b) => (b.lastUsedAt ?? 0) - (a.lastUsedAt ?? 0) || b.updatedAt - a.updatedAt)
    .slice(0, 5);
  const when = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" });

  const open = async (id: string) => {
    useSettings.getState().update({ explorerOpen: true });
    const { toggle, connect, loadDatabases } = useConnections.getState();
    toggle(nodeKey.connection(id), true);
    if ((await connect(id)) && !useConnections.getState().databases[nodeKey.connection(id)]) {
      await loadDatabases(id);
    }
  };

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-4xl flex-col gap-8 px-10 py-12">
        <header className="flex items-center gap-5">
          <BrandMark className="size-[76px] shrink-0 drop-shadow-[0_6px_14px_rgba(255,122,24,0.28)]" />
          <div className="min-w-0 flex-1">
            <h1 className="text-[26px] leading-tight font-bold tracking-tight">{t("welcome.title")}</h1>
            <p className="mt-1 text-[15px] text-fg-muted">{t("welcome.subtitle")}</p>
          </div>
        </header>

        <div className="flex flex-wrap gap-2">
          <Button variant="primary" icon={<Plus className="size-4" />} onClick={() => openConnectionDialog()}>
            {t("welcome.newConnection")}
          </Button>
          <Button icon={<Command className="size-3.5" />} onClick={() => setPaletteOpen(true)}>
            {t("welcome.commandPalette")}
            <kbd className="ml-1 font-sans text-xs text-fg-subtle">⌘K</kbd>
          </Button>
        </div>

        <div className="grid gap-6 lg:grid-cols-2">
          <section>
            <h2 className="mb-2 text-xs font-semibold tracking-wider text-fg-subtle uppercase">{t("welcome.recent")}</h2>
            <div className="overflow-hidden rounded-xl border border-border">
              {recent.length === 0 ? (
                <p className="px-4 py-6 text-center text-[13px] text-fg-muted">{t("welcome.noRecent")}</p>
              ) : (
                recent.map((c) => (
                  <button
                    key={c.id}
                    type="button"
                    onClick={() => void open(c.id)}
                    className="group flex w-full items-center gap-3 border-b border-border px-3.5 py-2.5 text-left last:border-b-0 hover:bg-sunken focus-visible:bg-sunken"
                  >
                    <EngineTag engine={c.engine} className="size-7 rounded-lg text-xs" />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-2">
                        <span className="truncate text-[13px] font-medium">{c.name}</span>
                        <EnvBadge environment={c.environment} />
                      </span>
                      <span className="block truncate text-xs text-fg-subtle">
                        {c.host}:{c.port} · {c.lastUsedAt ? when.format(c.lastUsedAt) : t("welcome.neverUsed")}
                      </span>
                    </span>
                    <ArrowRight className="size-4 shrink-0 text-fg-subtle opacity-0 transition-opacity group-hover:opacity-100" />
                  </button>
                ))
              )}
            </div>
          </section>

          <section>
            <h2 className="mb-2 text-xs font-semibold tracking-wider text-fg-subtle uppercase">{t("welcome.status")}</h2>
            <div className="rounded-xl border border-border px-4 py-3.5">
              <p className="mb-3 text-xs leading-relaxed text-fg-muted">{t("welcome.statusNote")}</p>
              <ul className="flex flex-col gap-2">
                {READY.map((k) => (
                  <li key={k} className="flex gap-2 text-[13px] leading-snug">
                    <CircleCheck aria-label={t("welcome.ready")} className="mt-px size-4 shrink-0 text-ok" />
                    {t(`welcome.features.${k}`)}
                  </li>
                ))}
              </ul>
              <div className="mt-4 mb-2 text-[11px] font-semibold tracking-wider text-fg-subtle uppercase">{t("welcome.next")}</div>
              <ul className="flex flex-col gap-2">
                {NEXT.map((k) => (
                  <li key={k} className="flex gap-2 text-[13px] leading-snug text-fg-muted">
                    <CircleDashed aria-hidden className="mt-px size-4 shrink-0 text-fg-subtle" />
                    {t(`welcome.features.${k}`)}
                  </li>
                ))}
              </ul>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
