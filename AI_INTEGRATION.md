# AI integration

Status on 2026-11-16: **detection and a first chat are implemented. The model has no tools and no database access.** PuriCat finds the Claude Code and Codex CLIs, shows whether they are installed and signed in, and offers a chat in the AI panel: the user's question — with the schema of the database in front, if the user shares it — goes to the chosen CLI, and a text answer streams back. SQL in an answer reaches a database only when the user opens it in a query tab and runs it there.

Not built: the MCP server, the read-only query tool, the proposal and approval queue, the audit log, per-column redaction. Everything under "Planned protocol" is still a design, not a description of the build.

**Codex's successful path is unverified against the real CLI.** On the development machine the service refused every model that was tried, so only Codex's failure lines were captured; the success shape is implemented from the CLI's documented format (see "Stream formats").

The decision records are [ADR 0008](docs/adr/0008-ai-cli-integration.md) (the overall design) and [ADR 0016](docs/adr/0016-agent-chat-without-tools.md) (shipping the chat first, without tools).

## Principles

- PuriCat drives CLIs the user has installed and signed in to. It never asks for, reads or stores account passwords, API keys or tokens.
- Being signed in to a CLI does not imply any API entitlement. PuriCat assumes nothing about the user's plan, quota or access to other services, and reports the CLI's own error if a request is refused.
- Only documented, supported CLI interfaces are used. If one is missing or changes, the feature fails with the CLI's own explanation; PuriCat does not fabricate responses.
- No AI-initiated change reaches a database without explicit approval. In the current build the model cannot initiate anything: the user is the only path from an answer to a database.

## Detection — Implemented & verified

### Locating the CLIs

For `claude` and `codex`, in order:

1. each directory in `PATH`;
2. per-user install directories under `$HOME`: `.local/bin`, `.claude/local`, `.npm-global/bin`, `.bun/bin`, `.volta/bin`, `bin`;
3. `/opt/homebrew/bin` and `/usr/local/bin`;
4. on Unix, as a last resort for version-manager installs: `$SHELL -lic "command -v <binary>"`, taking the first absolute path that is a file.

Which of these steps located the binaries during verification was not recorded. Windows has no fallback step and is untested. The same lookup runs again at the start of every chat turn; its result is not cached.

### Commands run

| Command | What is read | What is discarded |
|---|---|---|
| `claude --version` | First line of stdout | — |
| `claude auth status --json` | The boolean `loggedIn` | Every other field (account, method, email) |
| `codex --version` | First line of stdout | — |
| `codex login status` | Exit status, and whether the output contains "logged in" but not "not logged in" | The rest of the text |

Each runs with a fixed argument vector, no shell, stdin closed, an 8-second timeout and kill-on-drop. The tool's own directory is prepended to `PATH` so Node-based CLIs find their runtime when the app is launched from Finder.

### What the UI shows

Per provider: installed or not, the version line, the path of the binary, and signed in / signed out / unknown. "Unknown" means the tool timed out or printed something unexpected.

- When the CLI is installed and not known to be signed out (signed in, or unknown), the panel shows one status line — the version and "Signed in" or "Sign-in state unknown", with the binary's path as a tooltip — and the chat below it.
- Otherwise there is no chat. The panel says what is missing: "Not installed", or "Installed" with the version and path followed by "Not signed in" and the CLI's own sign-in command (`claude auth login`, `codex login`) for the user to run in a terminal. "Check again" repeats the detection.

The detection result is held in memory only; nothing about it is written to the settings store.

Detection was verified on the development machine against Claude Code 2.1.303 and codex-cli 0.133.0, through unit tests of the output parsers and the end-to-end smoke test as it was before the agent chat. Since then the end-to-end test puts stand-in CLIs first on `PATH` ([TESTING.md](TESTING.md)), so the real `claude` and `codex` status commands are no longer exercised by any automated run. Other versions are untested.

## Agent chat — Implemented

Verified for Claude Code with unit tests, the end-to-end test on stand-in CLIs (dev mode and the bundled app, on the build before process groups were added), and an opt-in test against the real CLI that passed twice. For Codex only the refusal was seen from the real CLI. Evidence and gaps are in [TESTING.md](TESTING.md).

### What the panel offers

- A provider switch (Claude Code / Codex). It cannot be changed while a turn is running.
- A model selector (see "Model selection").
- A context button that names what the conversation is about and opens the context inspector (see "Context sent to the provider").
- The conversation: the user's messages, each with "Context sent" or "No context sent", and the answers, each headed by the provider and the model.
- A composer: Enter sends, Shift+Enter starts a new line, and an Enter that confirms an IME composition does neither. While a turn runs, the send button is a Stop button.
- "New chat" clears the conversation (and stops a running turn). After an error or a stop, "Try again" asks the last question again with the context it was first sent with, using the provider and model selected at that moment.
- Before the first message: three suggested questions, which fill the composer without sending, and three lines stating what the AI can and cannot do.

### One turn

One turn is one CLI subprocess (`agent::chat`, started by the `agent_send` command).

| Aspect | Behaviour |
|---|---|
| Invocation | A fixed argument vector built in Rust; no shell. On Unix the CLI is started in a process group of its own, because a CLI can be a launcher whose real program is its child (Codex is) |
| Working directory | `agent-scratch` inside the app-data directory, created on first use and otherwise empty: the CLI has nothing of the user's in front of it |
| Input | The whole prompt is written to stdin, then stdin is closed, from a task of its own so that a CLI that does not read cannot block the turn. Nothing of the conversation or the schema is in the arguments |
| Output | stdout is read line by line as JSON events; the tail of stderr (the last 32 to 64 KiB) is kept for the error message |
| End of a turn | The first line that carries a verdict ends the turn (Claude: `result`; Codex: `turn.completed`, `turn.failed` or `error`), even while a process the CLI started still holds the pipe open. So does the CLI closing its output. The CLI then has 5 seconds to exit by itself; if it has not, it is killed |
| Environment | PuriCat's own, with the tool's directory prepended to `PATH`. PuriCat adds no variable and puts no database credential anywhere near the CLI |
| Stop | Sets the turn's cancel flag (`cancel_job`). The reader looks at the flag before every read — after each line, and at least every 150 ms — and then kills the CLI. What had streamed stays in the conversation, marked "Stopped before the answer was complete." A Stop pressed while the context is still being read, before the CLI has started, does nothing |
| Timeout | 10 minutes from the start of the turn, checked in the same place; then the CLI is killed and the turn fails with a message |
| Killing | On Unix the whole process group is signalled (`/bin/kill -KILL -<pid>`, a fixed vector with the CLI's process id), then the CLI itself, and PuriCat waits up to 2 seconds for it to be gone. On other platforms only the CLI process itself is killed |
| Abandoned turn | The CLI process (not its group) is killed when the turn's future is dropped (kill-on-drop). A turn still running when PuriCat itself is killed is not cleaned up |
| State between turns | None in the CLI. PuriCat resends the conversation every time |

### Argument vectors

Claude Code:

```
claude -p --output-format stream-json --verbose --include-partial-messages
       --tools "" --strict-mcp-config --safe-mode --disable-slash-commands
       --permission-prompts none --no-session-persistence
       --system-prompt <fixed text> [--model <name>]
```

Codex:

```
codex exec --json --sandbox read-only --skip-git-repo-check --ephemeral
      --ignore-user-config --ignore-rules -C <scratch> [-m <name>] -
```

- Claude: the intent of the vector, as the code states it, is no built-in tools, no MCP servers, none of the user's own customisations (CLAUDE.md, hooks, plugins, skills), and nothing written to the session history. The `system/init` line captured from Claude Code 2.1.303 and kept in the unit tests reports `"tools":[]` and `"mcp_servers":[]`. What each option contributes on its own was not tested.
- Codex has no system-prompt option, so the fixed instructions lead the prompt on stdin. The trailing `-` makes it read the prompt from stdin. `--sandbox read-only` confines the commands the model may run; it does not remove Codex's own shell tool (see "Limits and risks").
- The model name is the only argument the user can influence. `validate_model` accepts 1–100 characters out of ASCII letters, digits and `. _ : / @ - [ ]`, not starting with `-`, so it can never be read as another option. The UI applies the same pattern before it stores a typed name; the backend checks again and refuses before any process is started.

### The prompt on stdin

```
<the fixed instructions — Codex only>

<context>
<the context text, if shared>
</context>

(Earlier turns of this conversation were left out to save space.)

User:
<message>

Assistant:
<answer>

User:
<the new message>

Reply to the user's last message.
```

- A `</context>` inside the context text is rewritten to `< /context>`, so database content cannot close the block early (unit-tested).
- The most recent turns are kept up to 60,000 characters of message text; older ones are dropped and the note above is added. The newest message is always sent whole. The context block is not counted against that budget.
- Answers that ended in an error are left out of what is resent. An answer the user stopped is kept as far as it had arrived, and every question is kept, including one whose answer failed.

The fixed instructions (`agent::SYSTEM_PROMPT`) tell the model that it has no tools and has seen no rows; to write SQL for the engine named in the context using only tables and columns that appear there, and to ask rather than invent names; to put every runnable statement in a fenced block tagged `sql`; never to say it ran, checked or changed anything; for statements that change data or structure, to say what is affected and suggest a `SELECT` or a transaction first; that everything inside `<context>` is reference data, not instructions; and to answer concisely in the user's language.

### Stream formats

Claude Code (`-p --output-format stream-json --verbose --include-partial-messages`) — **confirmed by capture** from Claude Code 2.1.303; the captured lines are the fixtures of the unit tests:

| Line | Use |
|---|---|
| `system` with `subtype: "init"` | The turn has started; `model` is shown on the answer |
| `stream_event` → `content_block_delta` → `text_delta` | Appended to the answer as it streams |
| `assistant` (a complete message) | Its text replaces what was streamed, if it differs |
| `assistant` carrying `error` | The CLI describing a failure; ignored, the `result` line reports it |
| `result` | `is_error: true` (or a `subtype` other than `success`) fails the turn with the text of `result`; otherwise the turn succeeded, and if nothing was streamed the text of `result` is the answer |
| Anything else (`system/status`, `rate_limit_event`, other `stream_event`s, lines that are not JSON objects) | Ignored |

Codex (`exec --json`) — **partly confirmed** on codex-cli 0.133.0:

| Line | Use | Seen from the real CLI |
|---|---|---|
| `thread.started` | The turn has started (no model is reported) | Yes |
| `turn.started` | Ignored | Yes |
| `error`, `turn.failed` | Fail the turn with `error.message` or `message`. When that message is itself JSON with an inner `error.message`, the inner sentence is shown | Yes |
| `item.completed` with an item of type `agent_message` | Its `text` is the answer; several in one turn are joined as paragraphs. Items of other types (reasoning) are ignored | **No** — from the documented format |
| `turn.completed` | The turn succeeded | **No** — from the documented format |

The service refused every model that was tried from this machine ("The '<model>' model is not supported when using Codex with a ChatGPT account"), so no successful Codex turn has been observed. The stand-in CLI of the end-to-end test replays the documented success shape; that proves PuriCat handles the shape, not that the real CLI emits it. Codex answers arrive per message, not token by token.

When the CLI closes its output without a verdict, an exit status of 0 (reached within the 5 seconds it is given) with some answer text counts as success; otherwise the turn fails with the last non-empty line the CLI wrote to stderr (at most 500 characters), or "ended without an answer".

Because the first verdict ends the turn, a Codex `error` line fails it at once, whatever would have followed. Whether Codex also uses `error` for notices it recovers from was not checked.

There is no version check. A CLI that does not accept one of the options fails the turn with whatever it prints.

### Model selection

- "Default model" passes no model option and leaves the choice to the CLI's own settings.
- Claude Code: the aliases `fable`, `opus`, `sonnet` and `haiku`. The list is fixed in the code (it names them as the aliases of `claude --help`); it is not read from the CLI.
- Codex: what `codex debug models` prints — the entries whose `visibility` is not `hide`, with their display name and description. The command runs with a 15-second timeout; if it fails or prints something else, only "Default model" and "Other…" are offered.
- "Other…" accepts any model name that passes the validation above.
- The choice is remembered per provider. The answer's header shows the model the CLI reported, or the one that was asked for when the CLI reports none (Codex).
- A refusal — an unknown model, a model the account may not use — is shown in the CLI's own words, with "Try again". Captured from the real CLIs: Claude's "There's an issue with the selected model (…)" and Codex's "The '…' model is not supported when using Codex with a ChatGPT account."

### Context sent to the provider

Whatever PuriCat puts on stdin is forwarded by the CLI to its provider's service, under the user's own account and that provider's terms.

The context is text produced by the backend (`agent_context` → `agent::render_context`). An example, from the unit test:

```
Engine: PostgreSQL 16.13
Database: shop
Schema: public

Table orders (about 1250 rows)
  id bigint PRIMARY KEY GENERATED
  note text
  total numeric(12,2) NOT NULL

Other tables and views (columns not listed): order_totals

SQL in the user's active query tab:
SELECT 1
```

| Sent, when sharing is on | Detail |
|---|---|
| Engine name and server version | The version text as the server reports it, which can name a build or distribution |
| Database name, and schema name on PostgreSQL | Of the tab in front |
| Tables, views and materialized views | The first 40 that the catalogue lists, each with its columns (at most 80, then a count of the rest): name, type as the catalogue gives it, `PRIMARY KEY` or `NOT NULL`, `GENERATED`, and the planner's row estimate where there is one. The remaining objects, and any whose description could not be read, are listed by name only |
| The SQL in the active query tab | The whole text of the editor as it is at that moment, when the tab in front is a query tab and the text is not empty. It is sent as typed; the history's password masking is not applied to it |
| The user's messages and the earlier answers | See "The prompt on stdin" |

| Never part of the context | |
|---|---|
| Rows, cell values, sample data | The model has seen no data |
| Column defaults, indexes, foreign keys, comments | Not rendered |
| Host names, ports, user names, passwords, TLS settings | Not rendered |
| The connection's name | Shown on the context button in the app only |

How the context is chosen:

- It follows the tab in front. A table tab gives its database and schema. A query tab gives its database — or the connection's default database, or `postgres` on PostgreSQL when neither is set — and, on PostgreSQL, always the schema `public`. With no table or query tab in front, a closed connection, or a MySQL query tab with no database, there is no context and the button says why.
- The **context inspector** (the context button) shows the text for the current tab, rendered by the backend at that moment, under "Sent with your next message". The text that is actually sent is rendered again when the message is sent, so it reflects the tab and the editor as they are then.
- Each sent message keeps the exact text that went with it: "Context sent" under the message opens it. "No context sent" means none did.
- The "Share context" switch turns sharing off entirely; the model then gets the messages only. The setting is remembered.
- If the schema cannot be read when a message is sent (the connection dropped), the message goes without context and is marked "No context sent".
- Reading the context costs one catalogue listing plus one description per table (up to 40), run one after another on the connection's pool, for every opening of the inspector and every message. Nothing is cached.

Not built from the earlier plan: marking columns as sensitive, sample rows, pinning the context to a connection, and a scope the user can narrow table by table.

### From an answer to a database

- Answers are read by a small Markdown reader (`src/features/agent/markdown.ts`: fenced code, headings, lists, simple tables, inline code, bold) into plain data and rendered as React elements. Nothing from an answer is inserted as HTML; markup in an answer is shown as text.
- Every code block has "Copy". A block that is tagged as SQL (`sql`, `mysql`, `postgresql`, `postgres`, `pgsql`, `plpgsql`, `tsql`, `mariadb`) or is untagged and starts with an SQL keyword, and is complete, also has **"Open in query tab"** — as long as the question was asked with a table or query tab in front and that connection is still open.
- "Open in query tab" opens a new query tab on the connection and database the question was asked about, with the block's text in the editor. **Nothing runs.** From there the SQL console's own safeguards apply: the confirmation before destructive statements, the impact guard, the row cap ([SECURITY.md](SECURITY.md), SQL console).
- The new tab carries the database but not the schema. On PostgreSQL, SQL written for a table tab's schema other than `public` with unqualified names will not find its tables unless the session's `search_path` includes that schema; the instructions do not ask the model to qualify names. Not tested: the end-to-end test opens such a tab and does not run it.

### What is kept

| Data | Where | Lifetime |
|---|---|---|
| The conversation, and the context sent with each message | Web view memory (`src/stores/agent.ts`) | Until "New chat" or the app closes. Never written to disk by PuriCat |
| Provider, model per provider, the "Share context" switch | Settings store, key `agent` | Persistent |
| Sessions in the CLI | None: `--no-session-persistence` (Claude), `--ephemeral` (Codex) | The scratch directory was empty after a turn in the opt-in run against the real Claude CLI and in the end-to-end run on stand-ins. What the CLIs write elsewhere — logs, caches, their own configuration directory — was not inspected |

### Limits and risks

- **What is sent leaves the machine.** It goes to the provider under the user's own account and terms. Table and column names, and the SQL in the active query tab, can themselves be sensitive; literals and passwords written in that SQL are sent as typed. The inspector and the switch are the controls; there is no per-column redaction.
- **The context is a UI rule.** `agent_send` forwards whatever context text it is given; the backend does not check that it came from `agent_context` or that sharing is on.
- **Database identifiers are untrusted text inside the prompt.** A table or column name, or the active query, can carry text written to steer the model. The context is fenced and the instructions call it reference data, but that is a request to the model, not a guarantee. Because the model has no tools, a successful injection can change the text of an answer and nothing else — so the SQL in an answer must still be read before it is run.
- **Claude Code runs with no tools at all. Codex does not.** Codex keeps its own shell tool, and PuriCat knows no documented option that switches it off. It runs in the read-only sandbox, in the empty scratch directory: its commands cannot write, and the sandbox blocks the network for them, but a manipulated Codex could read local files the user can read and quote them in its answer.
- **Codex's successful path is unverified** against the real CLI (see "Stream formats").
- **No version gating.** The options and stream formats belong to the CLIs and can change; an incompatible CLI fails the turn with its own message.
- **The CLIs' own behaviour is outside PuriCat's control**: their telemetry, update checks, logs and any other network calls they make.
- **The schema view is partial.** At most 40 tables are described, in the catalogue's order rather than by relevance, so the table in front may be among those listed by name only; relationships (foreign keys) are not included; a PostgreSQL query tab always describes `public`.
- **The conversation is not saved**, cannot be exported, and there is one conversation at a time. Switching the provider keeps the conversation; the earlier turns are then resent to the other provider.
- **Backend messages are English only** (timeouts, "ended without an answer", the model-name refusal). The CLIs' own errors are shown in whatever language the CLI uses.
- **Unix only in the tests.** The stand-in tests are compiled for Unix; nothing about the chat has run on Windows.

## CLI interfaces confirmed to exist

Confirmed by `--help` on Claude Code 2.1.303 and codex-cli 0.133.0, before the chat was written. **Presence only.**

| CLI | Relevant options |
|---|---|
| Claude Code | `-p/--print`, `--output-format` (including `stream-json`), `--input-format`, `--include-partial-messages`, `--verbose`, `--mcp-config`, `--strict-mcp-config`, `--permission-mode`, `--allowedTools` / `--disallowedTools`, `--tools`, `--system-prompt` / `--append-system-prompt`, `--model`, `--session-id`, `--no-session-persistence`, `--json-schema`, `--setting-sources`, `--bare`; `claude auth status [--json\|--text]` |
| Codex | `codex exec [PROMPT]` (prompt may come from stdin) with `--json`, `--output-schema <FILE>`, `-o/--output-last-message <FILE>`, `-s/--sandbox read-only\|workspace-write\|danger-full-access`, `-m/--model`, `-C/--cd`, `--skip-git-repo-check`, `--ephemeral`, `--ignore-user-config`, `-c key=value`; `codex login status`; `codex mcp`; `codex mcp-server` |

The chat also passes options that this survey does not list: `--safe-mode`, `--disable-slash-commands` and `--permission-prompts none` (Claude Code), `--ignore-rules` and the `codex debug models` command (Codex). The survey was not repeated for this document. What is known about them: the whole Claude vector was accepted by the real Claude Code 2.1.303 in the opt-in test on 2026-11-16, which returned an answer both times it was run; for Codex, the real CLI's refusal lines were captured, but the exact command that produced them was not recorded.

## Planned protocol — Planned (Phase 5, not built)

```
PuriCat (Rust) --spawn--> claude -p --output-format stream-json --verbose ...     [built, without tools]
                          codex exec --json ...                                   [built, without tools]
      ^                         |
      |  streamed events        |  MCP tool calls                                 [not built]
      |                         v
      +---------------- PuriCat MCP server (local, owned by PuriCat)              [not built]
                          read-only tools ----> database (read-only transaction)
                          write tools --------> proposal --> approval queue --> PuriCat executes
```

| Step | Status |
|---|---|
| 1. PuriCat spawns the selected CLI in non-interactive streaming mode and parses its event stream | Built (see "Agent chat") |
| 2. The CLI reaches the database only through an MCP server that PuriCat runs and configures for that invocation | Not built. Today the CLI reaches no database at all |
| 3. Cancelling in the UI kills the subprocess. A timeout does the same | Built |
| 4. A run is shown as completed only after PuriCat has verified the outcome against the database | Not built; there is nothing to verify while the model cannot act |

## Permission boundaries

| Boundary | Mechanism | Status |
|---|---|---|
| The model cannot act | No tools are given to it, and no MCP server is configured | Built for Claude Code (`--tools ""`, `--strict-mcp-config`). Codex keeps its sandboxed shell tool |
| Default tools are read-only | List schemas, describe a table, run a row-limited `SELECT` in a read-only transaction. The tool also rejects anything that is not a single `SELECT`, because a read-only transaction alone is a guard rail (see [SECURITY.md](SECURITY.md)) | Planned (Phase 5) |
| Writes need approval | A write tool returns a proposal. The user sees the SQL, target connection and environment, impact estimate and transaction behaviour. PuriCat executes after approval; the CLI never does | Planned (Phase 5). Today the user copies or opens the SQL and runs it in a query tab |
| The CLI's own tools are restricted | Claude: `--tools ""`, `--strict-mcp-config`, `--permission-prompts none`. Codex: `--sandbox read-only`, `--ignore-user-config`, `--ignore-rules` | Built. With tools, Claude will need a permission mode that allows the PuriCat MCP tools only (open question) |
| Filesystem | The CLI runs in an empty scratch directory | Built (asserted by unit, opt-in and end-to-end tests) |
| Credentials | Never in prompts, arguments or environment | Built: the context contains none, and PuriCat adds nothing to the arguments or the environment. With tools, the MCP server will use PuriCat's existing sessions |
| Untrusted data | Database content and model output are treated as untrusted | Built for what exists: the context is fenced, answers are rendered as text. Tool results do not exist yet |
| Audit | Approvals and outcomes are logged without secrets | Planned (Phase 5). Chat turns are not logged; SQL run from a query tab appears in that tab's history like any other |

The planned boundaries apply to every environment, not only production, unless the user deliberately changes the policy for a connection.

## Context — still planned (Phase 5)

| Context | Plan | Today |
|---|---|---|
| Names and types of explicitly scoped schemas, tables and columns | Sent | Sent for the database (and schema) of the tab in front; the scope cannot be narrowed |
| The user's question, the SQL in the active tab, error messages | Sent | The question and the active tab's SQL are sent. Error messages are not added automatically; the user can paste one |
| Sample rows | Not sent unless the user selects them | Not built; never sent |
| Rows returned by the read-only query tool | Row-limited; visible in the conversation | Not built |
| Passwords, connection strings, host names, user names | Never sent | Never sent |

- A preview shows the exact context before the first message and when the scope changes — built as the context inspector, which the user opens; it is not forced open before the first message.
- Columns can be marked sensitive and are redacted from samples and tool results — not built. The detection rules for suggesting sensitive columns are not designed yet.
- Context follows the active tab and connection — built. Explicit pinning — not built.

## Open questions

Answered by the first slice:

| Question | Answer | Evidence |
|---|---|---|
| The event schema of `claude --output-format stream-json` | The lines listed under "Stream formats" | Captured from Claude Code 2.1.303; unit tests on the captured lines; the opt-in test against the real CLI |
| The event schema of `codex exec --json` | Start and failure lines confirmed; the success lines are taken from the documented format | Captured from codex-cli 0.133.0 (failure only). **The success path is unverified** |
| Whether either CLI writes session history in these modes, and how to turn that off | `--no-session-persistence` and `--ephemeral` are passed, and PuriCat resends the conversation itself | The working directory was empty after a turn (opt-in test on the real Claude CLI; stand-ins in the end-to-end run). What the CLIs write outside the working directory was not inspected |
| How the CLIs behave when the account lacks access to a requested model | Claude: an `assistant` line carrying `error` and a `result` line with `is_error: true` and a sentence. Codex: `error` and `turn.failed` lines whose message wraps the service's JSON error | Captured from both real CLIs; unit tests; end-to-end on stand-ins |

Still open:

- The stability of both stream formats across CLI versions, the minimum supported version of each CLI, and how to detect an incompatible one. Nothing checks versions today.
- A successful Codex turn observed from the real CLI.
- How Codex is given a per-invocation MCP configuration (`-c key=value`, `codex mcp`, or another mechanism) without changing the user's global configuration.
- Which Claude permission mode allows the PuriCat MCP tools and denies everything else without an interactive prompt.
- MCP transport (stdio is preferred so no port is opened) and which MCP library to use.
- Whether Codex can be run with no shell tool at all.
- What the CLIs write outside their working directory in these modes, and what network calls they make besides the request itself.
