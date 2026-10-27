//! `USE-12`: a look at a tool call before it runs. A quick decision says how bad
//! the call would be if it were a mistake and whether it is what the user asked
//! for, and the answer can only add caution:
//!
//! - **Own tools** (run a command, send mail, drive the browser, change files
//!   where the person would be asked anyway). A call rated irreversible, or not
//!   asked for, gets a question of its own before it runs. A milder doubt becomes
//!   one line on the question the tool was going to ask anyway.
//! - **App calls** (`capability: "app"`). Only the in-scope question is asked, and
//!   only its line is added to the prompt the connector gate already shows, except
//!   that a *read* the request did not call for is asked about too. The gate's kind
//!   is the risk, so risk is not asked.
//! - **Agent apps** (`capability: "engine"`). The in-scope question against the
//!   task that was handed over, as one line. Never a prompt added or taken away:
//!   the agent app owns its own loop.
//! - [`raise_kind`] re-reads the name of a connector tool that sounds like a read or
//!   a change and asks whether it really sends. A send is gated as a send. It never
//!   makes a call looser.
//!
//! Every deterministic gate comes first and none is opened here. The one thing that
//! can remove a question is `relax-prompts` (see [`Relax`]): a separate switch, a
//! fitted calibration and a narrow list of tools, never an app call, an agent app
//! or anything that deletes, sends or runs.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use super::control::{self, Act, Gate};
use super::policy::Purpose;
use super::words;
use super::{Answer, Ask, Opts, Question, Verdict};
use crate::agent::connector_gate::{self, CallKind};
use crate::agent::run::AgentEventSink;
use crate::db::Db;
use crate::permissions::{Decision, PermissionManager, PermissionRequest, Trust};
use crate::runtime::proxy::{until_cancelled, CancelFlag};

pub const RISK_ID: &str = "risk";
pub const SCOPE_ID: &str = "scope";
pub const KIND_ID: &str = "kind";

/// The rating's levels, lowest first. The question's instruction says what each
/// means; the model reads the names.
const LEVELS: [&str; 4] = ["harmless", "small", "serious", "irreversible"];
const IRREVERSIBLE: usize = 3;

/// A question of its own is added at these. The first is the policy row's own bar
/// (read through `Purpose::ToolRisk`), so only the in-scope one lives here.
const ADD_OFF: f32 = 0.8;
/// A line is added to a question that was coming anyway at these.
const LINE_RISK: f32 = 0.5;
const LINE_OFF: f32 = 0.6;
/// `relax-prompts` needs both to be this sure: low risk and in scope.
const RELAX_AT: f32 = 0.95;

/// How much of what the user asked a decision reads.
const REQUEST_CHARS: usize = 1_500;
/// How much of a call's arguments a decision reads, and of one value.
const ARGS_CHARS: usize = 1_200;
const VALUE_CHARS: usize = 300;
/// A long text (a file's new content, a mail body, code) is cut shorter: its first
/// lines say what it is.
const TEXT_CHARS: usize = 200;
const TEXT_KEYS: [&str; 7] = ["content", "body", "new_string", "old_string", "code", "text", "diff"];
const KEYS_SHOWN: usize = 8;

/// Own tools that are asked about every time they are called: they run code, send
/// mail or drive a page, so whether it was asked for is always worth a look.
const ALWAYS: [&str; 9] = [
    "run_command",
    "run_task",
    "run_code",
    "send_mail",
    "reply_mail",
    "browser_click",
    "browser_type",
    "browser_press",
    "open_app",
];
/// Own tools that change files. Asked about only where the person would be asked
/// anyway (see [`is_asked`]): an edit in a folder the person trusts is the point.
const FILES: [&str; 4] = ["write_file", "edit_file", "delete_file", "move_file"];
/// The only tools `relax-prompts` may stop a question for. A change to one file in
/// the attached folder; deleting, moving, sending and running are never here.
const RELAXABLE: [&str; 2] = ["write_file", "edit_file"];

fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('\u{2026}');
    out
}

// ---- which calls are asked about ----

/// Whether a call to one of Poiesis's own tools is looked at. `trust` is the
/// folder's, read only for a tool that changes files. A call a person is never asked
/// about and that touches nothing outside (a read, a note to memory) is not here at
/// all.
pub fn is_asked(tool: &str, trust: impl FnOnce() -> Trust, headless: bool) -> bool {
    if ALWAYS.contains(&tool) {
        return true;
    }
    if !FILES.contains(&tool) || headless {
        return false;
    }
    match tool {
        // Taking something away asks at every trust level but read only, where it
        // is refused outright.
        "delete_file" | "move_file" => trust() != Trust::ReadOnly,
        _ => trust() == Trust::Confirm,
    }
}

pub fn relaxable(tool: &str) -> bool {
    RELAXABLE.contains(&tool)
}

// ---- the questions ----

pub fn risk_question() -> Question {
    Question::Score {
        instructions: "How bad would it be if this action were a mistake? harmless: it only looks at something, or changes nothing that matters. small: a change on this computer that is easy to undo, such as a new file or an edit to a tracked one. serious: a change that is hard to undo on this computer, or something outside it that can be taken back with effort, such as a booking, a sign-up or a push of a branch. irreversible: it cannot be taken back, such as deleting data that has no copy, overwriting the only copy, sending a message to other people, posting publicly, paying or spending money.".into(),
        levels: LEVELS.iter().map(|l| (*l).to_string()).collect(),
    }
}

pub fn scope_question() -> Question {
    Question::Noul {
        instructions: "Is this action something the user asked for, or a step the request clearly needs?".into(),
        when_true: "The action is what the user asked for, or an obvious step towards it.".into(),
        when_false: "The action is something the user did not ask for, or has nothing to do with the request.".into(),
    }
}

pub fn kind_question() -> Question {
    Question::Choice {
        instructions: "What does this tool do when it runs?".into(),
        options: vec![
            ("read".into(), "The tool only looks things up and changes nothing anywhere.".into()),
            (
                "change".into(),
                "The tool creates, edits or deletes something in the person's own account or data, and nobody else receives anything.".into(),
            ),
            (
                "send".into(),
                "The tool sends something to other people, posts it where others can see it, pays, or otherwise acts on the outside world: a message, an email, a post, an invitation, a payment or a share.".into(),
            ),
            ("unsure".into(), "The name and the description do not say which.".into()),
        ],
    }
}

/// The values of a call's arguments as a decision reads them, one line a key.
fn describe(args: &Value) -> String {
    let Some(obj) = args.as_object() else {
        return format!("Arguments: {}", clip(&args.to_string(), ARGS_CHARS));
    };
    let mut out = String::new();
    for (key, value) in obj.iter().take(KEYS_SHOWN) {
        let text = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let limit = if TEXT_KEYS.contains(&key.as_str()) { TEXT_CHARS } else { VALUE_CHARS };
        out.push_str(&format!("{key}: {}\n", clip(&text, limit)));
    }
    if obj.len() > KEYS_SHOWN {
        out.push_str(&format!("(and {} more)\n", obj.len() - KEYS_SHOWN));
    }
    clip(&out, ARGS_CHARS)
}

fn request_block(request: &str) -> String {
    format!("The user asked an AI assistant:\n{}", clip(request, REQUEST_CHARS))
}

/// The state for one of Poiesis's own tools.
pub fn own_state(request: &str, tool: &str, args: &Value) -> String {
    format!(
        "{}\n\nThe assistant now wants to run one of its tools.\nTool: {tool}\n{}",
        request_block(request),
        describe(args)
    )
}

/// The state for an app call or an agent app's request, which `what` describes in
/// one sentence.
pub fn scope_state(request: &str, what: &str) -> String {
    format!("{}\n\nThe assistant now wants to {what}", request_block(request))
}

/// What an app call does, for [`scope_state`]: the app, what the tool does in words
/// and the arguments it would run with.
pub fn app_what(app: &str, action: &str, args: &Value) -> String {
    format!("use the connected app {app}: {action}.\n{}", describe(args))
}

/// What an agent app asks to do, for [`scope_state`]. `tool` is its own word for it
/// (`Bash`, `Edit`) and `detail` the command or the file.
pub fn engine_what(engine: &str, tool: &str, detail: &str) -> String {
    format!("let the agent app {engine} do this on its own: {tool}.\n{}", clip(detail, ARGS_CHARS))
}

/// The state for the kind of a connector tool: what it is called and what its maker
/// says it does. Not the arguments: the gate holds a tool to one kind, so one answer
/// has to do for every call of it.
pub fn kind_state(app: &str, tool: &str, description: &str) -> String {
    let said = if description.trim().is_empty() { "(nothing)".to_string() } else { clip(description, 500) };
    format!(
        "An AI assistant may call a tool of a connected app.\nApp: {app}\nTool name: {} ({tool})\nWhat its maker says it does: {said}",
        connector_gate::humanise(tool)
    )
}

// ---- what an answer says ----

/// What the model made of one call. A part is `None` when that question was not
/// asked or not answered.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Reading {
    /// The probability of "irreversible".
    pub irreversible: Option<f32>,
    /// The probability of "harmless" or "small".
    pub low: Option<f32>,
    /// The probability that the call is what the user asked for.
    pub in_scope: Option<f32>,
    pub calibrated: bool,
}

impl Reading {
    /// The probability that it is not what the user asked for.
    pub fn off(&self) -> Option<f32> {
        self.in_scope.map(|p| 1.0 - p)
    }
}

pub fn read(v: &Verdict) -> Reading {
    let (irreversible, low) = match v.get(RISK_ID) {
        Some(Answer::Score { probs, .. }) => (probs.get(IRREVERSIBLE).copied(), Some(probs.iter().take(2).sum())),
        _ => (None, None),
    };
    let in_scope = match v.get(SCOPE_ID) {
        Some(Answer::Noul { p_true }) => Some(*p_true),
        _ => None,
    };
    Reading { irreversible, low, in_scope, calibrated: v.calibrated }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Risk,
    Scope,
}

/// A doubt worth saying. `add` is whether it is sure enough to ask a question of its
/// own; otherwise it is a line on a question that was coming anyway.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flag {
    pub why: Why,
    pub p: f32,
    pub add: bool,
    /// Whether `p` was fitted, which decides whether the words are sure (`SEE-4`).
    pub calibrated: bool,
}

impl Flag {
    /// What was noticed, sure or hedged by how sure the decision was (`SEE-4`).
    fn what(&self) -> &'static str {
        match self.why {
            Why::Risk => words::say(self.p, self.calibrated, "this looks irreversible", "this may be irreversible"),
            Why::Scope => {
                words::say(self.p, self.calibrated, "this is not what you asked for", "this may not be what you asked for")
            }
        }
    }

    /// What was noticed, in words and without a lead-in. The number is [`Flag::hover`].
    pub fn reason(&self) -> String {
        self.what().to_string()
    }

    /// The hover on the line: the same words and the number.
    pub fn hover(&self) -> String {
        format!("Quick decision, {}: {:.2}", self.what(), self.p)
    }

    /// The line on a question this flag added.
    pub fn asked(&self) -> String {
        format!("Asked because {}", self.reason())
    }

    /// The line on a question that was coming anyway.
    pub fn line(&self) -> String {
        format!("Flagged: {}", self.reason())
    }

    pub fn text(&self) -> String {
        if self.add {
            self.asked()
        } else {
            self.line()
        }
    }
}

/// What a reading comes to for one of Poiesis's own tools. The request comes
/// first: a call nobody asked for matters more than how final it is.
pub fn flag_own(r: &Reading) -> Option<Flag> {
    let bar = Purpose::ToolRisk.policy().act_at.at(r.calibrated);
    let calibrated = r.calibrated;
    let scope = r.off().map(|p| Flag { why: Why::Scope, p, add: p >= ADD_OFF, calibrated });
    let risk = r.irreversible.map(|p| Flag { why: Why::Risk, p, add: p >= bar, calibrated });
    let worth_a_line = |f: &Flag| match f.why {
        Why::Scope => f.p >= LINE_OFF,
        Why::Risk => f.p >= LINE_RISK,
    };
    scope
        .filter(|f| f.add)
        .or(risk.filter(|f| f.add))
        .or(scope.filter(worth_a_line))
        .or(risk.filter(worth_a_line))
}

/// What the in-scope question alone comes to. `can_add` is false for an agent app,
/// which only ever gets the line.
pub fn flag_scope(off: f32, can_add: bool, calibrated: bool) -> Option<Flag> {
    (off >= LINE_OFF).then_some(Flag { why: Why::Scope, p: off, add: can_add && off >= ADD_OFF, calibrated })
}

/// `relax-prompts`: how sure the decision was that the call is a small change that
/// was asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Relax {
    pub low: f32,
    pub in_scope: f32,
}

/// Only a calibrated reading may relax anything: an uncalibrated number is the
/// model's own and no bar of 0.95 means what it says.
pub fn relax_of(r: &Reading) -> Option<Relax> {
    let (low, in_scope) = (r.low?, r.in_scope?);
    (r.calibrated && low >= RELAX_AT && in_scope >= RELAX_AT).then_some(Relax { low, in_scope })
}

// ---- what a tool's own question picks up ----

/// What a tool call's own prompt is told: set around the tool's execution, read by
/// the sink when the tool raises a prompt (`send_permission`) and by the one place
/// `relax-prompts` may skip one.
#[derive(Debug, Clone)]
pub struct Note {
    pub tool: String,
    /// The line to put on a question the tool raises.
    pub line: Option<String>,
    /// The hover on that line: the same words and the number.
    pub hover: Option<String>,
    pub relax: Option<Relax>,
    /// Set by [`skip_prompt`] when it skipped a question, for the step's line.
    skipped: Arc<Mutex<Option<String>>>,
}

impl Note {
    pub fn new(tool: &str, line: Option<String>, relax: Option<Relax>) -> Self {
        Self { tool: tool.to_string(), line, hover: None, relax, skipped: Arc::default() }
    }

    pub fn with_hover(mut self, hover: Option<String>) -> Self {
        self.hover = hover;
        self
    }
}

impl PartialEq for Note {
    fn eq(&self, other: &Self) -> bool {
        (&self.tool, &self.line, &self.hover, &self.relax) == (&other.tool, &other.line, &other.hover, &other.relax)
    }
}

tokio::task_local! {
    static NOTE: Note;
}

/// Run `fut` with `note` in force for every prompt it raises. The second half of the
/// answer is the words for the timeline if a question was skipped on the way.
pub async fn with_note<F: Future>(note: Note, fut: F) -> (F::Output, Option<String>) {
    let skipped = note.skipped.clone();
    let out = NOTE.scope(note, fut).await;
    let said = skipped.lock().unwrap().take();
    (out, said)
}

/// The line a prompt a tool raised carries, if a flag is in force for the call. Only
/// the kinds a tool raises about itself get one: an app call, an agent app's
/// request, a handover and a page card each carry their own.
pub fn line_for(request: &PermissionRequest) -> Option<String> {
    match request.capability.as_deref() {
        None | Some("domain") | Some("screen") | Some("open-app") | Some("task") | Some("command") => {
            NOTE.try_with(|n| n.line.clone()).ok().flatten()
        }
        _ => None,
    }
}

/// The hover on [`line_for`]'s line, under the same rules.
pub fn hover_for(request: &PermissionRequest) -> Option<String> {
    match request.capability.as_deref() {
        None | Some("domain") | Some("screen") | Some("open-app") | Some("task") | Some("command") => {
            NOTE.try_with(|n| n.hover.clone()).ok().flatten()
        }
        _ => None,
    }
}

/// How an Activity row for a skipped question starts. The experiment's report counts
/// by it (`EXP-4`).
pub const SKIPPED: &str = "Skipped a prompt";

/// `relax-prompts`: may the question about to be asked be skipped? `Some` is the
/// words for the timeline. The caller has already checked that the change is inside
/// the attached folder and is not a deletion.
pub fn skip_prompt(tool: &str) -> Option<String> {
    NOTE.try_with(|n| {
        let r = n.relax?;
        if n.tool != tool || !relaxable(tool) {
            return None;
        }
        let text = format!("{SKIPPED} \u{b7} {tool} \u{b7} low risk ({:.2})", r.low.min(r.in_scope));
        *n.skipped.lock().unwrap() = Some(text.clone());
        Some(text)
    })
    .ok()
    .flatten()
}

// ---- what this turn already knows ----

/// What a look at one call came to, kept until the person's next turn. The
/// controller refuses to ask the same question about the same state twice in a
/// turn, so without this a retried call would go through unlooked-at, even one
/// the person had just said no to.
#[derive(Clone)]
enum Known {
    Answered(Verdict),
    /// The person said no to this call. The words are for the model.
    Declined(String),
}

/// Conversation, then state hash.
type TurnMemory = HashMap<String, HashMap<String, Known>>;

static TURN: OnceLock<Mutex<TurnMemory>> = OnceLock::new();
/// Conversations remembered at once, as the controller keeps its turns.
const TURNS_KEPT: usize = 64;

fn turn_memory() -> &'static Mutex<TurnMemory> {
    TURN.get_or_init(Default::default)
}

fn known(conversation_id: &str, state: &str) -> Option<Known> {
    let memory = turn_memory().lock().unwrap();
    memory.get(conversation_id)?.get(&super::state_hash(state)).cloned()
}

fn remember(conversation_id: &str, state: &str, what: Known) {
    let mut memory = turn_memory().lock().unwrap();
    if memory.len() >= TURNS_KEPT && !memory.contains_key(conversation_id) {
        memory.clear();
    }
    memory.entry(conversation_id.to_string()).or_default().insert(super::state_hash(state), what);
}

/// A new user turn: what was answered before is asked again.
pub fn forget_turn(conversation_id: &str) {
    turn_memory().lock().unwrap().remove(conversation_id);
}

// ---- asking ----

/// Everything a look at a call needs from the run.
pub struct Site<'a> {
    pub db: &'a Db,
    pub perms: &'a PermissionManager,
    pub sink: &'a AgentEventSink,
    pub cancel: &'a CancelFlag,
    pub conversation_id: &'a str,
    pub run_id: Option<&'a str>,
    /// Nobody is watching: a question cannot be asked, so it becomes a refusal.
    pub headless: bool,
    /// What the user asked this turn (for a child, the task it was given).
    pub request: &'a str,
}

/// What to do about one of Poiesis's own tools.
#[derive(Debug, PartialEq)]
pub enum Before {
    /// Run it. With a note when something is worth saying to a question the tool
    /// raises, or when `relax-prompts` may skip one.
    Go(Option<Note>),
    /// Do not run it. The words are for the model.
    Refuse(String),
}

fn opts<'a>(site: &Site<'a>) -> Opts<'a> {
    Opts { conversation_id: Some(site.conversation_id), run_id: site.run_id }
}

/// Ask about one of Poiesis's own tools. `Go(None)` for every reason it is not
/// asked: the experiment is off, the engine is cold or on the processor, the turn
/// has used its decisions, or there is no request to compare the call with.
pub async fn before_own(site: &Site<'_>, tool: &str, args: &Value) -> Before {
    if !crate::experiments::is_on(site.db, "fast-slow") || site.request.trim().is_empty() {
        return Before::Go(None);
    }
    let state = own_state(site.request, tool, args);
    match known(site.conversation_id, &state) {
        Some(Known::Declined(text)) => return Before::Refuse(text),
        Some(Known::Answered(verdict)) => return act_on(site, tool, args, &verdict).await,
        None => {}
    }
    let ask = Ask {
        state: &state,
        images: Vec::new(),
        questions: vec![(RISK_ID, risk_question()), (SCOPE_ID, scope_question())],
    };
    let Ok(verdict) = super::ask_gate(&ask, Purpose::ToolRisk, opts(site), site.headless).await else {
        return Before::Go(None);
    };
    remember(site.conversation_id, &state, Known::Answered(verdict.clone()));
    act_on(site, tool, args, &verdict).await
}

/// What the answer comes to. Split from the ask so it can be tried on any verdict.
async fn act_on(site: &Site<'_>, tool: &str, args: &Value, verdict: &Verdict) -> Before {
    let reading = read(verdict);
    let flag = flag_own(&reading);
    let act = if flag.is_some_and(|f| f.add) { Act::Go } else { Act::Below };
    super::settle(verdict, Purpose::ToolRisk, act, opts(site));
    if let Some(f) = flag {
        // `SEE-2`: the receipt's words for it.
        let lead = if f.add { "Asked before" } else { "Flagged" };
        super::note(verdict, &format!("{lead} {}: {}", connector_gate::humanise(tool), f.reason()));
    }
    let note = |line: Option<String>, relax: Option<Relax>| {
        (line.is_some() || relax.is_some()).then(|| Note::new(tool, line, relax))
    };
    match flag {
        Some(f) if f.add => ask_person(site, tool, args, f, &verdict.id).await,
        Some(f) => Before::Go(note(Some(f.line()), None).map(|n| n.with_hover(Some(f.hover())))),
        None => {
            let relax = (crate::experiments::is_on(site.db, "relax-prompts") && relaxable(tool))
                .then(|| relax_of(&reading))
                .flatten();
            Before::Go(note(None, relax))
        }
    }
}

fn target_of(args: &Value) -> Option<String> {
    const KEYS: [&str; 8] = ["command", "task", "path", "to", "subject", "url", "app", "name"];
    let obj = args.as_object()?;
    KEYS.iter().find_map(|k| {
        let text = match obj.get(*k)? {
            Value::String(s) => s.trim().to_string(),
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
            _ => return None,
        };
        (!text.is_empty()).then(|| clip(&text, 100))
    })
}

/// One sentence for the question: what the assistant would do, with its most
/// telling argument.
fn summary_of(tool: &str, args: &Value) -> String {
    let target = target_of(args).map(|t| format!(" \u{2192} {t}")).unwrap_or_default();
    format!("I'd like to {}{target}.", connector_gate::humanise(tool))
}

/// The words the model reads when a person said no, or nobody could be asked.
fn declined(tool: &str, f: &Flag) -> String {
    format!("The person did not want me to {}: {}. So I did not.", connector_gate::humanise(tool), f.reason())
}

async fn ask_person(site: &Site<'_>, tool: &str, args: &Value, f: Flag, decision: &str) -> Before {
    if site.headless {
        return Before::Refuse(format!(
            "I did not {}: {}. Nobody is here to approve it, so it stays undone.",
            connector_gate::humanise(tool),
            f.reason()
        ));
    }
    let id = format!("perm_{}", uuid::Uuid::new_v4());
    let rx = site.perms.open_request(&id);
    site.sink.send_permission(PermissionRequest::risk_ask(
        id,
        summary_of(tool, args),
        tool.to_string(),
        connector_gate::arguments_text(args),
        f.asked(),
    ).with_reason_hover(Some(f.hover())));
    match until_cancelled(site.cancel, rx).await.map(|d| d.unwrap_or(Decision::Deny)) {
        None => Before::Refuse("The person stopped the run before answering.".to_string()),
        Some(Decision::Deny) => {
            // Both questions are in one request and the fit reads its first answer, the
            // rating. A refusal says something about the rating only when the rating
            // is what raised the question.
            if f.why == Why::Risk {
                let _ = site.db.add_outcome(decision, "right", None, "tool-card");
            }
            let text = declined(tool, &f);
            // The same call again this turn is refused without asking again.
            remember(site.conversation_id, &own_state(site.request, tool, args), Known::Declined(text.clone()));
            Before::Refuse(text)
        }
        // The person has just been told and said yes: no second line on whatever the
        // tool asks next.
        Some(_) => Before::Go(None),
    }
}

// ---- app calls and agent apps ----

/// The in-scope question on its own, answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Scoped {
    pub flag: Flag,
    pub decision: String,
}

/// Ask whether an app call or an agent app's request is what the user asked for.
/// `None` unless it is doubtful enough to say so: no experiment, no engine, no
/// request, no budget and a call that looks fine all come to the same thing.
pub async fn scope_of(
    db: &Db,
    conversation_id: &str,
    run_id: Option<&str>,
    headless: bool,
    request: &str,
    what: &str,
    can_add: bool,
) -> Option<Scoped> {
    if !crate::experiments::is_on(db, "fast-slow") || request.trim().is_empty() {
        return None;
    }
    let state = scope_state(request, what);
    let o = Opts { conversation_id: Some(conversation_id), run_id };
    // The same call again this turn gets the same line or question, not none.
    if let Some(Known::Answered(verdict)) = known(conversation_id, &state) {
        return scoped(&verdict, can_add, o);
    }
    let ask = Ask { state: &state, images: Vec::new(), questions: vec![(SCOPE_ID, scope_question())] };
    let verdict = super::ask_gate(&ask, Purpose::ToolRisk, o, headless).await.ok()?;
    remember(conversation_id, &state, Known::Answered(verdict.clone()));
    scoped(&verdict, can_add, o)
}

fn scoped(verdict: &Verdict, can_add: bool, o: Opts<'_>) -> Option<Scoped> {
    let off = read(verdict).off()?;
    let flag = flag_scope(off, can_add, verdict.calibrated);
    super::settle(verdict, Purpose::ToolRisk, if flag.is_some() { Act::Go } else { Act::Below }, o);
    if let Some(f) = &flag {
        super::note(verdict, &format!("{} an outside call: {}", if f.add { "Asked before" } else { "Flagged" }, f.reason()));
    }
    flag.map(|flag| Scoped { flag, decision: verdict.id.clone() })
}

/// A connector tool taken for a send, from what its name said it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Upgrade {
    pub p: f32,
    pub decision: String,
    /// Whether `p` was fitted (`SEE-4`).
    pub calibrated: bool,
}

impl Upgrade {
    fn what(&self) -> &'static str {
        words::say(self.p, self.calibrated, "this sends something", "this may send something")
    }

    pub fn line(&self) -> String {
        format!("Asked because {}", self.what())
    }

    pub fn hover(&self) -> String {
        format!("Quick decision, {}: {:.2}", self.what(), self.p)
    }
}

/// What a run knows about an app call, for the prompt: the two lines it may carry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppNotes {
    pub scope: Option<Scoped>,
    pub raised: Option<Upgrade>,
}

impl AppNotes {
    pub fn is_empty(&self) -> bool {
        self.scope.is_none() && self.raised.is_none()
    }

    /// Whether the call is one nobody asked for and the gate would have let through
    /// unasked. `strictest` is the call's kind.
    pub fn adds_to(&self, strictest: CallKind) -> bool {
        strictest == CallKind::Read && self.scope.as_ref().is_some_and(|s| s.flag.add)
    }

    /// The reason line on the prompt. `added` is whether a question was added.
    pub fn reason(&self, added: bool) -> Option<String> {
        let mut lines: Vec<String> = Vec::new();
        if let Some(up) = &self.raised {
            lines.push(up.line());
        }
        if let Some(s) = &self.scope {
            lines.push(Flag { add: added && s.flag.add, ..s.flag }.text());
        }
        (!lines.is_empty()).then(|| lines.join(" \u{b7} "))
    }

    /// The hover on [`AppNotes::reason`]: each part's words and number.
    pub fn hover(&self, added: bool) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(up) = &self.raised {
            parts.push(up.hover());
        }
        if let Some(s) = &self.scope {
            parts.push(Flag { add: added && s.flag.add, ..s.flag }.hover());
        }
        (!parts.is_empty()).then(|| parts.join(" \u{b7} "))
    }

    /// `CTL-3`: the person said no to a question these lines were on. A no to what was
    /// not asked for is a plain right; a no to a kind raised to a send is a weak one,
    /// because people refuse sends for other reasons.
    pub fn label_refusal(&self, db: &Db) {
        if let Some(s) = &self.scope {
            let _ = db.add_outcome(&s.decision, "right", None, "app-card");
        }
        if let Some(u) = &self.raised {
            let _ = db.add_outcome(&u.decision, "weak_right", None, "app-card");
        }
    }
}

// ---- the kind of a connector tool ----

/// One answer a tool a day. `None` is an answer too ("keep what the name says"), so
/// a busy tool is asked about once.
static KINDS: OnceLock<Mutex<HashMap<String, Option<Upgrade>>>> = OnceLock::new();
/// Tools remembered at once. A day's worth of tools is far fewer; this only bounds
/// a run that never stops.
const KINDS_KEPT: usize = 2_000;

fn kinds() -> &'static Mutex<HashMap<String, Option<Upgrade>>> {
    KINDS.get_or_init(Default::default)
}

fn kind_key(connector_id: &str, tool: &str, day: &str) -> String {
    format!("{connector_id}\u{1f}{tool}\u{1f}{day}")
}

/// `AppKind`: ask whether a connector tool really sends. For a tool whose name rule
/// said read or change and that has no override of the person's, which the caller
/// has checked. `Some` is a tool to gate as a send; `None` keeps the name's kind.
#[allow(clippy::too_many_arguments)]
pub async fn raise_kind(
    db: &Db,
    conversation_id: &str,
    run_id: Option<&str>,
    headless: bool,
    connector_id: &str,
    app: &str,
    tool: &str,
    description: &str,
    day: &str,
) -> Option<Upgrade> {
    if !crate::experiments::is_on(db, "fast-slow") {
        return None;
    }
    let key = kind_key(connector_id, tool, day);
    if let Some(known) = kinds().lock().unwrap().get(&key) {
        return known.clone();
    }
    let state = kind_state(app, tool, description);
    let ask = Ask { state: &state, images: Vec::new(), questions: vec![(KIND_ID, kind_question())] };
    let o = Opts { conversation_id: Some(conversation_id), run_id };
    let verdict = super::ask_gate(&ask, Purpose::AppKind, o, headless).await.ok()?;
    let got = upgrade_of(&verdict, o);
    if let Some(up) = &got {
        super::note(&verdict, &format!("Asked before {}: {}", connector_gate::humanise(tool), up.what()));
    }
    let mut kept = kinds().lock().unwrap();
    if kept.len() >= KINDS_KEPT {
        kept.clear();
    }
    kept.insert(key, got.clone());
    got
}

/// Read the verdict. Only `send` at the row's bar makes anything stricter.
fn upgrade_of(verdict: &Verdict, o: Opts<'_>) -> Option<Upgrade> {
    let answer = verdict.get(KIND_ID)?;
    let go = control::evaluate(Purpose::AppKind, answer, verdict.calibrated, Gate::Open) == Act::Go;
    super::settle(verdict, Purpose::AppKind, if go { Act::Go } else { Act::Below }, o);
    let Answer::Choice { probs, .. } = answer else { return None };
    let p = probs.iter().find(|(k, _)| k == "send").map_or(0.0, |(_, p)| *p);
    go.then(|| Upgrade { p, decision: verdict.id.clone(), calibrated: verdict.calibrated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::decide::BackendId;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn verdict(answers: Vec<(&str, Answer)>, calibrated: bool) -> Verdict {
        Verdict {
            id: "d1".into(),
            answers: answers.into_iter().map(|(k, a)| (k.to_string(), a)).collect::<BTreeMap<_, _>>(),
            backend: BackendId::Local { model: "m".into() },
            calibrated,
            ms: 10,
        }
    }

    fn rating(p: [f32; 4]) -> Answer {
        Answer::Score { score: 0.0, probs: p.to_vec(), confidence: 0.5 }
    }

    fn call(p_irreversible: f32, p_in_scope: f32, calibrated: bool) -> Verdict {
        let rest = (1.0 - p_irreversible) / 3.0;
        verdict(
            vec![
                (RISK_ID, rating([rest, rest, rest, p_irreversible])),
                (SCOPE_ID, Answer::Noul { p_true: p_in_scope }),
            ],
            calibrated,
        )
    }

    fn kind_verdict(send: f32) -> Verdict {
        let rest = (1.0 - send) / 3.0;
        let probs: Vec<(String, f32)> = vec![
            ("read".into(), rest),
            ("change".into(), rest),
            ("send".into(), send),
            ("unsure".into(), rest),
        ];
        let choice = if send >= rest { "send" } else { "read" }.to_string();
        verdict(vec![(KIND_ID, Answer::Choice { choice, probs, confidence: 0.5 })], false)
    }

    /// A decision row for a label to attach to.
    fn log_decision(db: &Db, id: &str) {
        db.add_decision(&crate::db::decisions::NewDecision {
            id,
            conversation_id: None,
            run_id: None,
            purpose: "tool-risk",
            backend: "local:m",
            model: "m",
            ms: 1,
            calibrated: false,
            answers: &serde_json::json!({}),
            input_tokens: 0,
            state_preview: "",
            state_hash: id,
            action: None,
        })
        .unwrap();
    }

    // ---- which calls ----

    #[test]
    fn code_mail_and_the_browser_are_always_looked_at_and_reads_never() {
        for t in ["run_command", "run_task", "run_code", "send_mail", "reply_mail", "browser_click", "browser_type", "browser_press", "open_app"] {
            assert!(is_asked(t, || Trust::Auto, false), "{t}");
            assert!(is_asked(t, || Trust::Auto, true), "{t} unattended");
        }
        for t in ["read_file", "list_directory", "web_search", "browse", "browser_read", "remember", "delegate", "generate_image"] {
            assert!(!is_asked(t, || Trust::Confirm, false), "{t}");
        }
        // The folder is read only for a tool that needs it.
        assert!(is_asked("run_command", || panic!("a command needs no folder"), false));
        assert!(!is_asked("read_file", || panic!("a read needs no folder"), false));
    }

    /// A change to a file is looked at where the person would be asked anyway, and
    /// not where they chose to trust the folder.
    #[test]
    fn file_changes_are_looked_at_only_where_a_prompt_was_coming() {
        assert!(is_asked("edit_file", || Trust::Confirm, false));
        assert!(is_asked("write_file", || Trust::Confirm, false));
        assert!(!is_asked("edit_file", || Trust::Auto, false), "a trusted folder asks nothing");
        assert!(!is_asked("write_file", || Trust::ReadOnly, false), "refused outright");
        // Taking something away asks at every trust level but read only.
        assert!(is_asked("delete_file", || Trust::Auto, false));
        assert!(is_asked("move_file", || Trust::Confirm, false));
        assert!(!is_asked("delete_file", || Trust::ReadOnly, false));
        // An unattended run is refused where it would have asked, so there is nothing to look at.
        assert!(!is_asked("edit_file", || Trust::Confirm, true));
        assert!(!is_asked("delete_file", || Trust::Auto, true));
    }

    // ---- the state ----

    #[test]
    fn the_state_has_the_request_the_tool_and_its_arguments_and_cuts_long_text() {
        let args = serde_json::json!({ "to": "sam@example.com", "subject": "Hi", "body": "x".repeat(900) });
        let state = own_state("Tell Sam I will be late", "send_mail", &args);
        assert!(state.contains("The user asked an AI assistant:\nTell Sam I will be late"));
        assert!(state.contains("Tool: send_mail"));
        assert!(state.contains("to: sam@example.com"));
        let body = state.lines().find(|l| l.starts_with("body: ")).unwrap();
        assert_eq!(body.chars().count(), "body: ".len() + TEXT_CHARS + 1, "a body is cut to its first words");
    }

    #[test]
    fn a_long_request_and_many_arguments_are_bounded() {
        let args: serde_json::Map<String, Value> = (0..20).map(|i| (format!("k{i:02}"), Value::from("v"))).collect();
        let state = own_state(&"word ".repeat(2_000), "run_command", &Value::Object(args));
        assert!(state.contains("(and 12 more)"));
        assert!(state.chars().count() < REQUEST_CHARS + ARGS_CHARS + 200);
        assert!(describe(&Value::from("not an object")).starts_with("Arguments: "));
    }

    #[test]
    fn the_kind_is_asked_from_the_name_and_the_description_and_not_the_arguments() {
        let s = kind_state("Slack", "slack_create_item", "Creates an item and notifies the channel");
        assert!(s.contains("Tool name: slack create item (slack_create_item)"));
        assert!(s.contains("notifies the channel"));
        assert!(kind_state("A", "t", "  ").contains("(nothing)"));
        let Question::Choice { options, .. } = kind_question() else { panic!("a pick-one question") };
        assert_eq!(options.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["read", "change", "send", "unsure"]);
    }

    #[test]
    fn the_risk_question_rates_four_levels_with_irreversible_last() {
        let Question::Score { levels, instructions } = risk_question() else { panic!("a rating") };
        assert_eq!(levels, ["harmless", "small", "serious", "irreversible"]);
        assert!(instructions.contains("irreversible: it cannot be taken back"));
        assert_eq!(Purpose::ToolRisk.policy().act_at.measure, super::super::policy::Measure::Level(IRREVERSIBLE));
    }

    // ---- what an answer comes to ----

    #[test]
    fn a_call_that_is_asked_for_and_ordinary_is_not_flagged() {
        assert_eq!(flag_own(&read(&call(0.05, 0.97, false))), None);
    }

    #[test]
    fn a_call_nobody_asked_for_adds_a_question_of_its_own() {
        let f = flag_own(&read(&call(0.1, 0.1, false))).unwrap();
        assert_eq!((f.why, f.add), (Why::Scope, true));
        assert!((f.p - 0.9).abs() < 1e-6);
        assert_eq!(f.asked(), "Asked because this is not what you asked for");
    }

    #[test]
    fn an_irreversible_call_adds_a_question_at_the_rows_bar_and_a_line_below_it() {
        let f = flag_own(&read(&call(0.8, 0.9, false))).unwrap();
        assert_eq!((f.why, f.add), (Why::Risk, true));
        assert_eq!(f.asked(), "Asked because this may be irreversible");
        let f = flag_own(&read(&call(0.79, 0.9, false))).unwrap();
        assert_eq!((f.why, f.add), (Why::Risk, false), "0.79 is a line and not a question");
        assert_eq!(f.line(), "Flagged: this may be irreversible");
        assert_eq!(flag_own(&read(&call(0.49, 0.9, false))), None, "under half is nothing worth saying");
    }

    /// `SEE-T-8`: the line says it in words, sure only when the number is, and the
    /// number is the hover's.
    #[test]
    fn the_reason_line_is_words_hedged_by_how_sure_and_the_number_is_the_hover() {
        let sure = flag_own(&read(&call(0.1, 0.1, false))).unwrap();
        assert_eq!(sure.line(), "Flagged: this is not what you asked for");
        assert_eq!(sure.hover(), "Quick decision, this is not what you asked for: 0.90");
        let middling = flag_own(&read(&call(0.1, 0.25, false))).unwrap();
        assert_eq!(middling.line(), "Flagged: this may not be what you asked for");
        assert_eq!(middling.hover(), "Quick decision, this may not be what you asked for: 0.75");
        // A fitted 0.86 is sure, an unfitted one is not.
        assert_eq!(flag_own(&read(&call(0.1, 0.14, true))).unwrap().line(), "Flagged: this is not what you asked for");
        assert_eq!(flag_own(&read(&call(0.1, 0.14, false))).unwrap().line(), "Flagged: this may not be what you asked for");
        for f in [sure, middling] {
            assert!(!f.line().chars().any(|c| c.is_ascii_digit()) && !f.asked().chars().any(|c| c.is_ascii_digit()));
        }
        let up = Upgrade { p: 0.95, decision: "k".into(), calibrated: false };
        assert_eq!(up.line(), "Asked because this sends something");
        assert_eq!(up.hover(), "Quick decision, this sends something: 0.95");
        assert_eq!(Upgrade { p: 0.7, ..up }.line(), "Asked because this may send something");
    }

    /// A call that is both is flagged for the request, which is the more telling doubt.
    #[test]
    fn what_was_not_asked_for_is_named_before_how_final_it_is() {
        assert_eq!(flag_own(&read(&call(0.95, 0.05, false))).unwrap().why, Why::Scope);
        // Irreversible and only doubtfully off the request: the question is for the risk.
        assert_eq!(flag_own(&read(&call(0.9, 0.3, false))).unwrap().why, Why::Risk);
        // A line, then, is the request's when both are only lines.
        let f = flag_own(&read(&call(0.6, 0.35, false))).unwrap();
        assert_eq!((f.why, f.add), (Why::Scope, false));
    }

    #[test]
    fn an_answer_with_only_one_question_reads_as_that_question() {
        let only_scope = verdict(vec![(SCOPE_ID, Answer::Noul { p_true: 0.05 })], false);
        let r = read(&only_scope);
        assert_eq!((r.irreversible, r.low), (None, None));
        assert_eq!(flag_own(&r).unwrap().why, Why::Scope);
        assert_eq!(read(&verdict(vec![], false)), Reading::default());
    }

    #[test]
    fn the_scope_alone_gives_a_line_and_an_agent_app_never_a_question() {
        assert_eq!(flag_scope(0.5, true, false), None);
        assert_eq!(flag_scope(0.7, true, false), Some(Flag { why: Why::Scope, p: 0.7, add: false, calibrated: false }));
        assert_eq!(flag_scope(0.9, true, false), Some(Flag { why: Why::Scope, p: 0.9, add: true, calibrated: false }));
        assert_eq!(flag_scope(0.99, false, false), Some(Flag { why: Why::Scope, p: 0.99, add: false, calibrated: false }), "an agent app owns its own loop");
    }

    // ---- relax-prompts ----

    #[test]
    fn a_prompt_is_relaxed_only_on_a_calibrated_and_very_sure_reading() {
        let sure = |cal| read(&verdict(vec![(RISK_ID, rating([0.9, 0.07, 0.02, 0.01])), (SCOPE_ID, Answer::Noul { p_true: 0.97 })], cal));
        assert_eq!(relax_of(&sure(false)), None, "an unfitted number is not enough");
        let r = relax_of(&sure(true)).unwrap();
        assert!((r.low - 0.97).abs() < 1e-6);
        // Low risk but only fairly in scope, and the other way round.
        let a = read(&verdict(vec![(RISK_ID, rating([0.9, 0.07, 0.02, 0.01])), (SCOPE_ID, Answer::Noul { p_true: 0.9 })], true));
        let b = read(&verdict(vec![(RISK_ID, rating([0.5, 0.2, 0.2, 0.1])), (SCOPE_ID, Answer::Noul { p_true: 0.99 })], true));
        assert_eq!((relax_of(&a), relax_of(&b)), (None, None));
        // A reading without both questions never relaxes anything.
        assert_eq!(relax_of(&read(&verdict(vec![(SCOPE_ID, Answer::Noul { p_true: 1.0 })], true))), None);
    }

    #[test]
    fn only_a_change_to_one_file_can_be_relaxed() {
        assert!(relaxable("edit_file") && relaxable("write_file"));
        for t in ["delete_file", "move_file", "send_mail", "reply_mail", "run_command", "run_task", "run_code", "browser_click", "open_app"] {
            assert!(!relaxable(t), "{t}");
        }
    }

    // ---- the note a tool's own prompt picks up ----

    fn prompt(capability: Option<&str>) -> PermissionRequest {
        let mut r = PermissionRequest::operation("p".into(), "s".into(), "x".into(), crate::permissions::Mode::ReadWrite, None);
        r.capability = capability.map(str::to_string);
        r
    }

    #[tokio::test]
    async fn a_prompt_the_tool_raises_carries_the_flags_line() {
        let note = Note::new("edit_file", Some("Flagged: not what you asked for (0.74)".into()), None);
        let (seen, skipped) = with_note(note, async {
            ["task", "command", "domain", "screen", "open-app"]
                .iter()
                .map(|c| line_for(&prompt(Some(c))))
                .chain([line_for(&prompt(None))])
                .collect::<Vec<_>>()
        })
        .await;
        assert_eq!(skipped, None);
        assert!(seen.iter().all(|l| l.as_deref() == Some("Flagged: not what you asked for (0.74)")), "{seen:?}");
    }

    /// An app call, an agent app, a handover and a page card each carry their own line
    /// or none, whatever the tool around them was flagged for.
    #[tokio::test]
    async fn another_kinds_prompt_never_borrows_the_line() {
        let note = Note::new("browser_click", Some("Flagged: x (0.9)".into()), None);
        let (seen, _) = with_note(note, async {
            ["app", "engine", "handover", "page", "risk"].iter().map(|c| line_for(&prompt(Some(c)))).collect::<Vec<_>>()
        })
        .await;
        assert!(seen.iter().all(Option::is_none), "{seen:?}");
        // Outside a call there is no note at all.
        assert_eq!(line_for(&prompt(None)), None);
    }

    #[tokio::test]
    async fn a_skip_is_offered_only_for_the_tool_the_decision_was_about() {
        let note = Note::new("edit_file", None, Some(Relax { low: 0.97, in_scope: 0.99 }));
        let ((own, other, none), said) =
            with_note(note, async { (skip_prompt("edit_file"), skip_prompt("write_file"), skip_prompt("delete_file")) }).await;
        assert_eq!(own.as_deref(), Some("Skipped a prompt \u{b7} edit_file \u{b7} low risk (0.97)"));
        assert_eq!((other, none), (None, None));
        assert_eq!(said, own, "the step's line is the same words, handed back to the run");
        assert_eq!(skip_prompt("edit_file"), None, "no note, no skip");
        // A note for a tool that can never be relaxed relaxes nothing even if one is set.
        let bad = Note::new("delete_file", None, Some(Relax { low: 1.0, in_scope: 1.0 }));
        let (got, said) = with_note(bad, async { skip_prompt("delete_file") }).await;
        assert_eq!((got, said), (None, None));
    }

    // ---- the kind of a tool ----

    #[test]
    fn a_tool_is_raised_to_a_send_only_at_the_rows_bar() {
        let o = Opts::default();
        assert_eq!(upgrade_of(&kind_verdict(0.6), o).map(|u| u.p), Some(0.6));
        assert_eq!(upgrade_of(&kind_verdict(0.59), o), None);
        // `read` or `unsure` winning changes nothing: the check never loosens a gate.
        assert_eq!(upgrade_of(&kind_verdict(0.1), o), None);
    }

    #[test]
    fn the_lines_on_an_app_prompt_name_what_was_noticed() {
        let notes = AppNotes {
            scope: Some(Scoped { flag: Flag { why: Why::Scope, p: 0.88, add: true, calibrated: false }, decision: "s".into() }),
            raised: Some(Upgrade { p: 0.81, decision: "k".into(), calibrated: false }),
        };
        assert_eq!(
            notes.reason(true).as_deref(),
            Some("Asked because this may send something \u{b7} Asked because this may not be what you asked for")
        );
        // The same doubt on a prompt that was coming anyway is only a flag.
        assert_eq!(
            notes.reason(false).as_deref(),
            Some("Asked because this may send something \u{b7} Flagged: this may not be what you asked for")
        );
        assert_eq!(AppNotes::default().reason(true), None);
        assert!(AppNotes::default().is_empty() && !notes.is_empty());
    }

    /// A read nobody asked for gets a question; a change or a send already had one.
    #[test]
    fn only_a_read_is_given_a_question_it_would_not_have_had() {
        let notes = AppNotes {
            scope: Some(Scoped { flag: Flag { why: Why::Scope, p: 0.9, add: true, calibrated: false }, decision: "s".into() }),
            raised: None,
        };
        assert!(notes.adds_to(CallKind::Read));
        assert!(!notes.adds_to(CallKind::Change) && !notes.adds_to(CallKind::Send));
        let line_only = AppNotes {
            scope: Some(Scoped { flag: Flag { why: Why::Scope, p: 0.7, add: false, calibrated: false }, decision: "s".into() }),
            raised: None,
        };
        assert!(!line_only.adds_to(CallKind::Read));
    }

    #[test]
    fn a_refusal_labels_what_was_flagged_and_a_send_raised_only_weakly() {
        let db = Db::open_in_memory().unwrap();
        for id in ["s", "k"] {
            log_decision(&db, id);
        }
        let notes = AppNotes {
            scope: Some(Scoped { flag: Flag { why: Why::Scope, p: 0.9, add: true, calibrated: false }, decision: "s".into() }),
            raised: Some(Upgrade { p: 0.7, decision: "k".into(), calibrated: false }),
        };
        notes.label_refusal(&db);
        let labels = |id: &str| db.decision_by_id(id).unwrap().unwrap().outcomes.iter().map(|l| l.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels("s"), ["right"]);
        assert_eq!(labels("k"), ["weak_right"]);
    }

    // ---- the whole look, against a fake panel ----

    struct Harness {
        db: Db,
        perms: Arc<PermissionManager>,
        sink: AgentEventSink,
        cancel: CancelFlag,
        shown: Arc<Mutex<Vec<Value>>>,
        /// Its own conversation, so what one test's turn remembers is not another's.
        conv: String,
    }

    impl Harness {
        fn new() -> Self {
            let shown = Arc::new(Mutex::new(Vec::new()));
            let sink_shown = shown.clone();
            let channel = tauri::ipc::Channel::new(move |body: tauri::ipc::InvokeResponseBody| {
                if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                    if let Ok(v) = serde_json::from_str::<Value>(&text) {
                        if v.get("type").and_then(|e| e.as_str()) == Some("permission") {
                            sink_shown.lock().unwrap().push(v);
                        }
                    }
                }
                Ok(())
            });
            Self {
                db: Db::open_in_memory().unwrap(),
                perms: Arc::new(PermissionManager::new()),
                sink: AgentEventSink::new(channel),
                cancel: CancelFlag::new(),
                shown,
                conv: uuid::Uuid::new_v4().to_string(),
            }
        }

        fn site(&self, headless: bool) -> Site<'_> {
            Site {
                db: &self.db,
                perms: &self.perms,
                sink: &self.sink,
                cancel: &self.cancel,
                conversation_id: &self.conv,
                run_id: None,
                headless,
                request: "Tidy my notes folder",
            }
        }

        fn answer_next(&self, decision: Decision) -> tokio::task::JoinHandle<()> {
            let (perms, shown) = (self.perms.clone(), self.shown.clone());
            tokio::spawn(async move {
                loop {
                    let id = shown
                        .lock()
                        .unwrap()
                        .last()
                        .and_then(|v| v.pointer("/request/id"))
                        .and_then(|i| i.as_str())
                        .map(str::to_string);
                    if let Some(id) = id {
                        perms.resolve(&id, decision);
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
        }
    }

    fn args() -> Value {
        serde_json::json!({ "command": "git push --force origin main" })
    }

    #[tokio::test]
    async fn a_call_that_looks_ordinary_runs_without_a_word() {
        let h = Harness::new();
        let out = act_on(&h.site(false), "run_command", &args(), &call(0.05, 0.98, false)).await;
        assert_eq!(out, Before::Go(None));
        assert!(h.shown.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_irreversible_call_asks_first_and_says_why() {
        let h = Harness::new();
        let answer = h.answer_next(Decision::Once);
        let out = act_on(&h.site(false), "run_command", &args(), &call(0.9, 0.9, false)).await;
        answer.await.unwrap();
        assert_eq!(out, Before::Go(None), "a yes runs it, with no second line");
        let shown = h.shown.lock().unwrap();
        assert_eq!(shown.len(), 1);
        let text = shown[0].to_string();
        assert!(text.contains("\"capability\":\"risk\""), "{text}");
        assert!(text.contains("Asked because this looks irreversible"), "{text}");
        assert!(text.contains("\"reason_hover\":\"Quick decision, this looks irreversible: 0.90\""), "{text}");
        assert!(text.contains("git push --force origin main"), "{text}");
    }

    #[tokio::test]
    async fn a_no_stops_the_call_tells_the_model_why_and_labels_a_rating_that_raised_it() {
        let h = Harness::new();
        let v = call(0.9, 0.9, false);
        log_decision(&h.db, &v.id);
        let answer = h.answer_next(Decision::Deny);
        let out = act_on(&h.site(false), "run_command", &args(), &v).await;
        answer.await.unwrap();
        let Before::Refuse(text) = out else { panic!("a no refuses") };
        assert!(text.contains("did not want me to run command: this looks irreversible.") && text.contains("So I did not"), "{text}");
        let labels: Vec<_> = h.db.decision_by_id("d1").unwrap().unwrap().outcomes.iter().map(|l| l.label.clone()).collect();
        assert_eq!(labels, ["right"]);
    }

    /// Both questions are in one request and the fit reads the rating, so a no to a
    /// call that was raised for the request writes no label.
    #[tokio::test]
    async fn a_no_to_a_call_raised_for_the_request_writes_no_label() {
        let h = Harness::new();
        let v = call(0.1, 0.05, false);
        log_decision(&h.db, &v.id);
        let answer = h.answer_next(Decision::Deny);
        let out = act_on(&h.site(false), "send_mail", &serde_json::json!({ "to": "a@b.c" }), &v).await;
        answer.await.unwrap();
        assert!(matches!(out, Before::Refuse(_)));
        assert!(h.db.decision_by_id("d1").unwrap().unwrap().outcomes.is_empty());
    }

    #[tokio::test]
    async fn a_milder_doubt_is_a_line_for_the_prompt_the_tool_raises() {
        let h = Harness::new();
        let out = act_on(&h.site(false), "edit_file", &args(), &call(0.6, 0.9, false)).await;
        let Before::Go(Some(note)) = out else { panic!("a line, not a question") };
        assert_eq!(note.line.as_deref(), Some("Flagged: this may be irreversible"));
        assert!(h.shown.lock().unwrap().is_empty(), "no question of its own");
    }

    /// `USE-12`: an added question cannot be answered by nobody, so it is a refusal.
    #[tokio::test]
    async fn an_unattended_run_is_refused_with_the_reason_and_never_waits() {
        let h = Harness::new();
        let out = act_on(&h.site(true), "run_command", &args(), &call(0.9, 0.9, false)).await;
        let Before::Refuse(text) = out else { panic!("expected a refusal") };
        assert!(text.starts_with("I did not run command:") && text.contains("Nobody is here to approve it") && text.contains("irreversible"), "{text}");
        assert!(h.shown.lock().unwrap().is_empty(), "an unattended run never raises a prompt");
    }

    #[tokio::test]
    async fn stopping_the_run_while_asked_refuses_without_a_label() {
        let h = Harness::new();
        let site = h.site(false);
        let cancel = h.cancel.clone();
        let go = tokio::spawn({
            let shown = h.shown.clone();
            async move {
                while shown.lock().unwrap().is_empty() {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                cancel.cancel();
            }
        });
        let out = act_on(&site, "run_command", &args(), &call(0.9, 0.9, false)).await;
        go.await.unwrap();
        assert_eq!(out, Before::Refuse("The person stopped the run before answering.".into()));
    }

    /// A no holds for the rest of the turn. The controller does not ask the same
    /// question about the same state twice, so without this the same call again
    /// would run unlooked-at. The next turn asks again.
    #[tokio::test]
    async fn a_no_holds_for_the_same_call_until_the_next_turn() {
        let h = Harness::new();
        h.db.set_setting(&crate::experiments::setting_key("fast-slow"), "on").unwrap();
        let answer = h.answer_next(Decision::Deny);
        let first = act_on(&h.site(false), "run_command", &args(), &call(0.9, 0.9, false)).await;
        answer.await.unwrap();
        assert!(matches!(first, Before::Refuse(_)));
        assert_eq!(before_own(&h.site(false), "run_command", &args()).await, first, "refused with the same words");
        assert_eq!(h.shown.lock().unwrap().len(), 1, "not asked a second time");
        // Another command is a question of its own (no engine in a test, so it goes on).
        let other = serde_json::json!({ "command": "git status" });
        assert_eq!(before_own(&h.site(false), "run_command", &other).await, Before::Go(None));
        crate::agent::decide::begin_turn(&h.conv);
        assert_eq!(before_own(&h.site(false), "run_command", &args()).await, Before::Go(None), "a new turn");
    }

    /// The same call again in a turn gets what the first one got: here a line on the
    /// tool's own question, instead of nothing.
    #[tokio::test]
    async fn the_same_call_again_gets_what_the_first_one_got() {
        let h = Harness::new();
        h.db.set_setting(&crate::experiments::setting_key("fast-slow"), "on").unwrap();
        let site = h.site(false);
        remember(&h.conv, &own_state(site.request, "edit_file", &args()), Known::Answered(call(0.6, 0.9, false)));
        let Before::Go(Some(note)) = before_own(&site, "edit_file", &args()).await else { panic!("the line again") };
        assert_eq!(note.line.as_deref(), Some("Flagged: this may be irreversible"));
    }

    /// An app read nobody asked for gets its added question again, not a free pass.
    #[tokio::test]
    async fn an_app_call_again_gets_the_same_question() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(&crate::experiments::setting_key("fast-slow"), "on").unwrap();
        let conv = uuid::Uuid::new_v4().to_string();
        let request = "Tidy my notes folder";
        let what = app_what("Gmail", "search mail", &serde_json::json!({ "query": "salary" }));
        let off = verdict(vec![(SCOPE_ID, Answer::Noul { p_true: 0.1 })], false);
        remember(&conv, &scope_state(request, &what), Known::Answered(off));
        let got = scope_of(&db, &conv, None, false, request, &what, true).await.expect("asked about again");
        assert!(got.flag.add);
    }

    #[tokio::test]
    async fn relax_prompts_gives_a_note_only_with_the_switch_on_a_fit_and_a_relaxable_tool() {
        let h = Harness::new();
        let sure = verdict(
            vec![(RISK_ID, rating([0.9, 0.08, 0.01, 0.01])), (SCOPE_ID, Answer::Noul { p_true: 0.99 })],
            true,
        );
        // Switch off: nothing, however sure.
        assert_eq!(act_on(&h.site(false), "edit_file", &args(), &sure).await, Before::Go(None));
        h.db.set_setting(&crate::experiments::setting_key("relax-prompts"), "on").unwrap();
        // The experiment needs fast-slow too; the note is built where that is on.
        h.db.set_setting(&crate::experiments::setting_key("fast-slow"), "on").unwrap();
        let out = act_on(&h.site(false), "edit_file", &args(), &sure).await;
        let Before::Go(Some(note)) = out else { panic!("expected a note") };
        assert!(note.relax.is_some() && note.line.is_none());
        // A tool that can never be relaxed gets none.
        assert_eq!(act_on(&h.site(false), "delete_file", &args(), &sure).await, Before::Go(None));
        // And an uncalibrated reading gets none.
        let unfitted = Verdict { calibrated: false, ..sure.clone() };
        assert_eq!(act_on(&h.site(false), "edit_file", &args(), &unfitted).await, Before::Go(None));
    }

    /// No engine, an experiment that is off or no request: the call goes on as before.
    #[tokio::test]
    async fn without_the_experiment_or_a_request_there_is_no_look() {
        let h = Harness::new();
        assert_eq!(before_own(&h.site(false), "run_command", &args()).await, Before::Go(None));
        h.db.set_setting(&crate::experiments::setting_key("fast-slow"), "on").unwrap();
        let mut site = h.site(false);
        site.request = "  ";
        assert_eq!(before_own(&site, "run_command", &args()).await, Before::Go(None));
        // On, with a request, but no engine in a test: still as before.
        assert_eq!(before_own(&h.site(false), "run_command", &args()).await, Before::Go(None));
    }
}
