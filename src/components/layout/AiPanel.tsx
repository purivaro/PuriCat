import { CircleCheck, CircleHelp, CircleX, MessageSquarePlus, RefreshCw, Sparkles } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { AgentChat } from "../../features/agent/AgentChat";
import { cn } from "../../lib/cn";
import { ipc } from "../../lib/ipc";
import type { AiProvider, CliStatus } from "../../lib/types";
import { useAgent } from "../../stores/agent";
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
 * The agent panel: which CLI answers, whether it is ready, and the conversation.
 * The chat is only offered for a CLI that is installed and not known to be signed out;
 * otherwise the panel says what is missing and how to fix it.
 */
export function AiPanel() {
  const { t } = useTranslation();
  const provider = useAgent((s) => s.provider);
  const hasTurns = useAgent((s) => s.turns.length > 0);
  const running = useAgent((s) => s.running !== null);
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
  useEffect(() => void useAgent.getState().load(), []);

  const status = statuses?.find((s) => s.provider === provider);
  const name = t(`ai.${provider}`);
  const usable = !!status?.installed && status.authenticated !== false;

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex h-11 shrink-0 items-center gap-1 pr-2 pl-3.5">
        <Sparkles className="size-4 text-accent-text" />
        <h2 className="ml-1 flex-1 truncate text-[13px] font-semibold">{t("ai.title")}</h2>
        <IconButton label={t("ai.newChat")} disabled={!hasTurns} onClick={() => useAgent.getState().reset()}>
          <MessageSquarePlus className="size-4" />
        </IconButton>
        <IconButton label={t("ai.recheck")} disabled={checking} onClick={check}>
          <RefreshCw className={cn("size-3.5", checking && "animate-spin")} />
        </IconButton>
      </div>

      <div className="flex shrink-0 flex-col gap-2 px-3 pb-2.5">
        <Segmented
          label={t("ai.provider")}
          value={provider}
          // The provider answers the turn that is running; switch once it is done.
          onChange={(next) => !running && useAgent.getState().setProvider(next)}
          className="w-full"
          options={[
            { value: "claude", label: t("ai.claude") },
            { value: "codex", label: t("ai.codex") },
          ]}
        />
        {usable && status && (
          <div data-agent-status className="flex items-center gap-1.5 px-0.5 text-[11px] text-fg-subtle" title={status.path ?? undefined}>
            {status.authenticated ? <CircleCheck className="size-3 shrink-0 text-ok" /> : <CircleHelp className="size-3 shrink-0" />}
            <span className="truncate">
              {status.version ?? name} · {status.authenticated ? t("ai.signedIn") : t("ai.authUnknown")}
            </span>
          </div>
        )}
      </div>

      {usable ? (
        <AgentChat provider={provider} />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-3 pb-3">
          <section data-agent-status className="flex flex-col gap-3 rounded-xl border border-border p-3">
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
                <StatusLine tone="bad" icon={<CircleX className="size-4" />} title={t("ai.signedOut")}>
                  {t("ai.signedOutBody", { command: SIGN_IN_COMMAND[provider] })}
                </StatusLine>
              </>
            )}
          </section>
          <ul className="flex list-disc flex-col gap-1.5 rounded-xl bg-sunken py-3 pr-3 pl-7 text-xs leading-relaxed text-fg-muted">
            <li>{t("ai.policy1")}</li>
            <li>{t("ai.policy2")}</li>
            <li>{t("ai.policy3")}</li>
          </ul>
        </div>
      )}
    </div>
  );
}
