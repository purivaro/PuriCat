# 0002 — Core crate without Tauri, thin IPC layer

## Status

Accepted. Implemented.

## Context

Database, storage and secret logic must be testable without a window. Two later components need the same logic outside the desktop process: a backup helper started by the OS scheduler ([ADR 0010](0010-backup-scheduler.md)) and an MCP server that exposes database tools to AI CLIs ([ADR 0008](0008-ai-cli-integration.md)).

## Decision

A Cargo workspace with two members:

| Crate | Path | Contents |
|---|---|---|
| `puricat-core` | `crates/core` | Model, settings store, secrets, database adapters, AI CLI discovery. No Tauri dependency |
| `puricat` | `src-tauri` | Tauri setup, IPC commands, `AppState` (store, secret store, open sessions), debug-only test hooks |

Rules:

- IPC commands are wrappers: resolve the session or secret, call one core function, return its result.
- Core types derive serde and define the wire format (camelCase). `src/lib/types.ts` mirrors them by hand.
- Errors cross the boundary as `{ kind, message }`.
- The secret store is a trait in core; the app chooses the implementation.

## Alternatives considered

| Option | Why not |
|---|---|
| Single crate inside `src-tauri` | Helper binaries would have to link Tauri, and unit tests would compile the whole shell |
| Generated TypeScript bindings (for example `specta` or `ts-rs`) | Worth revisiting as the surface grows; at 16 commands the hand-written mirror is small. Not evaluated in depth |
| Core as a separate service process | Extra lifecycle and local-socket authentication for no present benefit |

## Consequences

- `cargo test -p puricat-core` runs without Tauri, a window or a web view.
- The helper binaries of Phases 5 and 6 can depend on `puricat-core` directly.
- The TypeScript mirror can drift from the Rust types; nothing checks it automatically.
- Keychain calls block, so the app runs them through `spawn_blocking`; this stays in the app crate.

## Licensing

No third-party component is introduced by this decision.
