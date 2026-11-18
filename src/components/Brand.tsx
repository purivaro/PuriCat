import { useTranslation } from "react-i18next";

import brandMark from "../assets/brand-mark.png";
import { cn } from "../lib/cn";
import type { Engine, Environment } from "../lib/types";

/** The PuriCat mark: the app icon itself, so the window and the Dock always match. */
export function BrandMark({ className }: { className?: string }) {
  return <img src={brandMark} alt="" draggable={false} className={cn("pointer-events-none select-none", className)} />;
}

// Engine tags are PuriCat's own glyphs, not vendor logos.
const ENGINES: Record<Engine, { short: string; className: string }> = {
  mysql: { short: "My", className: "bg-[#e6f1f5] text-[#0b6480] dark:bg-[#0b6480]/25 dark:text-[#7cc7de]" },
  postgres: { short: "Pg", className: "bg-[#e8eef7] text-[#2f5d8f] dark:bg-[#2f5d8f]/30 dark:text-[#8fb6e3]" },
  sqlite: { short: "Sq", className: "bg-[#e7f3ec] text-[#256b45] dark:bg-[#256b45]/30 dark:text-[#8fd3ab]" },
  sqlserver: { short: "Ms", className: "bg-[#f8e9e9] text-[#a3262b] dark:bg-[#a3262b]/25 dark:text-[#ec9a9d]" },
};

export function EngineTag({ engine, className }: { engine: Engine; className?: string }) {
  const { t } = useTranslation();
  const e = ENGINES[engine];
  return (
    <span
      title={t(`engine.${engine}`)}
      className={cn(
        "inline-flex size-5 shrink-0 items-center justify-center rounded-md font-mono text-[10px] font-bold tracking-tight",
        e.className,
        className,
      )}
    >
      {e.short}
    </span>
  );
}

export const ENV_TONE: Record<Environment, string> = {
  development: "bg-ok-soft text-ok",
  staging: "bg-info-soft text-info",
  production: "bg-danger-soft text-danger",
};

export function EnvBadge({ environment, long, className }: { environment: Environment; long?: boolean; className?: string }) {
  const { t } = useTranslation();
  return (
    <span
      title={t(`env.${environment}`)}
      className={cn(
        "inline-flex h-[18px] shrink-0 items-center rounded px-1.5 text-[10px] leading-none font-bold tracking-wide",
        ENV_TONE[environment],
        className,
      )}
    >
      {long ? t(`env.${environment}`) : t(`env.short.${environment}`)}
    </span>
  );
}
