# 0007 — SQL editor: Monaco

## Status

Accepted. Implemented for query tabs on MySQL and PostgreSQL (`src/features/sql/monaco.ts`); the surrounding console is recorded in [ADR 0015](0015-sql-console.md).

Update 2026-11-16, answering the open questions below:

- **CSP.** Monaco (`monaco-editor` 0.57) runs in WKWebView under the unchanged production Content-Security-Policy; neither `script-src` nor a `worker-src` had to be loosened. The worker is bundled through Vite's `?worker` import and served from the app itself. Verified by the end-to-end run against the bundled debug app, which now fails on any CSP violation ([TESTING.md](../../TESTING.md)). That run also found that the editor did not appear in the bundle because the production CSS build shortens colour tokens to a form Monaco's theme rejects; tokens are now normalised to `#rrggbb[aa]`.
- **Size.** The editor is its own chunk, loaded when the first query tab opens: about 3.1 MB of JavaScript before compression plus a worker of about 0.3 MB. Only the core editor, a chosen set of features and the `sql` / `mysql` / `pgsql` grammars are included. The effect on start-up was not measured.
- **Formatter.** `sql-formatter` (MIT), loaded on demand.
- **Completion** is PuriCat's own provider: table names the explorer has loaded, columns through `describe_table`, and a keyword list. It is not alias-aware.
- **Fallback.** CodeMirror 6 was not needed.

Not built from the list in "Context": a T-SQL configuration (there is no SQL Server adapter; the generic `sql` grammar is mapped to that engine value), inline error markers, and the diff view for the Phase 5 approval UI. Monaco's own context menu is switched off.

## Context

The SQL workspace needs an editor with MySQL, T-SQL and PostgreSQL highlighting, schema-aware completion, formatting, multiple cursors, selection execution and inline error markers. The brief names Monaco.

## Decision

Use Monaco Editor, loaded only when a query tab is first opened, with:

- completion providers fed from the explorer's cached metadata;
- one language configuration per dialect;
- workers and assets bundled locally (the production CSP allows `script-src 'self'` only; nothing is loaded from a CDN).

## Alternatives considered

| Option | Assessment |
|---|---|
| CodeMirror 6 (MIT) | Much smaller and modular, with a maintained SQL language package that has dialect support. Fewer built-in IDE features (multi-cursor UX, diff view) |
| Plain `<textarea>` with a highlighter | Not adequate for the stated requirements |

## Consequences

- Monaco is large; it must be code-split so start-up is unaffected. The cost has not been measured.
- Monaco's web workers and any inline styles must be checked against the production CSP. Unverified.
- Monaco's built-in SQL support is highlighting only; dialect-aware completion and formatting are PuriCat's work or an additional library (to be chosen in Phase 4).
- The Phase 5 approval UI wants a SQL diff view, which Monaco provides.

## Open questions

- Does Monaco run under the current CSP in WKWebView without loosening `script-src` or `worker-src`?
- Which formatter, and under what licence?
- If the bundle or CSP cost is too high, CodeMirror 6 is the fallback; decide with a spike at the start of Phase 4.

## Licensing

Monaco Editor MIT. CodeMirror 6 MIT.
