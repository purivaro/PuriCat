import { CircleCheck, CircleHelp, CircleX, RefreshCw, ShieldCheck, Sparkles } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "../../lib/cn";
import { ipc } from "../../lib/ipc";
import type { AiProvider, CliStatus } from "../../lib/types";
import { IconButton, Segmented, Spinner } from "../ui/primitives";

const SIGN_IN_COMMAND: Record<AiProvider, string> = { claude: "claude auth login", codex: "codex login" };

function StatusLine({ tone, icon, title, children }: { tone: "ok" | "bad" | "unknown"; icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex gap-2.5">
      <span className={cn("mt-0.5 shrink-0", tone === "ok" ? "text-ok" : tone === "bad" ? "text-danger" : "text-fg-subtle")}>{icon}</span>
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium">{title}</div>
        {children && (
          <div data-selectable className="mt-0.5 text-xs leading-snug break-words text-fg-muted">
            {children}
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * Phase 1 scope: verify that the CLI the agent will drive is installed and signed in.
 * There is deliberately no chat box here until it is backed by a real integration.
 */
export function AiPanel() {
  const { t } = useTranslation();
  const [provider, setProvider] = useState<AiProvider>("claude");
  const [statuses, setStatuses] = useState<CliStatus[] | null>(null);
  const [checking, setChecking] = useState(false);

  const check = useCallback(() => {
    setChecking(true);
    ipc
      .detectAiClis()
      .then(setStatuses)
      .catch(() => setStatuses([]))
      .finally(() => setChecking(false));
  }, []);
  useEffect(check, [check]);

  const status = statuses?.find((s) => s.provider === provider);
  const name = t(`ai.${provider}`);

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex h-11 shrink-0 items-center gap-2 pr-2 pl-3.5">
        <Sparkles className="size-4 text-accent-text" />
        <h2 className="flex-1 truncate text-[13px] font-semibold">{t("ai.title")}</h2>
        <IconButton label={t("ai.recheck")} disabled={checking} onClick={check}>
          <RefreshCw className={cn("size-3.5", checking && "animate-spin")} />
        </IconButton>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-3 pb-3">
        <Segmented
          label={t("ai.provider")}
          value={provider}
          onChange={setProvider}
          className="w-full"
          options={[
            { value: "claude", label: t("ai.claude") },
            { value: "codex", label: t("ai.codex") },
          ]}
        />

        <section className="flex flex-col gap-3 rounded-xl border border-border p-3">
          {!status ? (
            <div className="flex items-center gap-2 py-1 text-[13px] text-fg-muted">
              <Spinner className="size-3.5" /> {t("ai.checking")}
            </div>
          ) : !status.installed ? (
            <StatusLine tone="bad" icon={<CircleX className="size-4" />} title={t("ai.notInstalled")}>
              {t("ai.notInstalledBody", { name })}
            </StatusLine>
          ) : (
            <>
              <StatusLine tone="ok" icon={<CircleCheck className="size-4" />} title={t("ai.installed")}>
                {status.version && <div>{status.version}</div>}
                {status.path && <div className="font-mono text-[11px] text-fg-subtle">{status.path}</div>}
              </StatusLine>
              {status.authenticated === true ? (
                <StatusLine tone="ok" icon={<CircleCheck className="size-4" />} title={t("ai.signedIn")} />
              ) : status.authenticated === false ? (
                <StatusLine tone="bad" icon={<CircleX className="size-4" />} title={t("ai.signedOut")}>
                  {t("ai.signedOutBody", { command: SIGN_IN_COMMAND[provider] })}
                </StatusLine>
              ) : (
                <StatusLine tone="unknown" icon={<CircleHelp className="size-4" />} title={t("ai.authUnknown")} />
              )}
            </>
          )}
        </section>

        <section className="rounded-xl bg-accent-soft p-3">
          <div className="text-[13px] font-semibold text-accent-text">{t("ai.comingTitle")}</div>
          <p className="mt-1 text-xs leading-relaxed text-fg-muted">{t("ai.comingBody")}</p>
        </section>

        <section className="rounded-xl border border-border p-3">
          <div className="flex items-center gap-1.5 text-[13px] font-semibold">
            <ShieldCheck className="size-4 text-ok" /> {t("ai.policyTitle")}
          </div>
          <ul className="mt-2 flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-fg-muted">
            <li>{t("ai.policy1")}</li>
            <li>{t("ai.policy2")}</li>
            <li>{t("ai.policy3")}</li>
          </ul>
        </section>
      </div>
    </div>
  );
}
