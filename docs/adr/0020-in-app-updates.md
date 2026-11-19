# 0020 — In-app updates: a signed package from GitHub Releases, installed when the user says so

## Status

Accepted. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-19 (in-app updates)".

**Verified with stand-in servers only.** A bundled debug build found, verified, installed and restarted into a real package signed with the release key, and refused three packages it must refuse. No released build has updated itself yet: the first release that carries the updater has nothing newer to find, so the path through GitHub (the redirect of `releases/latest`, a notarized package, HTTPS only) is first exercised when the release after it is published.

## Context

Until v0.3.0 a new version reached a user in one way: they heard of it, downloaded the `.dmg` and dragged the app over the old one. The owner asked for the app to look for updates itself, or from a menu.

Constraints:

- PuriCat makes no network request except to the databases the user connects ([REQUIREMENTS.md](../../REQUIREMENTS.md), section 8). An update check is the first exception and must be small, visible and switchable.
- An updater replaces the program that holds the user's database passwords. What it installs must be what the project released, whoever controls the network or the download server.
- The web view has no shell, filesystem or HTTP access ([SECURITY.md](../../SECURITY.md), Tauri capabilities). That must stay.
- A restart ends every query tab: staged edits and open transactions are lost.
- There is no server of the project's own. Releases are files on GitHub.

## Decision

### 1. Tauri's updater plugin, driven from the backend

The plugin (`tauri-plugin-updater`) does the request, the signature check and the replacement of the app bundle. The capability file grants the web view **none** of its permissions. The web view has three commands of PuriCat's own:

| Command | Does |
|---|---|
| `check_update` | Asks the manifest whether a newer version exists; keeps what was found in the backend |
| `install_update` | Downloads, verifies and installs **what the last check found**. It takes no URL, version or path from the web view |
| `restart_app` | Starts the app again |

### 2. One static manifest on GitHub Releases

- The app asks `https://github.com/purivaro/PuriCat/releases/latest/download/latest.json`. The address has no variable part: the request carries no version, no identifier and no query. What GitHub sees is an IP address and the plugin's `User-Agent` (`tauri-plugin-updater/<its version>`); the only other headers are `Accept` and `Host` (logged by the tests' stand-in server).
- `latest.json` names the version, the date and, for `darwin-aarch64` and `darwin-x86_64`, the same universal package (`PuriCat-mac.app.tar.gz` of that release) with its signature.
- The release workflow writes it from the build, and uploads it after the package. A pre-release (a tag with a suffix) never becomes `releases/latest`, so it is never offered.
- A tagged build fails when the update key is missing, and when the tag is not the app's version: either would publish a release that installed apps cannot be offered, or a manifest that points nowhere.

### 3. Nothing is installed without the release key's signature for that very version

- The package is signed in the release workflow with a minisign key kept as a repository secret (`TAURI_SIGNING_PRIVATE_KEY`, with its password). The public half is in `src-tauri/tauri.conf.json` and is compiled into the app.
- `requireSignedVersion` is on from the first release: the signature carries the version it was made for, and a manifest that announces another version for that package is refused. Without it, whoever can answer the manifest request could pair a high version number with an old, genuinely signed package and so install an old release.
- HTTPS is required for the manifest in release builds (the plugin refuses anything else). The signature, not the transport, is what the install rests on.
- This signature is separate from Apple's. The package holds the same signed, notarized and stapled app as the `.dmg`; the workflow unpacks it and checks that.

### 4. The app looks at start-up; the user installs

- One check when the app starts, unless the user turned it off. It is quiet: no dialog, and a failure (offline, GitHub unreachable) shows nothing.
- A newer version shows as a pill in the status bar, in place of the version number. Nothing is downloaded until the user opens it and chooses "Download and install".
- The version number in the status bar, "Check for updates…" in the command palette and "Check for Updates…" in the macOS application menu check on request and show the result, including "up to date" and the reason for a failure.
- The dialog holds the switch for the check at start-up, with a sentence on what is sent.

The check is **on by default**: the owner asked for an automatic check (2026-11-19), an app whose users never learn of fixes is the worse default, and the request says nothing about the user.

### 5. Installing does not restart; restarting asks when something would be lost

- `install_update` replaces the bundle on disk and returns. The running process is the old version until it ends.
- "Restart now" is a separate step. When a tab holds staged edits, an open transaction or an unsaved script, the dialog says how many and the button becomes "Restart and discard".
- "Later" is always there: the new version starts with the next launch.

### 6. Only an installed app replaces itself

The plugin replaces the directory it derives from the executable's path. For anything but `…/Name.app/Contents/MacOS/<exe>` that would be the directory the executable is in, so `install_update` refuses before any download when the process does not run from an app bundle (`pnpm tauri dev`, `cargo run`).

### 7. Development builds never ask GitHub

In a debug build the manifest address comes only from `PURICAT_UPDATE_ENDPOINT`; without it the check is refused. Tests point it at a stand-in server on `127.0.0.1` that logs every request. Release builds do not read the variable.

## Alternatives considered

| Option | Why not |
|---|---|
| Keep manual downloads; only tell the user that a newer version exists | Still a network request, and the step that goes wrong (finding, downloading, dragging) is left to the user |
| Sparkle | A second framework to embed and sign beside Tauri's own, which already does the same job and is maintained with the runtime |
| The plugin's JavaScript API, with `updater:default` in the capability file | It gives the web view `download`, `install` and the plugin's resources. Three commands of PuriCat's own are a smaller surface, and installing takes no argument |
| A dynamic endpoint (`…/{{target}}/{{current_version}}`) | Needs a server, and sends the version and platform with every check |
| Install and restart without asking | It would end tabs with staged edits and open transactions, and replace the program without the user seeing it |
| Check off by default | The owner asked for the automatic check. The switch is in the dialog every check opens |
| No `requireSignedVersion` | The manifest is not signed; without it the version number in it is believed |
| Sign the manifest as well | The plugin has no such check. The version bound into the package's signature closes the same gap |
| Accept a release without the update package | Installed apps would stay on the older version without a word |

## Consequences

- A user on a version with the updater learns of the next one from the app and installs it in two clicks. Version 0.3.0 and older have no updater: they are updated by hand once.
- PuriCat now makes a network request of its own. [REQUIREMENTS.md](../../REQUIREMENTS.md) and [SECURITY.md](../../SECURITY.md) say so.
- The update key is a second secret to keep, and it cannot be replaced from afar: an installed app trusts only the public key it was built with. If the private key is lost, installed apps can no longer be updated by the app and need a manual download of a version with a new key. If it leaks, whoever has it and can serve the manifest (by controlling GitHub's answer, or the user's network together with a certificate the system trusts) can install their own program.
- New dependencies in the app: `tauri-plugin-updater` and what it brings (an HTTP client with rustls, tar, gzip, zip, minisign verification). The download grows accordingly.
- Three IPC commands, no new capability, no change to the Content-Security-Policy.
- The release workflow builds, checks and publishes three more files: `PuriCat-mac.app.tar.gz`, its `.sig` and `latest.json`.

Limitations:

- **macOS only**, like the app.
- **The app must be where the user can write.** In `/Applications`, for an administrator, it is. An app run from the disk image or from a quarantined download location cannot be replaced; the install fails with the system's message and the app is left as it was.
- **An app started through a symbolic link is not updated.** Tauri refuses to work out where such an app is installed, and the check fails with that message. Seen in the test, whose temporary directory is behind `/var`; an app in `/Applications` is not affected.
- **The whole package is downloaded** (the size of the app), into memory, then verified. There are no delta updates.
- **No release notes in the dialog.** The manifest carries none; the dialog shows the version and the date.
- **The native menu item is English** in both languages, like the rest of the application menu.
- **One check per start.** An app left open for weeks does not look again until it is asked or restarted.
- **A check that fails at start-up is silent.** Someone whose network blocks GitHub sees nothing, and learns it only from a check they ask for.
- **Downgrades are not offered**: only a higher version than the running one counts.

## Licensing

`tauri-plugin-updater` is MIT OR Apache-2.0; `minisign-verify` and `reqwest` are MIT (or MIT OR Apache-2.0). They are listed in [LICENSES.md](../../LICENSES.md).
