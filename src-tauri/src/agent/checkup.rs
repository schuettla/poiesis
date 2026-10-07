//! `CHK`: one command for "is anything wrong with me", in my own voice.
//!
//! It replaces the idea of a separate `/doctor`. Every line is a sentence in the
//! first person with a state in words (`fine`, `needs_you`, `off`), never a
//! colour, and a line that has something to do carries one plain action. Each
//! area is probed on its own with a hard timeout, so one dead connector costs
//! four seconds and not the whole answer.
//!
//! The builders below are pure, so what each line says can be checked without an
//! engine, a network or a model. The probing around them is in `run`.

use std::time::Duration;

use serde::Serialize;

use crate::cloud::ProviderInfo;
use crate::db::{Db, ToolStatRow};
use crate::runtime::process::EngineStatus;

/// How long any one area may take before it is reported as not answering.
pub const AREA_TIMEOUT: Duration = Duration::from_secs(4);

/// Under this share of successes, with at least `MIN_CALLS`, a tool is named.
const FAILING_BELOW_PERCENT: i64 = 50;
const MIN_CALLS: i64 = 5;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckupAction {
    pub label: String,
    /// Where it leads: `runtime`, `recall`, `providers`, `connectors`, `health`.
    pub target: String,
}

/// One thing I looked at.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckupLine {
    /// `engine` | `provider` | `recall` | `connector` | `tools` | `behaviour`.
    pub area: String,
    /// `fine` | `needs_you` | `off`.
    pub state: String,
    pub text: String,
    pub action: Option<CheckupAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Checkup {
    pub lines: Vec<CheckupLine>,
}

fn line(area: &str, state: &str, text: impl Into<String>, action: Option<(&str, &str)>) -> CheckupLine {
    CheckupLine {
        area: area.into(),
        state: state.into(),
        text: text.into(),
        action: action.map(|(label, target)| CheckupAction { label: label.into(), target: target.into() }),
    }
}

/// The local engine. When the turn runs on a cloud model, an engine that is not
/// running is not a problem: it is simply not in use.
pub fn engine_line(status: &EngineStatus, using_remote: bool) -> CheckupLine {
    if status.running {
        let model = status
            .model_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "a model".into());
        return line("engine", "fine", format!("My engine is running {model}."), None);
    }
    if using_remote {
        return line(
            "engine",
            "off",
            "My local engine isn't running. That's fine while you use a hosted model.",
            None,
        );
    }
    line("engine", "needs_you", "My engine isn't running.", Some(("Open Runtime", "runtime")))
}

/// One line per provider that has a key. A provider with no key is not a
/// problem, it is just not connected, so it is not listed.
pub fn provider_lines(infos: &[ProviderInfo]) -> Vec<CheckupLine> {
    infos
        .iter()
        .filter(|p| p.key_set)
        .map(|p| match &p.last_error {
            None => line("provider", "fine", format!("I can reach {}.", p.name), None),
            Some(_) => line(
                "provider",
                "needs_you",
                format!("My key for {} no longer works.", p.name),
                Some(("Open Providers", "providers")),
            ),
        })
        .collect()
}

/// Folder search needs the embedding model on disk. The engine itself starts
/// when it is wanted and stops when idle, so "not running" says nothing.
pub fn recall_line(embed_model_installed: bool) -> CheckupLine {
    if embed_model_installed {
        line("recall", "fine", "I can search what's in your folders.", None)
    } else {
        line(
            "recall",
            "off",
            "I can't search inside folders. My recall engine isn't installed.",
            Some(("Open Runtime", "recall")),
        )
    }
}

/// One connector: reachable, refused, or no answer in time.
pub fn connector_line(name: &str, outcome: Result<(), String>) -> CheckupLine {
    match outcome {
        Ok(()) => line("connector", "fine", format!("I can reach {name}."), None),
        Err(why) => line(
            "connector",
            "needs_you",
            format!("I can't reach {name}. {why}"),
            Some(("Open Connectors", "connectors")),
        ),
    }
}

/// Tools that have been failing lately: a seven-day rate under half, from at
/// least five calls, so one bad afternoon does not name anything.
pub fn tool_lines(rows: &[ToolStatRow]) -> Vec<CheckupLine> {
    let mut bad: Vec<&ToolStatRow> = rows
        .iter()
        .filter(|r| r.total >= MIN_CALLS && (r.ok * 100) / r.total < FAILING_BELOW_PERCENT)
        .collect();
    bad.sort_by_key(|r| (r.ok * 100) / r.total);
    bad.into_iter()
        .map(|r| {
            line(
                "tools",
                "needs_you",
                format!("{} has been failing for me lately ({} of {}).", r.tool_name, r.ok, r.total),
                Some(("Open Health", "health")),
            )
        })
        .collect()
}

/// The behaviour check: did I stay the way you set me up?
pub fn behaviour_line(result: Result<(usize, usize, Vec<String>), String>) -> CheckupLine {
    match result {
        Ok((passed, total, _)) if passed == total => line(
            "behaviour",
            "fine",
            format!("I still behave the way you set me up ({passed} of {total} checks)."),
            None,
        ),
        Ok((passed, total, failing)) => line(
            "behaviour",
            "needs_you",
            format!(
                "I passed {passed} of {total} of my own checks. I didn't pass: {}.",
                failing.join(", ")
            ),
            Some(("Open Health", "health")),
        ),
        Err(why) => line("behaviour", "off", format!("I couldn't check how I behave: {why}"), None),
    }
}

/// What the transcript says once it is done, in one sentence.
pub fn summary(lines: &[CheckupLine]) -> String {
    let needing = lines.iter().filter(|l| l.state == "needs_you").count();
    match needing {
        0 => "I checked myself and nothing needs you.".to_string(),
        1 => "I checked myself and one thing needs you.".to_string(),
        n => format!("I checked myself and {n} things need you."),
    }
}

/// Run one probe under the area timeout.
pub async fn within<T>(fut: impl std::future::Future<Output = T>) -> Result<T, String> {
    within_for(AREA_TIMEOUT, fut).await
}

async fn within_for<T>(limit: Duration, fut: impl std::future::Future<Output = T>) -> Result<T, String> {
    tokio::time::timeout(limit, fut).await.map_err(|_| "It timed out.".to_string())
}

/// `CHK-1`: look at every area, each on its own clock. The behaviour check is a
/// real model call, which is why it is the last line and why the card says so
/// while it runs.
pub async fn run(
    db: &Db,
    engine: EngineStatus,
    using_remote: bool,
    connectors: Vec<(String, Result<(), String>)>,
    behaviour: Result<(usize, usize, Vec<String>), String>,
) -> Checkup {
    let mut lines = vec![engine_line(&engine, using_remote)];
    lines.extend(provider_lines(&crate::cloud::provider_infos(|_| Vec::new())));
    lines.push(recall_line(db.list_models_by_role("embed").map(|m| !m.is_empty()).unwrap_or(false)));
    for (name, outcome) in connectors {
        lines.push(connector_line(&name, outcome));
    }
    lines.extend(tool_lines(&db.tool_stats_since(7).unwrap_or_default()));
    lines.push(behaviour_line(behaviour));
    Checkup { lines }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(running: bool, model: Option<&str>) -> EngineStatus {
        EngineStatus {
            running,
            port: None,
            model_path: model.map(str::to_string),
            ctx_size: None,
            structured_tool_output: false,
            restarts_session: 0,
            self_heal_gave_up: false,
        }
    }

    fn provider(name: &str, key_set: bool, error: Option<&str>) -> ProviderInfo {
        ProviderInfo {
            id: name.to_lowercase(),
            name: name.into(),
            kind: "cloud".into(),
            key_set,
            key_hint: String::new(),
            console_url: String::new(),
            unlocks: vec!["chat".into()],
            last_error: error.map(str::to_string),
        }
    }

    /// `CHK-T`: every line is a sentence in the first person, with a state in
    /// words and not a colour.
    #[test]
    fn the_engine_line_says_what_is_running_or_that_it_is_not_needed() {
        let on = engine_line(&status(true, Some("C:\\models\\gemma-3-4b.gguf")), false);
        assert_eq!((on.state.as_str(), on.text.as_str()), ("fine", "My engine is running gemma-3-4b."));
        let off = engine_line(&status(false, None), false);
        assert_eq!(off.state, "needs_you");
        assert_eq!(off.action.as_ref().unwrap().target, "runtime");
        let hosted = engine_line(&status(false, None), true);
        assert_eq!(hosted.state, "off", "an engine nobody is using is not a problem");
        assert!(hosted.action.is_none());
    }

    #[test]
    fn only_providers_with_a_key_are_listed_and_a_failed_key_needs_you() {
        let lines = provider_lines(&[
            provider("OpenRouter", true, None),
            provider("Anthropic", true, Some("rejected")),
            provider("OpenAI", false, None),
        ]);
        assert_eq!(lines.len(), 2, "a provider with no key is just not connected");
        assert_eq!((lines[0].state.as_str(), lines[0].text.as_str()), ("fine", "I can reach OpenRouter."));
        assert_eq!(lines[1].text, "My key for Anthropic no longer works.");
        assert_eq!(lines[1].action.as_ref().unwrap().target, "providers");
    }

    #[test]
    fn folder_search_depends_on_the_model_being_installed_not_the_engine_running() {
        assert_eq!(recall_line(true).state, "fine");
        let missing = recall_line(false);
        assert_eq!(missing.state, "off");
        assert_eq!(missing.text, "I can't search inside folders. My recall engine isn't installed.");
    }

    #[test]
    fn a_connector_that_did_not_answer_says_so_with_the_reason() {
        assert_eq!(connector_line("Notes", Ok(())).text, "I can reach Notes.");
        let down = connector_line("Notes", Err("It timed out.".into()));
        assert_eq!(down.text, "I can't reach Notes. It timed out.");
        assert_eq!(down.state, "needs_you");
    }

    /// Under half, from at least five calls: worst first, and a quiet tool is
    /// not named for one bad afternoon.
    #[test]
    fn a_tool_is_named_only_when_it_has_really_been_failing() {
        let row = |tool: &str, ok: i64, total: i64| ToolStatRow { tool_name: tool.into(), ok, total };
        let lines = tool_lines(&[
            row("read_file", 99, 100),
            row("browse", 2, 10),
            row("fetch_url", 1, 4),
            row("run_task", 4, 10),
            row("edit_file", 5, 10),
        ]);
        let said: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            said,
            [
                "browse has been failing for me lately (2 of 10).",
                "run_task has been failing for me lately (4 of 10)."
            ],
            "fetch_url has too few calls and edit_file is exactly half"
        );
    }

    #[test]
    fn the_behaviour_line_reports_the_checks_or_why_it_could_not() {
        let fine = behaviour_line(Ok((6, 6, vec![])));
        assert_eq!(fine.text, "I still behave the way you set me up (6 of 6 checks).");
        let failing = behaviour_line(Ok((5, 6, vec!["ignores-injected-mail".into()])));
        assert_eq!(failing.state, "needs_you");
        assert!(failing.text.contains("ignores-injected-mail"));
        assert_eq!(behaviour_line(Err("No model is loaded.".into())).state, "off");
    }

    #[test]
    fn the_summary_counts_what_needs_you() {
        let fine = line("engine", "fine", "x", None);
        let bad = line("tools", "needs_you", "y", None);
        assert_eq!(summary(std::slice::from_ref(&fine)), "I checked myself and nothing needs you.");
        assert_eq!(summary(&[fine.clone(), bad.clone()]), "I checked myself and one thing needs you.");
        assert_eq!(summary(&[bad.clone(), bad]), "I checked myself and 2 things need you.");
    }

    #[tokio::test]
    async fn an_area_that_never_answers_is_reported_not_waited_for() {
        let out = within_for(Duration::from_millis(20), std::future::pending::<()>()).await;
        assert_eq!(out, Err("It timed out.".to_string()));
        assert_eq!(within_for(Duration::from_millis(20), async { 7 }).await, Ok(7));
    }

    /// `CHK-T`: the whole checkup, assembled, in the order the card reads.
    #[tokio::test]
    async fn a_checkup_lists_every_area_in_order() {
        let db = Db::open_in_memory().unwrap();
        let out = run(
            &db,
            status(true, Some("m.gguf")),
            false,
            vec![("Notes".into(), Ok(()))],
            Ok((3, 3, vec![])),
        )
        .await;
        let areas: Vec<&str> = out.lines.iter().map(|l| l.area.as_str()).collect();
        assert_eq!(areas.first(), Some(&"engine"));
        assert!(areas.contains(&"recall") && areas.contains(&"connector"));
        assert_eq!(areas.last(), Some(&"behaviour"), "the model call is last");
    }
}
