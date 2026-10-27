//! `MCG`: the gate every MCP connector call passes through.
//!
//! Before this, a connector's tool ran the moment the model named it. Reading a
//! calendar and posting to a channel were the same act as far as the loop could
//! tell, and what came back entered the prompt as if the person had typed it.
//!
//! This module is the answer, and it is deliberately free of the loop's state:
//!
//! - [`classify`] says what a tool does: read, change or send (`MCG-2`).
//! - [`admit`] decides whether a call may run now, asks the person through the
//!   permission panel they already know (`MCG-3`), and counts sends (`MCG-4`).
//! - [`render_app_proposal`] / [`run_approved`] carry a call nobody was around to
//!   approve into the inbox and out again, the way `MAIL-3` does for mail.
//!
//! The rules are a pure function of the tool's name, the server's own hints and
//! the person's saved policy. The loop only has to call [`admit`] and, on `Run`,
//! make the call.

use std::collections::HashMap;

use super::decide::toolgate::AppNotes;
use crate::autonomy::{autonomy_gate, Rung};
use crate::db::{ConnectorPolicy, Db};
use crate::mcp::{McpClient, McpTool};
pub use crate::permissions::AppTool;
use crate::permissions::{Decision, PermissionManager, PermissionRequest};
use crate::runtime::proxy::{until_cancelled, CancelFlag};

use super::run::AgentEventSink;

/// What one call does to the world. Ordered by how much it needs a person's
/// say: the strictest of several calls is their maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallKind {
    Read,
    Change,
    Send,
}

impl CallKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CallKind::Read => "read",
            CallKind::Change => "change",
            CallKind::Send => "send",
        }
    }

    pub fn parse(s: &str) -> Option<CallKind> {
        match s {
            "read" => Some(CallKind::Read),
            "change" => Some(CallKind::Change),
            "send" => Some(CallKind::Send),
            _ => None,
        }
    }
}

/// What the server itself said about a tool. Only ever used to say "reads".
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolHints {
    pub read_only: Option<bool>,
}

pub fn hints_of(tool: &McpTool) -> ToolHints {
    ToolHints { read_only: tool.annotations.as_ref().and_then(|a| a.read_only_hint) }
}

// ---- classification (MCG-2) ----

const READ_VERBS: &[&str] = &[
    "get", "list", "fetch", "search", "read", "find", "retrieve", "lookup", "download", "view", "check", "count",
];
const SEND_VERBS: &[&str] = &[
    "send", "reply", "respond", "forward", "post", "publish", "tweet", "retweet", "broadcast", "invite", "share",
    "refund", "charge", "pay", "payout", "transfer",
];
const CREATE_VERBS: &[&str] = &["create", "add", "new"];
const SENT_NOUNS: &[&str] = &[
    "message", "comment", "post", "email", "mail", "invoice", "payment", "refund", "reply", "tweet", "dm",
];
/// Taking something away is a change even when the thing taken is a post.
const REMOVE_VERBS: &[&str] = &["delete", "remove", "destroy", "archive", "trash", "cancel", "revoke"];
/// Verbs that only help find where the action starts, so `notion-update-page`
/// and `GMAIL_LABEL_THREAD` are not read as being about "notion" or "gmail".
const OTHER_VERBS: &[&str] = &[
    "update", "edit", "set", "modify", "patch", "put", "upload", "move", "rename", "copy", "duplicate", "close", "open",
    "merge", "assign", "complete", "mark", "reopen", "label", "unlabel", "star", "unstar", "pin", "unpin", "block",
    "unblock", "follow", "unfollow", "subscribe", "unsubscribe", "restore", "run", "execute", "start", "stop",
    "enable", "disable", "import", "export", "sync", "schedule", "reschedule", "accept", "decline", "approve",
    "reject", "append", "insert", "replace", "clear", "reset", "submit", "apply", "attach", "detach", "login",
    "logout",
];

/// `get` in `GET_POST` and `posts` in `LIST_POSTS` are the same word.
fn singular(word: &str) -> &str {
    if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        &word[..word.len() - 1]
    } else {
        word
    }
}

fn is_in(list: &[&str], word: &str) -> bool {
    list.contains(&word) || list.contains(&singular(word))
}

fn is_verb(word: &str) -> bool {
    is_in(READ_VERBS, word)
        || is_in(SEND_VERBS, word)
        || is_in(CREATE_VERBS, word)
        || is_in(REMOVE_VERBS, word)
        || is_in(OTHER_VERBS, word)
}

/// Split a tool name into lowercase words on `_`, `-`, spaces and camelCase,
/// with a leading `mcp__server__` dropped.
fn words(tool: &str) -> Vec<String> {
    let name = tool.rsplit("__").next().unwrap_or(tool);
    let chars: Vec<char> = name.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if c.is_uppercase() && !cur.is_empty() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The words from the first verb on. A toolkit prefix (`GMAIL_`, `notion-`,
/// `GOOGLE_CALENDAR_`) comes before the verb, so everything ahead of it is
/// dropped. A name with no known verb keeps all its words.
fn action(words: &[String]) -> &[String] {
    match words.iter().position(|w| is_verb(w)) {
        Some(i) => &words[i..],
        None => words,
    }
}

/// Does the name say it sends, posts or pays?
fn sends_by_name(action: &[String]) -> bool {
    let Some(first) = action.first() else { return false };
    // A read verb first wins over a send noun later: `GET_POST` reads a post.
    if is_in(READ_VERBS, first) || is_in(REMOVE_VERBS, first) {
        return false;
    }
    if action.iter().any(|w| is_in(SEND_VERBS, w)) {
        return true;
    }
    // `CREATE_EMAIL_DRAFT` makes something nobody has received. The draft only
    // cancels this rule: `SEND_DRAFT` still sends, and says so with its verb.
    let drafts = action.iter().any(|w| singular(w) == "draft");
    !drafts
        && action.iter().any(|w| is_in(CREATE_VERBS, w))
        && action.iter().any(|w| is_in(SENT_NOUNS, w))
}

/// What one target tool does. `hint` is the server's own annotation when it
/// gave one; the name is the fallback. Send by name beats read by hint beats
/// read by name beats change, so a server that calls `SEND_EMAIL` read-only is
/// still treated as sending. `destructiveHint` and `openWorldHint` never make a
/// call looser, and are not read at all.
pub fn classify(tool: &str, hint: &ToolHints) -> CallKind {
    let all = words(tool);
    let action = action(&all);
    if sends_by_name(action) {
        CallKind::Send
    } else if hint.read_only == Some(true) {
        CallKind::Read
    } else if action.first().is_some_and(|w| is_in(READ_VERBS, w)) {
        CallKind::Read
    } else {
        CallKind::Change
    }
}

/// `MCG-7`: the person's per-tool corrections, `{tool: kind}`.
pub fn overrides_of(policy: &ConnectorPolicy) -> HashMap<String, CallKind> {
    policy
        .tool_overrides
        .as_deref()
        .and_then(|s| serde_json::from_str::<HashMap<String, String>>(s).ok())
        .map(|m| m.into_iter().filter_map(|(tool, kind)| Some((tool, CallKind::parse(&kind)?))).collect())
        .unwrap_or_default()
}

/// The kind a call is held to: the person's override if they made one, else
/// what [`classify`] says.
pub fn effective_kind(overrides: &HashMap<String, CallKind>, tool: &McpTool) -> CallKind {
    overrides.get(&tool.name).copied().unwrap_or_else(|| classify(&tool.name, &hints_of(tool)))
}

// ---- the gate (MCG-3, MCG-4) ----

/// The day a send is counted against. One place, so a local-time boundary is a
/// change here and nowhere else.
pub fn send_day() -> String {
    crate::memory::today()
}

/// Everything [`admit`] needs from the run, and nothing else.
pub struct GateEnv<'a> {
    pub db: &'a Db,
    pub perms: &'a PermissionManager,
    pub sink: &'a AgentEventSink,
    pub cancel: &'a CancelFlag,
    pub conversation_id: &'a str,
    /// Nobody is watching (a Task, a background child). Asking would hang.
    pub headless: bool,
    /// `YYYY-MM-DD`: the day this call counts as.
    pub day: String,
    /// `USE-12`: what a quick decision noticed about this call. It can add a line to
    /// the question, and ask about a read nobody asked for. It never lets a call
    /// through that the policy below would stop.
    pub notes: Option<&'a AppNotes>,
}

/// One app's share of a call (`CAP-5`). A call to an ordinary connector has one;
/// a Composio batch has one per app it touches, so each app's own policy and
/// daily cap apply to its own targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The app slug the policy is kept under.
    pub app: String,
    /// What the person calls it, for the panel.
    pub name: String,
    /// The strictest kind among this app's targets.
    pub kind: CallKind,
    /// How many of this app's targets send. A cap counts these, not calls.
    pub sends: i64,
}

/// One call to a connector's tool, as the gate sees it.
pub struct Call<'a> {
    pub connector_id: &'a str,
    pub connector_name: &'a str,
    /// `''` for an ordinary connector.
    pub app: &'a str,
    pub tool: &'a str,
    pub kind: CallKind,
    pub args: &'a serde_json::Value,
    /// Empty for an ordinary connector. Several apps in one call (`CAP-5`) fill
    /// it, and the call is then asked about once, naming all of them.
    pub parts: &'a [Part],
    /// The sentence to ask with, when the caller knows better than `summary`
    /// does what the call comes to (a batch names each of its targets).
    pub said: Option<&'a str>,
    /// What the panel lists as the arguments, when the real ones are a wrapper
    /// (a batch's `tools` array). Display only: what runs is `args`.
    pub shown: Option<&'a serde_json::Value>,
    /// `MCG-7`: a Composio call's app tools, for the panel to offer a correction
    /// of their kind. Empty for an ordinary connector, whose chips are on the
    /// Apps page.
    pub tools: &'a [AppTool],
}

impl Call<'_> {
    /// The apps this call touches, each with its own kind.
    fn groups(&self) -> Vec<Part> {
        if !self.parts.is_empty() {
            return self.parts.to_vec();
        }
        vec![Part {
            app: self.app.to_string(),
            name: self.connector_name.to_string(),
            kind: self.kind,
            sends: i64::from(self.kind == CallKind::Send),
        }]
    }
}

/// What the gate decided.
#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    /// Go ahead and call the server.
    Run,
    /// Nobody was there to ask. The call waits in the inbox, and this is what
    /// the model is told. Not an error: the work is parked, not failed.
    Left(String),
}

/// How a decision is written down (`MCG-6`).
fn log_decision(env: &GateEnv<'_>, call: &Call<'_>, by: &str) {
    let _ = env.db.log_activity(
        Some(env.conversation_id),
        "mcp",
        &format!("{} \u{b7} {} \u{b7} {} \u{b7} {by}", call.connector_name, call.kind.as_str(), call.tool),
    );
}

/// The label the person sees for the app or apps: the connector's name, or
/// every app a batch touches ("Gmail and Slack").
fn label(call: &Call<'_>) -> String {
    let names: Vec<&str> = call.parts.iter().map(|p| p.name.as_str()).collect();
    match names.as_slice() {
        [] => call.connector_name.to_string(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The action part of a tool name without its toolkit prefix, in words:
/// `GOOGLE_CALENDAR_UPDATE_EVENT` -> `update event`.
pub fn action_text(tool: &str) -> String {
    let all = words(tool);
    action(&all).join(" ")
}

/// `read_file` -> `read file`, `GMAIL_SEND_EMAIL` -> `gmail send email`.
pub fn humanise(tool: &str) -> String {
    words(tool).join(" ")
}

/// The most telling argument of a call, for the one-line summary.
pub fn telling_argument(args: &serde_json::Value) -> Option<String> {
    const KEYS: &[&str] = &[
        "to", "recipient", "recipients", "channel", "channel_id", "chat_id", "title", "subject", "name", "email",
        "url", "query", "id",
    ];
    let obj = args.as_object()?;
    KEYS.iter().find_map(|k| {
        let v = obj.get(*k)?;
        let text = match v {
            serde_json::Value::String(s) => s.trim().to_string(),
            serde_json::Value::Array(a) => a
                .iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            serde_json::Value::Number(n) => n.to_string(),
            _ => return None,
        };
        (!text.is_empty()).then(|| crate::media::ellipsize(&text, 80))
    })
}

/// `MCG-3`: one sentence in the first person that names the app, what it would
/// do and the target, so the person can say yes or no to the real thing.
pub fn summary(call: &Call<'_>) -> String {
    if let Some(said) = call.said {
        return said.to_string();
    }
    let what = humanise(call.tool);
    let target = telling_argument(call.args).map(|t| format!(" \u{2192} {t}")).unwrap_or_default();
    let app = label(call);
    match call.kind {
        CallKind::Read => format!("I'd like to read from {app}: {what}{target}"),
        CallKind::Change => format!("I'd like to change something in {app}: {what}{target}"),
        CallKind::Send => format!("I'd like to send something through {app}: {what}{target}"),
    }
}

/// The arguments the call would run with, pretty-printed and capped at 4 KB so
/// the panel can show them without a runaway payload filling the screen.
pub fn arguments_text(args: &serde_json::Value) -> String {
    const CAP: usize = 4096;
    let text = serde_json::to_string_pretty(args).unwrap_or_default();
    if text.len() <= CAP {
        return text;
    }
    let mut end = CAP;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &text[..end])
}

/// Decide whether `call` may run now.
///
/// - **Read** runs unless the person asked to be asked.
/// - **Change** asks unless the person said "always" for this app.
/// - **Send** asks unless the person allowed it *and* today's cap holds. A cap
///   that is reached turns an "allow" back into a question.
///
/// `Err` is a refusal, worded for the model. A send that ran is counted before
/// this returns, whoever said yes.
pub async fn admit(env: &GateEnv<'_>, call: &Call<'_>) -> Result<Admit, String> {
    let groups = call.groups();
    let policies: Vec<ConnectorPolicy> = groups
        .iter()
        .map(|g| {
            env.db
                .get_connector_policy(call.connector_id, &g.app)
                .unwrap_or_else(|_| ConnectorPolicy::defaults(call.connector_id, &g.app))
        })
        .collect();
    let app = label(call);
    let strictest = groups.iter().map(|g| g.kind).max().unwrap_or(call.kind);
    // `USE-12`: a read the request did not call for is asked about, which is the one
    // thing a quick decision can add here.
    let added = env.notes.is_some_and(|n| n.adds_to(strictest));

    // A call that touches several apps asks once if any of them needs asking.
    let mut reason: Option<String> = None;
    let mut all_allowed = true;
    for (g, policy) in groups.iter().zip(&policies) {
        let allowed = match g.kind {
            CallKind::Read => policy.read_policy != "ask" && !added,
            CallKind::Change => policy.change_policy == "allow",
            CallKind::Send => {
                // `Off` withdraws send tools from the registry; this is the same
                // rule for a call that names one anyway.
                let rung = autonomy_gate(env.db, "app_send");
                if rung == Rung::Off {
                    return Err("The person has turned off sending through connected apps.".to_string());
                }
                if policy.send_policy == "allow" || rung == Rung::Auto {
                    let used = env.db.app_send_count(call.connector_id, &g.app, &env.day).unwrap_or(0);
                    if used + g.sends > policy.send_cap {
                        reason.get_or_insert_with(|| format!("I've already sent {used} things through {} today.", g.name));
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }
        };
        all_allowed &= allowed;
    }

    if all_allowed {
        count_sends(env, call);
        return Ok(Admit::Run);
    }
    // The panel shows the cap of the first app that sends.
    let send_cap = groups
        .iter()
        .zip(&policies)
        .find(|(g, _)| g.kind == CallKind::Send)
        .map(|(_, p)| p.send_cap);

    let base = summary(call);
    let summary = match &reason {
        Some(r) => format!("{r} {base}"),
        None => base,
    };
    // `USE-12`: the line the question carries. A question added for a read says so.
    let line = env.notes.and_then(|n| n.reason(added));

    if env.headless {
        let said = match (&line, added) {
            (Some(l), true) => format!("{summary} ({l})"),
            _ => summary,
        };
        return Ok(leave_for_approval(env, call, &said, reason.is_some()));
    }

    let id = format!("perm_{}", uuid::Uuid::new_v4());
    let rx = env.perms.open_request(&id);
    let mut request = PermissionRequest::app_call(
        id,
        summary,
        app.clone(),
        strictest.as_str(),
        arguments_text(call.shown.unwrap_or(call.args)),
        send_cap,
    );
    if !call.tools.is_empty() {
        request.tools = Some(call.tools.to_vec());
    }
    request.reason = line;
    request.reason_hover = env.notes.and_then(|n| n.hover(added));
    env.sink.send_permission(request);
    let decision = until_cancelled(env.cancel, rx).await.map(|d| d.unwrap_or(Decision::Deny));
    match decision {
        None => {
            log_decision(env, call, "declined");
            Err("The person stopped the run before answering.".to_string())
        }
        Some(Decision::Deny) => {
            log_decision(env, call, "declined");
            if let Some(n) = env.notes {
                n.label_refusal(env.db);
            }
            Err(format!("The person declined to let me use {} in {app}.", humanise(call.tool)))
        }
        Some(Decision::Once) | Some(Decision::Chat) => {
            log_decision(env, call, "once");
            count_sends(env, call);
            Ok(Admit::Run)
        }
        Some(Decision::Forever) => {
            // "Always" is said for every app the question named, each at the
            // kind of its own targets.
            for (g, mut saved) in groups.iter().zip(policies) {
                match g.kind {
                    CallKind::Read => saved.read_policy = "run".into(),
                    CallKind::Change => saved.change_policy = "allow".into(),
                    CallKind::Send => saved.send_policy = "allow".into(),
                }
                let _ = env.db.save_connector_policy(&saved);
            }
            log_decision(env, call, "always");
            count_sends(env, call);
            Ok(Admit::Run)
        }
    }
}

/// A send is counted when it is admitted, per app and per target.
fn count_sends(env: &GateEnv<'_>, call: &Call<'_>) {
    for g in call.groups() {
        if g.sends > 0 {
            let _ = env.db.bump_app_send(call.connector_id, &g.app, &env.day, g.sends);
        }
    }
}

/// A call nobody can approve right now. It is written to the inbox as a
/// proposal and the model is told so; it never waits on a prompt that no one
/// will see.
fn leave_for_approval(env: &GateEnv<'_>, call: &Call<'_>, summary: &str, at_cap: bool) -> Admit {
    let text = render_app_proposal(call, summary);
    let rationale = summary.to_string();
    if let Ok(proposal) = env.db.add_change_proposal("app", None, &text, &rationale, Some(&rationale)) {
        env.sink.emit(super::AgentEvent::Proposal {
            id: proposal.id,
            target: "app".to_string(),
            rationale,
        });
    }
    log_decision(env, call, if at_cap { "cap" } else { "headless-refused" });
    Admit::Left(format!("I need you to approve this: {summary}. I've left it for you."))
}

// ---- the inbox leg (MCG-3, headless) ----

/// What a parked call needs to run later.
#[derive(Debug, PartialEq, Eq)]
pub struct AppProposal {
    pub connector_id: String,
    pub app: String,
    pub tool: String,
    pub kind: CallKind,
    pub summary: String,
    pub args: serde_json::Value,
}

/// The proposal's text: a few header lines the person can read, then the exact
/// arguments. Accepting runs what is written here, nothing recomputed.
pub fn render_app_proposal(call: &Call<'_>, summary: &str) -> String {
    format!(
        "Connector: {}\nApp: {}\nTool: {}\nKind: {}\nSummary: {}\n\n{}",
        call.connector_id,
        call.app,
        call.tool,
        call.kind.as_str(),
        summary.replace('\n', " "),
        serde_json::to_string_pretty(call.args).unwrap_or_default(),
    )
}

/// Parse [`render_app_proposal`] back. `None` when a required header is gone or
/// the arguments no longer parse, in which case nothing runs.
pub fn parse_app_proposal(text: &str) -> Option<AppProposal> {
    let (header, body) = text.split_once("\n\n")?;
    let (mut connector_id, mut app, mut tool, mut kind, mut summary) = (None, None, None, None, None);
    for line in header.lines() {
        let Some((key, value)) = line.split_once(": ") else { continue };
        match key {
            "Connector" => connector_id = Some(value.to_string()),
            "App" => app = Some(value.to_string()),
            "Tool" => tool = Some(value.to_string()),
            "Kind" => kind = CallKind::parse(value),
            "Summary" => summary = Some(value.to_string()),
            _ => {}
        }
    }
    Some(AppProposal {
        connector_id: connector_id?,
        // An empty `App:` line has no trailing space, so `split_once(": ")`
        // does not see it; absent means the whole connector.
        app: app.unwrap_or_default(),
        tool: tool?,
        kind: kind?,
        summary: summary.unwrap_or_default(),
        args: serde_json::from_str(body).ok()?,
    })
}

/// Run a call the person approved from the inbox. The decision was theirs, so
/// there is no gate here, but a send still counts against the day and the
/// connector must still be there and switched on.
pub async fn run_approved(db: &Db, http: &reqwest::Client, proposal: &AppProposal) -> Result<String, String> {
    let connector = db
        .get_connector(&proposal.connector_id)
        .map_err(|e| e.to_string())?
        .ok_or("That connector is gone.")?;
    if !connector.enabled {
        return Err(format!("{} is turned off.", connector.name));
    }
    let composio = connector.transport == crate::connectors::composio::TRANSPORT;
    let mut client = if composio {
        crate::connectors::composio::mcp_client(db, http).await?
    } else {
        let url = connector.url.clone().ok_or("That connector has no address.")?;
        if connector.transport == "stdio" {
            McpClient::new_stdio(url)
        } else {
            let token = crate::secrets::get_secret(crate::secrets::SERVICE_MCP, &connector.id).ok().flatten();
            McpClient::new(http.clone(), url, token)
        }
    };
    client.initialize().await.map_err(|e| e.to_string())?;
    if proposal.kind == CallKind::Send {
        if composio {
            // A parked batch counts each send against its own app.
            for (app, sends) in super::app_calls::send_counts(db, &connector.id, &proposal.tool, &proposal.args) {
                let _ = db.bump_app_send(&connector.id, &app, &send_day(), sends);
            }
        } else {
            let _ = db.bump_app_send(&proposal.connector_id, &proposal.app, &send_day(), 1);
        }
    }
    let out = client
        .call_tool(&proposal.tool, proposal.args.clone())
        .await
        .map_err(|e| e.to_string())?;
    let _ = db.log_activity(
        None,
        "mcp",
        &format!(
            "{} \u{b7} {} \u{b7} {} \u{b7} once",
            connector.name,
            proposal.kind.as_str(),
            proposal.tool
        ),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn kind(name: &str) -> CallKind {
        classify(name, &ToolHints::default())
    }

    /// `MCG-T-1`.
    #[test]
    fn names_are_read_change_or_send() {
        assert_eq!(kind("GMAIL_SEND_EMAIL"), CallKind::Send);
        assert_eq!(kind("REDDIT_GET_POST"), CallKind::Read);
        assert_eq!(kind("GMAIL_CREATE_EMAIL_DRAFT"), CallKind::Change);
        assert_eq!(kind("SLACK_CREATE_MESSAGE"), CallKind::Send);
        assert_eq!(kind("notion-update-page"), CallKind::Change);
        assert_eq!(kind("createInvoice"), CallKind::Send);
        // A server that says read-only but names a send is still a send.
        let read_only = ToolHints { read_only: Some(true) };
        assert_eq!(classify("GMAIL_SEND_EMAIL", &read_only), CallKind::Send);
        // The hint is trusted for a name that doesn't say send.
        assert_eq!(classify("frobnicate", &read_only), CallKind::Read);
        assert_eq!(kind("frobnicate"), CallKind::Change);
    }

    #[test]
    fn a_toolkit_prefix_is_not_the_verb() {
        assert_eq!(kind("notion-search"), CallKind::Read);
        assert_eq!(kind("GOOGLE_CALENDAR_LIST_EVENTS"), CallKind::Read);
        assert_eq!(kind("GOOGLE_CALENDAR_UPDATE_EVENT"), CallKind::Change);
        assert_eq!(kind("mcp__slack__list_channels"), CallKind::Read);
        assert_eq!(kind("slack_post_message"), CallKind::Send);
        assert_eq!(kind("listPosts"), CallKind::Read);
    }

    #[test]
    fn a_draft_is_not_a_send_but_sending_one_is() {
        assert_eq!(kind("create_draft"), CallKind::Change);
        assert_eq!(kind("GMAIL_SEND_DRAFT"), CallKind::Send);
        assert_eq!(kind("list_drafts"), CallKind::Read);
    }

    #[test]
    fn taking_a_post_away_is_a_change() {
        assert_eq!(kind("TWITTER_DELETE_TWEET"), CallKind::Change);
        assert_eq!(kind("delete_post"), CallKind::Change);
        assert_eq!(kind("share_document"), CallKind::Send);
        assert_eq!(kind("STRIPE_CREATE_PAYMENT"), CallKind::Send);
        assert_eq!(kind("STRIPE_LIST_PAYMENTS"), CallKind::Read);
    }

    #[test]
    fn camel_case_and_acronyms_split() {
        assert_eq!(words("createInvoice"), vec!["create", "invoice"]);
        assert_eq!(words("getHTTPPost"), vec!["get", "http", "post"]);
        assert_eq!(words("mcp__github__list_issues"), vec!["list", "issues"]);
        assert_eq!(words("GMAIL_SEND_EMAIL"), vec!["gmail", "send", "email"]);
    }

    #[test]
    fn the_strictest_kind_is_the_largest() {
        assert!(CallKind::Send > CallKind::Change && CallKind::Change > CallKind::Read);
    }

    #[test]
    fn a_persons_override_beats_the_name() {
        let tool = McpTool {
            name: "weird_thing".into(),
            description: String::new(),
            input_schema: serde_json::Value::Null,
            annotations: None,
        };
        let mut policy = ConnectorPolicy::defaults("c", "");
        assert_eq!(effective_kind(&overrides_of(&policy), &tool), CallKind::Change);
        policy.tool_overrides = Some(r#"{"weird_thing":"send","other":"nonsense"}"#.into());
        let overrides = overrides_of(&policy);
        assert_eq!(overrides.len(), 1, "a kind nobody knows is dropped, not guessed");
        assert_eq!(effective_kind(&overrides, &tool), CallKind::Send);
    }

    #[test]
    fn a_proposal_reads_back_exactly() {
        let args = serde_json::json!({ "channel": "#general", "text": "Hello\n\nthere" });
        let call = Call {
            connector_id: "c1",
            connector_name: "Slack",
            app: "",
            tool: "post_message",
            kind: CallKind::Send,
            args: &args,
            parts: &[],
            said: None,
            shown: None,
            tools: &[],
        };
        let text = render_app_proposal(&call, "I'd like to send something");
        let parsed = parse_app_proposal(&text).expect("reads back");
        assert_eq!(parsed.connector_id, "c1");
        assert_eq!(parsed.tool, "post_message");
        assert_eq!(parsed.kind, CallKind::Send);
        assert_eq!(parsed.args, args, "blank lines inside the arguments survive");
        assert!(parse_app_proposal("Connector: c1\n\n{}").is_none(), "no tool, nothing runs");
    }

    #[test]
    fn the_summary_names_the_app_and_the_target() {
        let args = serde_json::json!({ "channel": "#general", "text": "x" });
        let call = Call {
            connector_id: "c",
            connector_name: "Slack",
            app: "",
            tool: "post_message",
            kind: CallKind::Send,
            args: &args,
            parts: &[],
            said: None,
            shown: None,
            tools: &[],
        };
        let s = summary(&call);
        assert!(s.contains("Slack") && s.contains("post message") && s.contains("#general"), "{s}");
    }

    #[test]
    fn long_arguments_are_cut_on_a_character() {
        let args = serde_json::json!({ "text": "\u{e9}".repeat(5000) });
        let text = arguments_text(&args);
        assert!(text.len() <= 4096 + 3);
        assert!(text.ends_with('\u{2026}'));
    }

    // ---- the gate, end to end against a fake panel ----

    struct Harness {
        db: Db,
        perms: Arc<PermissionManager>,
        sink: AgentEventSink,
        cancel: CancelFlag,
        /// Permission requests the panel was shown, in order.
        shown: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl Harness {
        fn new() -> Self {
            let shown = Arc::new(Mutex::new(Vec::new()));
            let sink_shown = shown.clone();
            let channel = tauri::ipc::Channel::new(move |body: tauri::ipc::InvokeResponseBody| {
                if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
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
            }
        }

        fn env(&self, headless: bool, day: &str) -> GateEnv<'_> {
            GateEnv {
                db: &self.db,
                perms: &self.perms,
                sink: &self.sink,
                cancel: &self.cancel,
                conversation_id: "conv",
                headless,
                day: day.to_string(),
                notes: None,
            }
        }

        /// Answer the next permission request the gate raises.
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

    fn call<'a>(kind: CallKind, args: &'a serde_json::Value) -> Call<'a> {
        Call { connector_id: "c1", connector_name: "Slack", app: "", tool: "do_it", kind, args, parts: &[], said: None, shown: None, tools: &[] }
    }

    /// `MCG-T-3`: a change with no policy asks once; "no" does not call.
    #[tokio::test]
    async fn a_change_with_no_policy_asks_and_no_means_no() {
        let h = Harness::new();
        let args = serde_json::json!({ "title": "Standup" });
        let answer = h.answer_next(Decision::Deny);
        let out = admit(&h.env(false, "2026-10-13"), &call(CallKind::Change, &args)).await;
        answer.await.unwrap();
        assert!(out.is_err(), "declined means refused");
        let shown = h.shown.lock().unwrap();
        assert_eq!(shown.len(), 1, "exactly one request");
        assert!(shown[0].to_string().contains("\"capability\":\"app\""), "{}", shown[0]);
    }

    #[tokio::test]
    async fn always_for_changes_is_remembered_and_stops_the_asking() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let answer = h.answer_next(Decision::Forever);
        let first = admit(&h.env(false, "d"), &call(CallKind::Change, &args)).await;
        answer.await.unwrap();
        assert_eq!(first, Ok(Admit::Run));
        assert_eq!(h.db.get_connector_policy("c1", "").unwrap().change_policy, "allow");
        // The second call does not raise another request.
        let second = admit(&h.env(false, "d"), &call(CallKind::Change, &args)).await;
        assert_eq!(second, Ok(Admit::Run));
        assert_eq!(h.shown.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reading_runs_unless_the_person_asked_to_be_asked() {
        let h = Harness::new();
        let args = serde_json::json!({});
        assert_eq!(admit(&h.env(false, "d"), &call(CallKind::Read, &args)).await, Ok(Admit::Run));
        assert!(h.shown.lock().unwrap().is_empty());

        let mut p = ConnectorPolicy::defaults("c1", "");
        p.read_policy = "ask".into();
        h.db.save_connector_policy(&p).unwrap();
        let answer = h.answer_next(Decision::Once);
        assert_eq!(admit(&h.env(false, "d"), &call(CallKind::Read, &args)).await, Ok(Admit::Run));
        answer.await.unwrap();
        assert_eq!(h.shown.lock().unwrap().len(), 1);
    }

    /// `MCG-T-4`: nobody is there, so the call is parked, not made.
    #[tokio::test]
    async fn a_headless_send_without_a_policy_is_left_for_the_person() {
        let h = Harness::new();
        let args = serde_json::json!({ "channel": "#general" });
        let out = admit(&h.env(true, "d"), &call(CallKind::Send, &args)).await.unwrap();
        let Admit::Left(text) = out else { panic!("expected the call to be left") };
        assert!(text.starts_with("I need you to approve this:"), "{text}");
        assert!(h.shown.lock().unwrap().is_empty(), "a headless run never raises a prompt");
        let waiting = h.db.list_change_proposals().unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].target, "app");
        assert!(parse_app_proposal(&waiting[0].proposed_text).is_some());
        assert_eq!(h.db.app_send_count("c1", "", "d").unwrap(), 0, "a parked send is not a sent one");
    }

    #[tokio::test]
    async fn a_headless_send_at_the_cap_is_left_even_with_allow() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let mut p = ConnectorPolicy::defaults("c1", "");
        p.send_policy = "allow".into();
        p.send_cap = 2;
        h.db.save_connector_policy(&p).unwrap();
        let env = h.env(true, "2026-10-13");
        assert_eq!(admit(&env, &call(CallKind::Send, &args)).await, Ok(Admit::Run));
        assert_eq!(admit(&env, &call(CallKind::Send, &args)).await, Ok(Admit::Run));
        let third = admit(&env, &call(CallKind::Send, &args)).await.unwrap();
        let Admit::Left(text) = third else { panic!("the cap should park the third send") };
        assert!(text.contains("already sent 2 things through Slack today"), "{text}");
        assert_eq!(h.db.app_send_count("c1", "", "2026-10-13").unwrap(), 2);
    }

    /// `MCG-T-6`: the count survives across runs and starts again the next day.
    #[tokio::test]
    async fn the_cap_counts_across_runs_and_resets_on_the_next_day() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let mut p = ConnectorPolicy::defaults("c1", "");
        p.send_policy = "allow".into();
        p.send_cap = 1;
        h.db.save_connector_policy(&p).unwrap();

        assert_eq!(admit(&h.env(true, "2026-10-13"), &call(CallKind::Send, &args)).await, Ok(Admit::Run));
        // A different run, same day: already at the cap.
        assert!(matches!(
            admit(&h.env(true, "2026-10-13"), &call(CallKind::Send, &args)).await,
            Ok(Admit::Left(_))
        ));
        // The next day starts at zero.
        assert_eq!(admit(&h.env(true, "2026-10-14"), &call(CallKind::Send, &args)).await, Ok(Admit::Run));
        assert_eq!(h.db.app_send_count("c1", "", "2026-10-13").unwrap(), 1);
        assert_eq!(h.db.app_send_count("c1", "", "2026-10-14").unwrap(), 1);
    }

    #[tokio::test]
    async fn a_send_the_person_approved_once_still_counts() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let answer = h.answer_next(Decision::Once);
        assert_eq!(admit(&h.env(false, "d"), &call(CallKind::Send, &args)).await, Ok(Admit::Run));
        answer.await.unwrap();
        assert_eq!(h.db.app_send_count("c1", "", "d").unwrap(), 1);
        let shown = h.shown.lock().unwrap();
        assert!(shown[0].to_string().contains("\"cap\":25"), "the button needs the cap: {}", shown[0]);
    }

    // ---- a call that touches several apps (CAP-T-5) ----

    fn part(app: &str, name: &str, kind: CallKind, sends: i64) -> Part {
        Part { app: app.into(), name: name.into(), kind, sends }
    }

    fn batch<'a>(parts: &'a [Part], said: &'a str, args: &'a serde_json::Value) -> Call<'a> {
        Call {
            connector_id: "comp",
            connector_name: "Composio",
            app: "",
            tool: "COMPOSIO_MULTI_EXECUTE_TOOL",
            kind: parts.iter().map(|p| p.kind).max().unwrap(),
            args,
            parts,
            said: Some(said),
            shown: None,
            tools: &[],
        }
    }

    #[tokio::test]
    async fn a_batch_with_one_send_asks_once_naming_both_apps() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let parts = [part("gmail", "Gmail", CallKind::Read, 0), part("slack", "Slack", CallKind::Send, 1)];
        let said = "I'd like to send something through Gmail and Slack: post message";
        let answer = h.answer_next(Decision::Once);
        let out = admit(&h.env(false, "d"), &batch(&parts, said, &args)).await;
        answer.await.unwrap();
        assert_eq!(out, Ok(Admit::Run));
        let shown = h.shown.lock().unwrap();
        assert_eq!(shown.len(), 1, "one question for the whole batch");
        let text = shown[0].to_string();
        assert!(text.contains("Gmail and Slack"), "{text}");
        assert!(text.contains("\"call_kind\":\"send\""), "the strictest kind is shown: {text}");
        drop(shown);
        assert_eq!(h.db.app_send_count("comp", "slack", "d").unwrap(), 1);
        assert_eq!(h.db.app_send_count("comp", "gmail", "d").unwrap(), 0, "a read is not a send");
    }

    #[tokio::test]
    async fn always_on_a_batch_is_saved_for_each_app_at_its_own_kind() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let parts = [part("gmail", "Gmail", CallKind::Change, 0), part("slack", "Slack", CallKind::Send, 1)];
        let answer = h.answer_next(Decision::Forever);
        assert_eq!(admit(&h.env(false, "d"), &batch(&parts, "ask", &args)).await, Ok(Admit::Run));
        answer.await.unwrap();
        assert_eq!(h.db.get_connector_policy("comp", "gmail").unwrap().change_policy, "allow");
        assert_eq!(h.db.get_connector_policy("comp", "gmail").unwrap().send_policy, "ask", "only its own kind");
        assert_eq!(h.db.get_connector_policy("comp", "slack").unwrap().send_policy, "allow");
        // Now neither asks.
        assert_eq!(admit(&h.env(false, "d"), &batch(&parts, "ask", &args)).await, Ok(Admit::Run));
        assert_eq!(h.shown.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_send_cap_counts_targets_not_calls() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let mut p = ConnectorPolicy::defaults("comp", "slack");
        p.send_policy = "allow".into();
        p.send_cap = 25;
        h.db.save_connector_policy(&p).unwrap();
        let env = h.env(true, "2026-10-13");

        let ten = [part("slack", "Slack", CallKind::Send, 10)];
        assert_eq!(admit(&env, &batch(&ten, "ten", &args)).await, Ok(Admit::Run));
        assert_eq!(h.db.app_send_count("comp", "slack", "2026-10-13").unwrap(), 10, "ten sends, one call");

        // Sixteen more would pass 25, so the whole batch waits for a person.
        let sixteen = [part("slack", "Slack", CallKind::Send, 16)];
        let out = admit(&env, &batch(&sixteen, "sixteen", &args)).await.unwrap();
        assert!(matches!(out, Admit::Left(ref t) if t.contains("already sent 10 things through Slack today")), "{out:?}");
        assert_eq!(h.db.app_send_count("comp", "slack", "2026-10-13").unwrap(), 10, "nothing was added");

        // Fifteen fit exactly.
        let fifteen = [part("slack", "Slack", CallKind::Send, 15)];
        assert_eq!(admit(&env, &batch(&fifteen, "fifteen", &args)).await, Ok(Admit::Run));
    }

    #[tokio::test]
    async fn each_apps_own_policy_decides_whether_the_batch_needs_asking() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let mut p = ConnectorPolicy::defaults("comp", "gmail");
        p.change_policy = "allow".into();
        h.db.save_connector_policy(&p).unwrap();
        // Gmail's change is allowed, Slack's is not: the batch still asks.
        let parts = [part("gmail", "Gmail", CallKind::Change, 0), part("slack", "Slack", CallKind::Change, 0)];
        let answer = h.answer_next(Decision::Deny);
        assert!(admit(&h.env(false, "d"), &batch(&parts, "x", &args)).await.is_err());
        answer.await.unwrap();
        // Only Gmail: it runs without a question.
        let only = [part("gmail", "Gmail", CallKind::Change, 0)];
        assert_eq!(admit(&h.env(false, "d"), &batch(&only, "x", &args)).await, Ok(Admit::Run));
        assert_eq!(h.shown.lock().unwrap().len(), 1);
    }

    // ---- what a quick decision noticed (USE-12) ----

    use crate::agent::decide::toolgate::{Flag, Scoped, Upgrade, Why};

    fn off_request(p: f32, add: bool) -> AppNotes {
        AppNotes {
            scope: Some(Scoped { flag: Flag { why: Why::Scope, p, add, calibrated: false }, decision: "s".into() }),
            raised: None,
        }
    }

    /// `USE-T-2`: a read that would have run unasked is asked about when the request did
    /// not call for it, and the question says why.
    #[tokio::test]
    async fn a_read_nobody_asked_for_is_asked_about_with_the_reason() {
        let h = Harness::new();
        let args = serde_json::json!({ "query": "salary" });
        let notes = off_request(0.9, true);
        let mut env = h.env(false, "d");
        env.notes = Some(&notes);
        let answer = h.answer_next(Decision::Once);
        assert_eq!(admit(&env, &call(CallKind::Read, &args)).await, Ok(Admit::Run));
        answer.await.unwrap();
        let shown = h.shown.lock().unwrap();
        assert_eq!(shown.len(), 1);
        let text = shown[0].to_string();
        assert!(text.contains("Asked because this is not what you asked for"), "{text}");
    }

    /// A doubt that is only a line never adds a question to a read, and never removes one.
    #[tokio::test]
    async fn a_doubt_that_is_only_a_line_leaves_a_read_alone_and_a_change_gets_the_line() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let notes = off_request(0.7, false);
        let mut env = h.env(false, "d");
        env.notes = Some(&notes);
        assert_eq!(admit(&env, &call(CallKind::Read, &args)).await, Ok(Admit::Run));
        assert!(h.shown.lock().unwrap().is_empty(), "a read is not stopped by a line");
        let answer = h.answer_next(Decision::Once);
        assert_eq!(admit(&env, &call(CallKind::Change, &args)).await, Ok(Admit::Run));
        answer.await.unwrap();
        let text = h.shown.lock().unwrap()[0].to_string();
        assert!(text.contains("Flagged: this may not be what you asked for"), "{text}");
    }

    /// A question that was never going to be asked is not made unnecessary by a note.
    #[tokio::test]
    async fn a_note_never_lets_a_call_through_that_the_policy_would_stop() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let notes = off_request(0.0, false);
        let mut env = h.env(false, "d");
        env.notes = Some(&notes);
        let answer = h.answer_next(Decision::Deny);
        assert!(admit(&env, &call(CallKind::Send, &args)).await.is_err());
        answer.await.unwrap();
        assert_eq!(h.shown.lock().unwrap().len(), 1, "a send still asks");
    }

    #[tokio::test]
    async fn a_no_to_a_flagged_question_labels_both_decisions() {
        let h = Harness::new();
        for id in ["s", "k"] {
            h.db.add_decision(&crate::db::decisions::NewDecision {
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
        let args = serde_json::json!({});
        let notes = AppNotes {
            scope: Some(Scoped { flag: Flag { why: Why::Scope, p: 0.9, add: true, calibrated: false }, decision: "s".into() }),
            raised: Some(Upgrade { p: 0.7, decision: "k".into(), calibrated: false }),
        };
        let mut env = h.env(false, "d");
        env.notes = Some(&notes);
        let answer = h.answer_next(Decision::Deny);
        assert!(admit(&env, &call(CallKind::Send, &args)).await.is_err());
        answer.await.unwrap();
        let labels = |id: &str| h.db.decision_by_id(id).unwrap().unwrap().outcomes.iter().map(|l| l.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels("s"), ["right"]);
        assert_eq!(labels("k"), ["weak_right"]);
    }

    /// `USE-T-2`: an added question on an unattended run is parked "left for you", never a hang.
    #[tokio::test]
    async fn an_added_question_on_an_unattended_read_is_left_for_the_person() {
        let h = Harness::new();
        let args = serde_json::json!({});
        let notes = off_request(0.95, true);
        let mut env = h.env(true, "d");
        env.notes = Some(&notes);
        let out = admit(&env, &call(CallKind::Read, &args)).await.unwrap();
        let Admit::Left(text) = out else { panic!("expected the call to be left") };
        assert!(text.contains("not what you asked for"), "{text}");
        assert!(h.shown.lock().unwrap().is_empty());
        assert_eq!(h.db.list_change_proposals().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn sending_turned_off_refuses_without_asking() {
        let h = Harness::new();
        h.db.set_setting(&crate::autonomy::setting_key("app_send"), "off").unwrap();
        let args = serde_json::json!({});
        let out = admit(&h.env(false, "d"), &call(CallKind::Send, &args)).await;
        assert!(out.is_err());
        assert!(h.shown.lock().unwrap().is_empty());
    }
}
