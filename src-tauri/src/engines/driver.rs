//! The contract every agent app is driven through (`CLI-3`).
//!
//! One trait, three implementations (`claude`, `codex`, `acp`). A driver starts
//! the app, sends it one task, follows its events and returns what it said.
//!
//! Phase 1 has the narrow form of a turn: a task in a folder, with every
//! approval request declined. That is what **Try it** (`CLI-13`) needs, and it
//! cannot change anything on disk. The sink for events, the permission port, the
//! run handle and the MCP ticket join `ExternalTurn` in Phase 2, when the
//! delegate tool and the permission panel have something to connect them to.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;

use crate::agent::fleet::RunHandle;
use crate::agent::run::AgentEventSink;
use crate::db::engines::EngineRow;
use crate::permissions::Trust;
use crate::runtime::proxy::{CancelFlag, Usage};

use super::approve::Port;
use super::tools::Tools;
use super::catalog::{self, CatalogEntry};
use super::{process, EngineKind, PaidBy};

/// How an agent asks for permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approvals {
    /// Through an MCP tool we host (`--permission-prompt-tool`). Claude Code.
    PromptTool,
    /// As a request on the same connection. Codex and ACP agents.
    InBand,
}

/// What the app can do. Used to decide what to offer and what to show; a
/// missing ability is never faked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// A native session continues across turns.
    pub resume: bool,
    /// Can take a user message while it works.
    pub steer_mid_turn: bool,
    /// Has a protocol-level cancel (otherwise Stop kills it).
    pub interrupt: bool,
    /// Can mount tools we host (`MCPS-2`).
    pub mcp_tools: bool,
    pub approvals: Approvals,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DriverError {
    #[error("I can't find the app. Check the path under External agents.")]
    NotFound,
    #[error("{0}")]
    Start(String),
    #[error("It isn't signed in: {0}")]
    NeedsSignIn(String),
    /// The app itself said the turn failed.
    #[error("{0}")]
    Failed(String),
    /// Something came that this version of me cannot follow.
    #[error("I couldn't follow what it sent: {0}")]
    Protocol(String),
    #[error("It went quiet for two minutes, so I stopped it.")]
    Stalled,
    #[error("It took longer than the limit, so I stopped it.")]
    TimedOut,
    #[error("Stopped.")]
    Cancelled,
    #[error("It stopped by itself{0}")]
    Exited(String),
}

/// What a finished turn returns.
#[derive(Debug, Clone, Default)]
pub struct ExternalOutcome {
    /// The app's final report.
    pub text: String,
    /// The app's own session id, to resume later.
    pub session: Option<String>,
    /// The model it ran on, as the app names it.
    pub model: Option<String>,
    pub usage: Option<Usage>,
    /// What the run would have cost at list prices. For display only.
    pub notional_cost_usd: Option<f64>,
    pub paid_by: Option<PaidBy>,
    pub stop_reason: String,
    /// Tools it asked for and was refused.
    pub denials: Vec<String>,
    /// Events this version of me did not recognise. Counted, never fatal.
    pub unknown_events: u32,
    /// The last of what it wrote on stderr, for an error message.
    pub stderr_tail: String,
    /// Steps its own settings let through without asking us (`CLI-6`'s footer).
    pub self_allowed: u32,
    /// The session it was asked to continue was gone, so it started fresh.
    pub resume_failed: bool,
    /// Its sandbox was not set up, so it was made to ask about every step.
    pub sandbox_off: bool,
}

/// How to start one app, resolved from its row and the catalog.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: PathBuf,
    /// The arguments that make it speak its protocol (`app-server`, `acp`).
    pub base_args: Vec<String>,
    /// From the engine's settings, appended last.
    pub extra_args: Vec<String>,
    /// The cleaned environment (`CLI-9`).
    pub env: Vec<(String, String)>,
    /// What replaces part of `env` in a folder where the user lets agents edit
    /// freely (an ACP agent's own permission setting). Empty = nothing changes.
    pub auto_env: Vec<(String, String)>,
    pub model: Option<String>,
}

/// One turn. **Try it** (`CLI-13`) is the narrow form: a task in a folder, no
/// tools, every approval declined (`job` is `None`). A delegated job (`CLI-4`)
/// adds what connects it to the rest of Poiesis.
pub struct ExternalTurn<'a> {
    pub launch: &'a Launch,
    pub task: String,
    pub cwd: PathBuf,
    /// No tools at all. For **Try it**.
    pub tool_free: bool,
    pub cancel: CancelFlag,
    pub timeout: Duration,
    pub job: Option<Job<'a>>,
}

/// What a delegated job adds to a turn.
pub struct Job<'a> {
    /// The fixed words the agent is told about its situation (`CLI-5`). The task
    /// itself is in `ExternalTurn::task`, fenced.
    pub system_append: String,
    /// The most the agent may do here: the folder's trust, held down by the
    /// agent's own ceiling (`CLI-6`).
    pub trust: Trust,
    /// The agent's own session to continue (`conversations.engine_session`).
    pub resume: Option<String>,
    /// The child's sink: its steps, words and questions reach the Fleet card.
    pub sink: &'a AgentEventSink,
    /// The Fleet's handle on this run: the steer inbox and the step counter.
    pub run: &'a RunHandle,
    /// Where its questions go.
    pub port: &'a Port<'a>,
    /// The agent's name as the user knows it.
    pub name: String,
    /// Told the agent's own session id the moment it is known, so a Stop that
    /// comes later still leaves it for **Continue there**.
    pub session_seen: &'a (dyn Fn(&str) + Send + Sync),
    /// What the turn left behind, kept even when it ends in an error or a Stop.
    pub trace: &'a Trace,
    /// Poiesis's own tools (`MCPS-2`): `Some` only when the user switched on
    /// *Let it use my memory* for this agent and the agent can mount them.
    pub tools: Option<&'a Tools<'a>>,
}

/// One step as it is stored with the child's transcript.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StepRecord {
    pub id: String,
    pub verb: String,
    pub target: String,
    /// `running` | `done` | `error`.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// What a turn leaves behind whichever way it ends: the steps it took, the words
/// it said along the way, and the last of what it wrote on stderr.
#[derive(Default)]
pub struct Trace {
    pub steps: std::sync::Mutex<Vec<StepRecord>>,
    pub said: std::sync::Mutex<String>,
    pub stderr: std::sync::Mutex<String>,
}

/// Can Poiesis drive this agent for a delegated job yet? Claude Code and Codex, and
/// an ACP agent whose way of being made to ask first is recorded in the catalog (an
/// ACP agent added by hand is not: we cannot know it asks before it acts).
pub fn delegate_ready(kind: EngineKind, id: &str) -> bool {
    match kind {
        EngineKind::ClaudeCode => true,
        EngineKind::Acp => catalog::find(id).is_some_and(|e| !e.ask_everything_env.is_empty()),
        EngineKind::Codex => true,
    }
}

/// Can this agent be given Poiesis's own tools (`MCPS-2`)? Only where the way to
/// mount them has been seen to work.
pub fn memory_ready(kind: EngineKind, _id: &str) -> bool {
    driver_for(kind).capabilities().mcp_tools
}

/// One model an app offers, for the picker in Settings.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[async_trait]
pub trait EngineDriver: Send + Sync {
    fn kind(&self) -> EngineKind;
    fn capabilities(&self) -> Capabilities;
    async fn run_turn(&self, turn: ExternalTurn<'_>) -> Result<ExternalOutcome, DriverError>;
    /// The models the app can be told to use. Empty = the picker is not shown.
    async fn list_models(&self, _launch: &Launch) -> Result<Vec<ModelChoice>, DriverError> {
        Ok(Vec::new())
    }
}

pub fn driver_for(kind: EngineKind) -> Box<dyn EngineDriver> {
    match kind {
        EngineKind::ClaudeCode => Box::new(super::claude::ClaudeDriver),
        EngineKind::Codex => Box::new(super::codex::CodexDriver),
        EngineKind::Acp => Box::new(super::acp::AcpDriver),
    }
}

/// Resolve how to start an engine. The API key, when the engine is set to pay
/// with one, is read from the credential store here and goes into the child's
/// environment only: never into argv, a file or a log.
pub fn launch_for(row: &EngineRow) -> Result<Launch, DriverError> {
    let entry: Option<&CatalogEntry> = catalog::find(&row.id);
    let program = row.program.clone().or_else(|| row.found_path.clone()).ok_or(DriverError::NotFound)?;
    let kind = EngineKind::parse(&row.kind).ok_or_else(|| DriverError::Start(format!("Unknown kind {}.", row.kind)))?;
    let base_args = match kind {
        EngineKind::ClaudeCode => Vec::new(),
        EngineKind::Codex => vec!["app-server".to_string()],
        EngineKind::Acp => match entry {
            Some(e) => e.acp_args.clone(),
            None => serde_json::from_str(&row.args_json).unwrap_or_default(),
        },
    };
    let own_key = if row.payment == "api-key" {
        let var = entry.and_then(|e| e.api_key_env.as_deref()).ok_or_else(|| {
            DriverError::Failed("This app has no API-key mode I know. Switch it to its own sign-in.".into())
        })?;
        let key = crate::secrets::get_secret(crate::secrets::SERVICE_ENGINES, &row.id)
            .ok()
            .flatten()
            .ok_or_else(|| DriverError::Failed("No API key is saved for it. Paste one under Billing.".into()))?;
        Some((var.to_string(), key))
    } else {
        None
    };
    let configured: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&row.env_json).unwrap_or_default();
    let env = process::child_env(entry, &configured, own_key.as_ref().map(|(k, v)| (k.as_str(), v.as_str())));
    Ok(Launch {
        program: PathBuf::from(program),
        base_args,
        extra_args: serde_json::from_str(&row.extra_args).unwrap_or_default(),
        env,
        auto_env: entry.map(|e| e.auto_edits_env.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default(),
        model: row.model.clone(),
    })
}

/// How long a silent app is waited for before the run is called stalled.
/// Codex only: a wedged `app-server` accepts input and never answers. Not used
/// for ACP, where a slow free model can be quiet for a long time.
pub const CODEX_STALL: Duration = Duration::from_secs(120);

/// A shorter wait for tests that need to see the watchdog fire (milliseconds;
/// 0 = the real one).
#[cfg(test)]
pub static STALL_FOR_TESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn codex_stall() -> Duration {
    #[cfg(test)]
    {
        let ms = STALL_FOR_TESTS.load(std::sync::atomic::Ordering::Relaxed);
        if ms > 0 {
            return Duration::from_millis(ms);
        }
    }
    CODEX_STALL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_with_no_path_cannot_be_launched() {
        let row = EngineRow::new("codex", "codex", "Codex");
        assert_eq!(launch_for(&row).unwrap_err(), DriverError::NotFound);
    }

    #[test]
    fn launch_picks_the_arguments_that_make_each_kind_speak_its_protocol() {
        let mut row = EngineRow::new("codex", "codex", "Codex");
        row.found_path = Some("C:\\x\\codex.exe".into());
        assert_eq!(launch_for(&row).unwrap().base_args, vec!["app-server"]);

        let mut row = EngineRow::new("acp:opencode", "acp", "OpenCode");
        row.found_path = Some("C:\\x\\opencode.exe".into());
        assert_eq!(launch_for(&row).unwrap().base_args, vec!["acp"]);

        let mut row = EngineRow::new("acp:custom-1", "acp", "Mine");
        row.program = Some("C:\\x\\mine.exe".into());
        row.args_json = r#"["--serve","acp"]"#.into();
        assert_eq!(launch_for(&row).unwrap().base_args, vec!["--serve", "acp"]);
    }

    #[test]
    fn api_key_mode_without_a_known_variable_is_refused() {
        let mut row = EngineRow::new("codex", "codex", "Codex");
        row.found_path = Some("C:\\x\\codex.exe".into());
        row.payment = "api-key".into();
        assert!(matches!(launch_for(&row), Err(DriverError::Failed(_))), "no api_key_env in the catalog for codex");
    }
}
