import { CircleAlert, X } from "lucide-react";
import { Dialog } from "radix-ui";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, IconButton } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import type { ConnectionView } from "../../lib/types";

// The pieces every form of the schema menus is made of: the database forms and the table forms.

export function Header({ icon, title, connection, busy }: { icon: ReactNode; title: string; connection: ConnectionView; busy: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-border py-3 pr-3 pl-5">
      {icon}
      {/* The database's name is what matters here: it wraps rather than being cut. */}
      <Dialog.Title className="line-clamp-2 min-w-0 flex-1 text-[15px] leading-snug font-semibold break-words">{title}</Dialog.Title>
      <div className="flex h-7 max-w-[38%] shrink-0 items-center gap-1.5 rounded-lg border border-border px-1.5 text-xs">
        <EngineTag engine={connection.engine} />
        <span className="truncate">{connection.name}</span>
        <EnvBadge environment={connection.environment} long />
      </div>
      <Dialog.Close asChild>
        <IconButton label={t("common.close")} disabled={busy}>
          <X className="size-4" />
        </IconButton>
      </Dialog.Close>
    </div>
  );
}

export type Plan = { sql: string } | { problem: string } | null;

/** The backend's statement for the choices on screen. `stale`: it answers earlier choices. */
export function Statement({ plan, empty, stale = false }: { plan: Plan; empty: string; stale?: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs font-medium text-fg-muted">{t("schema.statement")}</span>
      {plan && "sql" in plan ? (
        <pre
          data-selectable
          data-ddl-preview
          aria-busy={stale}
          className={cn(
            "rounded-lg border border-border bg-sunken px-3 py-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap transition-opacity",
            stale && "opacity-50",
          )}
        >
          {plan.sql}
        </pre>
      ) : (
        <div data-ddl-preview-empty className="rounded-lg border border-dashed border-border px-3 py-2 text-xs text-fg-subtle">
          {plan ? <span className="text-danger">{plan.problem}</span> : empty}
        </div>
      )}
    </div>
  );
}

export function Notice({ tone, icon, children }: { tone: "muted" | "danger" | "warn"; icon: ReactNode; children: ReactNode }) {
  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-lg px-3 py-2 text-[13px]",
        tone === "muted" && "bg-sunken text-fg-muted",
        tone === "danger" && "bg-danger-soft font-medium text-danger",
        tone === "warn" && "bg-warn-soft text-warn",
      )}
    >
      <span className="mt-0.5 shrink-0">{icon}</span>
      <span className="min-w-0 flex-1">{children}</span>
    </div>
  );
}

export function Failure({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <div role="alert" data-ddl-error className="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
      <CircleAlert className="mt-0.5 size-4 shrink-0" />
      <span data-selectable className="min-w-0 flex-1 break-words">
        {message}
      </span>
    </div>
  );
}

export function Footer({ busy, children }: { busy: boolean; children: ReactNode }) {
  const { t } = useTranslation();
  return (
    <div className="flex shrink-0 items-center justify-end gap-2 border-t border-border px-5 py-3">
      <Dialog.Close asChild>
        <Button variant="ghost" disabled={busy}>
          {t("common.cancel")}
        </Button>
      </Dialog.Close>
      {children}
    </div>
  );
}
