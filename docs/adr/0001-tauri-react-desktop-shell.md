# 0001 — Tauri 2 + Rust backend + React/TypeScript frontend

## Status

Accepted. Implemented.

## Context

PuriCat is a public desktop database client: macOS first (Apple Silicon), Windows later. It needs native access to the OS credential store, subprocesses, TCP/TLS database drivers and, later, SSH and a background helper. The UI is dense (tree, tabs, grid, editor) and must be localisable.

## Decision

- Shell: Tauri 2 (`tauri` 2.12.x), using the system web view (WKWebView on macOS).
- Backend: Rust, edition 2021, `rust-version` 1.85.
- Frontend: React 19, TypeScript 7 in strict mode with `noUncheckedIndexedAccess`, Vite 8, Tailwind CSS 4.
- UI building blocks: the unified `radix-ui` package for accessible primitives, `cmdk` for the command palette, `lucide-react` icons, `sonner` toasts. Components are written in the repository on top of these; no component kit is vendored.
- State: Zustand stores, only for state shared across components.

## Alternatives considered

| Option | Why not |
|---|---|
| Electron | Ships a browser and Node runtime with every install: larger bundles and higher memory use. Privileged work would be in JavaScript rather than Rust |
| Native Swift / AppKit | Best macOS fidelity, but not portable to Windows; the product would need a second UI |
| Flutter | Would need a custom data grid and SQL editor from scratch; weaker ecosystem for both than the web |

## Consequences

- Rendering differs per platform (WKWebView now, WebView2 on Windows later); the Vite build targets `safari15`, and Windows needs its own test pass.
- WKWebView has no WebDriver, which shaped end-to-end testing ([ADR 0012](0012-e2e-testing-on-macos.md)).
- The web view is treated as untrusted relative to the backend: it gets no shell, filesystem or HTTP capability and talks only through the IPC commands ([SECURITY.md](../../SECURITY.md)).
- Bundle size and memory use have not been measured; no figures are claimed.
- macOS bundle minimum is set to 12.0; only macOS 26 on Apple Silicon has been run.

## Licensing

Tauri MIT/Apache-2.0; React, Vite, Tailwind CSS, Zustand, radix-ui, cmdk, sonner MIT; lucide ISC; TypeScript Apache-2.0. All permissive and compatible with any licence PuriCat may choose. Attribution notices are required on distribution ([LICENSES.md](../../LICENSES.md)).
