# 0012 — End-to-end testing on macOS by script injection in debug builds

## Status

Accepted. Implemented; one smoke suite (`e2e/run.sh`, `e2e/smoke.js`).

## Context

End-to-end tests should exercise the real window, the real IPC layer and real database servers. On macOS the web view is WKWebView, which has no WebDriver, and `tauri-driver` supports Linux and Windows only.

## Decision

Debug builds contain a hook: if `PURICAT_E2E_SCRIPT` names a file, its contents are evaluated in the web view after each page load. The script drives the UI through the DOM and reports progress through the `e2e_report` IPC command, which writes to the file named by `PURICAT_E2E_REPORT`.

The runner (`e2e/run.sh`):

- creates one database, `puricat_e2e`, on local MySQL and PostgreSQL servers from SQL fixtures and drops it on exit (kept with `KEEP_DB=1`);
- starts the app with `PURICAT_DATA_DIR` pointing at a temporary directory and `PURICAT_SECRET_STORE=memory`, so the user's settings and Keychain are untouched;
- polls the report file, optionally captures a window screenshot per stage (`SHOTS=1`), and fails on the `FAILED` stage or a timeout;
- with `APP=<binary>`, launches a bundled debug build instead of `pnpm tauri dev`.

All four environment hooks are behind `#[cfg(debug_assertions)]`.

## Alternatives considered

| Option | Why not |
|---|---|
| Mock the IPC layer and test in a browser (Playwright) | Does not test the backend, the drivers or WKWebView itself |
| `tauri-driver` / WebDriver | Not available for WKWebView on macOS |
| macOS accessibility or AppleScript automation | Brittle selectors, needs accessibility permissions on every machine |
| Ship a test-only build flavour | The debug profile already serves that purpose |

## Consequences

- Events are synthetic (`element.click()`, dispatched `KeyboardEvent`), so native behaviours — real key routing, drag, focus rings, the clipboard — are not exercised.
- The suite needs a logged-in macOS session with a window server and local database servers; it does not run in the current CI workflow.
- The `e2e_report` command is registered in release builds too, but its body is compiled out and it returns an `unsupported` error. Removing the registration from release builds would be cleaner.
- A debug build must never be distributed: anyone who can set its environment can run script in the web view. Release builds contain none of the hooks.
- Selectors rely on roles, labels and a few `data-*` attributes, and on English UI text; UI copy changes can break the suite.

## Licensing

No third-party component is introduced by this decision.
