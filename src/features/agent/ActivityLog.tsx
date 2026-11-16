import { History, Trash2, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, IconButton, Spinner, StateMessage } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { AuditEntry, AuditEvent } from "../../lib/types";

const EVENT_TONE: Record<AuditEvent, string> = {
  read: "bg-info-soft text-info",
  read_failed: "bg-warn-soft text-warn",
  proposed: "bg-sunken text-fg-muted",
  approved: "bg-accent-soft text-accent-text",
  rejected: "bg-sunken text-fg-muted",
  executed: "bg-ok-soft text-ok",
  blocked: "bg-warn-soft text-warn",
  failed: "bg-danger-soft text-danger",
};

/** The record of what the AI read and proposed, and what the user decided. Kept on this computer only. */
export function ActivityLog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const { t, i18n } = useTranslation();
  const [entries, setEntries] = useState<AuditEntry[] | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    if (!open) return;
    let stale = false;
    setEntries(null);
    setFailed(null);
    setConfirming(false);
    ipc
      .agentAudit(500)
      .then((list) => !stale && setEntries(list))
      .catch((e) => !stale && setFailed(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [open]);

  const clear = () =>
    void ipc.clearAgentAudit().then(
      () => {
        setEntries([]);
        setConfirming(false);
      },
      (e) => toast.error(errorMessage(e)),
    );
  const when = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "medium" });

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          data-agent-log
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(86vh,680px)] w-[min(94vw,760px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          <div className="flex shrink-0 items-start gap-3 border-b border-border py-3.5 pr-3 pl-5">
            <div className="min-w-0 flex-1">
              <Dialog.Title className="text-[15px] font-semibold">{t("ai.log.title")}</Dialog.Title>
              <Dialog.Description className="mt-0.5 text-xs leading-relaxed text-fg-muted">{t("ai.log.body")}</Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <IconButton label={t("common.close")}>
                <X className="size-4" />
              </IconButton>
            </Dialog.Close>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto">
            {failed ? (
              <StateMessage tone="danger" icon={<History className="size-5" />} title={t("ai.log.failed")} body={failed} />
            ) : entries === null ? (
              <div className="flex h-full items-center justify-center gap-2 text-[13px] text-fg-muted">
                <Spinner className="size-3.5" /> {t("common.loading")}
              </div>
            ) : entries.length === 0 ? (
              <StateMessage icon={<History className="size-5" />} title={t("ai.log.empty")} body={t("ai.log.emptyBody")} />
            ) : (
              <ul data-selectable>
                {entries.map((entry) => (
                  <li key={entry.id} data-agent-log-entry={entry.event} className="flex flex-col gap-1 border-b border-border px-5 py-2.5 last:border-0">
                    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                      <span className={cn("flex h-[18px] items-center rounded px-1.5 text-[10px] leading-none font-bold", EVENT_TONE[entry.event] ?? EVENT_TONE.proposed)}>
                        {t(`ai.log.event.${entry.event}`, { defaultValue: entry.event })}
                      </span>
                      <span className="font-medium">{entry.connectionName || t("ai.log.deletedConnection")}</span>
                      <span className="font-mono text-fg-muted">{entry.database}</span>
                      <span className="ml-auto text-fg-subtle">{when.format(entry.at)}</span>
                    </div>
                    <pre className="max-h-24 overflow-auto font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-words text-fg">{entry.sql}</pre>
                    <div className="text-xs break-words text-fg-muted">
                      {[t(`ai.${entry.provider}`, { defaultValue: entry.provider }), entry.model, entry.detail].filter(Boolean).join(" · ")}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
            {confirming ? (
              <>
                <span className="flex-1 text-xs text-fg-muted">{t("ai.log.clearConfirm")}</span>
                <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
                  {t("common.cancel")}
                </Button>
                <Button size="sm" variant="danger" onClick={clear}>
                  {t("ai.log.clear")}
                </Button>
              </>
            ) : (
              <>
                <span className="flex-1 text-xs text-fg-subtle">{entries && entries.length > 0 ? t("ai.log.count", { count: entries.length }) : ""}</span>
                <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} disabled={!entries || entries.length === 0} onClick={() => setConfirming(true)}>
                  {t("ai.log.clear")}
                </Button>
              </>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
