# AI integration

Status on 2026-11-16: **only detection is implemented.** PuriCat finds the Claude Code and Codex CLIs and shows whether they are installed and signed in. It sends no prompts, has no chat UI, and exposes no database tools to any model. Everything under "Planned protocol" is a design for Phase 5, not a description of the build.

The decision record is [ADR 0008](docs/adr/0008-ai-cli-integration.md).

## Principles

- PuriCat drives CLIs the user has installed and signed in to. It never asks for, reads or stores account passwords, API keys or tokens.
- Being signed in to a CLI does not imply any API entitlement. PuriCat assumes nothing about the user's plan, quota or access to other services, and will report the CLI's own error if a request is refused.
- Only documented, supported CLI interfaces are used. If one is missing or changes, the feature is disabled with an explanation; PuriCat does not fabricate responses.
- No AI-initiated change reaches a database without explicit approval.

## What exists now — Implemented & verified

### Locating the CLIs

For `claude` and `codex`, in order:

1. each directory in `PATH`;
2. per-user install directories under `$HOME`: `.local/bin`, `.claude/local`, `.npm-global/bin`, `.bun/bin`, `.volta/bin`, `bin`;
3. `/opt/homebrew/bin` and `/usr/local/bin`;
4. on Unix, as a last resort for version-manager installs: `$SHELL -lic "command -v <binary>"`, taking the first absolute path that is a file.

Which of these steps located the binaries during verification was not recorded. Windows has no fallback step and is untested.

### Commands run

| Command | What is read | What is discarded |
|---|---|---|
| `claude --version` | First line of stdout | — |
| `claude auth status --json` | The boolean `loggedIn` | Every other field (account, method, email) |
| `codex --version` | First line of stdout | — |
| `codex login status` | Exit status, and whether the output contains "logged in" but not "not logged in" | The rest of the text |

Each runs with a fixed argument vector, no shell, stdin closed, an 8-second timeout and kill-on-drop. The tool's own directory is prepended to `PATH` so Node-based CLIs find their runtime when the app is launched from Finder.

### What the UI shows

Per provider: installed or not, the version line, the path of the binary, and signed in / signed out / unknown. "Unknown" means the tool timed out or printed something unexpected. When signed out, the panel names the CLI's own sign-in command (`claude auth login`, `codex login`) for the user to run in a terminal. The result is held in memory only; nothing is written to the settings store.

Detection was verified on the development machine against Claude Code 2.1.303 and codex-cli 0.133.0, through unit tests of the output parsers and the end-to-end smoke test. Other versions are untested.

## CLI interfaces confirmed to exist

Confirmed by `--help` on the versions above. **Presence only — behaviour was not exercised.**

| CLI | Relevant options |
|---|---|
| Claude Code | `-p/--print`, `--output-format` (including `stream-json`), `--input-format`, `--include-partial-messages`, `--verbose`, `--mcp-config`, `--strict-mcp-config`, `--permission-mode`, `--allowedTools` / `--disallowedTools`, `--tools`, `--system-prompt` / `--append-system-prompt`, `--model`, `--session-id`, `--no-session-persistence`, `--json-schema`, `--setting-sources`, `--bare`; `claude auth status [--json|--text]` |
| Codex | `codex exec [PROMPT]` (prompt may come from stdin) with `--json`, `--output-schema <FILE>`, `-o/--output-last-message <FILE>`, `-s/--sandbox read-only|workspace-write|danger-full-access`, `-m/--model`, `-C/--cd`, `--skip-git-repo-check`, `--ephemeral`, `--ignore-user-config`, `-c key=value`; `codex login status`; `codex mcp`; `codex mcp-server` |

## Planned protocol — Planned (Phase 5)

```
PuriCat (Rust) --spawn--> claude -p --output-format stream-json --verbose ...
                          codex exec --json ...
      ^                         |
      |  streamed events        |  MCP tool calls
      |                         v
      +---------------- PuriCat MCP server (local, owned by PuriCat)
                          read-only tools ----> database (read-only transaction)
                          write tools --------> proposal --> approval queue --> PuriCat executes
```

1. PuriCat spawns the selected CLI in non-interactive streaming mode and parses its event stream.
2. The CLI reaches the database only through an MCP server that PuriCat runs and configures for that invocation.
3. Cancelling in the UI kills the subprocess. A timeout does the same.
4. A run is shown as completed only after PuriCat has verified the outcome against the database.

## Permission boundaries — Planned (Phase 5)

| Boundary | Mechanism |
|---|---|
| Default tools are read-only | List schemas, describe a table, run a row-limited `SELECT` in a read-only transaction. The tool also rejects anything that is not a single `SELECT`, because a read-only transaction alone is a guard rail (see [SECURITY.md](SECURITY.md)) |
| Writes need approval | A write tool returns a proposal. The user sees the SQL, target connection and environment, impact estimate and transaction behaviour. PuriCat executes after approval; the CLI never does |
| The CLI's own tools are restricted | Claude: `--tools` / `--allowedTools`, `--strict-mcp-config`, a non-bypass permission mode. Codex: `--sandbox read-only` |
| Filesystem | The CLI runs in an empty scratch directory |
| Credentials | Never in prompts, arguments or environment. The MCP server uses PuriCat's existing sessions |
| Untrusted data | Database content and model output are treated as untrusted; tool results are data, not instructions to PuriCat |
| Audit | Approvals and outcomes are logged without secrets |

These apply to every environment, not only production, unless the user deliberately changes the policy for a connection.

## Context sent to providers — Planned (Phase 5)

Nothing is sent today. In Phase 5, whatever PuriCat puts in a prompt or returns from a tool is forwarded by the CLI to its provider's service under the user's own account and that provider's terms.

Plan:

| Context | Default |
|---|---|
| Names and types of explicitly scoped schemas, tables and columns | Sent |
| The user's question, the SQL in the active tab, error messages | Sent |
| Sample rows | Not sent unless the user selects them |
| Rows returned by the read-only query tool | Row-limited; visible in the conversation |
| Passwords, connection strings, host names, user names | Never sent |

- A preview shows the exact context before the first message and when the scope changes.
- Columns can be marked sensitive and are redacted from samples and tool results. The detection rules for suggesting sensitive columns are not designed yet.
- Context follows the active tab and connection, with explicit pinning.

## Open questions

To be answered from official documentation when Phase 5 starts:

- The exact event schema of `claude --output-format stream-json` and of `codex exec --json`, and their stability across versions.
- How Codex is given a per-invocation MCP configuration (`-c key=value`, `codex mcp`, or another mechanism) without changing the user's global configuration.
- Which Claude permission mode allows the PuriCat MCP tools and denies everything else without an interactive prompt.
- Whether either CLI writes session history to disk in these modes and how to turn that off (`--no-session-persistence`, `--ephemeral` are candidates).
- The minimum supported version of each CLI, and how to detect an incompatible one.
- MCP transport (stdio is preferred so no port is opened) and which MCP library to use.
- How the CLIs behave when the account lacks access to a requested model.
