//! `CAP`: what the model is offered from Composio, who may use it, and what a
//! call to it comes to.
//!
//! Composio shows a model four meta-tools instead of a thousand. That makes the
//! interesting question "which app tool does this call actually run", and the
//! answer is inside the arguments of `COMPOSIO_MULTI_EXECUTE_TOOL`. Everything
//! here is a pure function of the database and those arguments, so the rules are
//! testable without a run and the loop only has to ask.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::connectors::composio::{
    self, Mode, GET_TOOL_SCHEMAS, MANAGE_CONNECTIONS, MULTI_EXECUTE, OFFERED, SEARCH_TOOLS,
};
use crate::db::{Connector, Db, PersonaAppGrant};
use crate::mcp::McpTool;

use super::connector_gate::{self, CallKind, Part, ToolHints};

// ---- who may use which app (CAP-10) ----

/// What a persona may reach through the Composio connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grants {
    /// No rows: every connected app, as connectors have always behaved.
    Open,
    /// Rows exist: only what they list. A row for app `''` is the explicit
    /// "none" and lists nothing.
    Only(Vec<PersonaAppGrant>),
}

impl Grants {
    /// Does this persona have any app at all?
    pub fn any(&self) -> bool {
        match self {
            Grants::Open => true,
            Grants::Only(rows) => rows.iter().any(|g| !g.app.is_empty()),
        }
    }

    pub fn app_allowed(&self, app: &str) -> bool {
        match self {
            Grants::Open => true,
            Grants::Only(rows) => rows.iter().any(|g| g.app == app),
        }
    }

    pub fn tool_allowed(&self, app: &str, tool: &str) -> bool {
        match self {
            Grants::Open => true,
            Grants::Only(rows) => rows.iter().any(|g| {
                g.app == app
                    && (g.tools == "*"
                        || serde_json::from_str::<Vec<String>>(&g.tools)
                            .is_ok_and(|list| list.iter().any(|t| t.eq_ignore_ascii_case(tool))))
            }),
        }
    }
}

/// The persona a conversation runs under. A delegated child that has none of its
/// own (the general-purpose one) takes its lead's, so a narrowed persona cannot
/// be widened by delegating.
pub fn persona_of(db: &Db, conversation_id: &str) -> Option<String> {
    let mut id = conversation_id.to_string();
    for _ in 0..8 {
        let conv = db.get_conversation(&id).ok().flatten()?;
        if let Some(persona) = conv.persona_id {
            return Some(persona);
        }
        id = conv.parent_conversation_id?;
    }
    None
}

pub fn grants_for(db: &Db, conversation_id: &str, connector_id: &str) -> Grants {
    let Some(persona) = persona_of(db, conversation_id) else { return Grants::Open };
    let rows: Vec<PersonaAppGrant> = db
        .persona_app_grants(&persona)
        .unwrap_or_default()
        .into_iter()
        .filter(|g| g.connector_id == connector_id)
        .collect();
    if rows.is_empty() {
        Grants::Open
    } else {
        Grants::Only(rows)
    }
}

// ---- what the model is offered (CAP-4, CAP-11) ----

/// What kind of act a meta-tool is. Discovery reads. The batch runner is held to
/// `Change` here only so plan-first leaves it out; its real kind is the
/// strictest of its targets, found per call by [`plan`].
pub fn meta_kind(name: &str) -> CallKind {
    if name == MULTI_EXECUTE {
        CallKind::Change
    } else {
        CallKind::Read
    }
}

/// What the model calls one sign-in in a batch's `account`: the person's name for
/// it, else Composio's own account id (it takes either).
fn account_name(a: &crate::db::AppAccount) -> String {
    if let Some(label) = a.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        return label.to_string();
    }
    composio::connect_account_of(&a.id, &a.app).unwrap_or_else(|| a.id.clone())
}

/// ACTIVE sign-ins per app: `{slug: [names]}`.
fn connected(db: &Db, connector_id: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for a in db.list_app_accounts(connector_id).unwrap_or_default() {
        if a.status.eq_ignore_ascii_case("ACTIVE") {
            out.entry(a.app.clone()).or_default().push(account_name(&a));
        }
    }
    out
}

/// "Gmail (work, personal), Slack" or "none yet".
fn named_list(apps: &BTreeMap<String, Vec<String>>, grants: &Grants) -> String {
    let parts: Vec<String> = apps
        .iter()
        .filter(|(app, _)| grants.app_allowed(app))
        .map(|(app, labels)| {
            let name = composio::app_label(app);
            if labels.len() > 1 {
                format!("{name} ({})", labels.join(", "))
            } else {
                name
            }
        })
        .collect();
    if parts.is_empty() {
        "none yet".to_string()
    } else {
        parts.join(", ")
    }
}

/// The sentence every offered meta-tool starts with.
///
/// `CAP-C4`: in Connect mode the list is only what Poiesis has learned of, since
/// Composio has no account list to read. The sentence says so, and never claims
/// an app is not connected merely because it is not on the list.
pub fn description_prefix(
    apps: &BTreeMap<String, Vec<String>>,
    grants: &Grants,
    mail_account: bool,
    mode: Mode,
) -> String {
    let named = named_list(apps, grants);
    let mut text = match mode {
        Mode::Project => format!(
            "The person's connected apps: {named}. Find a tool with {SEARCH_TOOLS}, read its arguments with \
             {GET_TOOL_SCHEMAS}, run it with {MULTI_EXECUTE}. If an app isn't connected, call \
             {MANAGE_CONNECTIONS} for it as your last call this turn; the person signs in and you continue."
        ),
        Mode::Connect => format!(
            "The person's connected apps that I know of: {named} (others may be connected in Composio too). \
             Find a tool with {SEARCH_TOOLS}, read its arguments with {GET_TOOL_SCHEMAS}, run it with \
             {MULTI_EXECUTE}. If an app isn't connected, or a call to it says it isn't signed in, call \
             {MANAGE_CONNECTIONS} for it as your last call this turn; the person signs in and you continue."
        ),
    };
    text.push_str(&sentences_after(apps, grants, mail_account));
    text
}

/// The sentences every audience gets last: which account to name when an app has
/// several (`CAP-7`), and where email goes when a Mail account exists (`CAP-11`).
fn sentences_after(apps: &BTreeMap<String, Vec<String>>, grants: &Grants, mail_account: bool) -> String {
    let mut text = String::new();
    if apps.iter().any(|(app, names)| names.len() > 1 && grants.app_allowed(app)) {
        text.push_str(
            " When an app has several accounts (named in brackets), say which in each tool's `account`; \
             if you can't tell which one the person means, ask them.",
        );
    }
    if mail_account {
        text.push_str(
            " For email, use the mail tools (send_mail, read_mail...). They go directly from this \
             computer. Use connected apps for email only if the person asks for that account specifically.",
        );
    }
    text
}

/// `CAP-14`: the sentence an agent app reads in front of each tool. It cannot
/// sign in to anything: that is the person's act, in Settings, so it is told to
/// say what is missing in its report instead of asking for it.
pub fn agent_prefix(
    apps: &BTreeMap<String, Vec<String>>,
    grants: &Grants,
    mail_account: bool,
    mode: Mode,
) -> String {
    let named = named_list(apps, grants);
    let known = match mode {
        Mode::Project => format!("The person's connected apps: {named}."),
        Mode::Connect => {
            format!("The person's connected apps that Poiesis knows of: {named} (others may be connected in Composio too).")
        }
    };
    format!(
        "{known} Find a tool with {SEARCH_TOOLS}, read its arguments with {GET_TOOL_SCHEMAS}, run it with \
         {MULTI_EXECUTE}. Poiesis asks the person before anything changes or is sent, and the answer may be \
         that it is left waiting for them. You cannot sign in to an app: if one isn't connected, say which in \
         your report.{}",
        sentences_after(apps, grants, mail_account)
    )
}

fn has_mail_account(db: &Db) -> bool {
    db.list_mail_accounts().unwrap_or_default().iter().any(|a| a.enabled)
}

/// The meta-tools this conversation is offered, in a fixed order, each with the
/// connected-apps sentence in front of its own description.
///
/// Only [`OFFERED`] ever appears: a tool Composio adds tomorrow, and its remote
/// workbench and bash tools, stay out of sight. Nothing is offered without a
/// key, or to a persona that is allowed no app.
pub fn offered(db: &Db, conversation_id: &str, c: &Connector, mode: Option<Mode>) -> Vec<McpTool> {
    offered_as(db, conversation_id, c, mode, false)
}

/// `CAP-14`: the same meta-tools for an agent app Poiesis started, narrowed by
/// the grants of the persona its job runs under. Empty when apps are not set up,
/// or the persona is allowed none.
pub fn offered_for_agent(db: &Db, conversation_id: &str) -> Vec<McpTool> {
    let Ok(Some(c)) = db.composio_connector() else { return vec![] };
    if !c.enabled {
        return vec![];
    }
    offered_as(db, conversation_id, &c, composio::current_mode(), true)
}

/// Where an agent app's calls to Composio are routed.
pub(crate) fn agent_binding(db: &Db) -> Option<super::run::McpBinding> {
    let c = db.composio_connector().ok().flatten().filter(|c| c.enabled)?;
    Some(super::run::McpBinding {
        connector_id: c.id,
        connector_name: c.name,
        url: String::new(),
        transport: c.transport,
        kind: CallKind::Change,
    })
}

/// What an agent app is told when it asks to connect an app. Poiesis answers it
/// itself and never forwards it, so no sign-in address is made for an agent that
/// could not show it to anyone.
pub fn agent_connections_note(db: &Db, conversation_id: &str) -> String {
    let Some(c) = db.composio_connector().ok().flatten() else {
        return "No apps are connected.".to_string();
    };
    let grants = grants_for(db, conversation_id, &c.id);
    let named = named_list(&connected(db, &c.id), &grants);
    format!(
        "I can't sign in to an app for you. Connected apps: {named}. If the app you need isn't there, say which \
         one in your report; the person can connect it under Settings, Apps."
    )
}

fn offered_as(db: &Db, conversation_id: &str, c: &Connector, mode: Option<Mode>, agent: bool) -> Vec<McpTool> {
    let Some(mode) = mode else { return vec![] };
    let grants = grants_for(db, conversation_id, &c.id);
    if !grants.any() {
        return vec![];
    }
    let apps = connected(db, &c.id);
    let prefix = if agent {
        agent_prefix(&apps, &grants, has_mail_account(db), mode)
    } else {
        description_prefix(&apps, &grants, has_mail_account(db), mode)
    };
    let cached = composio::cached_tools(c);
    OFFERED
        .iter()
        .filter_map(|name| cached.iter().find(|t| t.name == *name))
        .map(|t| {
            let mut t = t.clone();
            // An agent cannot sign in, so Composio's own text for connecting is
            // replaced by ours rather than set after a sentence that contradicts it.
            t.description = if t.description.trim().is_empty() || (agent && t.name == MANAGE_CONNECTIONS) {
                prefix.clone()
            } else {
                format!("{prefix}\n\n{}", t.description)
            };
            t
        })
        .collect()
}

// ---- what a call comes to (CAP-5) ----

/// One app tool a call would run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub slug: String,
    pub args: Value,
    /// Which of several sign-ins to use. Composio takes it beside `arguments`,
    /// not inside them.
    pub account: Option<String>,
}

/// The arguments don't say which app tools the call runs.
#[derive(Debug, PartialEq, Eq)]
pub struct Unreadable;

pub const UNREADABLE: &str = "I couldn't read which app tools this call runs, so I didn't run it.";

/// The app tools a meta-tool call would run. Discovery and connection run none.
/// A batch whose shape isn't understood is `Unreadable`, and is refused without
/// a network call: a call whose targets can't be seen can't be judged.
pub fn targets(tool: &str, args: &Value) -> Result<Vec<Target>, Unreadable> {
    fn one(v: &Value) -> Option<Target> {
        let slug = v.get("tool_slug")?.as_str()?.trim();
        (!slug.is_empty()).then(|| Target {
            slug: slug.to_string(),
            args: v.get("arguments").cloned().unwrap_or_else(|| json!({})),
            account: v.get("account").and_then(|a| a.as_str()).map(str::trim).filter(|a| !a.is_empty()).map(str::to_string),
        })
    }
    if tool == MULTI_EXECUTE {
        if let Some(list) = args.get("tools") {
            let list = list.as_array().ok_or(Unreadable)?;
            if list.is_empty() {
                return Err(Unreadable);
            }
            return list.iter().map(|v| one(v).ok_or(Unreadable)).collect();
        }
        // The single-call shape older sessions used.
        return one(args).map(|t| vec![t]).ok_or(Unreadable);
    }
    if OFFERED.contains(&tool) {
        return Ok(vec![]);
    }
    // A toolkit tool named directly is itself the target.
    Ok(vec![Target { slug: tool.to_string(), args: args.clone(), account: None }])
}

/// The app a tool slug belongs to: the longest known app slug that prefixes it,
/// so `bland_ai_call` is Bland AI and not Bland. Failing that, its first word.
pub fn app_of(slug: &str, known: &[String]) -> String {
    let low = slug.to_lowercase();
    known
        .iter()
        .filter(|app| low.starts_with(&format!("{app}_")))
        .max_by_key(|app| app.len())
        .cloned()
        .unwrap_or_else(|| low.split('_').next().unwrap_or("").to_string())
}

/// Every app in the catalog that ships with Poiesis, and the common ones.
///
/// The whole catalog, not only the common apps: a third of its slugs have an
/// underscore (`google_maps`, `active_campaign`), and without them `app_of` falls
/// back to the first word, so every `google_*` app would share one "Google" and
/// one "always" for its changes.
fn catalog_slugs() -> &'static [String] {
    static SLUGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    SLUGS.get_or_init(|| {
        let mut all: Vec<String> = composio::bundled().iter().map(|a| a.slug.clone()).collect();
        all.extend(composio::curated().into_iter().map(|a| a.slug));
        all.sort();
        all.dedup();
        all
    })
}

/// Every app slug Poiesis knows of: those with a sign-in, and the catalog's.
fn known_apps(db: &Db, connector_id: &str) -> Vec<String> {
    let mut known: Vec<String> = db
        .list_app_accounts(connector_id)
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.app)
        .collect();
    known.extend(catalog_slugs().iter().cloned());
    known.sort();
    known.dedup();
    known
}

/// What a tool does, in words, with its app's prefix taken off:
/// `GOOGLECALENDAR_EVENTS_LIST` under `googlecalendar` is "events list", where
/// starting at the first verb would leave only "list".
pub fn action_of(slug: &str, app: &str) -> String {
    let low = slug.to_lowercase();
    match low.strip_prefix(&format!("{app}_")) {
        Some(rest) if !app.is_empty() && !rest.is_empty() => connector_gate::humanise(rest),
        _ => connector_gate::action_text(slug),
    }
}

/// A target with its app and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub target: Target,
    pub app: String,
    pub kind: CallKind,
}

/// Name each target's app and kind. The kind is the person's override for that
/// tool under that app if there is one, else what the name says.
pub fn resolve(db: &Db, connector_id: &str, tool: &str, args: &Value) -> Result<Vec<Resolved>, String> {
    let targets = targets(tool, args).map_err(|_| UNREADABLE.to_string())?;
    let known = known_apps(db, connector_id);
    Ok(targets
        .into_iter()
        .map(|target| {
            let app = app_of(&target.slug, &known);
            let overrides = db
                .get_connector_policy(connector_id, &app)
                .map(|p| connector_gate::overrides_of(&p))
                .unwrap_or_default();
            let kind = overrides
                .get(&target.slug)
                .copied()
                .unwrap_or_else(|| connector_gate::classify(&target.slug, &ToolHints::default()));
            Resolved { target, app, kind }
        })
        .collect())
}

/// One app tool the person can see and correct (`MCG-7` for Composio).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownTool {
    pub slug: String,
    /// What the gate holds it to: the person's override, else `auto`.
    pub kind: CallKind,
    /// What its name says.
    pub auto: CallKind,
    /// How often it ran here.
    pub calls: i64,
}

/// The tools of one Composio app that Poiesis can name: those that ran, busiest
/// first, then those the person set a kind for without their having run (from
/// the approval panel). Composio shows no tool list per app, so this is the list.
pub fn known_tools(db: &Db, connector_id: &str, app: &str) -> Vec<KnownTool> {
    let overrides = db
        .get_connector_policy(connector_id, app)
        .map(|p| connector_gate::overrides_of(&p))
        .unwrap_or_default();
    let one = |slug: String, calls: i64| {
        let auto = connector_gate::classify(&slug, &ToolHints::default());
        let kind = overrides.get(&slug).copied().unwrap_or(auto);
        KnownTool { slug, kind, auto, calls }
    };
    let mut out: Vec<KnownTool> =
        db.app_tools_used(app).unwrap_or_default().into_iter().map(|(slug, calls)| one(slug, calls)).collect();
    let mut set: Vec<&String> = overrides.keys().filter(|s| !out.iter().any(|t| &t.slug == *s)).collect();
    set.sort();
    let set: Vec<String> = set.into_iter().cloned().collect();
    out.extend(set.into_iter().map(|slug| one(slug, 0)));
    out
}

/// The app tools a call runs, once each, as the approval panel offers them for
/// correction.
pub fn panel_tools(connector_id: &str, resolved: &[Resolved]) -> Vec<connector_gate::AppTool> {
    let mut out: Vec<connector_gate::AppTool> = Vec::new();
    for r in resolved {
        if out.iter().any(|t| t.app == r.app && t.name == r.target.slug) {
            continue;
        }
        let auto = connector_gate::classify(&r.target.slug, &ToolHints::default());
        out.push(connector_gate::AppTool {
            connector_id: connector_id.to_string(),
            app: r.app.clone(),
            app_name: composio::app_label(&r.app),
            name: r.target.slug.clone(),
            kind: r.kind.as_str().to_string(),
            auto_kind: auto.as_str().to_string(),
            overridden: r.kind != auto,
        });
    }
    out
}

/// Group resolved targets by app, in the order the apps first appear.
pub fn parts_of(resolved: &[Resolved]) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for r in resolved {
        let sends = i64::from(r.kind == CallKind::Send);
        match parts.iter_mut().find(|p| p.app == r.app) {
            Some(p) => {
                p.kind = p.kind.max(r.kind);
                p.sends += sends;
            }
            None => parts.push(Part { app: r.app.clone(), name: composio::app_label(&r.app), kind: r.kind, sends }),
        }
    }
    parts
}

/// A call, judged and ready to ask about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub resolved: Vec<Resolved>,
    pub parts: Vec<Part>,
    /// The strictest kind among the targets. A batch with one send is a send.
    pub kind: CallKind,
    /// The sentence to ask with. Names every app and every target.
    pub said: String,
    /// `CAP-C-F6`: each target's own arguments as the rows the panel lists, so a
    /// message about to be sent is readable and not buried in a `tools` array.
    pub shown: Value,
}

const SHOWN_TARGETS: usize = 5;

fn account_named(args: &Value) -> Option<String> {
    ["account", "account_alias", "alias"]
        .iter()
        .find_map(|k| args.get(*k).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn said(resolved: &[Resolved], parts: &[Part], kind: CallKind, mail_account: bool) -> String {
    let apps = match parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().as_slice() {
        [] => "an app".to_string(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    let items: Vec<String> = resolved
        .iter()
        .take(SHOWN_TARGETS)
        .map(|r| {
            let mut item = action_of(&r.target.slug, &r.app);
            if let Some(account) = r.target.account.clone().or_else(|| account_named(&r.target.args)) {
                item.push_str(&format!(" ({account})"));
            }
            if let Some(to) = connector_gate::telling_argument(&r.target.args) {
                item.push_str(&format!(" \u{2192} {to}"));
            }
            item
        })
        .collect();
    let more = resolved.len().saturating_sub(SHOWN_TARGETS);
    let mut list = items.join("; ");
    if more > 0 {
        list.push_str(&format!("; and {more} more"));
    }
    let phrase = match kind {
        CallKind::Read => "read from",
        CallKind::Change => "change something in",
        CallKind::Send => "send something through",
    };
    let mut text = format!("I'd like to {phrase} {apps}: {list}");
    // `CAP-11`: the app is the same as one Poiesis already reaches directly.
    if mail_account && resolved.iter().any(|r| matches!(r.app.as_str(), "gmail" | "outlook")) {
        text.push_str(". I'd normally use your Mail account for this.");
    }
    text
}

/// The arguments of every target as flat rows: one target's own arguments as they
/// are, several targets with a number and the action in front of each name.
fn shown_arguments(resolved: &[Resolved]) -> Value {
    let mut rows = serde_json::Map::new();
    let several = resolved.len() > 1;
    for (i, r) in resolved.iter().enumerate() {
        let who = format!("{}. {} {}", i + 1, composio::app_label(&r.app), action_of(&r.target.slug, &r.app));
        match r.target.args.as_object().filter(|o| !o.is_empty()) {
            Some(args) => {
                for (k, v) in args {
                    let key = if several { format!("{who} · {k}") } else { k.clone() };
                    rows.insert(key, v.clone());
                }
            }
            None => {
                let key = if several { who } else { action_of(&r.target.slug, &r.app) };
                rows.insert(key, json!("(no arguments)"));
            }
        }
    }
    Value::Object(rows)
}

/// Judge a call to a Composio meta-tool: what it runs, whether this persona may
/// run it, what kind of act it comes to, and what to ask.
///
/// A refusal is worded for the model and never lists what the persona *is*
/// granted.
pub fn plan(db: &Db, conversation_id: &str, connector_id: &str, tool: &str, args: &Value) -> Result<Planned, String> {
    let resolved = resolve(db, connector_id, tool, args)?;
    // `CAP-C-F7`: a Composio meta-tool is not an app tool. Named inside a batch it
    // would be judged as an ordinary change and could be allowed once, and the
    // remote workbench and bash tools are among them.
    if let Some(r) = resolved.iter().find(|r| r.target.slug.to_uppercase().starts_with("COMPOSIO_")) {
        return Err(format!(
            "{} isn't an app tool, so I can't run it inside a batch. Call the tools I was given directly.",
            r.target.slug
        ));
    }
    let grants = grants_for(db, conversation_id, connector_id);
    for r in &resolved {
        if !grants.tool_allowed(&r.app, &r.target.slug) {
            return Err(format!(
                "{} isn't one I'm allowed to use here. Tell the person; they can grant it under this persona.",
                r.target.slug
            ));
        }
    }
    // Which account: Composio refuses a call to an app with several sign-ins that
    // names none. Poiesis knows the sign-ins, so the model hears it here, with the
    // names to choose from, before anything is asked or sent.
    let accounts = connected(db, connector_id);
    for r in &resolved {
        let Some(names) = accounts.get(&r.app).filter(|n| n.len() > 1) else { continue };
        if r.target.account.is_none() && account_named(&r.target.args).is_none() {
            return Err(format!(
                "{} has several accounts here: {}. Say which in this tool's `account`. If you can't tell \
                 which one the person means, ask them.",
                composio::app_label(&r.app),
                names.join(", ")
            ));
        }
    }
    let parts = parts_of(&resolved);
    let kind = resolved.iter().map(|r| r.kind).max().unwrap_or(CallKind::Read);
    let said = said(&resolved, &parts, kind, has_mail_account(db));
    let shown = shown_arguments(&resolved);
    Ok(Planned { resolved, parts, kind, said, shown })
}

// ---- connecting from the chat (CAP-6) ----

/// One sign-in card to show. Its data names the app and a suggested label and
/// nothing else: the address to sign in at is made when the person clicks, and
/// is never written into a message, a block or a prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub name: String,
    pub title: String,
    pub data: Value,
}

/// What the model's `COMPOSIO_MANAGE_CONNECTIONS` call comes to.
#[derive(Debug, Default, PartialEq)]
pub struct ConnectRequest {
    pub cards: Vec<Card>,
    /// Things the model should know: already connected, not allowed, nobody here.
    pub notes: Vec<String>,
}

impl ConnectRequest {
    /// `CAP-C3`: in Connect mode, what Composio said about the apps that were to
    /// get a card. An app it reports as signed in gets no card, only a note; one
    /// it could not start gets a note too; the rest keep their cards. The answer
    /// itself, with its addresses, goes no further than this.
    pub fn settle(&mut self, outcome: Result<Vec<composio::ConnState>, String>) {
        let app_of = |c: &Card| c.data.get("app").and_then(|a| a.as_str()).unwrap_or("").to_string();
        match outcome {
            Err(why) => {
                self.cards.clear();
                self.notes.push(format!("I couldn't start the sign-in. {why}"));
            }
            Ok(states) => {
                let mut kept = Vec::new();
                for card in std::mem::take(&mut self.cards) {
                    let app = app_of(&card);
                    match states.iter().find(|s| s.app == app).map(|s| s.status) {
                        Some(composio::Conn::Active) => self.notes.push(format!("{} is already connected.", card.name)),
                        Some(composio::Conn::Initiated) => kept.push(card),
                        _ => self.notes.push(format!("Composio couldn't start the sign-in for {}.", card.name)),
                    }
                }
                self.cards = kept;
            }
        }
    }

    /// The words the model gets back, and what the run says if it had none of
    /// its own. `asked` are the apps whose cards were actually shown.
    pub fn reply(&self, asked: &[String]) -> (String, Option<String>) {
        let mut reply = String::new();
        let mut waiting = None;
        if !asked.is_empty() {
            let list = asked.join(" and ");
            reply = format!("I've asked the person to sign in to {list}. Stop here; I'll continue when it's connected.");
            waiting = Some(format!("I need you to sign in to {list} before I can go on."));
        }
        for n in &self.notes {
            if !reply.is_empty() {
                reply.push(' ');
            }
            reply.push_str(n);
        }
        (reply, waiting)
    }
}

/// The `(app, action)` pairs a `MANAGE_CONNECTIONS` call names. Composio's own
/// schema, which is what the model is shown, takes `[{name, action}]` with
/// `action` one of `add` (the default), `list`, `rename`, `remove`; a plain slug
/// is read as `add` too.
fn connection_ops(args: &Value) -> Vec<(String, String)> {
    let mut ops = Vec::new();
    {
        let mut take = |v: &Value| match v {
            Value::String(s) => ops.push((s.trim().to_lowercase(), "add".to_string())),
            Value::Object(o) => {
                if let Some(name) = o.get("name").or_else(|| o.get("toolkit")).and_then(|n| n.as_str()) {
                    let action = o.get("action").and_then(|a| a.as_str()).unwrap_or("add").trim().to_lowercase();
                    ops.push((name.trim().to_lowercase(), action));
                }
            }
            _ => {}
        };
        match args.get("toolkits") {
            Some(Value::Array(list)) => list.iter().for_each(&mut take),
            Some(one) => take(one),
            None => {}
        }
        if let Some(one) = args.get("toolkit") {
            take(one);
        }
    }
    let mut seen = std::collections::HashSet::new();
    ops.retain(|op| composio::valid_slug(&op.0) && seen.insert(op.clone()));
    ops
}

fn connected_note(name: &str, labels: &[&str]) -> String {
    // Connect mode has no labels: there is nothing to put in brackets.
    if labels.is_empty() {
        format!("{name} is already connected.")
    } else {
        format!("{name} is already connected ({}).", labels.join(", "))
    }
}

/// Read the apps the model wants connected and decide, for each, whether a card
/// is called for. Nothing is written here.
///
/// Only `add` can lead to a card. `list` is answered from what Poiesis knows;
/// `remove` and `rename` are the person's to do on the Apps page, so they are
/// declined in words and never reach Composio.
///
/// In Connect mode an app the mirror holds as ACTIVE still gets a card when
/// someone is watching: the mirror only learns, it is never told when a sign-in
/// stops, so Composio is asked (`ConnectRequest::settle` turns an app it calls
/// connected back into a note).
pub fn connect_request(
    db: &Db,
    conversation_id: &str,
    connector_id: &str,
    headless: bool,
    args: &Value,
) -> Result<ConnectRequest, String> {
    let ops = connection_ops(args);
    if ops.is_empty() {
        return Err("Name the apps to connect in `toolkits`, like [{\"name\": \"gmail\"}].".to_string());
    }
    let connect_mode = composio::current_mode() == Some(Mode::Connect);

    let grants = grants_for(db, conversation_id, connector_id);
    let accounts = db.list_app_accounts(connector_id).unwrap_or_default();
    let mut out = ConnectRequest::default();
    let mut carded = std::collections::HashSet::new();
    for (app, action) in ops {
        let name = composio::app_label(&app);
        if !grants.app_allowed(&app) {
            out.notes.push(format!("{name} isn't one I'm allowed to use here."));
            continue;
        }
        let mine: Vec<_> = accounts.iter().filter(|a| a.app == app).collect();
        let active: Vec<_> = mine.iter().filter(|a| a.status.eq_ignore_ascii_case("ACTIVE")).collect();
        let labels: Vec<&str> = active.iter().filter_map(|a| a.label.as_deref()).collect();
        match action.as_str() {
            "add" => {}
            "list" => {
                out.notes.push(if !active.is_empty() {
                    connected_note(&name, &labels).replace("already ", "")
                } else if connect_mode {
                    format!("{name} isn't connected as far as I know. Try it, or ask the person to sign in.")
                } else {
                    format!("{name} isn't connected.")
                });
                continue;
            }
            _ => {
                out.notes.push(format!(
                    "I don't remove or rename sign-ins, so I left {name} as it is. The person can manage it under Settings, Apps."
                ));
                continue;
            }
        }
        if !carded.insert(app.clone()) {
            continue;
        }
        if !active.is_empty() && (!connect_mode || headless) {
            out.notes.push(connected_note(&name, &labels));
            continue;
        }
        if headless {
            out.notes.push(format!(
                "{name} isn't connected, and nobody is here to sign in. Say so in your report: \
                 the person can connect it under Settings, Apps."
            ));
            continue;
        }
        // An app with a sign-in that is waiting or expired still needs its own
        // name for a new one; suggest the one that is missing.
        let have: Vec<&str> = mine.iter().filter_map(|a| a.label.as_deref()).collect();
        let suggested = (!have.is_empty())
            .then(|| ["personal", "work"].into_iter().find(|l| !have.iter().any(|h| h.eq_ignore_ascii_case(l))))
            .flatten();
        out.cards.push(Card {
            title: format!("Sign in to {name}"),
            data: json!({ "app": app, "name": name, "label": suggested, "state": "waiting" }),
            name,
        });
    }
    Ok(out)
}

/// `CA-UI-5`: a timeline step in the person's words, from the call alone (no
/// database): a verb, then the app and what was done.
///
/// A batch reads as its apps and counts ("Linear \u{d7}3, Notion \u{d7}1"). The
/// raw slugs stay in the step's expandable detail.
pub fn describe(name: &str, args: &Value) -> (String, String) {
    let known = catalog_slugs();
    match name {
        SEARCH_TOOLS => ("searched".to_string(), "my connected apps for a tool".to_string()),
        GET_TOOL_SCHEMAS => ("read".to_string(), "how an app tool works".to_string()),
        MANAGE_CONNECTIONS => {
            let ops = connection_ops(args);
            let mut apps: Vec<String> = ops.iter().map(|(app, _)| composio::app_label(app)).collect();
            apps.dedup();
            let verb = if ops.iter().any(|(_, action)| action == "add") {
                "asked you to sign in to"
            } else {
                "checked my sign-ins for"
            };
            (verb.to_string(), if apps.is_empty() { "an app".to_string() } else { apps.join(", ") })
        }
        _ => {
            let Ok(list) = targets(name, args) else {
                return ("used".to_string(), "a connected app".to_string());
            };
            let kind = list
                .iter()
                .map(|t| connector_gate::classify(&t.slug, &ToolHints::default()))
                .max()
                .unwrap_or(CallKind::Read);
            let verb = match kind {
                CallKind::Read => "read",
                CallKind::Change => "changed",
                CallKind::Send => "sent",
            };
            let target = match list.as_slice() {
                [one] => {
                    let app = app_of(&one.slug, known);
                    let mut text = format!("{}: {}", composio::app_label(&app), action_of(&one.slug, &app));
                    if let Some(to) = connector_gate::telling_argument(&one.args) {
                        text.push_str(&format!(" \u{2192} {to}"));
                    }
                    text
                }
                many => {
                    let mut counts: Vec<(String, usize)> = Vec::new();
                    for t in many {
                        let label = composio::app_label(&app_of(&t.slug, known));
                        match counts.iter_mut().find(|(l, _)| *l == label) {
                            Some((_, n)) => *n += 1,
                            None => counts.push((label, 1)),
                        }
                    }
                    counts.iter().map(|(l, n)| format!("{l} \u{d7}{n}")).collect::<Vec<_>>().join(", ")
                }
            };
            (verb.to_string(), target)
        }
    }
}

/// `(app, sends)` for a parked call that has just been approved, so its sends
/// are counted against the right apps.
pub fn send_counts(db: &Db, connector_id: &str, tool: &str, args: &Value) -> Vec<(String, i64)> {
    resolve(db, connector_id, tool, args)
        .map(|r| parts_of(&r).into_iter().filter(|p| p.sends > 0).map(|p| (p.app, p.sends)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> Value {
        json!({ "name": name, "description": format!("{name} docs"), "inputSchema": { "type": "object" } })
    }

    /// A Composio row that has cached its tools, with some signed-in accounts.
    fn row(db: &Db, accounts: &[(&str, &str, &str)]) -> Connector {
        let all = [
            SEARCH_TOOLS,
            GET_TOOL_SCHEMAS,
            MULTI_EXECUTE,
            MANAGE_CONNECTIONS,
            "COMPOSIO_REMOTE_WORKBENCH",
            "COMPOSIO_REMOTE_BASH_TOOL",
            "COMPOSIO_WAIT_FOR_CONNECTIONS",
            "COMPOSIO_BRAND_NEW_TOOL",
        ];
        let config = json!({ "user_id": "u", "tools": all.iter().map(|n| tool(n)).collect::<Vec<_>>() });
        let c = db.add_composio_connector(&config.to_string()).unwrap();
        let now = crate::db::now_ms();
        for (id, app, label) in accounts {
            db.upsert_app_account(&crate::db::AppAccount {
                id: id.to_string(),
                connector_id: c.id.clone(),
                app: app.to_string(),
                label: Some(label.to_string()),
                status: "ACTIVE".into(),
                updated_at: now,
            })
            .unwrap();
        }
        db.composio_connector().unwrap().unwrap()
    }

    fn conv(db: &Db) -> String {
        db.create_conversation("t", None, false).unwrap().id
    }

    fn persona(db: &Db) -> String {
        db.create_persona(&crate::db::NewPersona {
            name: "p".into(),
            system_prompt: "x".into(),
            model_id: None,
            params_json: None,
            tools_json: None,
            skills_json: None,
            description: None,
            spawnable: false,
        })
        .unwrap()
        .id
    }

    fn grant(connector: &str, app: &str, tools: &str) -> PersonaAppGrant {
        PersonaAppGrant { connector_id: connector.into(), app: app.into(), tools: tools.into() }
    }

    fn names(tools: &[McpTool]) -> Vec<&str> {
        tools.iter().map(|t| t.name.as_str()).collect()
    }

    // ---- CAP-T-3: the registry offers exactly the allowlist ----

    #[test]
    fn exactly_the_four_meta_tools_are_offered_in_a_fixed_order() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let got = offered(&db, &conv(&db), &c, Some(Mode::Project));
        assert_eq!(names(&got), OFFERED.to_vec());
        for hidden in ["COMPOSIO_REMOTE_WORKBENCH", "COMPOSIO_REMOTE_BASH_TOOL", "COMPOSIO_WAIT_FOR_CONNECTIONS", "COMPOSIO_BRAND_NEW_TOOL"] {
            assert!(!names(&got).contains(&hidden), "{hidden} must stay hidden");
        }
    }

    #[test]
    fn nothing_is_offered_without_a_key() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        assert!(offered(&db, &conv(&db), &c, None).is_empty());
    }

    #[test]
    fn each_description_starts_with_the_connected_apps() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "work"), ("a2", "gmail", "personal"), ("a3", "slack", "default")]);
        let got = offered(&db, &conv(&db), &c, Some(Mode::Project));
        let d = &got[0].description;
        assert!(d.starts_with("The person's connected apps: Gmail (personal, work), Slack."), "{d}");
        assert!(d.contains("COMPOSIO_SEARCH_TOOLS docs"), "the tool's own text follows");

        let none = row_without_accounts();
        assert!(none.contains("none yet"), "{none}");
    }

    fn row_without_accounts() -> String {
        description_prefix(&BTreeMap::new(), &Grants::Open, false, Mode::Project)
    }

    /// CAP-T-11.
    #[test]
    fn the_mail_sentence_appears_only_with_an_enabled_mail_account() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let convo = conv(&db);
        let without = offered(&db, &convo, &c, Some(Mode::Project))[0].description.clone();
        assert!(!without.contains("mail tools"), "{without}");

        db.add_mail_account(&crate::db::NewMailAccount {
            label: "Me".into(),
            email: "me@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            username: "me".into(),
            security: "tls".into(),
        })
        .unwrap();
        let with = offered(&db, &convo, &c, Some(Mode::Project))[0].description.clone();
        assert!(with.contains("use the mail tools"), "{with}");
        assert!(with.contains("only if the person asks for that account specifically"), "{with}");
    }

    // ---- CAP-T-6: grants ----

    #[test]
    fn a_persona_with_no_grants_sees_every_app_and_one_with_grants_sees_only_those() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "default"), ("a2", "slack", "default")]);
        let p = persona(&db);
        let convo = conv(&db);
        db.set_conversation_persona(&convo, Some(&p), None).unwrap();

        let all = offered(&db, &convo, &c, Some(Mode::Project));
        assert!(all[0].description.contains("Gmail, Slack"));

        db.set_persona_app_grants(&p, &[grant(&c.id, "slack", "*")]).unwrap();
        let narrowed = offered(&db, &convo, &c, Some(Mode::Project));
        assert!(narrowed[0].description.contains("connected apps: Slack."), "{}", narrowed[0].description);
        assert!(!narrowed[0].description.contains("Gmail"), "an ungranted app is not even named");
    }

    #[test]
    fn the_explicit_none_offers_no_composio_tools_at_all() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "default")]);
        let p = persona(&db);
        let convo = conv(&db);
        db.set_conversation_persona(&convo, Some(&p), None).unwrap();
        db.set_persona_app_grants(&p, &[grant(&c.id, "", "[]")]).unwrap();
        assert!(offered(&db, &convo, &c, Some(Mode::Project)).is_empty());
        // An empty list of grants is "no narrowing", not "none".
        db.set_persona_app_grants(&p, &[]).unwrap();
        assert!(!offered(&db, &convo, &c, Some(Mode::Project)).is_empty());
    }

    #[test]
    fn a_general_child_takes_its_leads_grants_and_a_call_is_checked_again() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "default"), ("a2", "slack", "default")]);
        let p = persona(&db);
        let lead = conv(&db);
        db.set_conversation_persona(&lead, Some(&p), None).unwrap();
        db.set_persona_app_grants(&p, &[grant(&c.id, "slack", r#"["SLACK_LIST_CHANNELS"]"#)]).unwrap();
        let child = conv(&db);
        db.set_conversation_parent(&child, &lead).unwrap();

        assert_eq!(persona_of(&db, &child).as_deref(), Some(p.as_str()));
        let batch = |slug: &str| json!({ "tools": [{ "tool_slug": slug, "arguments": {} }] });
        assert!(plan(&db, &child, &c.id, MULTI_EXECUTE, &batch("SLACK_LIST_CHANNELS")).is_ok());
        let refused = plan(&db, &child, &c.id, MULTI_EXECUTE, &batch("SLACK_SEND_MESSAGE")).unwrap_err();
        assert!(refused.contains("isn't one I'm allowed to use here"), "{refused}");
        let other_app = plan(&db, &child, &c.id, MULTI_EXECUTE, &batch("GMAIL_FETCH_EMAILS")).unwrap_err();
        assert!(other_app.contains("GMAIL_FETCH_EMAILS"), "{other_app}");
        assert!(!refused.contains("SLACK_LIST_CHANNELS"), "the refusal never lists what is granted");
    }

    // ---- CAP-T-4: targets ----

    #[test]
    fn targets_reads_the_batch_shapes_and_refuses_the_rest() {
        let ok = targets(
            MULTI_EXECUTE,
            &json!({ "tools": [
                { "tool_slug": "GMAIL_SEND_EMAIL", "arguments": { "to": "a@b.c" } },
                { "tool_slug": " SLACK_LIST_CHANNELS " },
            ]}),
        )
        .unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[0].slug, "GMAIL_SEND_EMAIL");
        assert_eq!(ok[0].args["to"], "a@b.c");
        assert_eq!(ok[1].slug, "SLACK_LIST_CHANNELS");
        assert_eq!(ok[1].args, json!({}));

        // The single-call shape.
        assert_eq!(targets(MULTI_EXECUTE, &json!({ "tool_slug": "NOTION_SEARCH" })).unwrap().len(), 1);

        // Anything else cannot be judged.
        for bad in [
            json!({}),
            json!({ "tools": [] }),
            json!({ "tools": "GMAIL_SEND_EMAIL" }),
            json!({ "tools": [{ "arguments": {} }] }),
            json!({ "tools": [{ "tool_slug": "" }] }),
            json!({ "tools": [{ "tool_slug": 5 }] }),
            json!({ "tools": [{ "tool_slug": "A_B" }, "oops"] }),
            json!(null),
        ] {
            assert_eq!(targets(MULTI_EXECUTE, &bad), Err(Unreadable), "{bad}");
        }

        // Discovery and connection run no app tool.
        for meta in [SEARCH_TOOLS, GET_TOOL_SCHEMAS, MANAGE_CONNECTIONS] {
            assert_eq!(targets(meta, &json!({ "anything": 1 })).unwrap(), vec![]);
        }
    }

    #[test]
    fn the_longest_connected_prefix_names_the_app() {
        let known = vec!["bland".to_string(), "bland_ai".to_string(), "gmail".to_string()];
        assert_eq!(app_of("BLAND_AI_CALL", &known), "bland_ai");
        assert_eq!(app_of("BLAND_CALL", &known), "bland");
        assert_eq!(app_of("GMAIL_SEND_EMAIL", &known), "gmail");
        // Unknown: the first word.
        assert_eq!(app_of("ZENDESK_GET_TICKET", &known), "zendesk");
        // A prefix must end at a word boundary.
        assert_eq!(app_of("GMAILISH_THING", &known), "gmailish");
    }

    // ---- CAP-T-5: the verdict ----

    #[test]
    fn a_batch_is_as_strict_as_its_strictest_target_and_names_every_app() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "work"), ("a2", "slack", "default")]);
        let convo = conv(&db);
        let args = json!({ "tools": [
            { "tool_slug": "GMAIL_FETCH_EMAILS", "arguments": { "query": "invoice" } },
            { "tool_slug": "SLACK_SEND_MESSAGE", "arguments": { "channel": "#general", "text": "hi" } },
        ]});
        let p = plan(&db, &convo, &c.id, MULTI_EXECUTE, &args).unwrap();
        assert_eq!(p.kind, CallKind::Send);
        assert_eq!(p.parts.len(), 2);
        assert_eq!(p.parts[0], Part { app: "gmail".into(), name: "Gmail".into(), kind: CallKind::Read, sends: 0 });
        assert_eq!(p.parts[1], Part { app: "slack".into(), name: "Slack".into(), kind: CallKind::Send, sends: 1 });
        assert!(p.said.starts_with("I'd like to send something through Gmail and Slack:"), "{}", p.said);
        assert!(p.said.contains("send message \u{2192} #general"), "{}", p.said);
        assert!(p.said.contains("fetch emails \u{2192} invoice"), "{}", p.said);
    }

    #[test]
    fn a_persons_override_for_a_tool_applies_under_its_app() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "work")]);
        let mut policy = db.get_connector_policy(&c.id, "gmail").unwrap();
        policy.tool_overrides = Some(r#"{"GMAIL_FETCH_EMAILS":"change"}"#.into());
        db.save_connector_policy(&policy).unwrap();
        let args = json!({ "tools": [{ "tool_slug": "GMAIL_FETCH_EMAILS" }] });
        let p = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &args).unwrap();
        assert_eq!(p.kind, CallKind::Change);
    }

    #[test]
    fn sends_are_counted_per_target_and_per_app() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "work"), ("a2", "slack", "default")]);
        let tools: Vec<Value> = (0..10).map(|_| json!({ "tool_slug": "SLACK_SEND_MESSAGE" })).chain([json!({ "tool_slug": "GMAIL_SEND_EMAIL" })]).collect();
        let counts = send_counts(&db, &c.id, MULTI_EXECUTE, &json!({ "tools": tools }));
        assert_eq!(counts, vec![("slack".to_string(), 10), ("gmail".to_string(), 1)]);
    }

    #[test]
    fn an_unreadable_batch_is_refused_in_the_models_words() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let err = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &json!({ "tools": "nope" })).unwrap_err();
        assert_eq!(err, UNREADABLE);
    }

    #[test]
    fn a_gmail_target_with_a_mail_account_says_so() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "work")]);
        db.add_mail_account(&crate::db::NewMailAccount {
            label: "Me".into(),
            email: "me@example.com".into(),
            imap_host: "i".into(),
            imap_port: 993,
            smtp_host: "s".into(),
            smtp_port: 465,
            username: "me".into(),
            security: "tls".into(),
        })
        .unwrap();
        let args = json!({ "tools": [{ "tool_slug": "GMAIL_SEND_EMAIL", "arguments": { "to": "x@y.z" } }] });
        let p = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &args).unwrap();
        assert!(p.said.ends_with("I'd normally use your Mail account for this."), "{}", p.said);
    }

    // ---- CAP-T-7: connecting from the chat ----

    #[test]
    fn an_unconnected_app_gets_a_card_with_no_address_in_it() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "slack", "default")]);
        let req = connect_request(
            &db,
            &conv(&db),
            &c.id,
            false,
            &json!({ "toolkits": ["Gmail", "slack", "gmail", "../x"] }),
        )
        .unwrap();
        assert_eq!(req.cards.len(), 1, "gmail once; slack is already in; the bad slug is dropped");
        let card = &req.cards[0];
        assert_eq!(card.title, "Sign in to Gmail");
        assert_eq!(card.data["app"], "gmail");
        assert_eq!(card.data["state"], "waiting");
        let keys: Vec<&str> = card.data.as_object().unwrap().keys().map(String::as_str).collect();
        assert!(keys.iter().all(|k| ["app", "name", "label", "state"].contains(k)), "{keys:?}");
        assert!(!card.data.to_string().contains("http"), "no address is stored");
        assert_eq!(req.notes, vec!["Slack is already connected (default).".to_string()]);

        let (reply, waiting) = req.reply(&["Gmail".to_string()]);
        assert!(reply.starts_with("I've asked the person to sign in to Gmail. Stop here;"), "{reply}");
        assert!(reply.contains("Slack is already connected"), "{reply}");
        assert_eq!(waiting.as_deref(), Some("I need you to sign in to Gmail before I can go on."));
    }

    #[test]
    fn a_second_account_suggests_the_label_that_is_missing() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        db.upsert_app_account(&crate::db::AppAccount {
            id: "x".into(),
            connector_id: c.id.clone(),
            app: "gmail".into(),
            label: Some("personal".into()),
            status: "EXPIRED".into(),
            updated_at: 0,
        })
        .unwrap();
        let req = connect_request(&db, &conv(&db), &c.id, false, &json!({ "toolkits": ["gmail"] })).unwrap();
        assert_eq!(req.cards[0].data["label"], "work");
    }

    #[test]
    fn nobody_to_sign_in_means_no_card_and_a_note() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let req = connect_request(&db, &conv(&db), &c.id, true, &json!({ "toolkits": ["gmail"] })).unwrap();
        assert!(req.cards.is_empty());
        let (reply, waiting) = req.reply(&[]);
        assert!(reply.contains("nobody is here to sign in"), "{reply}");
        assert!(waiting.is_none(), "a headless run does not stop to wait");
    }

    #[test]
    fn a_persona_cannot_connect_an_app_it_is_not_granted() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let p = persona(&db);
        let convo = conv(&db);
        db.set_conversation_persona(&convo, Some(&p), None).unwrap();
        db.set_persona_app_grants(&p, &[grant(&c.id, "slack", "*")]).unwrap();
        let req = connect_request(&db, &convo, &c.id, false, &json!({ "toolkits": ["gmail", "slack"] })).unwrap();
        assert_eq!(req.cards.len(), 1);
        assert_eq!(req.cards[0].data["app"], "slack");
        assert_eq!(req.notes, vec!["Gmail isn't one I'm allowed to use here.".to_string()]);
    }

    #[test]
    fn naming_no_app_is_an_error_the_model_can_fix() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let err = connect_request(&db, &conv(&db), &c.id, false, &json!({})).unwrap_err();
        assert!(err.contains("toolkits"), "{err}");
    }

    #[test]
    fn a_step_line_names_the_app_and_what_was_done() {
        let batch = |slugs: &[&str]| json!({ "tools": slugs.iter().map(|s| json!({ "tool_slug": s })).collect::<Vec<_>>() });
        assert_eq!(
            describe(MULTI_EXECUTE, &json!({ "tools": [{ "tool_slug": "SLACK_SEND_MESSAGE", "arguments": { "channel": "#general" } }] })),
            ("sent".to_string(), "Slack: send message \u{2192} #general".to_string())
        );
        assert_eq!(
            describe(MULTI_EXECUTE, &batch(&["LINEAR_LIST_ISSUES", "LINEAR_LIST_PROJECTS", "LINEAR_GET_ISSUE", "NOTION_SEARCH_PAGES"])),
            ("read".to_string(), "Linear \u{d7}3, Notion \u{d7}1".to_string())
        );
        assert_eq!(describe(MULTI_EXECUTE, &batch(&["GOOGLECALENDAR_UPDATE_EVENT"])).0, "changed");
        assert_eq!(describe(MULTI_EXECUTE, &json!({})).1, "a connected app", "an unreadable batch still reads sensibly");
        assert_eq!(describe(MANAGE_CONNECTIONS, &json!({ "toolkits": ["gmail"] })).1, "Gmail");
    }

    #[test]
    fn a_long_batch_shows_five_and_counts_the_rest() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let tools: Vec<Value> = (0..8).map(|i| json!({ "tool_slug": "NOTION_UPDATE_PAGE", "arguments": { "title": format!("p{i}") } })).collect();
        let p = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &json!({ "tools": tools })).unwrap();
        assert!(p.said.ends_with("; and 3 more"), "{}", p.said);
    }

    // ---- Phase 6 (CAP-C) ----

    #[test]
    fn in_connect_mode_the_sentence_says_what_it_knows_and_never_that_an_app_is_missing() {
        let mut known = BTreeMap::new();
        known.insert("slack".to_string(), vec!["default".to_string()]);
        let text = description_prefix(&known, &Grants::Open, false, Mode::Connect);
        assert!(text.starts_with("The person's connected apps that I know of: Slack"), "{text}");
        assert!(text.contains("others may be connected in Composio too"), "{text}");
        assert!(!text.contains("isn't one"), "never claims an app is not connected: {text}");
        let none = description_prefix(&BTreeMap::new(), &Grants::Open, false, Mode::Connect);
        assert!(none.contains("that I know of: none yet (others may be connected"), "{none}");
        // Project mode keeps its sentence exactly.
        assert!(description_prefix(&BTreeMap::new(), &Grants::Open, false, Mode::Project)
            .starts_with("The person's connected apps: none yet."));
    }

    /// `CAP-C-F7`: a Composio meta-tool inside a batch is refused outright.
    #[test]
    fn a_composio_meta_tool_inside_a_batch_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        for slug in ["COMPOSIO_REMOTE_BASH_TOOL", "composio_remote_workbench", "COMPOSIO_MANAGE_CONNECTIONS"] {
            let batch = json!({ "tools": [{ "tool_slug": slug, "arguments": { "command": "ls" } }] });
            let err = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &batch).unwrap_err();
            assert!(err.contains("isn't an app tool"), "{slug}: {err}");
        }
        // The same inside a mixed batch.
        let mixed = json!({ "tools": [
            { "tool_slug": "GMAIL_FETCH_EMAILS", "arguments": {} },
            { "tool_slug": "COMPOSIO_REMOTE_BASH_TOOL", "arguments": {} },
        ] });
        assert!(plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &mixed).is_err());
    }

    /// `CAP-C-T4`: in Connect mode the apps that need a card are sent to Composio.
    /// An app already signed in gets no card, one it cannot start gets a note, and
    /// nothing the model, a message or a block holds carries an address.
    #[tokio::test]
    async fn connect_mode_cards_follow_what_composio_says_and_carry_no_address() {
        use crate::connectors::composio::{connect_client, start_sign_ins, tests_pretend};
        use crate::connectors::stub::{connect_stub, connections_answer, Reply};
        let link = "https://connect.composio.dev/link/ln_secret99";
        let stub = connect_stub(move |tool, _| {
            assert_eq!(tool, MANAGE_CONNECTIONS);
            Reply::Text(connections_answer(json!({
                "gmail": { "status": "INITIATED", "redirect_url": link },
                "slack": { "status": "ACTIVE" },
                "notion": { "message": "no idea" },
            })))
        })
        .await;
        tests_pretend::key(true);
        tests_pretend::mode(Mode::Connect);
        tests_pretend::connect_url(Some(&stub.url));

        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let mut request =
            connect_request(&db, &conv(&db), &c.id, false, &json!({ "toolkits": ["gmail", "slack", "notion"] })).unwrap();
        assert_eq!(request.cards.len(), 3, "nothing is known to be connected yet");

        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let apps: Vec<String> = ["gmail", "slack", "notion"].map(String::from).to_vec();
        request.settle(start_sign_ins(&db, &mut client, &apps).await);

        assert_eq!(request.cards.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Gmail"]);
        assert!(request.notes.contains(&"Slack is already connected.".to_string()), "{:?}", request.notes);
        assert!(request.notes.contains(&"Composio couldn't start the sign-in for Notion.".to_string()), "{:?}", request.notes);
        let (reply, waiting) = request.reply(&["Gmail".to_string()]);
        let everything = format!(
            "{reply} {waiting:?} {} {:?}",
            request.cards[0].data,
            db.list_app_accounts(&c.id).unwrap()
        );
        assert!(!everything.contains("composio.dev") && !everything.contains("ln_secret99"), "{everything}");
        assert_eq!(request.cards[0].data["label"], Value::Null, "Connect has no labels");
        let held = db.list_app_accounts(&c.id).unwrap();
        let status = |app: &str| held.iter().find(|a| a.app == app).map(|a| a.status.clone());
        assert_eq!(status("gmail").as_deref(), Some("INITIATED"));
        assert_eq!(status("slack").as_deref(), Some("ACTIVE"));
        assert_eq!(status("notion"), None);

        // A refusal from Composio leaves no card and says so, in plain words.
        *stub.refuse.lock().unwrap() = Some(401);
        let mut refused = connect_request(&db, &conv(&db), &c.id, false, &json!({ "toolkits": ["linear"] })).unwrap();
        let mut client = connect_client(&http, "ck_bad");
        let outcome = start_sign_ins(&db, &mut client, &["linear".to_string()]).await;
        refused.settle(outcome);
        assert!(refused.cards.is_empty());
        assert!(refused.notes[0].contains("couldn't start the sign-in") && refused.notes[0].contains("ck_"), "{:?}", refused.notes);

        tests_pretend::key(false);
        tests_pretend::mode(Mode::Project);
        tests_pretend::connect_url(None);
    }

    #[test]
    fn an_app_connected_without_labels_is_said_without_brackets() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let row_ = db.composio_connector().unwrap().unwrap();
        db.upsert_app_account(&crate::db::AppAccount {
            id: "connect:slack".into(),
            connector_id: row_.id.clone(),
            app: "slack".into(),
            label: None,
            status: "ACTIVE".into(),
            updated_at: 1,
        })
        .unwrap();
        let request = connect_request(&db, &conv(&db), &c.id, false, &json!({ "toolkits": ["slack"] })).unwrap();
        assert_eq!(request.notes, ["Slack is already connected."]);
    }

    /// What the model is really shown (recorded 2026-10-19): `toolkits` is a list
    /// of `{name, action}`. `add` asks for a card; `list` is answered in words;
    /// `remove` and `rename` are declined and never reach Composio.
    #[test]
    fn composios_own_toolkit_shape_is_read_and_only_add_asks_for_a_card() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "slack", "default")]);
        let args = json!({ "toolkits": [
            { "name": "gmail" },
            { "name": "Slack", "action": "list" },
            { "name": "notion", "action": "list" },
            { "name": "linear", "action": "remove", "account_id": "linear_x" },
            { "name": "gmail", "action": "add", "alias": "work" },
        ] });
        let req = connect_request(&db, &conv(&db), &c.id, false, &args).unwrap();
        assert_eq!(req.cards.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Gmail"], "one card for gmail");
        assert!(req.notes.contains(&"Slack is connected (default).".to_string()), "{:?}", req.notes);
        assert!(req.notes.contains(&"Notion isn't connected.".to_string()), "{:?}", req.notes);
        assert!(req.notes.iter().any(|n| n.contains("I don't remove or rename") && n.contains("Linear")), "{:?}", req.notes);

        assert_eq!(describe(MANAGE_CONNECTIONS, &args), ("asked you to sign in to".to_string(), "Gmail, Slack, Notion, Linear".to_string()));
        let only_list = json!({ "toolkits": [{ "name": "slack", "action": "list" }] });
        assert_eq!(describe(MANAGE_CONNECTIONS, &only_list).0, "checked my sign-ins for");
        let nameless = json!({ "toolkits": [{ "action": "add" }] });
        assert!(connect_request(&db, &conv(&db), &c.id, false, &nameless).is_err());
    }

    /// In Connect mode the mirror only learns, so an app it holds as connected may
    /// have been taken away in Composio. Someone watching gets a card, and Composio
    /// decides; a run nobody watches is told what Poiesis knows.
    #[test]
    fn in_connect_mode_a_connected_app_is_asked_about_again_when_someone_is_watching() {
        use crate::connectors::composio::tests_pretend;
        tests_pretend::key(true);
        tests_pretend::mode(Mode::Connect);
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("connect:slack", "slack", "x")]);
        let watched = connect_request(&db, &conv(&db), &c.id, false, &json!({ "toolkits": [{ "name": "slack" }] })).unwrap();
        assert_eq!(watched.cards.len(), 1, "Composio is asked, not the mirror");
        let headless = connect_request(&db, &conv(&db), &c.id, true, &json!({ "toolkits": ["slack"] })).unwrap();
        assert!(headless.cards.is_empty());
        assert!(headless.notes[0].starts_with("Slack is already connected"), "{:?}", headless.notes);
        tests_pretend::key(false);
        tests_pretend::mode(Mode::Project);
    }

    /// A third of the catalog's slugs have an underscore. Each is its own app, so
    /// "always" for Google Maps is not "always" for every Google app.
    #[test]
    fn apps_with_an_underscore_in_their_slug_are_named_by_the_whole_slug() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let args = json!({ "tools": [
            { "tool_slug": "GOOGLE_MAPS_TEXT_SEARCH", "arguments": { "query": "cafe" } },
            { "tool_slug": "GOOGLE_ANALYTICS_RUN_REPORT", "arguments": {} },
        ] });
        let p = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &args).unwrap();
        let apps: Vec<&str> = p.parts.iter().map(|p| p.app.as_str()).collect();
        assert_eq!(apps, ["google_maps", "google_analytics"]);
        assert!(p.said.contains("Google Maps and Google Analytics"), "{}", p.said);
        assert_eq!(
            describe(MULTI_EXECUTE, &json!({ "tools": [{ "tool_slug": "GOOGLE_MAPS_TEXT_SEARCH", "arguments": {} }] })).1,
            "Google Maps: text search"
        );
    }

    /// The step line keeps the words of a name that puts its noun first.
    #[test]
    fn a_name_with_its_noun_first_reads_whole() {
        let one = |slug: &str| describe(MULTI_EXECUTE, &json!({ "tools": [{ "tool_slug": slug, "arguments": {} }] })).1;
        assert_eq!(one("GOOGLECALENDAR_EVENTS_LIST"), "Google Calendar: events list");
        assert_eq!(one("GOOGLECALENDAR_EVENTS_LIST_ALL_CALENDARS"), "Google Calendar: events list all calendars");
        assert_eq!(one("GMAIL_FETCH_EMAILS"), "Gmail: fetch emails");
        assert_eq!(action_of("ZENDESK_GET_TICKET", ""), "get ticket", "no app: from the first verb, as before");
    }

    /// Composio takes the account beside the arguments; the panel names it.
    #[test]
    fn the_account_a_batch_names_is_shown() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let args = json!({ "tools": [{ "tool_slug": "GMAIL_SEND_EMAIL", "account": "work", "arguments": { "to": "ana@example.com" } }] });
        assert_eq!(targets(MULTI_EXECUTE, &args).unwrap()[0].account.as_deref(), Some("work"));
        let p = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &args).unwrap();
        assert!(p.said.contains("send email (work) \u{2192} ana@example.com"), "{}", p.said);
    }

    /// `CAP-C-F6`: the panel lists what each target would send, not a wrapper.
    #[test]
    fn the_panel_is_shown_each_targets_own_arguments() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let one = json!({ "tools": [{ "tool_slug": "SLACK_SEND_MESSAGE", "arguments": { "channel": "#general", "text": "Release notes are out." } }] });
        let p = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &one).unwrap();
        assert_eq!(p.shown, json!({ "channel": "#general", "text": "Release notes are out." }));
        let two = json!({ "tools": [
            { "tool_slug": "SLACK_SEND_MESSAGE", "arguments": { "text": "hi" } },
            { "tool_slug": "GMAIL_SEND_EMAIL", "arguments": { "to": "ana@example.com" } },
            { "tool_slug": "NOTION_FETCH_PAGE", "arguments": {} },
        ] });
        let p = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &two).unwrap();
        let rows = p.shown.as_object().unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.keys().any(|k| k.starts_with("1. Slack ") && k.ends_with("· text")), "{rows:?}");
        assert!(rows.keys().any(|k| k.starts_with("2. Gmail ") && k.ends_with("· to")), "{rows:?}");
        assert!(rows.values().any(|v| v == "(no arguments)"));
    }

    // ---- several accounts of one app (CAP-C6) ----

    /// Composio refuses a call to an app with several sign-ins that names none.
    /// Poiesis knows them, so the model is told before anything is asked or sent,
    /// with the names it can pick from.
    #[test]
    fn a_call_to_an_app_with_two_accounts_must_name_one() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "work"), ("a2", "gmail", "personal"), ("a3", "slack", "default")]);
        let batch = |entry: Value| json!({ "tools": [entry] });
        let unnamed = batch(json!({ "tool_slug": "GMAIL_FETCH_EMAILS", "arguments": {} }));
        let err = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &unnamed).unwrap_err();
        assert!(err.starts_with("Gmail has several accounts here: personal, work."), "{err}");
        assert!(err.contains("`account`") && err.contains("ask them"), "{err}");

        let named = batch(json!({ "tool_slug": "GMAIL_FETCH_EMAILS", "arguments": {}, "account": "work" }));
        assert!(plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &named).is_ok());
        let inside = batch(json!({ "tool_slug": "GMAIL_FETCH_EMAILS", "arguments": { "account": "personal" } }));
        assert!(plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &inside).is_ok(), "an account inside the arguments counts");
        let one_account = batch(json!({ "tool_slug": "SLACK_LIST_CHANNELS", "arguments": {} }));
        assert!(plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &one_account).is_ok(), "one sign-in needs no name");
    }

    /// An account Composio holds without a name is offered by its id, which
    /// `account` takes as well.
    #[test]
    fn an_unnamed_account_is_offered_by_composios_id() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        let now = crate::db::now_ms();
        for (id, label) in [("connect:gmail:gmail_vole-negate", None), ("connect:gmail:gmail_livor-fultz", Some("work"))] {
            db.upsert_app_account(&crate::db::AppAccount {
                id: id.into(),
                connector_id: c.id.clone(),
                app: "gmail".into(),
                label: label.map(str::to_string),
                status: "ACTIVE".into(),
                updated_at: now,
            })
            .unwrap();
        }
        let d = offered(&db, &conv(&db), &c, Some(Mode::Connect))[0].description.clone();
        assert!(d.contains("Gmail (gmail_vole-negate, work)") || d.contains("Gmail (work, gmail_vole-negate)"), "{d}");
        assert!(d.contains("say which in each tool's `account`"), "{d}");
    }

    #[test]
    fn the_several_accounts_sentence_is_said_only_when_it_applies() {
        let one: BTreeMap<String, Vec<String>> = [("gmail".to_string(), vec!["work".to_string()])].into();
        assert!(!description_prefix(&one, &Grants::Open, false, Mode::Connect).contains("several accounts"));
        let two: BTreeMap<String, Vec<String>> = [("gmail".to_string(), vec!["work".into(), "home".into()])].into();
        assert!(description_prefix(&two, &Grants::Open, false, Mode::Connect).contains("several accounts"));
        // An app the persona may not use is not named, and neither is its sentence.
        let narrowed = Grants::Only(vec![grant("c", "slack", "*")]);
        assert!(!description_prefix(&two, &narrowed, false, Mode::Connect).contains("several accounts"));
    }

    // ---- correcting a Composio tool's kind (MCG-7) ----

    #[test]
    fn an_apps_tools_are_the_ones_that_ran_and_the_ones_given_a_kind() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[("a1", "gmail", "default")]);
        for _ in 0..3 {
            db.log_app_call("gmail", "read", "GMAIL_FETCH_EMAILS").unwrap();
        }
        db.log_app_call("gmail", "send", "GMAIL_SEND_EMAIL").unwrap();
        db.log_app_call("slack", "send", "SLACK_SEND_MESSAGE").unwrap();
        crate::commands::connectors::set_tool_kind(&db, &c.id, "gmail", "GMAIL_CREATE_EMAIL_DRAFT", Some("send")).unwrap();
        crate::commands::connectors::set_tool_kind(&db, &c.id, "gmail", "GMAIL_FETCH_EMAILS", Some("change")).unwrap();

        let got = known_tools(&db, &c.id, "gmail");
        let slugs: Vec<&str> = got.iter().map(|t| t.slug.as_str()).collect();
        assert_eq!(slugs, ["GMAIL_FETCH_EMAILS", "GMAIL_SEND_EMAIL", "GMAIL_CREATE_EMAIL_DRAFT"], "busiest first, then set ones");
        assert_eq!((got[0].kind, got[0].auto, got[0].calls), (CallKind::Change, CallKind::Read, 3));
        assert_eq!((got[2].kind, got[2].auto, got[2].calls), (CallKind::Send, CallKind::Change, 0));
        assert!(!slugs.contains(&"SLACK_SEND_MESSAGE"), "only this app's tools");

        // What is set for one app is what the gate then holds that app's calls to.
        let args = json!({ "tools": [{ "tool_slug": "GMAIL_FETCH_EMAILS", "arguments": {} }] });
        assert_eq!(plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &args).unwrap().kind, CallKind::Change);
        // Going back to the name drops the override.
        crate::commands::connectors::set_tool_kind(&db, &c.id, "gmail", "GMAIL_FETCH_EMAILS", None).unwrap();
        assert_eq!(plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &args).unwrap().kind, CallKind::Read);
        assert!(crate::commands::connectors::set_tool_kind(&db, &c.id, "../x", "T", Some("read")).is_err());
    }

    #[test]
    fn the_panel_is_offered_each_tool_once_with_its_kind() {
        let db = Db::open_in_memory().unwrap();
        let c = row(&db, &[]);
        crate::commands::connectors::set_tool_kind(&db, &c.id, "slack", "SLACK_LIST_CHANNELS", Some("send")).unwrap();
        let args = json!({ "tools": [
            { "tool_slug": "SLACK_LIST_CHANNELS", "arguments": {} },
            { "tool_slug": "SLACK_LIST_CHANNELS", "arguments": { "limit": 5 } },
            { "tool_slug": "GMAIL_SEND_EMAIL", "arguments": {} },
        ] });
        let p = plan(&db, &conv(&db), &c.id, MULTI_EXECUTE, &args).unwrap();
        let tools = panel_tools(&c.id, &p.resolved);
        assert_eq!(tools.len(), 2);
        assert_eq!((tools[0].name.as_str(), tools[0].app.as_str(), tools[0].app_name.as_str()), ("SLACK_LIST_CHANNELS", "slack", "Slack"));
        assert_eq!((tools[0].kind.as_str(), tools[0].auto_kind.as_str(), tools[0].overridden), ("send", "read", true));
        assert_eq!((tools[1].kind.as_str(), tools[1].overridden), ("send", false));
        assert!(tools.iter().all(|t| t.connector_id == c.id));
    }
}
