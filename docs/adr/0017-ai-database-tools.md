# 0017 — Database tools for the AI: a per-turn MCP server, a read-only query, proposals

## Status

Accepted. Implemented as the second slice of Phase 5: the tools and the approval step of [ADR 0008](0008-ai-cli-integration.md), on top of the chat of [ADR 0016](0016-agent-chat-without-tools.md). Verification is recorded in [TESTING.md](../../TESTING.md).

**Verified for Claude Code only.** Besides the runs with a stand-in CLI that is a real MCP client (live tests, and the end-to-end test through the window), the real Claude Code CLI 2.1.303 was run against PuriCat's own MCP server and its own bridge below the UI, in an opt-in test: the model called `list_tables`, `describe_table`, `run_select` and `propose_sql`, answered from what they returned, and nothing was written. Through the window the real CLI with tools was driven once, by hand (2026-11-16; not an automated test, and the script used is not in the repository): one turn of Claude Code 2.1.303 with the `haiku` alias through the bundled debug app, at "Structure and rows", with a PostgreSQL table tab in front. The model made two `run_select` calls, answered correctly from them, and proposed one `UPDATE`; the row was unchanged until "Approve and run" and changed by exactly the proposed amount afterwards; the activity log held `read`, `read`, `proposed`, `approved`, `executed`. What that turn's CLI had available besides PuriCat's tools was not looked at. The end-to-end test through the window also runs one stage on MySQL at the "Structure" level, on the stand-in. Codex was handed the server and completed the MCP handshake (`initialize`, `tools/list`) with codex-cli 0.133.0, but no Codex turn could run on the development machine (the service refused every model), so a Codex tool call has never been observed.

The tools are **off by default**. Nothing in this record changes what a chat does until the user turns them on.

## Context

After ADR 0016 the model could only write text. It guessed at anything the schema context did not show, and SQL it wrote reached a database only when the user opened it in a query tab. ADR 0008 had proposed tools from the start; four of its questions were open:

- how each CLI is handed an MCP server for one invocation, without touching the user's own configuration;
- which Claude Code options allow that server's tools and nothing else, without a prompt;
- the transport and the MCP library;
- a read-only query that SQL cannot switch off. The read-only session option is not one: a statement can undo it ([SECURITY.md](../../SECURITY.md), Read-only sessions).

Constraints:

- The database connection, its host and its credentials must stay inside PuriCat.
- Rows are data leaving the machine. Reading them has to be the user's explicit choice, and visible afterwards.
- A change must not be one click away from a model's text (ADR 0016 rejected a "Run" button on code blocks for that reason).
- The CLIs are the user's own installations: their settings, hooks and MCP servers must not leak into the session.

## Decision

### 1. Three levels, chosen by the user, off by default

| Level | The model gets |
|---|---|
| Off (default) | Nothing: the chat of ADR 0016, with its argument vector unchanged |
| Structure | `list_schemas` (PostgreSQL), `list_tables`, `describe_table`, `propose_sql` |
| Structure and rows | The same, and `run_select` |

The level is a remembered setting next to the model selector. The tools are bound, per turn, to the database (and schema) of the tab that was in front when the question was asked; with no table or query tab in front the turn runs without tools.

### 2. PuriCat is the MCP server; the CLI reaches it through a relay

- For each turn that has tools, PuriCat opens a Unix socket in a directory of its own (mode 0700, a random name under the temporary directory, created so that an existing name fails) and serves MCP on it. The socket, its directory and every connection to it are gone when the turn ends.
- The CLI cannot connect to a socket; it can start a stdio MCP server. It is told to start **PuriCat's own executable** with `--mcp-bridge <socket>`. In that mode the executable only copies its stdin to the socket and the socket to its stdout: no window, no settings store, no credentials, no database code path.
- No TCP port is opened. The tools run inside the app, on the session the user already has open.
- The protocol is implemented directly (`crates/core/src/mcp.rs`): JSON-RPC 2.0, one object per line, `initialize`, `ping`, `tools/list`, `tools/call`. Nothing else is answered.

### 3. Handing the server to each CLI for one invocation

Claude Code (confirmed with 2.1.303):

```
--tools "" --mcp-config '{"mcpServers":{"puricat":{"command":"<puricat>","args":["--mcp-bridge","<socket>"]}}}'
--strict-mcp-config --allowedTools mcp__puricat --setting-sources "" --permission-prompts none
```

- `--tools ""` still removes every built-in tool; the MCP tools are separate and remain.
- `--allowedTools mcp__puricat` allows that server's tools; `--permission-prompts none` denies whatever would otherwise prompt.
- `--safe-mode`, which the tool-less chat uses, also switches MCP servers off, so it cannot be used here. `--setting-sources ""` keeps the user's settings and instruction files out instead (checked: the model reported no personal instructions).

Codex (handshake confirmed with 0.133.0; no turn observed):

```
-c mcp_servers.puricat.command="<puricat>" -c mcp_servers.puricat.args=["--mcp-bridge","<socket>"]
```

alongside `--ignore-user-config`, which does not suppress `-c` overrides.

### 4. The reading tool is held by three things a statement cannot switch off

`run_select` runs through `Console::read_only`, on a connection of its own that is closed after the call:

1. **The text** must be one statement that reads (`script::single_read`): one statement, a reading kind, no `INTO`, no locking clause, no MySQL version comment (`/*! … */`, whose content the server executes), and none of a short list of functions that act outside the transaction.
2. **The server prepares it first.** Neither engine prepares more than one statement, so text that the reader took for one statement and the server takes for two is refused by the server. Nothing can follow the statement that would end the transaction.
3. It runs in a **`READ ONLY` transaction**, which is rolled back.

Rows are capped (50 by default, 200 at most), cells are cut at 300 characters, a query is cancelled on the server after 30 seconds and when the turn ends.

### 5. A change is a proposal; approval is a second, deliberate step

- `propose_sql` runs nothing. It validates that the text holds a statement and hands it to the app.
- The backend keeps the proposal. **What is approved is what the backend stored**, not text the web view sends back.
- The conversation shows a card with the SQL. "Review and run…" opens a dialog with the whole SQL, the connection, its environment and database, the statements the classifier flags, and what guards the run. Only "Approve and run" there runs it.
- PuriCat runs it on a console of its own with the SQL console's impact guard ([ADR 0015](0015-sql-console.md)): a row-changing statement above the connection's row threshold is rolled back, and running what was held back is a third, separate confirmation. That run covers only the held statement and what follows it in the proposal, without the limit, and the confirmation shows exactly that text; statements that ran before it are not run again.
- A read-only connection refuses the run. A proposal can also be rejected, or opened in a query tab.

### 6. Everything the model was given is shown, and what happened is logged

- Each tool call appears in the answer where it happened and opens to show what the model asked and **exactly the text it was given**.
- An activity log in the local store records every row-reading query, every proposal, and every approval, rejection and outcome, with the connection's name, the provider and the model. A reading query that was refused or failed is recorded too, as an event of its own. SQL is stored with password literals masked.
- The CLI keeps no session, so the next turn is told in one line per item which tools were used and what became of each proposal.

## Alternatives considered

| Option | Why not |
|---|---|
| An MCP server on a loopback TCP port (streamable HTTP) with a bearer token | Opens a port any local process can reach; the token would sit in the CLI's configuration or arguments. ADR 0008 preferred stdio for this reason |
| A separate small bridge binary | A second executable to build, sign and locate. The app's own executable is already where the app is |
| Giving the CLI the database credentials and a generic SQL MCP server | The credentials would leave the app, and the approval boundary would be the CLI's |
| An MCP library | The protocol surface needed is four methods. A dependency would be larger than the code it replaces and would bring its own transport and runtime assumptions |
| Rely on the read-only session option, or on a read-only transaction alone | A session option can be set back by SQL; a transaction can be ended by a second statement. Each needs the single-statement guarantee to hold |
| Run the query with the extended protocol only | It guarantees one statement, but returns binary values that PuriCat would have to render itself ([ADR 0004](0004-server-side-text-rendering.md)). Preparing first and then running the same text over the text protocol gives both |
| Make the tool call wait for the user's decision and return the outcome | The CLIs time tool calls out, and a turn would hang on a person. The proposal returns at once; the outcome reaches the model with the next turn |
| Approve directly on the card | One click between a model's text and a write (rejected in ADR 0016) |
| Tools on by default at the "Structure" level | It would change how every existing chat runs (a different argument vector, an MCP server) without the user asking, and Codex's tool path is unverified |
| Send tool results back to the model on later turns | Rows would be resent to the provider on every turn. The model's own answer carries what it took from them |
| A per-connection setting for what the AI may do | More surface than a first version needs. A warning is shown when rows are allowed on a Production connection |

## Consequences

- The model can look things up instead of guessing, and can ask for a change in a form the app controls.
- With the tools off, nothing changed: same argument vector, no socket, no MCP.
- With the tools on, Claude Code runs without `--safe-mode`. What keeps the user's customisations out is `--setting-sources ""`, `--strict-mcp-config` and `--disable-slash-commands`.
- A prompt injection through a table name, a column name or a row can now make the model call tools. What it can reach: the catalogue and (if allowed) rows of the database in front, which then go to the provider; and proposals, which the user has to approve. It cannot write.
- A new local surface exists for the length of a turn: the socket. It is reachable by processes of the same OS user only.
- New IPC commands: `agent_proposal_run`, `agent_proposal_reject`, `agent_audit`, `clear_agent_audit`. A third migration adds the `agent_audit` table. No new dependency; tokio gained its `net` feature. No new Tauri capability; the Content-Security-Policy is unchanged.

Limitations:

- **Codex is unverified.** The handshake was seen; a tool call, and whether `codex exec` lets an MCP tool run without asking, were not. The stream parser ignores Codex's tool items, so a Codex answer would show its text but the steps come from PuriCat's side either way.
- **Unix only.** The socket and the bridge use Unix domain sockets. On other platforms a turn with tools fails with a message; the tool-less chat is unaffected.
- **The read-only guarantee is about writing through SQL.** A function with effects outside the transaction can be called by a `SELECT` if the account may call it. The short list of refused names catches the plain spelling only. An account without write privileges remains the strong control.
- **The reading tool is not confined to the database in front.** On MySQL a query can name another database the account can read. The catalogue tools are confined.
- **Rows that are read leave the machine**, under the user's account with the provider. The cap is 200 rows per query, not per turn.
- **Proposals run statement by statement**, each committed on its own, like a script in a query tab; an error leaves the earlier statements applied. A script that leaves a transaction open has it rolled back.
- **Proposals live in memory.** They are lost when the app closes, and cleared with the conversation.
- **The level is one global setting**, not per connection.
- **Same-user processes are trusted.** Another process of the same user that finds the socket during a turn can call the same tools.
- **Step order in the conversation is best effort.** Tool events come from PuriCat and text from the CLI; the text is read first, but a step can land a few characters early.

## Licensing

No dependency was added. tokio (MIT), already a dependency, gained its `net` feature for the Unix socket. The Model Context Protocol is an open specification; PuriCat implements the part it needs and uses no SDK.
