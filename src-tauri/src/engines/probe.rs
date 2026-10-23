//! Asking an installed agent app what it is (`CLI-2`): its version and whether
//! it is signed in. Runs nothing except `<program> --version` and the status
//! command the catalog names for that agent, both with a 10 s limit and the
//! cleaned environment (`CLI-9`). Never reads a credential file.

use std::path::Path;
use std::time::Duration;

use super::catalog::{CatalogEntry, StatusCmd};
use super::process::{self, Captured};
use super::{PaidBy, Tested};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// What a probe learned about one installed agent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probed {
    pub version: Option<String>,
    /// `None` = not known, or the agent does not need it (OpenCode).
    pub signed_in: Option<bool>,
    pub paid_by: Option<PaidBy>,
    pub error: Option<String>,
}

/// The first `major.minor.patch` in a text: `2.1.301 (Claude Code)`,
/// `codex-cli 0.163.0`, `opencode v2.1.0`.
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    for token in text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',') {
        let token = token.trim_start_matches(|c: char| !c.is_ascii_digit());
        let core = token.split(['-', '+']).next().unwrap_or("");
        let mut parts = core.split('.');
        if let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next()) {
            if let (Ok(a), Ok(b), Ok(c)) = (a.parse(), b.parse(), c.parse()) {
                return Some((a, b, c));
            }
        }
    }
    None
}

/// Where a version sits against the range recorded in the catalog.
pub fn tested(entry: &CatalogEntry, version: &str) -> Tested {
    let (Some(v), Some(from), Some(below)) =
        (parse_version(version), parse_version(&entry.tested_from), parse_version(&entry.tested_below))
    else {
        return Tested::Unknown;
    };
    if v < from {
        Tested::Older
    } else if v >= below {
        Tested::Newer
    } else {
        Tested::Yes
    }
}

/// Read the agent's own answer about being signed in.
pub fn read_status(status: &StatusCmd, out: &Captured) -> (Option<bool>, Option<PaidBy>) {
    let text = match status.reads.as_str() {
        "stderr" => &out.stderr,
        _ => &out.stdout,
    };
    match status.format.as_str() {
        "claude-json" => {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
                return (None, None);
            };
            match v.get("loggedIn").and_then(|b| b.as_bool()) {
                Some(true) => {
                    let method = v.get("authMethod").and_then(|m| m.as_str()).unwrap_or("");
                    let paid = if method == "claude.ai" {
                        Some(PaidBy::Plan)
                    } else if method.to_ascii_lowercase().contains("api") {
                        Some(PaidBy::ApiKey)
                    } else {
                        None
                    };
                    (Some(true), paid)
                }
                Some(false) => (Some(false), None),
                None => (None, None),
            }
        }
        "codex-text" => {
            // The text is on stderr, and `stdout` is empty. Read both, so a
            // later version that moves it still works.
            let all = format!("{} {}", out.stderr, out.stdout).to_ascii_lowercase();
            if all.contains("not logged in") {
                (Some(false), None)
            } else if all.contains("logged in") {
                let paid = if all.contains("chatgpt") {
                    Some(PaidBy::Plan)
                } else if all.contains("api key") {
                    Some(PaidBy::ApiKey)
                } else {
                    None
                };
                (Some(true), paid)
            } else if out.code.is_some_and(|c| c != 0) {
                (Some(false), None)
            } else {
                (None, None)
            }
        }
        _ => (None, None),
    }
}

/// Probe one installed agent. `program` is the file to run.
pub async fn probe(
    entry: Option<&CatalogEntry>,
    program: &Path,
    env: &[(String, String)],
) -> Probed {
    let mut p = Probed::default();
    let cmd = match process::command(program, &["--version".to_string()], env) {
        Ok(c) => c,
        Err(e) => {
            p.error = Some(e);
            return p;
        }
    };
    let out = match process::run_capture(cmd, PROBE_TIMEOUT).await {
        Ok(o) => o,
        Err(e) => {
            p.error = Some(e);
            return p;
        }
    };
    if out.timed_out {
        p.error = Some("It did not answer within 10 seconds.".into());
        return p;
    }
    let first_line = |s: &str| s.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string);
    if out.code != Some(0) {
        p.error = Some(first_line(&out.stderr).or_else(|| first_line(&out.stdout)).unwrap_or_else(|| "It would not report its version.".into()));
        return p;
    }
    let text = if out.stdout.trim().is_empty() { &out.stderr } else { &out.stdout };
    p.version = parse_version(text)
        .map(|(a, b, c)| format!("{a}.{b}.{c}"))
        .or_else(|| first_line(text));

    if let Some(e) = entry {
        if let Some(status) = &e.status {
            if let Ok(cmd) = process::command(program, &status.args, env) {
                if let Ok(s) = process::run_capture(cmd, PROBE_TIMEOUT).await {
                    if !s.timed_out {
                        let (signed_in, paid) = read_status(status, &s);
                        p.signed_in = signed_in;
                        p.paid_by = paid;
                    }
                }
            }
        }
        if p.paid_by.is_none() {
            p.paid_by = e.paid_by.as_deref().and_then(PaidBy::parse);
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::catalog;

    fn cap(stdout: &str, stderr: &str, code: i32) -> Captured {
        Captured { code: Some(code), stdout: stdout.into(), stderr: stderr.into(), timed_out: false }
    }

    #[test]
    fn versions_are_read_from_what_each_agent_prints() {
        assert_eq!(parse_version("2.1.301 (Claude Code)"), Some((2, 1, 301)));
        assert_eq!(parse_version("codex-cli 0.163.0"), Some((0, 163, 0)));
        assert_eq!(parse_version("opencode v2.1.0"), Some((2, 1, 0)));
        assert_eq!(parse_version("1.2.3-beta.4"), Some((1, 2, 3)));
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn tested_compares_against_the_recorded_range() {
        let claude = catalog::find("claude-code").unwrap();
        assert_eq!(tested(claude, "2.1.301"), Tested::Yes);
        assert_eq!(tested(claude, "2.2.9"), Tested::Yes);
        assert_eq!(tested(claude, "2.1.100"), Tested::Older);
        assert_eq!(tested(claude, "2.3.0"), Tested::Newer);
        assert_eq!(tested(claude, "weird"), Tested::Unknown);
    }

    /// The real output of `claude auth status` (fixture), 2.1.301.
    #[test]
    fn claude_status_reads_the_recorded_json() {
        let fixture = include_str!("../../../fixtures/engines/claude-2.1.301-auth-status.json");
        let status = catalog::find("claude-code").unwrap().status.clone().unwrap();
        assert_eq!(read_status(&status, &cap(fixture, "", 0)), (Some(true), Some(PaidBy::Plan)));
        assert_eq!(
            read_status(&status, &cap(r#"{"loggedIn":false}"#, "", 1)),
            (Some(false), None)
        );
        assert_eq!(read_status(&status, &cap("not json", "", 0)), (None, None));
    }

    /// The real output of `codex login status` (fixture): on stderr, not stdout.
    #[test]
    fn codex_status_is_read_from_stderr() {
        let status = catalog::find("codex").unwrap().status.clone().unwrap();
        assert_eq!(
            read_status(&status, &cap("", "Logged in using ChatGPT\n", 0)),
            (Some(true), Some(PaidBy::Plan))
        );
        assert_eq!(read_status(&status, &cap("", "Not logged in\n", 1)), (Some(false), None));
        assert_eq!(read_status(&status, &cap("", "", 1)), (Some(false), None));
        assert_eq!(read_status(&status, &cap("", "", 0)), (None, None));
    }
}
