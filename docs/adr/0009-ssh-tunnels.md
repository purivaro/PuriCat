# 0009 — SSH tunnels with russh

## Status

Proposed (Phase 2b). Not implemented; the connection dialog states that SSH is planned.

## Context

Many databases are reachable only through a bastion host. The brief requires host-key verification, private keys with passphrases, agent integration, lifecycle and port-conflict handling, and no insecure host-key bypass by default.

## Decision

Implement tunnels in-process with `russh` (pure Rust):

- **Host keys:** verify against the user's `known_hosts`. An unknown host shows its fingerprint and asks once; a changed key blocks the connection. No "skip verification" option in the default UI.
- **Authentication:** private key file (passphrase stored in the OS credential store), SSH agent, and password as a last resort.
- **Forwarding:** bind a local listener on `127.0.0.1` with an OS-assigned port and point the database driver at it. The port is never fixed, so conflicts cannot occur.
- **Lifecycle:** the tunnel belongs to the `Session`; closing the session closes the tunnel. A dropped tunnel surfaces as a connection error, not a hang.

## Alternatives considered

| Option | Assessment |
|---|---|
| Spawn the system `ssh` binary with `-L` | Reuses the user's `ssh_config`, agent and `known_hosts` with no extra code. Harder lifecycle and error handling (parsing stderr, detecting readiness), and the binary is absent on some Windows setups |
| `ssh2` (libssh2 bindings) | C dependency, blocking API |
| Require users to open tunnels themselves | Fails the requirement |

## Consequences

- PuriCat owns `known_hosts` parsing, agent protocol use and key-format support. Each must be tested against a real SSH server; none of this has been verified.
- `~/.ssh/config` directives (ProxyJump, per-host identities) are not honoured automatically. Jump hosts are out of scope unless shown feasible.
- A local forward on `127.0.0.1` is reachable by other processes of any local user while it is open. Assess binding options when implementing.
- With a tunnel, TLS host-name verification sees `127.0.0.1`, not the server name; `verify-full` needs the expected name passed separately. Open design point.
- An SSH test fixture (disposable `sshd`) is needed; the development machine has no Docker.

## Licensing

russh Apache-2.0. Its cryptography dependencies must be included in the Phase 7 licence scan.
