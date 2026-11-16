import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentEvent, ChatRequest, RunOutcome } from "../lib/types";

const agentSend = vi.fn<(job: string, request: ChatRequest, onEvent: (event: AgentEvent) => void) => Promise<string>>();
const cancelJob = vi.fn<(job: string) => Promise<void>>(() => Promise.resolve());
const proposalRun = vi.fn<(id: string, liftLimit: boolean) => Promise<RunOutcome>>();
const proposalReject = vi.fn<(id: string) => Promise<void>>(() => Promise.resolve());
vi.mock("../lib/ipc", () => ({
  ipc: {
    agentSend: (...args: Parameters<typeof agentSend>) => agentSend(...args),
    cancelJob: (job: string) => cancelJob(job),
    agentProposalRun: (id: string, liftLimit: boolean) => proposalRun(id, liftLimit),
    agentProposalReject: (id: string) => proposalReject(id),
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
  proposalRun.mockReset();
  proposalReject.mockClear();
  useAgent.setState({ turns: [], running: null, provider: "claude", models: { claude: "", codex: "" }, access: "none" });
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
      tools: null,
    });
  });

  it("gives the turn tools only when the user allows it, bound to the tab the question was asked about", async () => {
    agentSend.mockResolvedValue("ok");
    const scope = () => agentSend.mock.calls.at(-1)![1].tools;
    await useAgent.getState().send("q", context(null), target);
    expect(scope()).toBeNull();
    useAgent.getState().setAccess("schema");
    await useAgent.getState().send("q", context(null), target);
    expect(scope()).toEqual({ connectionId: "c1", database: "shop", schema: null, readRows: false });
    useAgent.getState().setAccess("data");
    await useAgent.getState().send("q", context(null), { ...target, schema: "sales" });
    expect(scope()).toEqual({ connectionId: "c1", database: "shop", schema: "sales", readRows: true });
    // No database tab in front: there is nothing for the tools to work on.
    await useAgent.getState().send("q", context(null), null);
    expect(scope()).toBeNull();
  });

  it("records what the tools did where it happened, and tells the model on the next turn", async () => {
    useAgent.getState().setAccess("data");
    agentSend.mockImplementationOnce(async (_job, _request, onEvent) => {
      onEvent({ type: "delta", text: "Looking." });
      onEvent({ type: "toolStarted", id: "t1", name: "run_select", input: "SELECT 1\nFROM orders" });
      expect(useAgent.getState().turns[1]!.steps).toEqual([{ id: "t1", name: "run_select", input: "SELECT 1\nFROM orders", at: 8, status: "running" }]);
      onEvent({ type: "toolFinished", id: "t1", ok: true, output: "1 row." });
      onEvent({ type: "toolStarted", id: "t2", name: "run_select", input: "DELETE FROM orders" });
      onEvent({ type: "toolFinished", id: "t2", ok: false, output: "Only a statement that reads is accepted" });
      onEvent({ type: "toolStarted", id: "t3", name: "propose_sql", input: "DELETE FROM orders" });
      onEvent({ type: "proposal", id: "p1", sql: "DELETE FROM orders", reason: "tidy", statements: [{ text: "DELETE FROM orders", start: 0, kind: "dml", danger: "No WHERE clause" }] });
      onEvent({ type: "toolFinished", id: "t3", ok: true, output: "queued" });
      onEvent({ type: "toolStarted", id: "t4", name: "list_tables", input: "" });
      onEvent({ type: "delta", text: "\n\nProposed." });
      return "Looking.\n\nProposed.";
    });
    await useAgent.getState().send("clean up", context(null), target);
    const answer = useAgent.getState().turns[1]!;
    expect(answer.steps!.map((s) => [s.name, s.status, s.output])).toEqual([
      ["run_select", "ok", "1 row."],
      ["run_select", "error", "Only a statement that reads is accepted"],
      ["propose_sql", "ok", "queued"],
      ["list_tables", "error", undefined], // never answered: the turn ended first
    ]);
    expect(answer.proposals).toMatchObject([{ id: "p1", sql: "DELETE FROM orders", reason: "tidy", at: 8, state: "pending" }]);

    agentSend.mockResolvedValueOnce("ok");
    await useAgent.getState().send("and?", context(null), target);
    expect(agentSend.mock.calls[1]![1].messages[1]).toEqual({
      role: "assistant",
      text: [
        "Looking.\n\nProposed.",
        "[Tools used: run_select (SELECT 1 FROM orders); run_select (DELETE FROM orders); list_tables]",
        "[Proposed SQL: DELETE FROM orders — waiting for the user's decision; it has not been run]",
      ].join("\n"),
    });
  });

  it("runs a proposal only on approval, and keeps what the row limit held back for a second decision", async () => {
    const statement = (status: RunOutcome["statements"][number]["status"], rowsAffected: number | null, message: string | null = null) => ({
      sql: "DELETE FROM orders",
      kind: "dml" as const,
      status,
      results: [],
      rowsAffected,
      message,
      elapsedMs: 1,
    });
    const outcome = (...statements: RunOutcome["statements"]): RunOutcome => ({ statements, skipped: 0, resume: null, inTransaction: false, connectionLost: false, elapsedMs: 1 });
    const proposal = (id: string) => ({ id, sql: "DELETE FROM orders", reason: null, statements: [], at: 0, state: "pending" as const });
    const state = (id: string) =>
      useAgent
        .getState()
        .turns[0]!.proposals!.find((p) => p.id === id)!;
    useAgent.setState({ turns: [{ id: "a", role: "assistant", text: "", status: "done", proposals: [proposal("p1"), proposal("p2"), proposal("p3"), proposal("p4")] }] });
    const { decide } = useAgent.getState();

    // Rejected: nothing is run, and it cannot be approved afterwards.
    await decide("p1", "reject");
    expect(proposalReject).toHaveBeenCalledWith("p1");
    await decide("p1", "approve");
    expect(proposalRun).not.toHaveBeenCalled();
    expect(state("p1").state).toBe("rejected");

    // Approved and held back by the row limit: asked again, then run without it.
    proposalRun.mockResolvedValueOnce(outcome(statement("ok", 1), statement("blocked", 1250)));
    await decide("p2", "lift"); // not held back yet: ignored
    await decide("p2", "approve");
    expect(proposalRun).toHaveBeenLastCalledWith("p2", false);
    expect(state("p2").state).toBe("blocked");
    // Only what was held back is run again; the statement that ran before it stays on record.
    proposalRun.mockResolvedValueOnce(outcome(statement("ok", 1250)));
    await decide("p2", "lift");
    expect(proposalRun).toHaveBeenLastCalledWith("p2", true);
    expect(state("p2")).toMatchObject({ state: "done", outcome: { statements: [{ status: "ok", rowsAffected: 1 }, { status: "ok", rowsAffected: 1250 }] } });
    expect(state("p2").outcome!.statements).toHaveLength(2);
    await decide("p2", "approve"); // final: ignored
    expect(proposalRun).toHaveBeenCalledTimes(2);

    // An error from the server is final; a run that could not start is not.
    proposalRun.mockResolvedValueOnce(outcome(statement("error", null, "permission denied")));
    await decide("p3", "approve");
    expect(state("p3").state).toBe("failed");
    proposalRun.mockRejectedValueOnce({ kind: "invalid", message: "This connection is not open" });
    await decide("p4", "approve");
    expect(state("p4")).toMatchObject({ state: "pending", error: "This connection is not open" });

    expect(transcript(useAgent.getState().turns)[0]!.text.split("\n")).toEqual([
      "[Proposed SQL: DELETE FROM orders — rejected by the user; it was not run]",
      "[Proposed SQL: DELETE FROM orders — approved by the user and run by PuriCat (1251 rows affected)]",
      "[Proposed SQL: DELETE FROM orders — approved by the user, but it failed: permission denied]",
      "[Proposed SQL: DELETE FROM orders — waiting for the user's decision; it has not been run]",
    ]);

    // Clearing the conversation withdraws what was never decided.
    proposalReject.mockClear();
    useAgent.getState().reset();
    expect(proposalReject.mock.calls).toEqual([["p4"]]);
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
