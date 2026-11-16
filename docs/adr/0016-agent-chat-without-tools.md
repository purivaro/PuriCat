# 0016 — Agent chat first, with a model that has no tools

## Status

Accepted. Implemented as the first slice of Phase 5 for the Claude Code and Codex CLIs. Verified for Claude Code with unit tests on lines captured from the real CLI, the end-to-end test on stand-in CLIs (dev mode and the bundled app under the production CSP), and an opt-in test against the real CLI. **Codex's successful path is not verified against the real CLI**: only the service's refusal was observed, and the success shape is implemented from the documented format ([TESTING.md](../../TESTING.md)).

This record implements the transport and confinement of [ADR 0008](0008-ai-cli-integration.md) and deliberately leaves out its tools. The MCP server with read-only tools, proposals with an approval step and an activity log were built afterwards as the second slice ([ADR 0017](0017-ai-database-tools.md)), as tools the user turns on. They are off by default, and with them off a chat is what this record describes.

## Context

The brief asks for an AI agent driven through the user's installed Claude Code and Codex CLIs, with read-only tools by default and explicit approval for every mutation. [ADR 0008](0008-ai-cli-integration.md) proposed the whole design at once: subprocess transport, a PuriCat-owned MCP server, an approval queue.

Constraints when the work started:

- Several questions of ADR 0008 were open: the exact stream formats, how each CLI is handed an MCP configuration for one invocation, and which Claude permission mode allows only PuriCat's tools. The stream formats could be answered by running the CLIs; the others could not be answered quickly.
- The piece every later step needs — start the CLI, feed it a prompt, read its stream, stop it, report its errors — could be built and tested on its own.
- A tool that runs SQL for a model needs a read-only guarantee that SQL cannot switch off. The read-only session option is not one ([SECURITY.md](../../SECURITY.md), Read-only sessions).
- The SQL console ([ADR 0015](0015-sql-console.md)) already exists as the place where a person runs SQL, with a confirmation, an impact guard and a row cap.
- What a prompt contains is sent to a provider under the user's account. It has to be visible and limited from the first message.
- The CLIs are the user's: they carry the user's own configuration, MCP servers, hooks and project instructions, and they keep sessions on disk by default.

## Decision

### 1. The model gets no tools and no database access

The first slice is a chat. The model receives text and returns text. It cannot list, query or change anything, and it has not seen a row. **The user is the only path from an answer to a database.**

- Claude Code is started with `--tools ""` and `--strict-mcp-config` (no built-in tools, no MCP servers), `--safe-mode`, `--disable-slash-commands` and `--permission-prompts none`.
- Codex is started with `--sandbox read-only`, `--ignore-user-config` and `--ignore-rules`. Codex has no documented option that removes its own shell tool; it stays, inside the read-only sandbox.
- The fixed instructions tell the model the same thing, so that it does not claim to have run or checked anything.

### 2. One turn is one subprocess, and the turns are stateless

Each message starts the CLI once: a fixed argument vector, no shell, a process group of its own on Unix, kill-on-drop, a 10-minute timeout. Stop and the timeout kill the whole group, because a CLI can be a launcher whose real program is its child (Codex is). The first verdict in the stream ends the turn; a CLI that then does not exit within 5 seconds is killed. The CLI keeps nothing (`--no-session-persistence`, `--ephemeral`); PuriCat resends the conversation with every turn, cut to its most recent 60,000 characters. What the provider receives is therefore exactly what the chat shows plus the context the user can inspect. The conversation lives in the web view's memory and is gone when the app closes.

### 3. The prompt travels on stdin; the arguments are constant

The instructions (for Codex, which has no system-prompt option), the context block and the conversation are written to stdin. Nothing of the schema or the conversation appears in the argument vector, where other local processes could read it. The one argument a user can influence is the model name, and it is validated — a short set of characters, not starting with `-` — before any process is started.

### 4. The CLI starts in an empty directory that PuriCat owns

`agent-scratch` in the app-data directory. The CLI has no project of the user's in front of it: no files to read, no project instructions to pick up.

### 5. The context is names and types, rendered by the backend and shown

`agent_context` reads the catalogue of the open session and renders plain text: the engine and server version, the database and schema, tables and views with column names, types, key / NOT NULL / generated flags and the planner's row estimate — for at most 40 tables with at most 80 columns each, the rest by name — and the SQL of the active query tab. It never contains rows, column defaults, host names, ports, user names, passwords or the connection's name.

The text is shown in a context inspector, kept with every sent message exactly as it went out, and can be switched off as a whole. Inside the prompt it is fenced as `<context>`, declared reference data by the instructions, and a closing tag inside it is neutralised.

### 6. SQL in an answer is handed to the SQL console, not run

Answers are parsed by a small Markdown reader into data and rendered as React elements; nothing from an answer is inserted as HTML. A SQL code block gets "Copy" and "Open in query tab". The latter opens a new query tab on the connection the question was asked about, with the text in the editor, and runs nothing. From there the console's safeguards apply as they do to SQL a person typed.

### 7. The CLI speaks for itself

The model selector offers "Default model" (the CLI decides), the CLI's own names (Claude's aliases; what `codex debug models` lists) and any name the user types. A refused model, a missing sign-in or any other failure is shown in the CLI's own words with "Try again". PuriCat assumes nothing about the account's plan or access. The chat is offered only for a CLI that is installed and not known to be signed out.

### 8. Tests use stand-in CLIs; the real ones are opt-in

Stream parsers are tested against lines captured from the real CLIs. The process handling is tested with shell scripts standing in for the CLI, and the end-to-end test puts stand-in `claude` and `codex` scripts first on `PATH`, so it needs no account, network or quota and can check what the app handed over. One ignored test sends a single prompt through the real Claude Code CLI.

## Alternatives considered

| Option | Why not |
|---|---|
| Build ADR 0008 whole: chat, MCP server, read-only tools and approval queue together | Three open questions block the tools, and a model-driven query tool needs a read-only guarantee that does not exist yet. The transport would have waited for all of it |
| Give the model one read-only query tool straight away | A row-limited `SELECT` tool rests on a read-only transaction that a statement can leave, on a classifier that reads keywords, and on showing rows to a provider. Each needs its own decision |
| Let the CLI use its own tools (shell, file access) to reach the database | No approval boundary, and it would need credentials in the CLI's reach ([ADR 0008](0008-ai-cli-integration.md)) |
| Run SQL from an answer directly ("Run" on the code block) | Skips the place where the console's confirmation and impact guard live, and makes one click the whole distance between a model's text and a write |
| Insert the SQL into the active query tab | Would overwrite or mix with what the user is working on; a new tab cannot |
| Keep a CLI session and send only the new message (a session id) | The provider would hold state PuriCat cannot show; sessions are written to disk; a stale session would carry an old schema. Resending costs tokens instead |
| Pass the prompt as an argument | Visible to other local processes in the process list, limited in length, and schema text would sit next to option parsing |
| Start the CLI in the user's home or a project directory | It would read project instructions and could read files |
| Send sample rows, or let the user tick tables to include | Sample rows are data leaving the machine and need redaction first; a table picker is more UI than a first slice needs. Both stay planned |
| Render answers with a Markdown-to-HTML library | Would put model output through an HTML path that needs sanitising; a reader that produces data for React has no such path |
| Hard-code model names for both CLIs | The names change with the CLIs and the account. Claude Code names aliases in its help; Codex can list its catalogue; the default and a free-text field cover the rest |
| Drive the real CLIs in the automated tests | Needs an account, network and quota on every run, is not reproducible, and sends test prompts to a provider |

## Consequences

- The chat is useful at once for drafting SQL, explaining a query and diagnosing an error, and it cannot damage anything by itself.
- A prompt injection through a table name, a column name or the active query can change what an answer says. It cannot make anything happen, because there is nothing the model can do. The user must still read SQL before running it.
- AI-written SQL enters the database through the same door as typed SQL, so the SQL console's limits are now also the AI path's limits ([ADR 0015](0015-sql-console.md)).
- What is sent is inspectable before sending and recorded per message, and contains no data rows.
- The runner (`agent`) is the base for the tool-using agent: the same process handling, with an MCP configuration added in place of "no tools".
- The questions of ADR 0008 about the stream formats are answered for Claude Code and partly for Codex; the MCP questions are untouched.
- No new dependency and no new Tauri capability. The Content-Security-Policy is unchanged. Three IPC commands were added (`agent_models`, `agent_context`, `agent_send`); Stop reuses `cancel_job`.

Limitations:

- **Codex's successful path is unverified.** On the development machine the service refused every model that was tried, so only the start and failure lines were captured from codex-cli 0.133.0. The success lines (`item.completed` with an `agent_message`, `turn.completed`) follow the documented format.
- **Codex keeps a shell tool.** It runs in Codex's read-only sandbox in the empty scratch directory: no writes, and the sandbox blocks the network for its commands. A manipulated Codex could still read local files and quote them in its answer. Claude Code runs with no tools. PuriCat has not tested the sandbox.
- **The model cannot look anything up.** It knows the schema text it was given and nothing else: no rows, no row counts beyond the planner's estimate, no foreign keys, no indexes. It will sometimes guess.
- **The schema view is partial.** The first 40 tables in catalogue order, 80 columns each; a PostgreSQL query tab always describes the schema `public`.
- **Names and the active query can be sensitive.** Table and column names are sent as they are, and the SQL of the active query tab is sent as typed apart from password literals, which are masked the way the history masks them. There is no per-column redaction.
- **Sharing is decided in the UI.** `agent_send` forwards whatever context text it receives; the backend does not enforce the switch.
- **Every turn resends the conversation.** Cost grows with its length up to the 60,000-character cut; turns before the cut are forgotten. Switching the provider sends the earlier turns to the other provider.
- **Nothing is kept.** The conversation is lost when the app closes; there is one conversation at a time.
- **A new query tab carries the database, not the schema.** SQL written against a non-`public` PostgreSQL schema with unqualified names does not resolve there unless the `search_path` includes that schema.
- **No version check.** The options and stream formats are the CLIs'. A CLI that rejects an option fails the turn with its own message; minimum versions are not established.
- **Stop outside Unix is weaker.** Before the CLI has started (while the context is being read) Stop simply sends nothing; once it runs, the stop flag is looked at before every read. On platforms other than Unix only the CLI process itself is killed, not what it started.
- **The first verdict wins.** For Codex the verdict is `turn.completed` or `turn.failed`; an `error` line alone is kept as the explanation in case no verdict follows. Whether Codex emits `error` for conditions it recovers from was not observed.
- **The CLIs' own telemetry, update checks, logs and caches are outside PuriCat's control.** Only the working directory was checked for leftovers.
- **Backend messages are English only.**
- Verified with Claude Code 2.1.303 only, on macOS. Through the UI the chat was exercised on stand-in CLIs with a PostgreSQL context. The 10-minute timeout, the cut-offs of the context, the sharing switch and the real CLIs through the UI have no automated test. The process-group handling was added after the last end-to-end run; it is covered by stand-in unit-test cases and by the opt-in run against the real Claude Code CLI, not by the end-to-end test ([TESTING.md](../../TESTING.md)).

## Licensing

No dependency was added. tokio (MIT), already a dependency, gained its `io-util` feature for reading and writing the CLI's pipes.

The Claude Code and Codex CLIs are separate programs installed and licensed by the user; PuriCat does not bundle or redistribute them ([LICENSES.md](../../LICENSES.md)). What the chat sends is subject to the terms of the user's account with the provider.
