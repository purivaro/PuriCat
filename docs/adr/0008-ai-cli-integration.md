# 0008 — AI agents as CLI subprocesses with a PuriCat-owned MCP server

## Status

Proposed (Phase 5). Only CLI **detection** is implemented. Nothing in the current build sends a prompt.

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

- PuriCat depends on CLI flags and stream formats it does not control; versions must be detected and unsupported ones refused with an explanation.
- The MCP server is a new local attack surface: it must only be reachable by the spawned CLI (stdio transport preferred).
- A server-side read-only transaction is a guard rail, not a privilege boundary; the read-only tool must also reject anything that is not a single `SELECT`, and a read-only database role remains the strong control ([SECURITY.md](../../SECURITY.md)).

## Open questions (verify against official documentation at implementation time)

- Exact event schemas of `claude --output-format stream-json` and `codex exec --json`.
- How Codex is given a per-invocation MCP configuration (`-c key=value`, `codex mcp`, or another mechanism).
- Which Claude permission mode permits MCP tools while denying everything else, non-interactively.

The flags named here were confirmed by `--help` only (Claude Code 2.1.303, codex-cli 0.133.0); their behaviour was not exercised. See [AI_INTEGRATION.md](../../AI_INTEGRATION.md).

## Licensing

The CLIs are separate programs installed and licensed by the user; PuriCat does not bundle or redistribute them. Their terms of use apply to the user's account. No MCP library has been chosen.
