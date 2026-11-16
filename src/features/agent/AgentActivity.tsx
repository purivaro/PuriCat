import { Ban, Check, ChevronRight, CircleAlert, CircleCheck, FileSearch, ListTree, OctagonX, Play, ShieldAlert, Square, TerminalSquare, TriangleAlert } from "lucide-react";
import { AlertDialog } from "radix-ui";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";

import { EnvBadge } from "../../components/Brand";
import { Button, Spinner } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { ipc } from "../../lib/ipc";
import type { RunOutcome } from "../../lib/types";
import { type ChatProposal, type ChatTarget, type ToolStep, useAgent } from "../../stores/agent";
import { sessionOf, useConnections } from "../../stores/connections";
import { useWorkspace } from "../../stores/workspace";

const firstLine = (text: string) => text.trim().split("\n")[0] ?? "";
const preClass = "max-h-56 overflow-auto rounded-md bg-surface px-2.5 py-2 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-words text-fg";

// ---- what the model looked at -----------------------------------------------------------

const TOOL_ICON: Record<string, ReactNode> = {
  list_schemas: <ListTree className="size-3.5" />,
  list_tables: <ListTree className="size-3.5" />,
  describe_table: <FileSearch className="size-3.5" />,
  run_select: <Play className="size-3.5" />,
};

/** The tools the model called, in order. Each opens to show what it asked and exactly what it was given. */
export function ToolSteps({ steps }: { steps: ToolStep[] }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState<string | null>(null);
  return (
    <ul data-agent-steps className="overflow-hidden rounded-lg border border-border text-xs">
      {steps.map((step) => {
        const known = step.name in TOOL_ICON || step.name === "propose_sql";
        const expanded = open === step.id;
        // "3 rows" from the first line of what a query returned.
        const result = step.name === "run_select" && step.status === "ok" ? firstLine(step.output ?? "").split(". ")[0] : null;
        return (
          <li key={step.id} data-agent-step={step.name} data-status={step.status} className="border-b border-border last:border-0">
            <button
              type="button"
              aria-expanded={expanded}
              onClick={() => setOpen(expanded ? null : step.id)}
              className="flex w-full items-center gap-2 bg-sunken px-2.5 py-1.5 text-left transition-colors hover:bg-border/50"
            >
              <span className={cn("shrink-0", step.status === "error" ? "text-danger" : "text-fg-subtle")}>
                {step.status === "running" ? <Spinner className="size-3.5" /> : step.status === "error" ? <CircleAlert className="size-3.5" /> : (TOOL_ICON[step.name] ?? <Check className="size-3.5" />)}
              </span>
              <span className="min-w-0 flex-1 truncate">
                <span className={cn("font-medium", step.status === "error" ? "text-danger" : "text-fg-muted")}>
                  {step.status === "error" ? t(`ai.tool.failed.${known ? step.name : "other"}`, { name: step.name }) : known ? t(`ai.tool.${step.name}`) : step.name}
                </span>
                {step.input && step.name !== "list_tables" && <span className="ml-1.5 font-mono text-[11px] text-fg-subtle">{firstLine(step.input)}</span>}
              </span>
              {result && <span className="shrink-0 text-[11px] text-fg-subtle">{result}</span>}
              <ChevronRight className={cn("size-3 shrink-0 text-fg-subtle transition-transform", expanded && "rotate-90")} />
            </button>
            {expanded && (
              <div data-selectable className="flex flex-col gap-1.5 border-t border-border bg-sunken px-2.5 py-2">
                {step.input && (
                  <>
                    <div className="text-[10px] font-medium tracking-wide text-fg-subtle uppercase">{t("ai.tool.asked")}</div>
                    <pre className={preClass}>{step.input}</pre>
                  </>
                )}
                <div className="text-[10px] font-medium tracking-wide text-fg-subtle uppercase">{t(step.status === "error" ? "ai.tool.answeredError" : "ai.tool.answered")}</div>
                <pre data-agent-step-output className={preClass}>
                  {step.output ?? (step.status === "running" ? t("ai.tool.running") : t("ai.tool.noAnswer"))}
                </pre>
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

// ---- what the model wants to change -----------------------------------------------------

const STATE_TONE: Record<ChatProposal["state"], string> = {
  pending: "bg-warn-soft text-warn",
  running: "bg-accent-soft text-accent-text",
  done: "bg-ok-soft text-ok",
  blocked: "bg-warn-soft text-warn",
  failed: "bg-danger-soft text-danger",
  rejected: "bg-sunken text-fg-muted",
};

/** What running a proposal did, statement by statement. */
function Outcome({ outcome }: { outcome: RunOutcome }) {
  const { t } = useTranslation();
  return (
    <ul data-agent-outcome data-selectable className="flex flex-col gap-1 text-xs">
      {outcome.statements.map((s, i) => (
        <li key={i} className="flex items-start gap-1.5">
          <span className={cn("mt-px shrink-0", s.status === "ok" ? "text-ok" : s.status === "blocked" ? "text-warn" : "text-danger")}>
            {s.status === "ok" ? <CircleCheck className="size-3.5" /> : s.status === "blocked" ? <ShieldAlert className="size-3.5" /> : <CircleAlert className="size-3.5" />}
          </span>
          <div className="min-w-0 flex-1">
            <div className="truncate font-mono text-[11px] text-fg-muted" title={s.sql}>
              {firstLine(s.sql)}
            </div>
            <div className={cn(s.status === "ok" ? "text-fg-muted" : s.status === "blocked" ? "text-warn" : "text-danger")}>
              {s.status === "blocked"
                ? t("ai.proposal.heldBack", { count: s.rowsAffected ?? 0 })
                : s.status === "ok"
                  ? s.rowsAffected !== null
                    ? t("ai.proposal.affected", { count: s.rowsAffected })
                    : t("ai.proposal.ran")
                  : (s.message ?? t("ai.proposal.failedStatement"))}
            </div>
          </div>
        </li>
      ))}
      {outcome.skipped > 0 && <li className="pl-5 text-fg-subtle">{t("ai.proposal.skipped", { count: outcome.skipped })}</li>}
      {outcome.inTransaction && <li className="pl-5 text-warn">{t("ai.proposal.leftOpen")}</li>}
    </ul>
  );
}

/** SQL the model proposed. Nothing runs until the user has read it in the review dialog and approved. */
export function ProposalCard({ proposal, target }: { proposal: ChatProposal; target: ChatTarget | null }) {
  const { t } = useTranslation();
  const connection = useConnections((s) => (target ? s.connections.find((c) => c.id === target.connectionId) : undefined));
  const connected = useConnections((s) => !!target && sessionOf(s.sessions, target.connectionId).status === "connected");
  const [review, setReview] = useState<"approve" | "lift" | null>(null);
  const decide = useAgent((s) => s.decide);
  const dangers = proposal.statements.filter((s) => s.danger !== null);
  const held = proposal.outcome?.statements.at(-1);
  const open = proposal.state === "pending" || proposal.state === "blocked";

  return (
    <div data-agent-proposal data-state={proposal.state} className="overflow-hidden rounded-lg border border-border-strong">
      <div className="flex items-center gap-2 border-b border-border bg-sunken px-2.5 py-1.5">
        <TerminalSquare className="size-3.5 shrink-0 text-fg-subtle" />
        <span className="flex-1 truncate text-xs font-semibold">{t("ai.proposal.title")}</span>
        <span data-agent-proposal-state className={cn("flex h-[18px] shrink-0 items-center gap-1 rounded px-1.5 text-[10px] leading-none font-bold", STATE_TONE[proposal.state])}>
          {proposal.state === "running" && <Spinner className="size-2.5" />}
          {t(`ai.proposal.state.${proposal.state === "rejected" && proposal.outcome ? "notRun" : proposal.state}`)}
        </span>
      </div>
      <div className="flex flex-col gap-2 p-2.5">
        {proposal.reason && (
          <p data-selectable className="text-xs leading-relaxed break-words text-fg-muted">
            {proposal.reason}
          </p>
        )}
        <pre data-selectable className={cn(preClass, "border border-border")}>
          {proposal.sql}
        </pre>
        {dangers.map((s, i) => (
          <div key={i} className="flex items-start gap-1.5 text-xs font-medium text-danger">
            <TriangleAlert className="mt-px size-3.5 shrink-0" />
            <span className="min-w-0 break-words">{s.danger}</span>
          </div>
        ))}
        {proposal.outcome && <Outcome outcome={proposal.outcome} />}
        {proposal.error && (
          <div role="alert" data-selectable className="rounded-md bg-danger-soft px-2.5 py-1.5 text-xs break-words text-danger">
            {t("ai.proposal.notRun", { message: proposal.error })}
          </div>
        )}
        {proposal.state === "rejected" && !proposal.outcome && <div className="text-xs text-fg-subtle">{t("ai.proposal.rejectedNote")}</div>}

        {open && !target && <div className="text-xs text-fg-subtle">{t("ai.proposal.noTarget")}</div>}
        {open && target && !connected && <div className="text-xs text-fg-subtle">{t("ai.proposal.closed", { name: connection?.name ?? "" })}</div>}
        {open && target && (
          <div className="flex flex-wrap items-center gap-1.5">
            {proposal.state === "pending" ? (
              <Button size="sm" variant="primary" disabled={!connected} onClick={() => setReview("approve")}>
                {t("ai.proposal.review")}
              </Button>
            ) : (
              <Button size="sm" variant="danger" disabled={!connected} onClick={() => setReview("lift")}>
                {t("ai.proposal.lift")}
              </Button>
            )}
            <Button size="sm" variant="ghost" icon={<Ban className="size-3.5" />} onClick={() => void decide(proposal.id, "reject")}>
              {t(proposal.state === "pending" ? "ai.proposal.reject" : "ai.proposal.dontRun")}
            </Button>
            {connected && (
              <Button
                size="sm"
                variant="ghost"
                className="ml-auto"
                title={t("ai.openInQueryHint")}
                onClick={() => useWorkspace.getState().openSql(target.connectionId, target.database, proposal.sql)}
              >
                {t("ai.openInQuery")}
              </Button>
            )}
          </div>
        )}
        {proposal.state === "running" && (
          <div>
            <Button size="sm" variant="danger" icon={<Square className="size-3 fill-current" />} onClick={() => void ipc.cancelSql(`proposal:${proposal.id}`).catch(() => {})}>
              {t("ai.stop")}
            </Button>
          </div>
        )}
      </div>

      {/* The approval: the whole SQL, where it runs, and what guards it. */}
      <AlertDialog.Root open={review !== null} onOpenChange={(next) => !next && setReview(null)}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
          <AlertDialog.Content
            data-agent-review
            className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[84vh] w-[min(94vw,640px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised p-5 shadow-pop outline-none"
          >
            <div className="flex gap-3.5">
              <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-danger-soft text-danger">
                <OctagonX className="size-5" />
              </div>
              <div className="min-w-0 flex-1">
                <AlertDialog.Title className="text-[15px] font-semibold">{t(review === "lift" ? "ai.proposal.liftTitle" : "ai.proposal.reviewTitle")}</AlertDialog.Title>
                <AlertDialog.Description className="mt-1 text-[13px] leading-relaxed text-fg-muted">
                  {review === "lift" ? t("ai.proposal.liftBody", { count: held?.rowsAffected ?? 0 }) : t("ai.proposal.reviewBody")}
                </AlertDialog.Description>
              </div>
            </div>
            <div className="mt-4 flex flex-wrap items-center gap-2 rounded-lg bg-sunken px-3 py-2 text-xs">
              <span className="text-fg-subtle">{t("ai.proposal.runsOn")}</span>
              <span className="font-semibold">{connection?.name}</span>
              {connection && <EnvBadge environment={connection.environment} long />}
              <span className="font-mono text-fg-muted">{[target?.database, target?.schema].filter(Boolean).join(" › ")}</span>
            </div>
            {connection?.environment === "production" && (
              <div className="mt-2 flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-xs font-medium text-danger">
                <TriangleAlert className="mt-px size-3.5 shrink-0" />
                {t("ai.proposal.production")}
              </div>
            )}
            <pre data-selectable className="mt-3 min-h-16 flex-1 overflow-auto rounded-lg border border-border bg-surface p-3 font-mono text-[12px] leading-[1.55] whitespace-pre-wrap break-words">
              {/* What the backend runs: the held statement and everything after it. */}
              {review === "lift" ? (proposal.outcome?.resume ?? held?.sql ?? "") : proposal.sql}
            </pre>
            {review === "lift" && (proposal.outcome?.skipped ?? 0) > 0 && (
              <div data-agent-lift-rest className="mt-3 flex items-start gap-1.5 text-xs font-medium text-danger">
                <TriangleAlert className="mt-px size-3.5 shrink-0" />
                <span className="min-w-0 break-words">{t("ai.proposal.liftRest", { count: proposal.outcome?.skipped ?? 0 })}</span>
              </div>
            )}
            {review === "approve" && (
              <ul className="mt-3 flex flex-col gap-1 text-xs text-fg-muted">
                {dangers.map((s, i) => (
                  <li key={i} className="flex items-start gap-1.5 font-medium text-danger">
                    <TriangleAlert className="mt-px size-3.5 shrink-0" />
                    <span className="min-w-0 break-words">
                      <span className="font-mono">{firstLine(s.text)}</span> — {s.danger}
                    </span>
                  </li>
                ))}
                <li>{t("ai.proposal.guardNote", { count: connection?.bulkReviewRows ?? 10 })}</li>
                <li>{t("ai.proposal.orderNote")}</li>
              </ul>
            )}
            <div className="mt-5 flex justify-end gap-2">
              <AlertDialog.Cancel asChild>
                <Button variant="ghost">{t("common.cancel")}</Button>
              </AlertDialog.Cancel>
              <Button
                variant="danger"
                onClick={() => {
                  const action = review;
                  setReview(null);
                  if (action) void decide(proposal.id, action);
                }}
              >
                {t(review === "lift" ? "ai.proposal.liftRun" : "ai.proposal.approve")}
              </Button>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </div>
  );
}
