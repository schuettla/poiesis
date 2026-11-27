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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::subagents::ClaimGuard;
use crate::runtime::proxy::{CancelFlag, Usage};

/// Who is talking to a running run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteerSource {
    /// The person, typing into the composer while the run works (`HRN-UI-1`).
    User,
    /// A parent run redirecting a child it delegated to.
    Lead,
    /// `TEAM-15`: a post from someone in the team. The text already says who from.
    Team,
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
            SteerSource::User | SteerSource::Team => self.text,
            SteerSource::Lead => format!("New instruction: {}", self.text),
        };
        serde_json::json!({ "role": "user", "content": content })
    }
}

/// `AGC-3`: what the user answered to an `ask_user` question. Either may be
/// empty but not both: a choice, a few choices, their own words, or a choice and
/// something to add.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub text: Option<String>,
}

impl Answer {
    /// The tool result the model reads (`AGC-3`).
    pub fn to_result(&self) -> String {
        let chose = self.choices.join(", ");
        let wrote = self.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
        match (self.choices.is_empty(), wrote) {
            (false, None) => format!("The user chose: {chose}"),
            (false, Some(t)) => format!("The user chose: {chose}, and added: {t}"),
            (true, Some(t)) => format!("The user wrote: {t}"),
            (true, None) => "The user gave no answer. Carry on with your best judgement.".to_string(),
        }
    }

    /// What the step row says after the question: `— you chose A, C`.
    pub fn to_note(&self) -> String {
        let wrote = self.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
        match (self.choices.is_empty(), wrote) {
            (false, _) => format!("\u{2014} you chose {}", self.choices.join(", ")),
            (true, Some(t)) => {
                let short: String = t.chars().take(48).collect();
                let more = if t.chars().count() > 48 { "\u{2026}" } else { "" };
                format!("\u{2014} you wrote: {short}{more}")
            }
            (true, None) => "\u{2014} no answer".to_string(),
        }
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
    /// Not getting anywhere: no plan item finished for a long stretch, or the
    /// same call gave the same result again and again.
    Stuck,
    /// The model call itself failed.
    Error,
    /// `TEAM-12`: the team's task spent what it was given, in money or in minutes.
    Budget,
    /// `XA-11`: an agent app ran into the person's plan limit. It works again after a
    /// known time, so this is not a crash.
    PlanLimit,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::Completed => "completed",
            StopReason::Aborted => "aborted",
            StopReason::Timeout => "timeout",
            StopReason::MaxSteps => "max_steps",
            StopReason::Stuck => "stuck",
            StopReason::Error => "error",
            StopReason::Budget => "budget",
            StopReason::PlanLimit => "plan_limit",
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
    /// A run working to a plan is not held to `max_iterations`: it goes on while
    /// items keep getting done, up to this runaway guard. `None` keeps the plain
    /// budget (a helper, or a turn whose `/steps` was set by hand).
    pub plan_ceiling: Option<usize>,
    /// `TEAM-12`: what the whole team task may spend and how long it may take, shared by
    /// the coordinator and every member run of it. `None` for everything that is not a
    /// team task.
    pub allowance: Option<Arc<Allowance>>,
}

/// `TEAM-12`: the spend cap and the minutes of one team task, next to a run's own `deadline`.
///
/// One of these is shared by the coordinator's run and every member run of the task. Each run
/// reports what it has cost so far; when the sum reaches the cap, or the time is up, every
/// member that was enrolled is cancelled at once and the allowance reads as spent. The
/// coordinator is not cancelled: it is told (`RunLimits::spent`) and gets its closing turn to
/// say what is done and what is not.
#[derive(Debug)]
pub struct Allowance {
    cap_usd: Option<f64>,
    deadline: Option<Instant>,
    /// What each run has cost so far, by run id. A run that ended keeps its last figure.
    spent: Mutex<HashMap<String, f64>>,
    /// The member runs to stop, which the coordinator's is not.
    members: Mutex<Vec<CancelFlag>>,
    reached: Mutex<Option<Spent>>,
}

/// What ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spent {
    Money,
    Time,
}

impl Allowance {
    /// `cap_usd` and `minutes` of 0 or less mean no limit of that kind.
    pub fn new(cap_usd: f64, minutes: u32) -> Self {
        Self {
            cap_usd: (cap_usd > 0.0).then_some(cap_usd),
            deadline: (minutes > 0).then(|| Instant::now() + std::time::Duration::from_secs(u64::from(minutes) * 60)),
            spent: Mutex::new(HashMap::new()),
            members: Mutex::new(Vec::new()),
            reached: Mutex::new(None),
        }
    }

    /// A member run that this allowance stops. One enrolled after the limit was reached is
    /// stopped at once.
    pub fn enroll(&self, cancel: &CancelFlag) {
        if self.spent_out().is_some() {
            cancel.cancel();
            return;
        }
        if let Ok(mut members) = self.members.lock() {
            members.push(cancel.clone());
        }
    }

    /// What a run has cost so far (the whole figure, not the part since the last call).
    /// Reaching the limit stops every member.
    pub fn charge(&self, run_id: &str, usd: f64) {
        if let Ok(mut spent) = self.spent.lock() {
            spent.insert(run_id.to_string(), usd.max(0.0));
        }
        self.check();
    }

    pub fn spent_usd(&self) -> f64 {
        self.spent.lock().map(|s| s.values().sum()).unwrap_or(0.0)
    }

    /// What ran out, if something did. Looks at the clock too, so a quiet task still ends on time.
    pub fn spent_out(&self) -> Option<Spent> {
        self.check();
        self.reached.lock().ok().and_then(|r| *r)
    }

    fn check(&self) {
        let Ok(mut reached) = self.reached.lock() else { return };
        if reached.is_some() {
            return;
        }
        let what = if self.cap_usd.is_some_and(|cap| self.spent_usd() >= cap) {
            Spent::Money
        } else if self.deadline.is_some_and(|d| Instant::now() >= d) {
            Spent::Time
        } else {
            return;
        };
        *reached = Some(what);
        drop(reached);
        if let Ok(members) = self.members.lock() {
            for cancel in members.iter() {
                cancel.cancel();
            }
        }
    }
}

/// The runaway guard for a run working to a plan. Not a budget: the run stops
/// long before it when it stops making progress.
pub const PLAN_CEILING: usize = 100;

impl Default for RunLimits {
    fn default() -> Self {
        Self { max_iterations: 12, deadline: None, max_depth: 1, plan_ceiling: None, allowance: None }
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
        Self { max_iterations, plan_ceiling: Some(PLAN_CEILING), ..Self::default() }
    }

    /// `REG-5`: `/steps n` for one turn. Same clamp as the setting, so a chip
    /// can never ask for more than a runaway guard allows.
    pub fn with_max_steps(mut self, steps: Option<usize>) -> Self {
        if let Some(n) = steps {
            self.max_iterations = n.clamp(1, 50);
            // Asked for by hand, for this turn: it holds, plan or not.
            self.plan_ceiling = None;
        }
        self
    }

    /// A delegated child: less room than its parent, and always a clock,
    /// because nobody is watching it directly.
    pub fn subagent() -> Self {
        Self {
            max_iterations: 8,
            deadline: Some(Instant::now() + std::time::Duration::from_secs(300)),
            max_depth: 0,
            plan_ceiling: None,
            allowance: None,
        }
    }

    /// `TEAM-12`: the task this run belongs to has spent what it was given.
    pub fn spent(&self) -> Option<Spent> {
        self.allowance.as_ref().and_then(|a| a.spent_out())
    }

    pub fn out_of_time(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }
}

/// `PAR-4`: does a turn on this provenance run on the engine on this PC? Anything that is
/// neither a cloud provider nor the person's own server does (`resolve_turn`).
pub fn on_this_pc(provenance: &str) -> bool {
    !matches!(provenance, "cloud" | "endpoint")
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
    /// `PAR-5`: the chat this work is for. A run's own conversation, or for a helper the chat
    /// of the run that started it.
    pub chat: String,
    /// `PAR-4`: it runs on the engine on this PC, so that engine may not be swapped under it.
    pub local: bool,
    /// `SUB-10`: nobody waits for it, so the chat's Stop leaves it working.
    detached: AtomicBool,
    /// `PAR-5`: the working folder it holds against other chats, from its first change until
    /// it ends.
    folder_claim: Mutex<Option<ClaimGuard<'static>>>,
    pub started_at: Instant,
    steps: AtomicUsize,
    /// `OBS-1`: what this run has cost so far, summed over its turns. Atomics
    /// rather than a lock because a meter reads them while the loop writes.
    prompt_tokens: AtomicU64,
    output_tokens: AtomicU64,
    /// False until some turn actually reported usage. Without it a local run
    /// against a server that says nothing would read as a free one.
    usage_known: std::sync::atomic::AtomicBool,
    /// `AGC-3`: questions this run is waiting on, by question id. A sender is
    /// removed the moment it is answered, so a second answer to the same
    /// question finds nothing and reports it.
    questions: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Answer>>>,
    /// `USE-4`: the tool groups this run left off its menu and the ones that came
    /// back. Here, not on the loop's own state, because a click on the run bar
    /// arrives from outside the loop.
    pub held: Mutex<super::decide::tools::HeldSet>,
    /// `USE-13`: what the handover pre-match said about this turn, for the lead's
    /// own `delegate` call to be checked against.
    pub prematch: Mutex<Option<super::decide::handover::Prematch>>,
    /// `SEE-2`: what the quick decisions understood before the first word, which the
    /// strip above the run's text shows. Whole, so each change can be sent whole.
    pub thought: Mutex<super::decide::thought::Thought>,
    /// `SEE-1`: where the answer's text goes as it grows, so each sentence is checked once
    /// the stream has moved past it. `None` when this run's answer is not checked.
    pub check: Mutex<Option<super::autocheck::Stream>>,
    /// `HAB-1`: the lessons that spoke up at a tool call in this run, once each, for
    /// the record of what reached the answer.
    pub fired: Mutex<Vec<String>>,
    /// `WRK-7`: rules the person said "not this time" to, waiting for the loop to
    /// pick them up at the top of its next turn. Here because the click arrives from
    /// outside the loop.
    pub skips: Mutex<Vec<String>>,
    /// `WRK-13`: a preset (its slug) the person asked for in the middle of the run,
    /// waiting for the loop to apply it when no item is in hand.
    pub style_switch: Mutex<Option<String>>,
}

impl RunHandle {
    /// Register a question and get the receiver the run waits on.
    pub fn ask(&self, id: &str) -> tokio::sync::oneshot::Receiver<Answer> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Ok(mut questions) = self.questions.lock() {
            questions.insert(id.to_string(), tx);
        }
        rx
    }

    /// Deliver an answer. `false` when nothing is waiting on `id` any more.
    pub fn answer(&self, id: &str, answer: Answer) -> bool {
        let sender = self.questions.lock().ok().and_then(|mut q| q.remove(id));
        sender.is_some_and(|tx| tx.send(answer).is_ok())
    }

    /// Forget a question nobody will answer (the run was stopped).
    pub fn drop_question(&self, id: &str) {
        if let Ok(mut questions) = self.questions.lock() {
            questions.remove(id);
        }
    }

    /// `SUB-10`: this run works on after the turn that started it stops waiting for it.
    pub fn detach(&self) {
        self.detached.store(true, Ordering::Relaxed);
    }

    /// `PAR-5`: keep a claim on a folder for as long as the run lives.
    pub fn hold(&self, claim: ClaimGuard<'static>) {
        if let Ok(mut held) = self.folder_claim.lock() {
            *held = Some(claim);
        }
    }

    /// `PAR-5`: let other chats at the folder. The next change claims it again.
    pub fn release_folder(&self) {
        let held = self.folder_claim.lock().ok().and_then(|mut h| h.take());
        drop(held);
    }

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
    /// `PAR-3`: chats with a turn in hand, and whether it came from outside (a phone or a chat).
    chats: Arc<Mutex<HashMap<String, bool>>>,
}

/// `PAR-3`: held while a chat's turn runs; dropping it lets the chat take the next one.
pub struct ChatGuard {
    chats: Arc<Mutex<HashMap<String, bool>>>,
    conversation: String,
}

impl Drop for ChatGuard {
    fn drop(&mut self) {
        if let Ok(mut chats) = self.chats.lock() {
            chats.remove(&self.conversation);
        }
    }
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
        local: bool,
    ) -> Arc<RunHandle> {
        let chat = parent
            .as_deref()
            .and_then(|p| self.get(p))
            .map(|p| p.chat.clone())
            .unwrap_or_else(|| conversation_id.to_string());
        let handle = Arc::new(RunHandle {
            id: format!("run_{}", uuid::Uuid::new_v4().simple()),
            cancel,
            inbox: Mutex::new(Vec::new()),
            parent,
            depth,
            conversation_id: conversation_id.to_string(),
            chat,
            local,
            detached: AtomicBool::new(false),
            folder_claim: Mutex::new(None),
            started_at: Instant::now(),
            steps: AtomicUsize::new(0),
            prompt_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            usage_known: std::sync::atomic::AtomicBool::new(false),
            questions: Mutex::new(HashMap::new()),
            held: Mutex::new(Default::default()),
            thought: Mutex::new(Default::default()),
            check: Mutex::new(None),
            fired: Mutex::new(Vec::new()),
            skips: Mutex::new(Vec::new()),
            style_switch: Mutex::new(None),
            prematch: Mutex::new(None),
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
        let closed = self.runs.lock().ok().and_then(|mut runs| runs.remove(id));
        // Someone may still hold the handle, so the folder is let go here and not on drop.
        if let Some(run) = closed {
            run.release_folder();
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

    /// The live top-level runs of one chat.
    pub fn roots_of(&self, conversation_id: &str) -> Vec<Arc<RunHandle>> {
        self.roots().into_iter().filter(|r| r.conversation_id == conversation_id).collect()
    }

    /// `PAR-4`: a live run on the engine on this PC, if there is one.
    pub fn local_run(&self) -> Option<Arc<RunHandle>> {
        self.runs.lock().ok()?.values().find(|r| r.local).cloned()
    }

    /// `PAR-3`: take the chat for one turn. `None` when it already has one. `remote` says the
    /// turn came from a phone or a chat, so switching remote off can stop exactly those.
    pub fn enter_chat(&self, conversation_id: &str, remote: bool) -> Option<ChatGuard> {
        let mut chats = self.chats.lock().ok()?;
        if chats.contains_key(conversation_id) {
            return None;
        }
        chats.insert(conversation_id.to_string(), remote);
        Some(ChatGuard { chats: self.chats.clone(), conversation: conversation_id.to_string() })
    }

    pub fn chat_busy(&self, conversation_id: &str) -> bool {
        self.chats.lock().is_ok_and(|c| c.contains_key(conversation_id))
    }

    /// The chat has a turn in hand and it came from outside.
    pub fn chat_started_remotely(&self, conversation_id: &str) -> bool {
        self.chats.lock().is_ok_and(|c| c.get(conversation_id) == Some(&true))
    }

    /// `PAR-1`: the Stop of one chat. Its run and the helpers it waits for; helpers working in
    /// the background (`SUB-10`) carry on, and so does every other chat.
    pub fn stop_chat(&self, conversation_id: &str) {
        for root in self.roots_of(conversation_id) {
            self.cancel_where(&root.id, |run| !run.detached.load(Ordering::Relaxed));
        }
    }

    /// Stop a run and everything it started. Stopping a lead without its
    /// children would leave orphans burning tokens with no one to report to.
    pub fn cancel_tree(&self, id: &str) {
        self.cancel_where(id, |_| true);
    }

    /// Walk down from `id`, stopping each run. A child `takes` refuses is left alone with
    /// everything below it.
    fn cancel_where(&self, id: &str, takes: impl Fn(&RunHandle) -> bool) {
        let mut stack = vec![id.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if let Some(handle) = self.get(&current) {
                handle.cancel.cancel();
            }
            stack.extend(self.children_of(&current).into_iter().filter(|c| takes(c)).map(|c| c.id.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet_with_tree() -> (Fleet, Arc<RunHandle>, Arc<RunHandle>, Arc<RunHandle>) {
        let fleet = Fleet::new();
        let root = fleet.open("conv_a", CancelFlag::new(), None, 0, false);
        let child = fleet.open("conv_b", CancelFlag::new(), Some(root.id.clone()), 1, false);
        let other = fleet.open("conv_c", CancelFlag::new(), None, 0, false);
        (fleet, root, child, other)
    }

    /// `AGC-T2`: an answer resolves the call that is waiting, once; a second
    /// answer, or one for a run that moved on, finds nothing.
    #[tokio::test]
    async fn an_answer_reaches_the_waiting_question_once() {
        let fleet = Fleet::new();
        let run = fleet.open("conv", CancelFlag::new(), None, 0, false);
        let rx = run.ask("q1");
        assert!(run.answer("q1", Answer { choices: vec!["A".into(), "C".into()], text: None }));
        assert!(!run.answer("q1", Answer::default()), "a question is answered once");
        assert!(!run.answer("never-asked", Answer::default()));
        let got = rx.await.unwrap();
        assert_eq!(got.to_result(), "The user chose: A, C");
        assert_eq!(got.to_note(), "\u{2014} you chose A, C");
    }

    #[test]
    fn what_the_model_reads_says_what_the_user_chose_or_wrote() {
        let a = |choices: &[&str], text: Option<&str>| Answer {
            choices: choices.iter().map(|c| c.to_string()).collect(),
            text: text.map(str::to_string),
        };
        assert_eq!(a(&["A"], None).to_result(), "The user chose: A");
        assert_eq!(a(&["A"], Some("but small")).to_result(), "The user chose: A, and added: but small");
        assert_eq!(a(&[], Some("something else")).to_result(), "The user wrote: something else");
        assert!(a(&[], Some("  ")).to_result().contains("no answer"));
    }

    #[test]
    fn steers_drain_in_order_and_only_once() {
        let fleet = Fleet::new();
        let run = fleet.open("conv", CancelFlag::new(), None, 0, false);
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

    /// `PAR-1`: Stop takes one chat's run and the helpers it waits for. A helper in the
    /// background and every other chat carry on.
    #[test]
    fn stopping_a_chat_leaves_other_chats_and_background_helpers_working() {
        let fleet = Fleet::new();
        let a = fleet.open("conv_a", CancelFlag::new(), None, 0, false);
        let waited = fleet.open("a_helper", CancelFlag::new(), Some(a.id.clone()), 1, false);
        let below = fleet.open("a_helper_helper", CancelFlag::new(), Some(waited.id.clone()), 2, false);
        let background = fleet.open("a_background", CancelFlag::new(), Some(a.id.clone()), 1, false);
        background.detach();
        let b = fleet.open("conv_b", CancelFlag::new(), None, 0, false);
        let b_child = fleet.open("b_helper", CancelFlag::new(), Some(b.id.clone()), 1, false);

        fleet.stop_chat("conv_a");
        for run in [&a, &waited, &below] {
            assert!(run.cancel.is_cancelled(), "{}", run.conversation_id);
        }
        for run in [&background, &b, &b_child] {
            assert!(!run.cancel.is_cancelled(), "{}", run.conversation_id);
        }
        // Its own card still stops it.
        fleet.cancel_tree(&background.id);
        assert!(background.cancel.is_cancelled());
    }

    /// `PAR-5`: a helper works for the chat of whoever started it, however deep.
    #[test]
    fn a_helper_belongs_to_the_chat_that_started_it() {
        let fleet = Fleet::new();
        let top = fleet.open("conv_a", CancelFlag::new(), None, 0, true);
        let mid = fleet.open("helper", CancelFlag::new(), Some(top.id.clone()), 1, false);
        let low = fleet.open("helper_two", CancelFlag::new(), Some(mid.id.clone()), 2, false);
        assert_eq!((top.chat.as_str(), mid.chat.as_str(), low.chat.as_str()), ("conv_a", "conv_a", "conv_a"));
        assert_eq!(fleet.local_run().map(|r| r.id.clone()), Some(top.id.clone()));
        fleet.close(&top.id);
        assert!(fleet.local_run().is_none());
    }

    /// `PAR-3`: one turn per chat, whoever sends it.
    #[test]
    fn a_chat_takes_one_turn_at_a_time() {
        let fleet = Fleet::new();
        let guard = fleet.enter_chat("c1", false).expect("free");
        assert!(fleet.enter_chat("c1", true).is_none(), "a second turn in the same chat is refused");
        assert!(fleet.enter_chat("c2", true).is_some(), "another chat is not held up");
        assert!(fleet.chat_busy("c1"));
        assert!(!fleet.chat_started_remotely("c1"), "typed in the window");
        drop(guard);
        assert!(!fleet.chat_busy("c1"));
        let _remote = fleet.enter_chat("c1", true).expect("free again once the turn ends");
        assert!(fleet.chat_started_remotely("c1"));
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
        for reason in [StopReason::Aborted, StopReason::Timeout, StopReason::MaxSteps, StopReason::Error, StopReason::Budget, StopReason::PlanLimit] {
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

    /// `TEAM-12`: when what the task has cost, summed over the coordinator and every member,
    /// reaches the cap, every member is stopped and the coordinator is not: it is told.
    #[test]
    fn the_cap_stops_every_member_of_the_task_and_not_the_coordinator() {
        let fleet = Fleet::new();
        let allowance = Arc::new(Allowance::new(1.0, 90));
        let coordinator = fleet.open("session", CancelFlag::new(), None, 0, false);
        let api = fleet.open("api-chat", CancelFlag::new(), Some(coordinator.id.clone()), 1, false);
        let ui = fleet.open("ui-chat", CancelFlag::new(), Some(coordinator.id.clone()), 1, false);
        let stranger = fleet.open("other", CancelFlag::new(), None, 0, false);
        allowance.enroll(&api.cancel);
        allowance.enroll(&ui.cancel);
        let limits = RunLimits { allowance: Some(allowance.clone()), ..RunLimits::default() };

        allowance.charge(&coordinator.id, 0.40);
        allowance.charge(&api.id, 0.30);
        assert!(limits.spent().is_none());
        // A run reports the whole of what it has cost, not what is new: no double counting.
        allowance.charge(&api.id, 0.35);
        assert!((allowance.spent_usd() - 0.75).abs() < 1e-9);
        assert!(!api.cancel.is_cancelled() && !ui.cancel.is_cancelled());

        allowance.charge(&ui.id, 0.25);
        assert_eq!(limits.spent(), Some(Spent::Money));
        assert!(api.cancel.is_cancelled() && ui.cancel.is_cancelled(), "every member stops");
        assert!(!coordinator.cancel.is_cancelled(), "the coordinator is told, not cut off");
        assert!(!stranger.cancel.is_cancelled());

        // A member that was still queued stops the moment it is enrolled.
        let late = fleet.open("docs-chat", CancelFlag::new(), Some(coordinator.id.clone()), 1, false);
        allowance.enroll(&late.cancel);
        assert!(late.cancel.is_cancelled());
        assert_eq!(StopReason::Budget.as_str(), "budget");
    }

    /// `TEAM-12`: the minutes of the task stop the members the same way.
    #[test]
    fn the_minutes_of_the_task_stop_every_member_too() {
        let open = Allowance::new(0.0, 90);
        let member = CancelFlag::new();
        open.enroll(&member);
        assert!(open.spent_out().is_none() && !member.is_cancelled());

        let over = Allowance { deadline: Some(Instant::now() - std::time::Duration::from_secs(1)), ..Allowance::new(0.0, 90) };
        let other = CancelFlag::new();
        over.members.lock().unwrap().push(other.clone());
        assert_eq!(over.spent_out(), Some(Spent::Time));
        assert!(other.is_cancelled());
    }

    #[test]
    fn no_cap_means_money_alone_never_stops_a_task() {
        let allowance = Allowance::new(0.0, 90);
        allowance.charge("run", 1000.0);
        assert!(allowance.spent_out().is_none());
    }
}
