import { ArrowUp, Check, CircleAlert, Copy, Database, Eye, RotateCcw, Sparkles, Square, TerminalSquare, TriangleAlert, Wrench } from "lucide-react";
import { Popover } from "radix-ui";
import { Fragment, type KeyboardEvent, memo, type ReactNode, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, IconButton, Spinner, Switch } from "../../components/ui/primitives";
import { clipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { ipc } from "../../lib/ipc";
import type { AgentAccess, AiProvider, ModelChoice } from "../../lib/types";
import { type ChatProposal, type ChatTarget, type ChatTurn, type ToolStep, useAgent } from "../../stores/agent";
import { sessionOf, useConnections } from "../../stores/connections";
import { sqlEditors, useWorkspace } from "../../stores/workspace";
import { ProposalCard, ToolSteps } from "./AgentActivity";
import { type Block, type Inline, isSql, parseMarkdown } from "./markdown";

const MODEL_NAME = /^(?!-)[A-Za-z0-9._:/@[\]-]{1,100}$/;
const fieldClass =
  "h-7 rounded-md border border-border-strong bg-surface px-2 text-xs text-fg hover:border-fg-subtle focus:border-accent focus:outline-none disabled:opacity-50";
const popoverClass = "animate-pop z-[60] rounded-xl border border-border bg-raised p-3 text-[13px] text-fg shadow-pop outline-none";

// ---- what the conversation is about ----------------------------------------------------

interface Scope {
  target: ChatTarget | null;
  /** Shown in the app only; never sent. */
  label: string | null;
  /** Why there is no context to share. */
  missing: "noTab" | "closed" | "noDatabase" | null;
  /** The active query tab, whose text goes along with the schema. */
  sqlTabId: string | null;
}

/** The chat follows the tab in front: its connection, database and (for a query tab) its SQL. */
function useScope(): Scope {
  const tab = useWorkspace((s) => s.tabs.find((t) => t.id === s.activeId));
  const connection = useConnections((s) => (tab && tab.kind !== "welcome" ? s.connections.find((c) => c.id === tab.connectionId) : undefined));
  const session = useConnections((s) => (connection ? sessionOf(s.sessions, connection.id) : undefined));
  return useMemo(() => {
    if (!tab || tab.kind === "welcome" || !connection) return { target: null, label: null, missing: "noTab", sqlTabId: null };
    if (session?.status !== "connected") return { target: null, label: connection.name, missing: "closed", sqlTabId: null };
    const schemas = session.info.capabilities.schemas;
    const database = tab.kind === "table" ? tab.table.database : (tab.database ?? connection.database ?? (schemas ? "postgres" : null));
    if (!database) return { target: null, label: connection.name, missing: "noDatabase", sqlTabId: null };
    const schema = tab.kind === "table" ? (tab.table.schema ?? null) : schemas ? "public" : null;
    return {
      target: { connectionId: connection.id, database, schema },
      label: [connection.name, database, schema].filter(Boolean).join(" › "),
      missing: null,
      sqlTabId: tab.kind === "sql" ? tab.id : null,
    };
  }, [tab, connection, session]);
}

/** The context text for a scope, as the backend renders it: this is what gets sent. */
const readContext = (scope: Scope) =>
  scope.target
    ? ipc.agentContext(
        scope.target.connectionId,
        scope.target.database,
        scope.target.schema,
        scope.sqlTabId ? (sqlEditors.get(scope.sqlTabId)?.() ?? null) : null,
      )
    : Promise.resolve(null);

// ---- rendering an answer -----------------------------------------------------------------

function InlineText({ inline }: { inline: Inline[] }) {
  return (
    <>
      {inline.map((part, i) =>
        part.kind === "code" ? (
          <code key={i} className="rounded bg-sunken px-1 py-px font-mono text-[12px]">
            {part.text}
          </code>
        ) : part.kind === "strong" ? (
          <strong key={i} className="font-semibold">
            {part.text}
          </strong>
        ) : (
          <Fragment key={i}>{part.text}</Fragment>
        ),
      )}
    </>
  );
}

function CodeBlock({ block, target }: { block: Extract<Block, { kind: "code" }>; target: ChatTarget | null }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const connected = useConnections((s) => !!target && sessionOf(s.sessions, target.connectionId).status === "connected");
  const runnable = isSql(block) && !block.open && block.text.trim() !== "";

  const copy = () =>
    void clipboard.write(block.text).then(
      () => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      },
      () => toast.error(t("ai.copyFailed")),
    );

  return (
    <div data-agent-code className="overflow-hidden rounded-lg border border-border">
      <div className="flex h-8 items-center gap-1 border-b border-border bg-sunken pr-1 pl-2.5">
        <span className="flex-1 truncate font-mono text-[11px] text-fg-subtle uppercase">{block.lang || t("ai.code")}</span>
        {runnable && target && connected && (
          <Button
            size="sm"
            variant="ghost"
            className="h-6 px-2 text-accent-text hover:text-accent-text"
            icon={<TerminalSquare className="size-3.5" />}
            title={t("ai.openInQueryHint")}
            onClick={() => useWorkspace.getState().openSql(target.connectionId, target.database, block.text)}
          >
            {t("ai.openInQuery")}
          </Button>
        )}
        <IconButton label={copied ? t("ai.copied") : t("ai.copy")} className="size-6" onClick={copy}>
          {copied ? <Check className="size-3.5 text-ok" /> : <Copy className="size-3.5" />}
        </IconButton>
      </div>
      <pre data-selectable className="overflow-x-auto bg-surface px-3 py-2.5 font-mono text-[12px] leading-[1.55] text-fg">
        {block.text}
      </pre>
    </div>
  );
}

const Answer = memo(function Answer({ text, target }: { text: string; target: ChatTarget | null }) {
  const blocks = useMemo(() => parseMarkdown(text), [text]);
  return (
    <div data-selectable className="flex min-w-0 flex-col gap-2.5 text-[13px] leading-relaxed break-words">
      {blocks.map((block, i) => {
        switch (block.kind) {
          case "code":
            return <CodeBlock key={i} block={block} target={target} />;
          case "heading":
            return (
              <div key={i} className={cn("font-semibold", block.level <= 2 ? "text-[14px]" : "text-[13px]")}>
                <InlineText inline={block.inline} />
              </div>
            );
          case "list": {
            const List = block.ordered ? "ol" : "ul";
            return (
              <List key={i} className={cn("flex flex-col gap-1 pl-5", block.ordered ? "list-decimal" : "list-disc")}>
                {block.items.map((item, n) => (
                  <li key={n} className="whitespace-pre-wrap">
                    <InlineText inline={item} />
                  </li>
                ))}
              </List>
            );
          }
          case "table":
            return (
              <div key={i} className="overflow-x-auto rounded-lg border border-border">
                <table className="w-full border-collapse text-xs">
                  <thead className="bg-sunken text-left">
                    <tr>
                      {block.header.map((cell, c) => (
                        <th key={c} className="border-b border-border px-2 py-1.5 font-semibold whitespace-nowrap">
                          <InlineText inline={cell} />
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {block.rows.map((row, r) => (
                      <tr key={r} className="border-b border-border last:border-0">
                        {row.map((cell, c) => (
                          <td key={c} className="px-2 py-1.5 align-top">
                            <InlineText inline={cell} />
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          default:
            return (
              <p key={i} className="whitespace-pre-wrap">
                <InlineText inline={block.inline} />
              </p>
            );
        }
      })}
    </div>
  );
});

/** The context that went with one message, exactly as it was sent. */
function SentContext({ context }: { context: string | null | undefined }) {
  const { t } = useTranslation();
  if (!context) return <span className="text-[11px] text-fg-subtle">{t("ai.contextNotSent")}</span>;
  return (
    <Popover.Root>
      <Popover.Trigger className="flex items-center gap-1 rounded text-[11px] text-fg-subtle hover:text-fg">
        <Eye className="size-3" /> {t("ai.contextSent")}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content align="end" sideOffset={6} collisionPadding={12} className={cn(popoverClass, "w-[min(92vw,460px)]")}>
          <div className="font-semibold">{t("ai.contextSentTitle")}</div>
          <pre data-selectable className="mt-2 max-h-[50vh] overflow-auto rounded-lg bg-sunken p-2.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap">
            {context}
          </pre>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

/**
 * An answer with what the model did where it did it: text, the tools it called, the SQL it
 * proposed. A proposal that was not accepted shows as a failed step; an accepted one is
 * its card.
 */
function AnswerBody({ turn, target }: { turn: ChatTurn; target: ChatTarget | null }) {
  const marks: ({ at: number; step: ToolStep } | { at: number; proposal: ChatProposal })[] = [
    ...(turn.steps ?? []).filter((s) => s.name !== "propose_sql" || s.status === "error").map((step) => ({ at: step.at, step })),
    ...(turn.proposals ?? []).map((proposal) => ({ at: proposal.at, proposal })),
  ].sort((a, b) => a.at - b.at);

  const parts: ReactNode[] = [];
  let from = 0;
  let group: ToolStep[] = [];
  const flush = () => {
    if (group.length > 0) parts.push(<ToolSteps key={`steps-${group[0]!.id}`} steps={group} />);
    group = [];
  };
  const text = (to: number) => {
    const piece = turn.text.slice(from, to);
    from = to;
    if (piece.trim() === "") return;
    flush();
    parts.push(<Answer key={`text-${parts.length}`} text={piece} target={target} />);
  };
  for (const mark of marks) {
    text(Math.min(mark.at, turn.text.length));
    if ("step" in mark) group.push(mark.step);
    else {
      flush();
      parts.push(<ProposalCard key={mark.proposal.id} proposal={mark.proposal} target={target} />);
    }
  }
  flush();
  text(turn.text.length);
  return <>{parts}</>;
}

function Turn({ turn, target, last, busy }: { turn: ChatTurn; target: ChatTarget | null; last: boolean; busy: boolean }) {
  const { t } = useTranslation();
  if (turn.role === "user") {
    return (
      <div data-agent-turn="user" className="flex flex-col items-end gap-1">
        <div data-selectable className="max-w-[92%] rounded-2xl rounded-br-md bg-accent-soft px-3 py-2 text-[13px] leading-relaxed break-words whitespace-pre-wrap">
          {turn.text}
        </div>
        <SentContext context={turn.context} />
      </div>
    );
  }
  const acted = (turn.steps?.length ?? 0) + (turn.proposals?.length ?? 0) > 0;
  const waiting = turn.status === "streaming" && turn.text === "" && !acted;
  return (
    <div data-agent-turn="assistant" data-status={turn.status} className="flex min-w-0 flex-col gap-2">
      <div className="flex items-center gap-1.5 text-[11px] text-fg-subtle">
        <Sparkles className="size-3 text-accent-text" />
        <span className="font-medium text-fg-muted">{t(`ai.${turn.provider ?? "claude"}`)}</span>
        {turn.model && <span className="truncate font-mono">{turn.model}</span>}
      </div>
      {waiting ? (
        <div className="flex items-center gap-2 text-[13px] text-fg-muted">
          <Spinner className="size-3.5" /> {t("ai.thinking")}
        </div>
      ) : (
        <AnswerBody turn={turn} target={target} />
      )}
      {turn.status === "streaming" && !waiting && <span aria-hidden className="h-3.5 w-1.5 animate-pulse rounded-sm bg-accent" />}
      {turn.status === "stopped" && <div className="text-xs text-fg-subtle">{t("ai.stopped")}</div>}
      {turn.status === "error" && (
        <div role="alert" data-selectable className="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          <div className="min-w-0 flex-1">
            <div className="font-medium">{t("ai.failed", { name: t(`ai.${turn.provider ?? "claude"}`) })}</div>
            <div className="mt-0.5 text-xs break-words">{turn.error}</div>
          </div>
        </div>
      )}
      {last && (turn.status === "error" || turn.status === "stopped") && (
        <div>
          <Button size="sm" disabled={busy} icon={<RotateCcw className="size-3.5" />} onClick={() => void useAgent.getState().retry()}>
            {t("ai.retry")}
          </Button>
        </div>
      )}
    </div>
  );
}

// ---- composer controls -------------------------------------------------------------------

function ModelSelect({ provider, disabled }: { provider: AiProvider; disabled: boolean }) {
  const { t } = useTranslation();
  const model = useAgent((s) => s.models[provider]);
  const [choices, setChoices] = useState<ModelChoice[] | null>(null);
  const [typing, setTyping] = useState(false);
  const [draft, setDraft] = useState("");

  useEffect(() => {
    let stale = false;
    setChoices(null);
    setTyping(false);
    ipc
      .agentModels(provider)
      .then((list) => !stale && setChoices(list))
      .catch(() => !stale && setChoices([]));
    return () => {
      stale = true;
    };
  }, [provider]);

  if (typing) {
    const valid = draft.trim() === "" || MODEL_NAME.test(draft.trim());
    const commit = () => {
      if (!valid) return;
      useAgent.getState().setModel(draft);
      setTyping(false);
    };
    return (
      <input
        autoFocus
        data-agent-model-input
        aria-label={t("ai.modelCustom")}
        aria-invalid={!valid}
        placeholder={t("ai.modelPlaceholder")}
        value={draft}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => (valid ? commit() : setTyping(false))}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setTyping(false);
        }}
        title={valid ? t("ai.modelCustomHint") : t("ai.modelInvalid")}
        className={cn(fieldClass, "w-40 min-w-0 font-mono", !valid && "border-danger focus:border-danger")}
      />
    );
  }
  const listed = model === "" || !!choices?.some((c) => c.id === model);
  return (
    <select
      data-agent-model
      aria-label={t("ai.model")}
      title={t("ai.modelHint")}
      value={model}
      disabled={disabled}
      onChange={(e) => {
        if (e.target.value !== "\u0000custom") return useAgent.getState().setModel(e.target.value);
        setDraft(model);
        setTyping(true);
      }}
      className={cn(fieldClass, "max-w-36 min-w-0")}
    >
      <option value="">{t("ai.modelDefault")}</option>
      {choices?.map((c) => (
        <option key={c.id} value={c.id} title={c.description ?? undefined}>
          {c.label}
        </option>
      ))}
      {!listed && <option value={model}>{model}</option>}
      <option value={"\u0000custom"}>{t("ai.modelOther")}</option>
    </select>
  );
}

function ContextButton({ scope }: { scope: Scope }) {
  const { t } = useTranslation();
  const share = useAgent((s) => s.shareContext);
  const [open, setOpen] = useState(false);
  const [preview, setPreview] = useState<{ text: string | null; error?: boolean } | "loading">("loading");

  useEffect(() => {
    if (!open) return;
    let stale = false;
    setPreview("loading");
    readContext(scope)
      .then((text) => !stale && setPreview({ text }))
      .catch(() => !stale && setPreview({ text: null, error: true }));
    return () => {
      stale = true;
    };
  }, [open, scope]);

  const active = share && scope.target !== null;
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger
        data-agent-context
        title={t("ai.contextHint")}
        className={cn(
          "flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-md border px-2 text-xs transition-colors",
          active ? "border-accent/40 bg-accent-soft text-accent-text" : "border-border-strong text-fg-muted hover:border-fg-subtle",
        )}
      >
        <Database className="size-3.5 shrink-0" />
        <span className="truncate">{active ? scope.label : scope.target ? t("ai.contextOff") : t(`ai.contextMissing.${scope.missing ?? "noTab"}`)}</span>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content align="start" side="top" sideOffset={8} collisionPadding={12} className={cn(popoverClass, "w-[min(92vw,460px)]")}>
          <div className="flex items-start gap-3">
            <div className="min-w-0 flex-1">
              <div className="font-semibold">{t("ai.contextTitle")}</div>
              <p className="mt-0.5 text-xs leading-relaxed text-fg-muted">{t("ai.contextBody")}</p>
            </div>
            <Switch checked={share} onChange={(v) => useAgent.getState().setShareContext(v)} label={t("ai.contextShare")} />
          </div>
          {!scope.target ? (
            <p className="mt-3 rounded-lg bg-sunken px-3 py-2 text-xs text-fg-muted">{t(`ai.contextMissingBody.${scope.missing ?? "noTab"}`)}</p>
          ) : !share ? (
            <p className="mt-3 rounded-lg bg-sunken px-3 py-2 text-xs text-fg-muted">{t("ai.contextOffBody")}</p>
          ) : preview === "loading" ? (
            <div className="mt-3 flex items-center gap-2 text-xs text-fg-muted">
              <Spinner className="size-3.5" /> {t("common.loading")}
            </div>
          ) : preview.error || !preview.text ? (
            <p role="alert" className="mt-3 rounded-lg bg-danger-soft px-3 py-2 text-xs text-danger">
              {t("ai.contextFailed")}
            </p>
          ) : (
            <>
              <div className="mt-3 text-[11px] font-medium text-fg-subtle uppercase">{t("ai.contextPreview")}</div>
              <pre data-agent-context-preview data-selectable className="mt-1 max-h-[42vh] overflow-auto rounded-lg bg-sunken p-2.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap">
                {preview.text}
              </pre>
            </>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** What the AI may do with the database in front: nothing, look at its structure, or also read rows. */
function AccessButton({ scope, provider }: { scope: Scope; provider: AiProvider }) {
  const { t } = useTranslation();
  const access = useAgent((s) => s.access);
  const production = useConnections((s) => !!scope.target && s.connections.find((c) => c.id === scope.target!.connectionId)?.environment === "production");
  const active = access !== "none" && scope.target !== null;
  return (
    <Popover.Root>
      <Popover.Trigger
        data-agent-access={access}
        title={t("ai.access.hint")}
        aria-label={`${t("ai.access.title")}: ${t(`ai.access.${access}.label`)}`}
        className={cn(
          "flex h-7 shrink-0 items-center gap-1.5 rounded-md border px-2 text-xs transition-colors",
          active ? "border-accent/40 bg-accent-soft text-accent-text" : "border-border-strong text-fg-muted hover:border-fg-subtle",
        )}
      >
        <Wrench className="size-3.5 shrink-0" />
        <span className="whitespace-nowrap">{t(`ai.access.short.${access}`)}</span>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content align="start" side="top" sideOffset={8} collisionPadding={12} className={cn(popoverClass, "w-[min(92vw,400px)]")}>
          <div className="font-semibold">{t("ai.access.title")}</div>
          <p className="mt-0.5 text-xs leading-relaxed text-fg-muted">{t("ai.access.body")}</p>
          <div role="radiogroup" aria-label={t("ai.access.title")} className="mt-3 flex flex-col gap-1.5">
            {(["none", "schema", "data"] as AgentAccess[]).map((level) => (
              <button
                key={level}
                type="button"
                role="radio"
                aria-checked={access === level}
                data-agent-access-option={level}
                onClick={() => useAgent.getState().setAccess(level)}
                className={cn(
                  "flex items-start gap-2.5 rounded-lg border px-3 py-2 text-left transition-colors",
                  access === level ? "border-accent bg-accent-soft" : "border-border hover:border-fg-subtle",
                )}
              >
                <span className={cn("mt-0.5 flex size-3.5 shrink-0 items-center justify-center rounded-full border", access === level ? "border-accent" : "border-border-strong")}>
                  {access === level && <span className="size-1.5 rounded-full bg-accent" />}
                </span>
                <span className="min-w-0">
                  <span className="block text-[13px] font-medium">{t(`ai.access.${level}.label`)}</span>
                  <span className="mt-0.5 block text-xs leading-relaxed text-fg-muted">{t(`ai.access.${level}.body`)}</span>
                </span>
              </button>
            ))}
          </div>
          {access !== "none" && !scope.target && <p className="mt-3 rounded-lg bg-sunken px-3 py-2 text-xs text-fg-muted">{t("ai.access.noTab")}</p>}
          {access === "data" && production && (
            <p role="alert" className="mt-3 flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-xs font-medium text-danger">
              <TriangleAlert className="mt-px size-3.5 shrink-0" />
              {t("ai.access.production")}
            </p>
          )}
          {access !== "none" && provider === "codex" && <p className="mt-3 rounded-lg bg-warn-soft px-3 py-2 text-xs text-warn">{t("ai.access.codex")}</p>}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

// ---- the chat ----------------------------------------------------------------------------

export function AgentChat({ provider }: { provider: AiProvider }) {
  const { t } = useTranslation();
  const turns = useAgent((s) => s.turns);
  const running = useAgent((s) => s.running);
  const access = useAgent((s) => s.access);
  const scope = useScope();
  const [draft, setDraft] = useState("");
  const listRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const pinned = useRef(true);

  // Follow the answer as it streams, unless the reader scrolled up to look at something.
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list && pinned.current) list.scrollTop = list.scrollHeight;
  }, [turns]);

  // The box grows with what is typed, up to a few lines.
  useLayoutEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 132)}px`;
  }, [draft]);

  const send = () => {
    if (running || !draft.trim()) return;
    const { shareContext, send: submit } = useAgent.getState();
    pinned.current = true;
    void submit(draft, () => (shareContext ? readContext(scope) : Promise.resolve(null)), scope.target);
    setDraft("");
  };
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter sends; Shift+Enter is a new line. Enter that confirms an IME composition is neither.
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    send();
  };

  // SQL in an answer opens on the connection its question was asked about.
  let target: ChatTarget | null = null;
  const rows: ReactNode[] = turns.map((turn, i) => {
    if (turn.role === "user") target = turn.target ?? null;
    return <Turn key={turn.id} turn={turn} target={target} last={i === turns.length - 1} busy={running !== null} />;
  });

  return (
    <div data-agent-chat className="flex min-h-0 flex-1 flex-col">
      <div
        ref={listRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
        }}
        className="min-h-0 flex-1 overflow-y-auto px-3 pb-3"
      >
        {turns.length === 0 ? (
          <div className="flex min-h-full flex-col justify-center gap-4 py-4">
            <div className="flex flex-col items-center gap-2 text-center">
              <div className="flex size-10 items-center justify-center rounded-xl bg-accent-soft text-accent-text">
                <Sparkles className="size-5" />
              </div>
              <div className="text-[14px] font-semibold">{t("ai.emptyTitle")}</div>
              <p className="max-w-64 text-xs leading-relaxed text-fg-muted">{t("ai.emptyBody")}</p>
            </div>
            <div className="flex flex-col gap-1.5">
              {(["explain", "recent", "faster"] as const).map((key) => (
                <button
                  key={key}
                  type="button"
                  onClick={() => {
                    setDraft(t(`ai.suggestions.${key}`));
                    inputRef.current?.focus();
                  }}
                  className="rounded-lg border border-border px-3 py-2 text-left text-xs leading-snug text-fg-muted transition-colors hover:border-accent/50 hover:bg-accent-soft hover:text-accent-text"
                >
                  {t(`ai.suggestions.${key}`)}
                </button>
              ))}
            </div>
            <ul data-agent-policy className="flex list-disc flex-col gap-1.5 rounded-xl bg-sunken py-3 pr-3 pl-7 text-xs leading-relaxed text-fg-muted">
              <li>{t(access === "none" ? "ai.policy1" : `ai.policyAccess.${access}`)}</li>
              <li>{t(access === "none" ? "ai.policy2" : "ai.policyApprove")}</li>
              <li>{t(access === "data" ? "ai.policyShareRows" : "ai.policy3")}</li>
            </ul>
          </div>
        ) : (
          <div className="flex flex-col gap-4 pt-1">{rows}</div>
        )}
      </div>

      <div className="shrink-0 border-t border-border p-2.5">
        <div className="flex items-center gap-1.5">
          <ContextButton scope={scope} />
          <AccessButton scope={scope} provider={provider} />
          <ModelSelect provider={provider} disabled={running !== null} />
        </div>
        <div className="mt-2 flex items-end gap-1.5 rounded-xl border border-border-strong bg-surface p-1.5 focus-within:border-accent">
          <textarea
            ref={inputRef}
            data-agent-input
            rows={1}
            value={draft}
            aria-label={t("ai.inputLabel")}
            placeholder={t("ai.inputPlaceholder", { name: t(`ai.${provider}`) })}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onKeyDown}
            className="max-h-[132px] min-h-7 flex-1 resize-none bg-transparent px-1.5 py-1 text-[13px] leading-relaxed text-fg outline-none placeholder:text-fg-subtle"
          />
          {running ? (
            <Button size="sm" variant="danger" className="size-7 px-0" aria-label={t("ai.stop")} title={t("ai.stop")} onClick={() => useAgent.getState().stop()}>
              <Square className="size-3 fill-current" />
            </Button>
          ) : (
            <Button size="sm" variant="primary" className="size-7 px-0" aria-label={t("ai.send")} title={t("ai.sendHint")} disabled={!draft.trim()} onClick={send}>
              <ArrowUp className="size-4" />
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
