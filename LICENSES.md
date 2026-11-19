# Licences

Status on 2026-11-16. This is a hand-maintained summary of **direct** dependencies, with licences as declared in the manifests of the installed packages (`Cargo.toml` / `package.json`). It is not a complete third-party notice: transitive dependencies are not listed. A generated notice file is a Phase 7 task.

## PuriCat's own licence

**MIT**, decided on 2026-11-16. The text is in [LICENSE](LICENSE) at the repository root. The Cargo workspace is still marked `publish = false` and `package.json` is still `private`; neither manifest declares a `license` field (checked by reading them on 2026-11-16). The direct dependencies listed below declare permissive licences (MIT, Apache-2.0, ISC; OFL for the fonts); the transitive tree has not been reviewed, which is among the obligations at the end of this file.

## Shipped in the app — Rust

| Dependency | Version line | Licence | Used for |
|---|---|---|---|
| tauri | 2 | MIT OR Apache-2.0 | Desktop shell, IPC |
| sqlx | 0.8 | MIT OR Apache-2.0 | MySQL and PostgreSQL drivers, local SQLite store |
| SQLite (bundled through sqlx / `libsqlite3-sys`) | — | SQLite: public domain; `libsqlite3-sys` bindings: MIT | Local settings database |
| keyring | 3 | MIT OR Apache-2.0 | OS credential store |
| `tauri-plugin-clipboard-manager` (+ `@tauri-apps/plugin-clipboard-manager`) | 2 | MIT OR Apache-2.0 | Plain-text copy and paste in the data grid |
| `tauri-plugin-dialog` (+ `@tauri-apps/plugin-dialog`) | 2 | Apache-2.0 OR MIT (npm package: MIT OR Apache-2.0) | Native Save dialog for exports; only the `save` permission is granted |
| `tauri-plugin-updater` | 2 | Apache-2.0 OR MIT | Looking for a newer version, verifying and installing it ([ADR 0020](docs/adr/0020-in-app-updates.md)). Brings in `reqwest` (MIT OR Apache-2.0) with rustls, `minisign-verify` (MIT), `tar`, `flate2` and `zip` (MIT OR Apache-2.0 / MIT) |
| rust_xlsxwriter | 0.96 | MIT OR Apache-2.0 | Writing XLSX exports |
| csv (+ csv-core) | 1 | Unlicense OR MIT | Reading CSV and TSV files for Smart Paste |
| calamine (+ quick-xml, codepage, atoi_simd, fast-float2) | 0.35 | MIT (calamine, quick-xml); MIT OR Apache-2.0 (the others) | Reading Excel and OpenDocument workbooks for Smart Paste |
| encoding_rs | 0.8 | (Apache-2.0 OR MIT) AND BSD-3-Clause | Decoding text files that are not UTF-8 |
| futures-util | 0.3 | MIT OR Apache-2.0 | Reading the driver's result stream in the SQL console |
| tokio | 1 | MIT | Async runtime, subprocesses |
| thiserror | 2 | MIT OR Apache-2.0 | Error type |
| uuid | 1 | Apache-2.0 OR MIT | Connection ids |
| serde, serde_json | 1 | MIT OR Apache-2.0 | Serialisation |

TLS is provided by rustls (Apache-2.0 OR ISC OR MIT) with `ring` (Apache-2.0 AND ISC), pulled in through sqlx's `tls-rustls-ring-native-roots` feature. `tauri-plugin-dialog` brings in `rfd` and `tauri-plugin-fs` as dependencies (the filesystem plugin is compiled in but not registered; see [SECURITY.md](SECURITY.md)); their licences were not read. The rest of the transitive tree has not been reviewed and must be covered by the generated notice.

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
| monaco-editor | 0.57 | MIT | SQL editor in query tabs ([ADR 0007](docs/adr/0007-sql-editor.md)); loaded on demand |
| sql-formatter | 15 | MIT | "Format" in the SQL editor; loaded on demand |

`@tauri-apps/plugin-clipboard-manager` and `@tauri-apps/plugin-dialog` (both 2, MIT OR Apache-2.0) are listed with their Rust halves above.

`monaco-editor` ships its own `ThirdPartyNotices.txt` and depends on `marked` and `dompurify`; `sql-formatter` depends on `argparse` and `nearley`. Those notices and transitive licences were not read and must be carried into the generated notice. The Monaco chunk also bundles the Codicons icon font (`codicon.ttf`) that comes inside the `monaco-editor` package; that package declares MIT and its `ThirdPartyNotices.txt` does not mention the font, so the font's own terms are **not checked**.

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

Monaco Editor was a candidate here until Phase 4; it is now a dependency and listed under "Shipped in the app — JavaScript".

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
| Decide PuriCat's licence and add a licence file | Done: MIT, [LICENSE](LICENSE). The `license` field of the Cargo and npm manifests is not set |
| Generate a complete third-party notice covering transitive dependencies (for example `cargo about` for Rust and `license-checker` for npm) and ship it in the app | Planned (Phase 7) |
| Ship the OFL text and font copyright notices | Planned (Phase 7) |
| Include the copyright and permission notices required by MIT, ISC and Apache-2.0 dependencies (and any NOTICE files) | Planned (Phase 7), through the generated notice |
| Review the transitive dependency tree (and the "Not checked" entries above: Pillow, and the Codicons font bundled with `monaco-editor`) for copyleft or unusual terms | Planned (Phase 7) |
| Carry `monaco-editor`'s `ThirdPartyNotices.txt` into the shipped notice | Planned (Phase 7), through the generated notice |
| Add a licence check to CI so new dependencies are reviewed | Planned (Phase 7) |
| Re-check this file whenever a dependency is added or a major version changes | Ongoing |
