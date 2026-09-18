//! Run identity, steering and stop (`HRN-1`..`HRN-3`, `HRN-5`).
//!
//! Before this module there was exactly one cancellation flag in the process
//! (`RuntimeManager::new_cancel`), which is fine while only one turn can ever be
//! in flight and wrong the moment a run can start another run: the child's flag
//! would replace the parent's, and Stop would hit whichever run started last.
//!
//! `Fleet` replaces that single slot with a registry keyed by run id. A run is
//! addressable while it lives, so it can be steered mid-flight, stopped on its
//! own, or stopped together with everything it started.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::runtime::proxy::{CancelFlag, Usage};

/// Who is talking to a running run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteerSource {
    /// The person, typing into the composer while the run works (`HRN-UI-1`).
    User,
    /// A parent run redirecting a child it delegated to.
    Lead,
}

/// One mid-run instruction, waiting to be picked up at the next safe point.
#[derive(Debug, Clone)]
pub struct Steer {
    pub text: String,
    pub from: SteerSource,
}

impl Steer {
    pub fn user(text: impl Into<String>) -> Self {
        Self { text: text.into(), from: SteerSource::User }
    }

    /// The transcript message this becomes. A lead's steer is prefixed so the
    /// child can tell a redirection from its original brief; a user's steer is
    /// just what they typed, because to the model it is the same thing as
    /// having typed it a moment earlier.
    pub fn into_message(self) -> serde_json::Value {
        let content = match self.from {
            SteerSource::User => self.text,
            SteerSource::Lead => format!("New instruction: {}", self.text),
        };
        serde_json::json!({ "role": "user", "content": content })
    }
}

/// Why a run stopped. Borrowed from DeepSeek Harness's extensible union: the
/// caller always learns whether an answer is finished or merely what was in
/// hand when the run ran out of room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The model produced a final answer.
    Completed,
    /// Stop was pressed, or a parent cancelled the tree.
    Aborted,
    /// The wall clock ran out.
    Timeout,
    /// The tool-step budget ran out.
    MaxSteps,
    /// The model call itself failed.
    Error,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::Completed => "completed",
            StopReason::Aborted => "aborted",
            StopReason::Timeout => "timeout",
            StopReason::MaxSteps => "max_steps",
            StopReason::Error => "error",
        }
    }

    /// True when the text is everything the run had rather than everything it
    /// meant to say. The UI says so instead of passing a stump off as an answer.
    pub fn is_partial(self) -> bool {
        !matches!(self, StopReason::Completed)
    }
}

/// What a run returns. `text` is what the old `String` return was, so callers
/// that only want prose read `.text` and change nothing else.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub text: String,
    pub stop_reason: StopReason,
    pub steps: usize,
    /// `OBS-1`. `None` when no provider on this run reported its usage.
    pub usage: Option<Usage>,
}

/// The room one run is given (`HRN-5`). Replaces the bare `MAX_ITERATIONS`
/// constant so a delegated child can be given a smaller budget than the turn
/// that started it.
#[derive(Debug, Clone)]
pub struct RunLimits {
    /// Tool-call iterations, including the wrap-up turn.
    pub max_iterations: usize,
    /// Wall clock. `None` for an interactive top-level turn: a person watching
    /// can stop it themselves, and a clock that kills their run mid-thought is
    /// worse than one that never fires.
    pub deadline: Option<Instant>,
    /// How deep delegation may go below this run.
    pub max_depth: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self { max_iterations: 12, deadline: None, max_depth: 1 }
    }
}

impl RunLimits {
    /// A top-level turn, with `agent.max_steps` applied if it is set.
    ///
    /// 12 is a starting value, not a law: it is the number of tool rounds a
    /// question normally needs before the answer is worse for more of them.
    /// Building something large is the case it fits worst, so it is a setting.
    /// The ceiling of 50 is a runaway guard, not a judgement.
    pub fn top(db: &crate::db::Db) -> Self {
        let max_iterations = db
            .get_setting("agent.max_steps")
            .ok()
            .flatten()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(12)
            .clamp(1, 50);
        Self { max_iterations, ..Self::default() }
    }

    /// A delegated child: less room than its parent, and always a clock,
    /// because nobody is watching it directly.
    pub fn subagent() -> Self {
        Self {
            max_iterations: 8,
            deadline: Some(Instant::now() + std::time::Duration::from_secs(300)),
            max_depth: 0,
        }
    }

    pub fn out_of_time(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }
}

/// A live run, addressable by id for as long as it is running.
pub struct RunHandle {
    pub id: String,
    pub cancel: CancelFlag,
    /// FIFO of instructions that arrived mid-run, drained at the top of an
    /// iteration. Not applied on arrival: mid-tool-call the transcript is not
    /// in a valid state, and mid-stream a new message would interleave with
    /// tokens already on screen.
    inbox: Mutex<Vec<Steer>>,
    pub parent: Option<String>,
    pub depth: usize,
    pub conversation_id: String,
    pub started_at: Instant,
    steps: AtomicUsize,
    /// `OBS-1`: what this run has cost so far, summed over its turns. Atomics
    /// rather than a lock because a meter reads them while the loop writes.
    prompt_tokens: AtomicU64,
    output_tokens: AtomicU64,
    /// False until some turn actually reported usage. Without it a local run
    /// against a server that says nothing would read as a free one.
    usage_known: std::sync::atomic::AtomicBool,
}

impl RunHandle {
    /// Queue an instruction for the next iteration. Cheap and non-blocking, so
    /// it is safe to call from a Tauri command while the run holds the loop.
    pub fn steer(&self, steer: Steer) {
        if let Ok(mut inbox) = self.inbox.lock() {
            inbox.push(steer);
        }
    }

    /// Take everything queued. Empty is the common case and costs one lock.
    pub fn drain_steers(&self) -> Vec<Steer> {
        self.inbox
            .lock()
            .map(|mut inbox| std::mem::take(&mut *inbox))
            .unwrap_or_default()
    }

    pub fn bump_steps(&self) -> usize {
        self.steps.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn steps(&self) -> usize {
        self.steps.load(Ordering::Relaxed)
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    /// Add one turn's reported cost.
    pub fn add_usage(&self, usage: Usage) {
        self.prompt_tokens.fetch_add(usage.prompt_tokens, Ordering::Relaxed);
        self.output_tokens.fetch_add(usage.output_tokens, Ordering::Relaxed);
        self.usage_known.store(true, Ordering::Relaxed);
    }

    /// What the run has cost, or `None` when no turn ever reported.
    pub fn usage(&self) -> Option<Usage> {
        if !self.usage_known.load(Ordering::Relaxed) {
            return None;
        }
        Some(Usage {
            prompt_tokens: self.prompt_tokens.load(Ordering::Relaxed),
            output_tokens: self.output_tokens.load(Ordering::Relaxed),
        })
    }
}

/// Every run in flight in this process.
#[derive(Default)]
pub struct Fleet {
    runs: Mutex<HashMap<String, Arc<RunHandle>>>,
}

impl Fleet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a run and hand back its handle. The caller owns the flag so a
    /// run started by a path that already had one (the scheduler) keeps it.
    pub fn open(
        &self,
        conversation_id: &str,
        cancel: CancelFlag,
        parent: Option<String>,
        depth: usize,
    ) -> Arc<RunHandle> {
        let handle = Arc::new(RunHandle {
            id: format!("run_{}", uuid::Uuid::new_v4().simple()),
            cancel,
            inbox: Mutex::new(Vec::new()),
            parent,
            depth,
            conversation_id: conversation_id.to_string(),
            started_at: Instant::now(),
            steps: AtomicUsize::new(0),
            prompt_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            usage_known: std::sync::atomic::AtomicBool::new(false),
        });
        if let Ok(mut runs) = self.runs.lock() {
            runs.insert(handle.id.clone(), handle.clone());
        }
        handle
    }

    pub fn get(&self, id: &str) -> Option<Arc<RunHandle>> {
        self.runs.lock().ok()?.get(id).cloned()
    }

    /// Deregister a finished run. Always call this, including on the error
    /// paths — a handle left behind is a run the UI thinks is still working.
    pub fn close(&self, id: &str) {
        if let Ok(mut runs) = self.runs.lock() {
            runs.remove(id);
        }
    }

    pub fn children_of(&self, id: &str) -> Vec<Arc<RunHandle>> {
        self.runs
            .lock()
            .map(|runs| {
                runs.values()
                    .filter(|r| r.parent.as_deref() == Some(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The runs nobody started: one per live top-level turn.
    pub fn roots(&self) -> Vec<Arc<RunHandle>> {
        self.runs
            .lock()
            .map(|runs| runs.values().filter(|r| r.parent.is_none()).cloned().collect())
            .unwrap_or_default()
    }

    /// Stop a run and everything it started. Stopping a lead without its
    /// children would leave orphans burning tokens with no one to report to.
    pub fn cancel_tree(&self, id: &str) {
        let mut stack = vec![id.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if let Some(handle) = self.get(&current) {
                handle.cancel.cancel();
            }
            stack.extend(self.children_of(&current).into_iter().map(|c| c.id.clone()));
        }
    }

    /// Stop every live run. What the composer's Stop control means now that a
    /// turn can be a tree.
    pub fn cancel_all(&self) {
        if let Ok(runs) = self.runs.lock() {
            for run in runs.values() {
                run.cancel.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet_with_tree() -> (Fleet, Arc<RunHandle>, Arc<RunHandle>, Arc<RunHandle>) {
        let fleet = Fleet::new();
        let root = fleet.open("conv_a", CancelFlag::new(), None, 0);
        let child = fleet.open("conv_b", CancelFlag::new(), Some(root.id.clone()), 1);
        let other = fleet.open("conv_c", CancelFlag::new(), None, 0);
        (fleet, root, child, other)
    }

    #[test]
    fn steers_drain_in_order_and_only_once() {
        let fleet = Fleet::new();
        let run = fleet.open("conv", CancelFlag::new(), None, 0);
        run.steer(Steer::user("first"));
        run.steer(Steer::user("second"));
        let drained: Vec<String> = run.drain_steers().into_iter().map(|s| s.text).collect();
        assert_eq!(drained, vec!["first", "second"]);
        assert!(run.drain_steers().is_empty());
    }

    #[test]
    fn a_lead_steer_is_marked_as_a_redirection() {
        let user = Steer::user("check the other folder too").into_message();
        assert_eq!(user["content"], "check the other folder too");
        let lead = Steer { text: "stop and summarise".into(), from: SteerSource::Lead }.into_message();
        assert_eq!(lead["content"], "New instruction: stop and summarise");
    }

    #[test]
    fn cancel_tree_takes_children_and_leaves_strangers_alone() {
        let (fleet, root, child, other) = fleet_with_tree();
        fleet.cancel_tree(&root.id);
        assert!(root.cancel.is_cancelled());
        assert!(child.cancel.is_cancelled());
        assert!(!other.cancel.is_cancelled());
    }

    #[test]
    fn closing_a_run_makes_it_unreachable() {
        let (fleet, root, child, _) = fleet_with_tree();
        assert_eq!(fleet.children_of(&root.id).len(), 1);
        fleet.close(&child.id);
        assert!(fleet.get(&child.id).is_none());
        assert!(fleet.children_of(&root.id).is_empty());
        assert_eq!(fleet.roots().len(), 2);
    }

    #[test]
    fn a_partial_stop_reason_is_never_completed() {
        assert!(!StopReason::Completed.is_partial());
        for reason in [StopReason::Aborted, StopReason::Timeout, StopReason::MaxSteps, StopReason::Error] {
            assert!(reason.is_partial(), "{} should read as partial", reason.as_str());
        }
    }

    #[test]
    fn the_step_limit_is_a_setting_with_a_sane_default_and_a_ceiling() {
        let db = crate::db::Db::open_in_memory().unwrap();
        assert_eq!(RunLimits::top(&db).max_iterations, 12, "unset means the default");

        db.set_setting("agent.max_steps", "30").unwrap();
        assert_eq!(RunLimits::top(&db).max_iterations, 30);

        // A runaway guard and a floor, so neither a typo nor a zero can make a
        // run that never ends or one that can never call a tool.
        db.set_setting("agent.max_steps", "9999").unwrap();
        assert_eq!(RunLimits::top(&db).max_iterations, 50);
        db.set_setting("agent.max_steps", "0").unwrap();
        assert_eq!(RunLimits::top(&db).max_iterations, 1);

        db.set_setting("agent.max_steps", "lots").unwrap();
        assert_eq!(RunLimits::top(&db).max_iterations, 12, "nonsense falls back");
    }

    #[test]
    fn a_deadline_in_the_past_is_out_of_time() {
        let mut limits = RunLimits::default();
        assert!(!limits.out_of_time());
        limits.deadline = Some(Instant::now() - std::time::Duration::from_secs(1));
        assert!(limits.out_of_time());
    }
}
