# Licences

Status on 2026-11-16. This is a hand-maintained summary of **direct** dependencies, with licences as declared in the manifests of the installed packages (`Cargo.toml` / `package.json`). It is not a complete third-party notice: transitive dependencies are not listed. A generated notice file is a Phase 7 task.

## PuriCat's own licence

**Not decided.** The Cargo workspace is marked `publish = false`, `package.json` is `private`, and the repository contains no licence file. Until the product owner decides, no rights are granted to third parties. The decision is listed as a blocker in [ROADMAP.md](ROADMAP.md).

## Shipped in the app — Rust

| Dependency | Version line | Licence | Used for |
|---|---|---|---|
| tauri | 2 | MIT OR Apache-2.0 | Desktop shell, IPC |
| sqlx | 0.8 | MIT OR Apache-2.0 | MySQL and PostgreSQL drivers, local SQLite store |
| SQLite (bundled through sqlx / `libsqlite3-sys`) | — | SQLite: public domain; `libsqlite3-sys` bindings: MIT | Local settings database |
| keyring | 3 | MIT OR Apache-2.0 | OS credential store |
| `tauri-plugin-clipboard-manager` (+ `@tauri-apps/plugin-clipboard-manager`) | MIT OR Apache-2.0 | Plain-text copy and paste in the data grid |
| tokio | 1 | MIT | Async runtime, subprocesses |
| thiserror | 2 | MIT OR Apache-2.0 | Error type |
| uuid | 1 | Apache-2.0 OR MIT | Connection ids |
| serde, serde_json | 1 | MIT OR Apache-2.0 | Serialisation |

TLS is provided by rustls (Apache-2.0 OR ISC OR MIT) with `ring` (Apache-2.0 AND ISC), pulled in through sqlx's `tls-rustls-ring-native-roots` feature. The rest of the transitive tree has not been reviewed and must be covered by the generated notice.

## Shipped in the app — JavaScript

| Dependency | Version line | Licence | Used for |
|---|---|---|---|
| react, react-dom | 19 | MIT | UI |
| @tauri-apps/api | 2 | MIT OR Apache-2.0 | IPC from the web view |
| zustand | 5 | MIT | State |
| i18next, react-i18next | 27, 18 | MIT | Localisation |
| radix-ui | 1 | MIT | Accessible primitives |
| cmdk | 1 | MIT | Command palette |
| lucide-react | 1 | ISC | Icons |
| sonner | 2 | MIT | Toasts |
| @tanstack/react-virtual | 3 | MIT | Grid virtualization |
| clsx | 2 | MIT | Class-name helper |

## Build and test tools — not shipped

| Tool | Licence |
|---|---|
| TypeScript | Apache-2.0 |
| Vite | MIT |
| Tailwind CSS | MIT |
| @tauri-apps/cli, tauri-build | MIT OR Apache-2.0 |
| Vitest, @vitejs/plugin-react, @types/react, @types/react-dom | MIT |
| Pillow (used by `scripts/make-icon.py`) | Not checked |

Tailwind generates CSS that ships; the tool itself does not.

## Fonts — shipped

| Font | Package | Licence |
|---|---|---|
| Inter | `@fontsource-variable/inter` | SIL Open Font License 1.1 |
| Noto Sans Thai | `@fontsource-variable/noto-sans-thai` | SIL Open Font License 1.1 |
| JetBrains Mono | `@fontsource-variable/jetbrains-mono` | SIL Open Font License 1.1 |

The fonts are bundled; nothing is fetched from the network. The OFL requires its text and the copyright notices to accompany the fonts, and forbids selling the fonts by themselves.

## Candidates for later phases — not in the repository

| Candidate | Phase | Licence | Note |
|---|---|---|---|
| tiberius (SQL Server) | 2b | MIT OR Apache-2.0 | Subject to evaluation ([ADR 0003](docs/adr/0003-database-drivers.md)) |
| russh (SSH) | 2b | Apache-2.0 | [ADR 0009](docs/adr/0009-ssh-tunnels.md) |
| Glide Data Grid 6.0.3 | 3 (fallback) | MIT | Peer dependencies declare React 16–18 only |
| AG Grid Community | 3 (not chosen) | MIT | Range selection and clipboard paste are Enterprise-only (commercial licence) |
| Monaco Editor | 4 | MIT | [ADR 0007](docs/adr/0007-sql-editor.md) |

## External programs — not bundled

| Program | Relationship |
|---|---|
| Claude Code CLI, Codex CLI | Installed and licensed by the user. PuriCat runs them as separate processes and does not redistribute them. Their terms apply to the user's account |
| `mysqldump`, `pg_dump`, `pg_restore` (Phase 6) | Installed by the user and invoked as separate processes. Not redistributed |

## Brand assets and trademarks

- **App icon:** the artwork was generated with an AI image model for this project and then processed locally (`scripts/make-icon.py`). The copyright status of AI-generated artwork varies by jurisdiction; confirm with counsel before relying on exclusive rights or registering a trademark.
- **In-app mark:** a cropped copy of the app icon artwork.
- **Engine tags:** PuriCat's own two-letter glyphs ("My", "Pg", "Ms"), deliberately not vendor logos.
- MySQL, PostgreSQL, SQL Server, Claude, Codex and other product names are trademarks of their respective owners. PuriCat is not affiliated with or endorsed by them. Names are used only to identify compatibility.
- Whether "PuriCat" is available as a trademark has not been checked.

## Obligations before distribution

| Obligation | Status |
|---|---|
| Decide PuriCat's licence and add a licence file | Open (product owner) |
| Generate a complete third-party notice covering transitive dependencies (for example `cargo about` for Rust and `license-checker` for npm) and ship it in the app | Planned (Phase 7) |
| Ship the OFL text and font copyright notices | Planned (Phase 7) |
| Include the copyright and permission notices required by MIT, ISC and Apache-2.0 dependencies (and any NOTICE files) | Planned (Phase 7), through the generated notice |
| Review the transitive dependency tree (and the one "Not checked" entry above) for copyleft or unusual terms | Planned (Phase 7) |
| Add a licence check to CI so new dependencies are reviewed | Planned (Phase 7) |
| Re-check this file whenever a dependency is added or a major version changes | Ongoing |
