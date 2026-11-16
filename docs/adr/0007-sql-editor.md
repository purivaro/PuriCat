# 0007 — SQL editor: Monaco

## Status

Proposed (Phase 4). Not implemented; no editor dependency is in the repository.

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
