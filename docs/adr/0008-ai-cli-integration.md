# 0008 — AI agents as CLI subprocesses with a PuriCat-owned MCP server

## Status

Implemented in two slices (2026-11-16). The first slice of Phase 5 built the subprocess transport of this record as an agent chat **without tools** ([ADR 0016](0016-agent-chat-without-tools.md)). The second built the rest — the MCP server, the read-only tools, proposals and their approval — as tools the user turns on, off by default ([ADR 0017](0017-ai-database-tools.md)). The tools are verified for Claude Code; no Codex turn with tools has been observed. The open questions listed further down about the MCP configuration, the Claude options and the transport are answered in ADR 0017; the lists there are left as they were written after the first slice.

| Part of the decision | State |
|---|---|
| 1. Transport: the CLI as a subprocess in non-interactive streaming mode, fixed argument vector, no shell, prompt on stdin, kill on cancel or timeout | Built for both CLIs. For Codex only the start and failure of a turn were seen from the real CLI; its successful path is implemented from the documented format and unverified |
| 2. Tools: a PuriCat MCP server with read-only tools | Built (ADR 0017), off by default: the server runs inside the app on a Unix socket per turn, reached through the app's own executable as a stdio relay. Verified for Claude Code; Codex completed the handshake only |
| 3. Writes: proposals and an approval queue | Built (ADR 0017) as proposals shown in the conversation and approved one by one in a review dialog; there is no separate queue. With the tools off, the user still opens SQL from an answer in a query tab and runs it there |
| 4. Confinement: restricted built-in tools and sandbox, empty scratch directory | Built. With the tools off, in a stricter form than proposed: Claude runs with `--tools ""` and `--strict-mcp-config` (no tools, no MCP servers) instead of an allow-list; with them on, with `--tools ""` and an allow-list of PuriCat's MCP tools only; Codex with `--sandbox read-only`, though it keeps its own shell tool inside that sandbox. Both start in an empty directory owned by PuriCat |
| 5. Data handling: no secrets in prompts, arguments or environment; content treated as untrusted | Built for what exists: the context holds names and types only, the prompt is on stdin, PuriCat adds nothing to the arguments or the environment (the CLI inherits PuriCat's own environment), answers are rendered as text |

## Context

The brief requires the agent to run through locally installed, already authenticated Claude Code and Codex CLIs, using documented interfaces, with read-only tools by default and explicit approval for every mutation. PuriCat must not ask for account passwords or assume that a CLI login grants API access.

## Decision

1. **Transport:** spawn the CLI from Rust in non-interactive streaming mode: `claude -p --output-format stream-json --verbose ...` or `codex exec --json ...`. Fixed argument vector, no shell, the prompt on stdin where the CLI accepts it, kill on cancel or timeout.
2. **Tools:** PuriCat runs a local MCP server (built on `puricat-core`) and gives it to the CLI. Default tools are read-only: list schemas, describe a table, run a row-limited `SELECT` inside a read-only transaction.
3. **Writes:** a write tool does not execute. It returns a *proposal* that enters an in-app approval queue. After the user approves, PuriCat executes it; the CLI never does.
4. **Confinement:** restrict the CLI's built-in tools and sandbox (Claude: `--tools` / `--allowedTools`, `--strict-mcp-config`, a non-bypass permission mode; Codex: `--sandbox read-only`), and run it in an empty scratch working directory.
5. **Data handling:** no secrets in prompts, arguments or environment. Database content and model output are treated as untrusted input.

## Alternatives considered

| Option | Why not |
|---|---|
| Call vendor HTTP APIs directly | Needs API keys and billing; the brief prioritises the user's existing CLI login |
| Let the CLI run SQL through its own shell tool | No approval boundary, would need credentials in the CLI's environment |
| Embed an agent SDK in the app | Ties the app to one vendor's runtime and credential model |
| Drive the interactive TUI through a pseudo-terminal | Screen scraping an unstructured, unversioned interface |

## Consequences

- PuriCat depends on CLI flags and stream formats it does not control; versions must be detected and unsupported ones refused with an explanation. (Not built in the first slice: an incompatible CLI fails the turn with its own message.)
- The MCP server is a new local attack surface: it must only be reachable by the spawned CLI (stdio transport preferred).
- A server-side read-only transaction is a guard rail, not a privilege boundary; the read-only tool must also reject anything that is not a single `SELECT`, and a read-only database role remains the strong control ([SECURITY.md](../../SECURITY.md)).

## Open questions (verify against official documentation at implementation time)

Answered by the first slice (2026-11-16):

| Question | Answer | Evidence |
|---|---|---|
| Exact event schema of `claude --output-format stream-json` | With `--verbose --include-partial-messages`: `system` / `init` (carries `model`), `stream_event` → `content_block_delta` → `text_delta`, `assistant` (the complete message; one carrying `error` describes a failure), `result` (`is_error`, `result`) | Lines captured from Claude Code 2.1.303, kept as unit-test fixtures in `crates/core/src/agent.rs`; the opt-in test `real_claude_cli_answers` against the real CLI passed on 2026-11-16 |
| Exact event schema of `codex exec --json` | **Partly.** `thread.started`, `turn.started`, `error` and `turn.failed` were captured. The success lines (`item.completed` with an `agent_message` item, `turn.completed`) are taken from the CLI's documented format and were not observed | Lines captured from codex-cli 0.133.0 for a refused model. No successful turn could be produced: the service refused every model tried ("The '<model>' model is not supported when using Codex with a ChatGPT account") |

Two further things were learned that the record did not ask: both CLIs can be told to keep no session (`--no-session-persistence`, `--ephemeral`), which is why PuriCat resends the conversation itself; and a model the account cannot use is reported inside the stream, in a sentence that can be shown to the user as it is.

Still open:

- A successful Codex turn observed from the real CLI.
- How Codex is given a per-invocation MCP configuration (`-c key=value`, `codex mcp`, or another mechanism).
- Which Claude permission mode permits MCP tools while denying everything else, non-interactively. The chat avoids the question by allowing no tools.
- Whether Codex can be run without its own shell tool.
- Version detection: which CLI versions accept these options and emit these streams. Nothing checks today.

Of the flags named in this record, those the chat passes were exercised: the Claude vector by the opt-in test against the real CLI, both vectors by stand-in CLIs in unit and end-to-end tests (which prove what PuriCat passes, not what the real CLIs do with it). The remaining flags (`--allowedTools`, `--mcp-config`, permission modes) are still confirmed by `--help` only (Claude Code 2.1.303, codex-cli 0.133.0). See [AI_INTEGRATION.md](../../AI_INTEGRATION.md).

## Licensing

The CLIs are separate programs installed and licensed by the user; PuriCat does not bundle or redistribute them. Their terms of use apply to the user's account, including to what the chat sends. No MCP library has been chosen.
