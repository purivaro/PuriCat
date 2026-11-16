import { create } from "zustand";

import { ipc, toAppError } from "../lib/ipc";
import type { AgentAccess, AiProvider, ChatMessage, RunOutcome, SqlStatement, ToolScope } from "../lib/types";

/** One call the model made to a tool. */
export interface ToolStep {
  id: string;
  name: string;
  /** The SQL or the name it asked about. */
  input: string;
  /** Where in the answer's text the call happened. */
  at: number;
  status: "running" | "ok" | "error";
  /** Exactly the text the model was given. */
  output?: string;
}

/** SQL the model wants run, and what became of it. */
export interface ChatProposal {
  id: string;
  sql: string;
  reason: string | null;
  statements: SqlStatement[];
  at: number;
  /**
   * `pending`: waiting for the user. `blocked`: approved, then rolled back by the row limit
   * (what was held back can still be run). `done`, `failed`, `rejected` are final.
   */
  state: "pending" | "running" | "done" | "failed" | "blocked" | "rejected";
  /** What running it did. */
  outcome?: RunOutcome;
  /** Why it could not be run at all (nothing ran; it can be approved again). */
  error?: string;
}

export interface ChatTurn {
  id: string;
  role: "user" | "assistant";
  text: string;
  /** Assistant turns: how the answer ended. User turns are always "done". */
  status: "streaming" | "done" | "stopped" | "error";
  /** The CLI's own error message. */
  error?: string;
  /** Assistant turns: the provider asked, and the model the CLI says it used. */
  provider?: AiProvider;
  model?: string | null;
  /** User turns: the context that went with the message, exactly as sent (`null`: none). */
  context?: string | null;
  /** User turns: where SQL from the answer should open. */
  target?: ChatTarget | null;
  /** Assistant turns: the tools the model called and the SQL it proposed, in order. */
  steps?: ToolStep[];
  proposals?: ChatProposal[];
}

/** The connection and database a conversation turn is about. */
export interface ChatTarget {
  connectionId: string;
  database: string;
  schema: string | null;
}

interface Preferences {
  provider: AiProvider;
  /** Model per provider; "" leaves the choice to the CLI. */
  models: Record<AiProvider, string>;
  shareContext: boolean;
  /** What the AI may do with the database in front. Off unless the user turns it on. */
  access: AgentAccess;
}

interface AgentState extends Preferences {
  loaded: boolean;
  turns: ChatTurn[];
  /** Job id of the turn that is running, for Stop. */
  running: string | null;
  load: () => Promise<void>;
  setProvider: (provider: AiProvider) => void;
  setModel: (model: string) => void;
  setShareContext: (share: boolean) => void;
  setAccess: (access: AgentAccess) => void;
  /** Approve a proposal (PuriCat runs it), run what the row limit held back, or turn it down. */
  decide: (proposalId: string, action: "approve" | "lift" | "reject") => Promise<void>;
  /** Send a message. `context` resolves to the text to share, or `null` for none. */
  send: (text: string, context: () => Promise<string | null>, target: ChatTarget | null) => Promise<void>;
  /** Ask again with the last user message (after an error or a stop). */
  retry: () => Promise<void>;
  stop: () => void;
  reset: () => void;
}

const SETTINGS_KEY = "agent";
const isProvider = (v: unknown): v is AiProvider => v === "claude" || v === "codex";
const isAccess = (v: unknown): v is AgentAccess => v === "none" || v === "schema" || v === "data";

const oneLine = (sql: string) => {
  const flat = sql.replace(/\s+/g, " ").trim();
  return flat.length > 300 ? `${flat.slice(0, 300)}…` : flat;
};

/** What happened to a proposal, for the model: it only knows what it is told. */
function proposalNote(p: ChatProposal): string {
  const last = p.outcome?.statements.at(-1);
  const affected = p.outcome?.statements.reduce((n, s) => n + (s.rowsAffected ?? 0), 0) ?? 0;
  const held = `stopped by PuriCat's row limit (it would have changed ${last?.rowsAffected ?? "too many"} rows) and rolled back`;
  const fate = {
    pending: "waiting for the user's decision; it has not been run",
    running: "approved by the user; it is running",
    rejected: p.outcome ? `${held}; the user decided not to run it` : "rejected by the user; it was not run",
    done: `approved by the user and run by PuriCat (${affected} rows affected)`,
    blocked: `approved by the user, then ${held}; the user has not decided whether to run it without the limit`,
    failed: `approved by the user, but it failed${last?.message ? `: ${last.message}` : ""}`,
  }[p.state];
  return `[Proposed SQL: ${oneLine(p.sql)} — ${fate}]`;
}

/**
 * The conversation as the provider should see it: answers that failed are left out. The
 * CLI keeps nothing between turns, so what the tools did and what became of each proposal
 * is told again in a line under the answer it belongs to.
 */
export function transcript(turns: ChatTurn[]): ChatMessage[] {
  return turns
    .filter((t) => t.role === "user" || t.status === "done" || t.status === "stopped")
    .map((t) => {
      const calls = (t.steps ?? []).filter((s) => s.name !== "propose_sql").map((s) => (s.input ? `${s.name} (${oneLine(s.input)})` : s.name));
      const notes = [...(calls.length > 0 ? [`[Tools used: ${calls.join("; ")}]`] : []), ...(t.proposals ?? []).map(proposalNote)];
      return { role: t.role, text: [t.text.trim(), ...notes].filter(Boolean).join("\n") };
    })
    .filter((m) => m.text !== "");
}

/** The tools of a turn: bound to the tab the question was asked about, if the user allows any. */
function toolScope(access: AgentAccess, target: ChatTarget | null | undefined): ToolScope | null {
  if (access === "none" || !target) return null;
  return { connectionId: target.connectionId, database: target.database, schema: target.schema, readRows: access === "data" };
}

export const useAgent = create<AgentState>((set, get) => {
  const persist = () => {
    const { provider, models, shareContext, access } = get();
    void ipc.setSetting(SETTINGS_KEY, JSON.stringify({ provider, models, shareContext, access }));
  };
  const patchProposal = (id: string, patch: Partial<ChatProposal>) =>
    set((s) => ({
      turns: s.turns.map((t) => (t.proposals?.some((p) => p.id === id) ? { ...t, proposals: t.proposals.map((p) => (p.id === id ? { ...p, ...patch } : p)) } : t)),
    }));
  const patchTurn = (id: string, patch: Partial<ChatTurn> | ((t: ChatTurn) => Partial<ChatTurn>)) =>
    set((s) => ({ turns: s.turns.map((t) => (t.id === id ? { ...t, ...(typeof patch === "function" ? patch(t) : patch) } : t)) }));

  /** Run the turn for the user message that is already last in `turns`. */
  const answer = async (context: string | null) => {
    const { provider, models, turns, access } = get();
    const job = crypto.randomUUID();
    const id = crypto.randomUUID();
    const messages = transcript(turns);
    const question = [...turns].reverse().find((t) => t.role === "user");
    const tools = toolScope(access, question?.target);
    set({ running: job, turns: [...turns, { id, role: "assistant", text: "", status: "streaming", provider, model: models[provider] || null }] });
    try {
      const text = await ipc.agentSend(job, { provider, model: models[provider] || null, context, messages, tools }, (event) => {
        switch (event.type) {
          // The CLI names the model it resolved to; until then (or if it does not) show what was asked for.
          case "started":
            return patchTurn(id, (t) => ({ model: event.model ?? t.model }));
          case "delta":
            return patchTurn(id, (t) => ({ text: t.text + event.text }));
          case "message":
            return patchTurn(id, { text: event.text });
          case "toolStarted":
            return patchTurn(id, (t) => ({ steps: [...(t.steps ?? []), { id: event.id, name: event.name, input: event.input, at: t.text.length, status: "running" }] }));
          case "toolFinished":
            return patchTurn(id, (t) => ({
              steps: t.steps?.map((s) => (s.id === event.id ? { ...s, status: event.ok ? "ok" : "error", output: event.output } : s)),
            }));
          case "proposal":
            return patchTurn(id, (t) => ({
              proposals: [...(t.proposals ?? []), { id: event.id, sql: event.sql, reason: event.reason, statements: event.statements, at: t.text.length, state: "pending" }],
            }));
        }
      });
      patchTurn(id, { text, status: "done" });
    } catch (e) {
      const error = toAppError(e);
      // "Stopped" is the user's own doing: keep what had arrived, without an error.
      if (error.kind === "conflict") patchTurn(id, { status: "stopped" });
      else patchTurn(id, { status: "error", error: error.message });
    } finally {
      // A call that never got its answer did not finish.
      patchTurn(id, (t) => ({ steps: t.steps?.map((s) => (s.status === "running" ? { ...s, status: "error" } : s)) }));
      if (get().running === job) set({ running: null });
    }
  };

  return {
    provider: "claude",
    models: { claude: "", codex: "" },
    shareContext: true,
    access: "none",
    loaded: false,
    turns: [],
    running: null,

    load: async () => {
      if (get().loaded) return;
      let saved: Partial<Preferences> = {};
      try {
        const all = await ipc.getSettings();
        saved = all[SETTINGS_KEY] ? (JSON.parse(all[SETTINGS_KEY]) as Partial<Preferences>) : {};
      } catch {
        /* missing or corrupt: defaults */
      }
      const model = (p: AiProvider) => (typeof saved.models?.[p] === "string" ? saved.models[p] : "");
      set({
        loaded: true,
        provider: isProvider(saved.provider) ? saved.provider : "claude",
        models: { claude: model("claude"), codex: model("codex") },
        shareContext: saved.shareContext !== false,
        access: isAccess(saved.access) ? saved.access : "none",
      });
    },

    setProvider: (provider) => {
      set({ provider });
      persist();
    },
    setModel: (model) => {
      set((s) => ({ models: { ...s.models, [s.provider]: model.trim() } }));
      persist();
    },
    setShareContext: (shareContext) => {
      set({ shareContext });
      persist();
    },
    setAccess: (access) => {
      set({ access });
      persist();
    },

    decide: async (proposalId, action) => {
      const proposal = get()
        .turns.flatMap((t) => t.proposals ?? [])
        .find((p) => p.id === proposalId);
      if (!proposal || proposal.state === "running") return;
      if (action === "reject") {
        if (proposal.state !== "pending" && proposal.state !== "blocked") return;
        // Turning down what the row limit held back keeps the outcome of what did run.
        patchProposal(proposalId, { state: "rejected", error: undefined });
        return void ipc.agentProposalReject(proposalId).catch(() => {});
      }
      const lift = action === "lift";
      if (proposal.state !== (lift ? "blocked" : "pending")) return;
      patchProposal(proposalId, { state: "running", error: undefined });
      try {
        const run = await ipc.agentProposalRun(proposalId, lift);
        // Running what was held back replaces the held statement; what ran before it stays on record.
        const before = lift ? (proposal.outcome?.statements.slice(0, -1) ?? []) : [];
        const outcome = { ...run, statements: [...before, ...run.statements] };
        const status = outcome.statements.at(-1)?.status ?? "error";
        patchProposal(proposalId, { outcome, state: status === "ok" ? "done" : status === "blocked" ? "blocked" : "failed" });
      } catch (e) {
        // Nothing ran (the connection is closed, or read-only): it is still the user's to decide.
        patchProposal(proposalId, { state: proposal.state, error: toAppError(e).message });
      }
    },

    send: async (text, context, target) => {
      const message = text.trim();
      if (!message || get().running) return;
      // Claim the turn before the context is fetched, so a second Enter cannot start another.
      const id = crypto.randomUUID();
      const claim = `starting:${id}`;
      set((s) => ({ running: claim, turns: [...s.turns, { id, role: "user", text: message, status: "done", context: null, target }] }));
      let shared: string | null = null;
      try {
        shared = await context();
      } catch {
        /* the context could not be read (connection closed): ask without it */
      }
      // Stopped, or the chat was cleared, while the context was being read: nothing is sent.
      if (get().running !== claim) return;
      patchTurn(id, { context: shared });
      await answer(shared);
    },

    retry: async () => {
      const { turns, running } = get();
      if (running) return;
      const last = turns.map((t) => t.role).lastIndexOf("user");
      if (last < 0) return;
      // Drop the failed or stopped answer; the question is asked again as it was.
      set({ turns: turns.slice(0, last + 1) });
      await answer(turns[last]!.context ?? null);
    },

    stop: () => {
      const { running } = get();
      if (!running) return;
      // Before the CLI has started there is no job to cancel: giving up the claim is enough.
      if (running.startsWith("starting:")) set({ running: null });
      else void ipc.cancelJob(running);
    },

    reset: () => {
      get().stop();
      // Proposals nobody decided on go with the conversation: they can no longer be approved.
      for (const p of get().turns.flatMap((t) => t.proposals ?? [])) {
        if (p.state === "pending" || p.state === "blocked") void ipc.agentProposalReject(p.id).catch(() => {});
      }
      set({ turns: [], running: null });
    },
  };
});
