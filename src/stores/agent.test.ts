import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentEvent, ChatRequest } from "../lib/types";

const agentSend = vi.fn<(job: string, request: ChatRequest, onEvent: (event: AgentEvent) => void) => Promise<string>>();
const cancelJob = vi.fn<(job: string) => Promise<void>>(() => Promise.resolve());
vi.mock("../lib/ipc", () => ({
  ipc: {
    agentSend: (...args: Parameters<typeof agentSend>) => agentSend(...args),
    cancelJob: (job: string) => cancelJob(job),
    setSetting: () => Promise.resolve(),
    getSettings: () => Promise.resolve({}),
  },
  toAppError: (e: unknown) => e,
}));

const { transcript, useAgent } = await import("./agent");

const target = { connectionId: "c1", database: "shop", schema: null };
const context = (text: string | null) => () => Promise.resolve(text);

beforeEach(() => {
  agentSend.mockReset();
  cancelJob.mockClear();
  useAgent.setState({ turns: [], running: null, provider: "claude", models: { claude: "", codex: "" } });
});

describe("agent chat store", () => {
  it("streams an answer into the turn and sends the context with the conversation", async () => {
    agentSend.mockImplementation(async (_job, _request, onEvent) => {
      onEvent({ type: "started", model: "claude-haiku-5-5" });
      onEvent({ type: "delta", text: "SELECT" });
      onEvent({ type: "delta", text: " 1" });
      expect(useAgent.getState().turns[1]).toMatchObject({ text: "SELECT 1", status: "streaming" });
      return "SELECT 1;";
    });
    await useAgent.getState().send("  count orders ", context("Engine: MySQL"), target);

    const [question, answer] = useAgent.getState().turns;
    expect(question).toMatchObject({ role: "user", text: "count orders", context: "Engine: MySQL", target });
    expect(answer).toMatchObject({ role: "assistant", text: "SELECT 1;", status: "done", provider: "claude", model: "claude-haiku-5-5" });
    expect(useAgent.getState().running).toBeNull();
    expect(agentSend.mock.calls[0]![1]).toEqual({
      provider: "claude",
      model: null,
      context: "Engine: MySQL",
      messages: [{ role: "user", text: "count orders" }],
    });
  });

  it("shows the CLI's error, and asks the same question again on retry with the chosen model", async () => {
    agentSend.mockRejectedValueOnce({ kind: "unsupported", message: "model not supported" });
    await useAgent.getState().send("hello", context("ctx"), target);
    expect(useAgent.getState().turns[1]).toMatchObject({ status: "error", error: "model not supported" });

    useAgent.getState().setModel(" opus ");
    agentSend.mockResolvedValueOnce("hi");
    await useAgent.getState().retry();
    const turns = useAgent.getState().turns;
    expect(turns).toHaveLength(2);
    expect(turns[1]).toMatchObject({ status: "done", text: "hi", model: "opus" });
    expect(agentSend.mock.calls[1]![1]).toMatchObject({ model: "opus", context: "ctx", messages: [{ role: "user", text: "hello" }] });
  });

  it("keeps what had arrived when the turn is stopped, and cancels the job", async () => {
    agentSend.mockImplementation((_job, _request, onEvent) => {
      onEvent({ type: "delta", text: "partial" });
      useAgent.getState().stop();
      return Promise.reject({ kind: "conflict", message: "Stopped" });
    });
    await useAgent.getState().send("slow one", context(null), null);
    expect(cancelJob).toHaveBeenCalledWith(agentSend.mock.calls[0]![0]);
    expect(useAgent.getState().turns[1]).toMatchObject({ status: "stopped", text: "partial" });
    expect(useAgent.getState().turns[1]!.error).toBeUndefined();
  });

  it("sends nothing when stopped or cleared while the context is still being read", async () => {
    let release: (text: string) => void = () => {};
    const slow = () => new Promise<string>((resolve) => (release = resolve));

    const stopped = useAgent.getState().send("first", slow, target);
    expect(useAgent.getState().running).toMatch(/^starting:/);
    await useAgent.getState().send("second", context(null), target); // ignored: a turn is already claimed
    useAgent.getState().stop();
    release("ctx");
    await stopped;
    expect(agentSend).not.toHaveBeenCalled();
    expect(useAgent.getState()).toMatchObject({ running: null });
    expect(useAgent.getState().turns.map((t) => t.text)).toEqual(["first"]);

    const cleared = useAgent.getState().send("again", slow, target);
    useAgent.getState().reset();
    release("ctx");
    await cleared;
    expect(agentSend).not.toHaveBeenCalled();
    expect(useAgent.getState().turns).toEqual([]);
  });

  it("resends only what the provider should see", () => {
    const turn = (role: "user" | "assistant", text: string, status: "done" | "error" | "stopped" | "streaming") => ({ id: text, role, text, status });
    expect(
      transcript([
        turn("user", "q1", "done"),
        turn("assistant", "", "error"),
        turn("user", "q2", "done"),
        turn("assistant", "half", "stopped"),
        turn("user", "q3", "done"),
        turn("assistant", "a3", "done"),
        turn("assistant", "typing", "streaming"),
      ]),
    ).toEqual([
      { role: "user", text: "q1" },
      { role: "user", text: "q2" },
      { role: "assistant", text: "half" },
      { role: "user", text: "q3" },
      { role: "assistant", text: "a3" },
    ]);
  });
});
