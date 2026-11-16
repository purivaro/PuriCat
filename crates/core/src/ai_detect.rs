//! Discovery of locally installed AI command-line tools.
//!
//! Only documented, read-only sub-commands are used:
//! - `claude --version`, `claude auth status --json`
//! - `codex --version`, `codex login status`
//!
//! Nothing here sends a prompt, and account details printed by the tools are discarded.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn binary(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub provider: Provider,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// `None` when the tool could not tell us (old version, timeout, unexpected output).
    pub authenticated: Option<bool>,
}

pub async fn detect_all() -> Vec<CliStatus> {
    let (claude, codex) = tokio::join!(detect(Provider::Claude), detect(Provider::Codex));
    vec![claude, codex]
}

pub async fn detect(provider: Provider) -> CliStatus {
    let Some(path) = locate(provider.binary()).await else {
        return CliStatus {
            provider,
            installed: false,
            path: None,
            version: None,
            authenticated: None,
        };
    };
    let version = run(&path, &["--version"])
        .await
        .map(|o| o.stdout.lines().next().unwrap_or("").trim().to_string())
        .filter(|v| !v.is_empty());
    let authenticated = match provider {
        Provider::Claude => run(&path, &["auth", "status", "--json"])
            .await
            .and_then(|o| parse_claude_auth(&o.stdout)),
        Provider::Codex => run(&path, &["login", "status"])
            .await
            .map(|o| parse_codex_auth(o.success, &o.stdout, &o.stderr)),
    };
    CliStatus {
        provider,
        installed: true,
        path: Some(path.to_string_lossy().into_owned()),
        version,
        authenticated,
    }
}

fn parse_claude_auth(stdout: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    v.get("loggedIn")?.as_bool()
}

fn parse_codex_auth(success: bool, stdout: &str, stderr: &str) -> bool {
    // `codex login status` exits 0 and prints "Logged in using ..." when signed in.
    let text = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    success && text.contains("logged in") && !text.contains("not logged in")
}

struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}

/// Run a tool with fixed arguments: no shell, no stdin, bounded time.
async fn run(path: &Path, args: &[&str]) -> Option<Output> {
    let mut cmd = Command::new(path);
    cmd.args(args)
        .env("PATH", path_with(path.parent()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let out = tokio::time::timeout(PROBE_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    Some(Output {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Node-based CLIs start with `#!/usr/bin/env node`; make sure `node` next to the
/// tool is reachable even when the app was launched from Finder with a bare PATH.
pub(crate) fn path_with(dir: Option<&Path>) -> std::ffi::OsString {
    let mut parts: Vec<PathBuf> = dir.map(|d| vec![d.to_path_buf()]).unwrap_or_default();
    parts.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(parts).unwrap_or_default()
}

pub(crate) async fn locate(binary: &str) -> Option<PathBuf> {
    if let Some(p) = find_in(candidate_dirs(), binary) {
        return Some(p);
    }
    locate_via_login_shell(binary).await
}

/// PATH plus the usual per-user install locations. Apps started from Finder inherit a
/// minimal PATH, so the well-known directories are checked explicitly.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for rel in [
            ".local/bin",
            ".claude/local",
            ".npm-global/bin",
            ".bun/bin",
            ".volta/bin",
            "bin",
        ] {
            dirs.push(home.join(rel));
        }
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    dirs
}

fn find_in(dirs: Vec<PathBuf>, binary: &str) -> Option<PathBuf> {
    dirs.into_iter()
        .map(|d| d.join(binary))
        .find(|p| p.is_file())
}

/// Last resort for version-manager installs (nvm, asdf, ...): ask the user's own login
/// shell. The command string is a constant plus a hard-coded binary name.
#[cfg(unix)]
async fn locate_via_login_shell(binary: &str) -> Option<PathBuf> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    let mut cmd = Command::new(shell);
    cmd.args(["-lic", &format!("command -v {binary}")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(PROBE_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('/'))
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

#[cfg(not(unix))]
async fn locate_via_login_shell(_binary: &str) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_auth_without_keeping_account_details() {
        let json =
            r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "someone@example.com"}"#;
        assert_eq!(parse_claude_auth(json), Some(true));
        assert_eq!(parse_claude_auth(r#"{"loggedIn": false}"#), Some(false));
        assert_eq!(parse_claude_auth("Not JSON"), None);
        assert_eq!(parse_claude_auth("{}"), None);
    }

    #[test]
    fn parses_codex_auth() {
        assert!(parse_codex_auth(true, "Logged in using ChatGPT\n", ""));
        assert!(parse_codex_auth(true, "", "Logged in using an API key"));
        assert!(!parse_codex_auth(false, "Not logged in", ""));
        assert!(!parse_codex_auth(true, "Not logged in", ""));
    }

    #[test]
    fn finds_binaries_only_where_they_exist() {
        let dir = std::env::temp_dir().join(format!("puricat-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fake-tool"), "").unwrap();
        assert_eq!(
            find_in(vec![dir.clone()], "fake-tool"),
            Some(dir.join("fake-tool"))
        );
        assert_eq!(find_in(vec![dir.clone()], "missing-tool"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
