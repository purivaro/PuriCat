import { create } from "zustand";

import { ipc, toAppError } from "../lib/ipc";
import type { AiProvider, ChatMessage } from "../lib/types";

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
  /** Send a message. `context` resolves to the text to share, or `null` for none. */
  send: (text: string, context: () => Promise<string | null>, target: ChatTarget | null) => Promise<void>;
  /** Ask again with the last user message (after an error or a stop). */
  retry: () => Promise<void>;
  stop: () => void;
  reset: () => void;
}

const SETTINGS_KEY = "agent";
const isProvider = (v: unknown): v is AiProvider => v === "claude" || v === "codex";

/** The conversation as the provider should see it: answers that failed are left out. */
export function transcript(turns: ChatTurn[]): ChatMessage[] {
  return turns
    .filter((t) => t.text.trim() !== "" && (t.role === "user" || t.status === "done" || t.status === "stopped"))
    .map((t) => ({ role: t.role, text: t.text }));
}

export const useAgent = create<AgentState>((set, get) => {
  const persist = () => {
    const { provider, models, shareContext } = get();
    void ipc.setSetting(SETTINGS_KEY, JSON.stringify({ provider, models, shareContext }));
  };
  const patchTurn = (id: string, patch: Partial<ChatTurn> | ((t: ChatTurn) => Partial<ChatTurn>)) =>
    set((s) => ({ turns: s.turns.map((t) => (t.id === id ? { ...t, ...(typeof patch === "function" ? patch(t) : patch) } : t)) }));

  /** Run the turn for the user message that is already last in `turns`. */
  const answer = async (context: string | null) => {
    const { provider, models, turns } = get();
    const job = crypto.randomUUID();
    const id = crypto.randomUUID();
    const messages = transcript(turns);
    set({ running: job, turns: [...turns, { id, role: "assistant", text: "", status: "streaming", provider, model: models[provider] || null }] });
    try {
      const text = await ipc.agentSend(job, { provider, model: models[provider] || null, context, messages }, (event) => {
        // The CLI names the model it resolved to; until then (or if it does not) show what was asked for.
        if (event.type === "started") patchTurn(id, (t) => ({ model: event.model ?? t.model }));
        else if (event.type === "delta") patchTurn(id, (t) => ({ text: t.text + event.text }));
        else patchTurn(id, { text: event.text });
      });
      patchTurn(id, { text, status: "done" });
    } catch (e) {
      const error = toAppError(e);
      // "Stopped" is the user's own doing: keep what had arrived, without an error.
      if (error.kind === "conflict") patchTurn(id, { status: "stopped" });
      else patchTurn(id, { status: "error", error: error.message });
    } finally {
      if (get().running === job) set({ running: null });
    }
  };

  return {
    provider: "claude",
    models: { claude: "", codex: "" },
    shareContext: true,
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
      set({ turns: [], running: null });
    },
  };
});
