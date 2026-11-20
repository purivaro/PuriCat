import { Copy, Gauge, ListTree, Square, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, Spinner } from "../../components/ui/primitives";
import { clipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage } from "../../lib/ipc";
import type { Engine, ParamValue, Plan } from "../../lib/types";

export interface PlanState {
  sql: string;
  /** Asked for: a plan, or a measured run of the statement. */
  analyze: boolean;
  plan: Plan | null;
  error: string | null;
  loading: boolean;
  /** The parameter values the statement was given, for a measured run of it. */
  params?: Record<string, ParamValue>;
}

/**
 * The server's plan for one statement, the lines where it reads a whole table marked. "Run
 * and measure" asks for EXPLAIN ANALYZE, which runs the statement (one that reads only).
 */
export function PlanDialog({
  state,
  engine,
  onAnalyze,
  onStop,
  onClose,
}: {
  state: PlanState | null;
  engine: Engine;
  onAnalyze: () => void;
  onStop: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const plan = state?.plan ?? null;
  const scanLines = new Set(plan?.scans ?? []);
  const marked = (line: string) => [...scanLines].some((s) => line.includes(s));

  const copy = async () => {
    if (!plan) return;
    try {
      await clipboard.write(`${plan.statement}\n\n${plan.text}`);
      toast.success(t("plan.copied"), { id: "plan-copied", duration: 2000 });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <Dialog.Root open={state !== null} onOpenChange={(open) => !open && !state?.loading && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-sql-plan
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(86vh,720px)] w-[min(95vw,960px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          <div className="flex shrink-0 items-center gap-3 border-b border-border px-5 py-3">
            <ListTree className="size-5 shrink-0 text-accent-text" />
            <Dialog.Title className="min-w-0 flex-1 text-[15px] font-semibold">{t(state?.analyze ? "plan.measuredTitle" : "plan.title")}</Dialog.Title>
            {plan && <span className="shrink-0 text-xs text-fg-subtle tabular-nums">{t("plan.took", { ms: plan.elapsedMs })}</span>}
          </div>
          <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-5 py-4">
            <pre data-selectable className="max-h-24 overflow-y-auto rounded-lg bg-sunken px-3 py-2 font-mono text-xs whitespace-pre-wrap text-fg-muted">
              {plan?.statement ?? state?.sql}
            </pre>
            {state?.loading && (
              <div className="flex items-center gap-2 text-[13px] text-fg-muted">
                <Spinner className="size-3.5" /> {t(state.analyze ? "plan.measuring" : "plan.loading")}
              </div>
            )}
            {state?.error && (
              <p role="alert" data-plan-error className="rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
                {state.error}
              </p>
            )}
            {plan && (
              <>
                {plan.scans.length > 0 ? (
                  <div data-plan-scans className="flex items-start gap-2 rounded-lg bg-warn-soft px-3 py-2 text-[13px] leading-relaxed text-warn">
                    <TriangleAlert className="mt-0.5 size-4 shrink-0" />
                    <span>
                      {t("plan.scans", { count: plan.scans.length })}
                      <span className="mt-1 block font-mono text-xs">{plan.scans.join(" · ")}</span>
                    </span>
                  </div>
                ) : (
                  <p data-plan-no-scans className="text-xs text-fg-subtle">
                    {t("plan.noScans")}
                  </p>
                )}
                <pre
                  data-selectable
                  data-plan-text
                  className="min-h-0 flex-1 overflow-auto rounded-lg border border-border bg-surface px-3 py-2 font-mono text-xs leading-relaxed"
                >
                  {plan.text.split("\n").map((line, i) => (
                    <div key={i} className={cn(marked(line) && "-mx-3 bg-warn-soft px-3 font-semibold text-warn")}>
                      {line || " "}
                    </div>
                  ))}
                </pre>
                {plan.analyzed && <p className="text-xs text-fg-subtle">{t("plan.analyzedNote")}</p>}
              </>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
            {engine !== "sqlite" && !state?.analyze && <span className="min-w-0 flex-1 text-xs text-fg-subtle">{t("plan.analyzeHint")}</span>}
            <span className="flex-1" />
            {state?.loading ? (
              <Button variant="danger" size="sm" icon={<Square className="size-3.5" />} onClick={onStop}>
                {t("plan.stop")}
              </Button>
            ) : (
              <>
                <Button size="sm" icon={<Copy className="size-3.5" />} disabled={!plan} onClick={() => void copy()}>
                  {t("plan.copy")}
                </Button>
                {engine !== "sqlite" && !state?.analyze && (
                  <Button size="sm" data-plan-analyze icon={<Gauge className="size-3.5" />} onClick={onAnalyze}>
                    {t("plan.analyze")}
                  </Button>
                )}
                <Button variant="primary" size="sm" data-plan-close onClick={onClose}>
                  {t("common.close")}
                </Button>
              </>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
