//! `SEE-2`: what the quick decisions understood before the slow model's first
//! word, said in the user's terms. The tool-menu decision and the handover
//! pre-match settle first; this holds what they found, in words, so the strip above
//! the run's text can show it (`ThoughtStrip.tsx`) and the receipt at its foot can
//! count it.
//!
//! The rule (`SEE-0`): what I will do or did, about the user's task. Never a count
//! of things not done, never a unit name the user did not choose, never a bare
//! probability. A unit the user never turned on is not mentioned as left out.

use serde::Serialize;

use super::tools::{Held, Unit};
use super::words::PROBABLY_FROM;
use super::{Answer, Verdict};

/// One place the message needs to look, in words: `on the web`.
pub type Place = String;

/// A tool group left off the menu that the user had turned on, with the words for
/// it and for the click that puts it back.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LeftOut {
    pub id: String,
    /// `Not searching the web`.
    pub words: String,
    /// `Search the web too`.
    pub action: String,
}

/// What one held-back group would have cost the prompt, measured from its specs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeldTokens {
    pub id: String,
    pub tokens: u32,
}

/// A handover rule the message matched.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Handing {
    /// `Claude Code`.
    pub agent: String,
    /// `U2`.
    pub rule: String,
}

/// Everything the strip says about one turn. Sent whole on every change, like the
/// plan, so a strip rebuilt from it cannot drift.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Thought {
    /// Where I will look, for each group the message needs.
    pub looking: Vec<Place>,
    /// The menu was asked about and the message needs nothing off it.
    pub direct: bool,
    /// Groups left off that the user had switched on.
    pub left_out: Vec<LeftOut>,
    /// Every group left off, with what its specs would have cost.
    pub held: Vec<HeldTokens>,
    pub handover: Option<Handing>,
    /// How long the decisions took, together.
    pub ms: u32,
    /// The hot path found no engine to ask: `cold` (installed, not running yet),
    /// `cpu` (running on the processor, which a turn does not wait for) or
    /// `unavailable` (no decision model is set up, here or in the cloud).
    pub engine: Option<String>,
}

impl Thought {
    /// Whether the strip has anything to say.
    pub fn is_empty(&self) -> bool {
        self.looking.is_empty()
            && !self.direct
            && self.left_out.is_empty()
            && self.handover.is_none()
            && self.engine.is_none()
    }
}

/// A group is one the user chose when they turned it on or connected it themselves.
/// The loop's own tools and the toolsets that are on by default are not theirs to
/// hear about.
fn user_chose(id: &str) -> bool {
    matches!(id, "web_search" | "indexing" | "apps") || id.starts_with("mcp:")
}

/// `Looking in Google Calendar`'s second half. `apps` is the granted app names, as
/// the menu decision read them: a single app is named, several are "your connected
/// apps".
pub fn place(unit: &Unit, apps: &str) -> Option<Place> {
    match unit.id.as_str() {
        "web_search" => Some("on the web".into()),
        "indexing" => Some("in your files".into()),
        "apps" => {
            let names: Vec<&str> = apps.split(',').map(str::trim).filter(|a| !a.is_empty()).collect();
            Some(match names.as_slice() {
                [one] => format!("in {one}"),
                _ => "in your connected apps".into(),
            })
        }
        id if id.starts_with("mcp:") => Some(format!("in {}", unit.label)),
        _ => None,
    }
}

/// What the strip says of a group left off, and the click that puts it back. `None`
/// for a group the user never chose.
pub fn left_out(unit: &Unit) -> Option<LeftOut> {
    if !user_chose(&unit.id) {
        return None;
    }
    let (words, action) = match unit.id.as_str() {
        "web_search" => ("Not searching the web".to_string(), "Search the web too".to_string()),
        "indexing" => ("Not searching your files".to_string(), "Search your files too".to_string()),
        "apps" => ("Not using your connected apps".to_string(), "Use your connected apps too".to_string()),
        _ => (format!("Not using {}", unit.label), format!("Use {} too", unit.label)),
    };
    Some(LeftOut { id: unit.id.clone(), words, action })
}

/// What the receipt says the held groups were: `web search, folder reading`.
pub fn held_names(held: &[Held]) -> String {
    let names: Vec<String> = held.iter().map(|h| h.unit.label.to_lowercase()).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The note on the tool-menu decision, for the receipt.
pub fn menu_note(held: &[Held]) -> String {
    format!("Left out {}; you did not ask for {}", held_names(held), if held.len() == 1 { "it" } else { "them" })
}

/// Read the menu decision into the strip: where the message needs to look among the
/// groups still on the menu, and what was left out. `kept` is every group asked
/// about, `held` the ones the decision left off, `apps` the granted app names.
pub fn from_menu(kept: &[Unit], held: &[Held], verdict: &Verdict, apps: &str) -> Thought {
    let held_ids: Vec<&str> = held.iter().map(|h| h.unit.id.as_str()).collect();
    let needed = |u: &Unit| {
        !held_ids.contains(&u.id.as_str())
            && matches!(verdict.get(&u.id), Some(Answer::Noul { p_true }) if *p_true >= PROBABLY_FROM)
    };
    let looking: Vec<Place> = kept.iter().filter(|u| needed(u)).filter_map(|u| place(u, apps)).collect();
    let asked = kept.iter().any(|u| matches!(verdict.get(&u.id), Some(Answer::Noul { .. })));
    // "Answering directly" only when the decision looked at the menu and the message
    // needs nothing off it at all.
    let anything_needed = kept.iter().any(needed);
    Thought {
        direct: asked && !anything_needed,
        looking,
        left_out: held.iter().filter_map(|h| left_out(&h.unit)).collect(),
        ms: verdict.ms,
        ..Default::default()
    }
}

/// Tokens a group's specs cost, measured: four characters to the token, the rule
/// `run.rs` uses for the meter.
pub fn spec_tokens(specs: &[serde_json::Value], tools: &[String]) -> u32 {
    let chars: usize = specs
        .iter()
        .filter(|s| s.pointer("/function/name").and_then(|n| n.as_str()).is_some_and(|n| tools.iter().any(|t| t == n)))
        .map(|s| s.to_string().len())
        .sum();
    chars.div_ceil(4) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::decide::BackendId;
    use std::collections::BTreeMap;

    fn unit(id: &str, label: &str) -> Unit {
        Unit { id: id.into(), label: label.into(), about: String::new(), tools: vec![format!("{id}_tool")] }
    }

    fn verdict(ps: &[(&str, f32)]) -> Verdict {
        Verdict {
            id: "d".into(),
            answers: ps.iter().map(|(k, p)| ((*k).to_string(), Answer::Noul { p_true: *p })).collect::<BTreeMap<_, _>>(),
            backend: BackendId::Local { model: "m".into() },
            calibrated: false,
            ms: 38,
        }
    }

    fn held(u: &Unit, p: f32) -> Held {
        Held { unit: u.clone(), p_needed: p }
    }

    /// `SEE-T-5`: a unit the message needs is a place to look, in words.
    #[test]
    fn the_places_are_the_units_the_message_needs_and_that_stayed_on_the_menu() {
        let web = unit("web_search", "Web search");
        let files = unit("indexing", "Folder reading");
        let apps = unit("apps", "Connected apps");
        let t = from_menu(&[web.clone(), files.clone(), apps.clone()], &[], &verdict(&[("web_search", 0.97), ("indexing", 0.7), ("apps", 0.1)]), "Google Calendar");
        assert_eq!(t.looking, ["on the web", "in your files"]);
        assert!(!t.direct);
        assert_eq!(t.ms, 38);
        let t = from_menu(&[apps.clone()], &[], &verdict(&[("apps", 0.9)]), "Google Calendar");
        assert_eq!(t.looking, ["in Google Calendar"]);
        let t = from_menu(&[apps], &[], &verdict(&[("apps", 0.9)]), "Google Calendar, Gmail");
        assert_eq!(t.looking, ["in your connected apps"]);
    }

    #[test]
    fn nothing_needed_is_answering_directly_and_only_when_the_menu_was_asked() {
        let web = unit("web_search", "Web search");
        let t = from_menu(&[web.clone()], &[held(&web, 0.02)], &verdict(&[("web_search", 0.02)]), "");
        assert!(t.direct && t.looking.is_empty());
        let t = from_menu(&[web.clone()], &[], &verdict(&[]), "");
        assert!(!t.direct, "no answer, nothing to say");
        let t = from_menu(&[], &[], &verdict(&[]), "");
        assert!(!t.direct && t.is_empty());
    }

    /// `SEE-T-5`: only a unit the user turned on is mentioned as left out.
    #[test]
    fn a_unit_the_user_never_chose_is_not_mentioned() {
        let web = unit("web_search", "Web search");
        let code = unit("code_exec", "Code execution");
        let agents = unit("subagents", "Other agents");
        let conn = unit("mcp:abc", "Notion");
        let all = [web.clone(), code.clone(), agents.clone(), conn.clone()];
        let h = [held(&web, 0.02), held(&code, 0.01), held(&agents, 0.03), held(&conn, 0.04)];
        let t = from_menu(&all, &h, &verdict(&[("web_search", 0.02), ("code_exec", 0.01), ("subagents", 0.03), ("mcp:abc", 0.04)]), "");
        let words: Vec<&str> = t.left_out.iter().map(|l| l.words.as_str()).collect();
        assert_eq!(words, ["Not searching the web", "Not using Notion"]);
        let actions: Vec<&str> = t.left_out.iter().map(|l| l.action.as_str()).collect();
        assert_eq!(actions, ["Search the web too", "Use Notion too"]);
        assert_eq!(t.left_out[0].id, "web_search", "the click names the unit");
    }

    #[test]
    fn the_receipt_names_what_was_left_out() {
        let web = unit("web_search", "Web search");
        let mail = unit("mail", "Mail");
        let code = unit("code_exec", "Code execution");
        assert_eq!(menu_note(&[held(&web, 0.0)]), "Left out web search; you did not ask for it");
        assert_eq!(
            menu_note(&[held(&web, 0.0), held(&mail, 0.0), held(&code, 0.0)]),
            "Left out web search, mail and code execution; you did not ask for them"
        );
    }

    #[test]
    fn what_the_held_specs_cost_is_measured_from_them() {
        let specs = vec![
            serde_json::json!({ "type": "function", "function": { "name": "web_search", "description": "x".repeat(396) } }),
            serde_json::json!({ "type": "function", "function": { "name": "read_file", "description": "y".repeat(100) } }),
        ];
        let tokens = spec_tokens(&specs, &["web_search".to_string()]);
        assert!(tokens > 90 && tokens < 130, "{tokens}");
        assert_eq!(spec_tokens(&specs, &["nope".to_string()]), 0);
    }

    #[test]
    fn a_thought_serialises_for_the_screen() {
        let mut t = Thought::default();
        assert!(t.is_empty());
        t.handover = Some(Handing { agent: "Claude Code".into(), rule: "U2".into() });
        assert!(!t.is_empty());
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["handover"], serde_json::json!({ "agent": "Claude Code", "rule": "U2" }));
        assert_eq!(json["looking"], serde_json::json!([]));
        assert!(json["engine"].is_null());
    }
}
