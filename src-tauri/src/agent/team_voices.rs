//! `TEAM-18`: unasked replies (Part I.8). In a session a team leads, after a reply, one cheap call asks
//! whether one of the other personas would add something the person needs, and who. The answer fails
//! closed: anything that is not a clear name from the short list is nobody.
//!
//! Three things bound it, all read from what is already saved, so nothing is kept in memory:
//! - **Lines.** Only a persona that may talk with the one who just spoke is asked about: the person has a
//!   `talks` line to both, or the team draws an `asks` line between them. A member with no line is never
//!   a candidate, so no unasked reply happens without one.
//! - **The turn rule.** At most one unasked reply per message of the person, so never more than two
//!   persona replies in a row without the person. (The plan also named three in a row and the team's
//!   `rounds`; with one unasked reply per message neither can ever be reached, so neither is checked.)
//! - **The coordinator is never asked about.** It runs the team's task; a persona that chimes in speaks
//!   as itself (`team::voice`), like a member the person spoke to.
//!
//! This file is what needs no run to test: who may chime in, whether the rule allows another voice,
//! what the call is shown and how its answer is read. The call itself is a closure given by the caller.

use std::future::Future;

use serde_json::{json, Value};

use crate::agent::team::{self, Addressable, YOU};
use crate::db::{Db, Message};
use crate::memory::ways::{LineKind, Team};
use crate::memory::MemoryStore;

/// What the persona that chimes in is told in place of a message from the person (never saved).
pub const NUDGE: &str = "Nobody asked you, but you may add one thing the person needs that has not been said yet. Keep it short and do not repeat what was said.";

/// What the call is shown of each earlier message, and of how many.
const SHOWN_CHARS: usize = 600;
const SHOWN_MESSAGES: usize = 4;

/// The replies since the person's last message, oldest first (finished ones only).
fn replies_since_the_person(messages: &[Message]) -> Vec<&Message> {
    let from = messages.iter().rposition(|m| m.role == "user").map_or(0, |i| i + 1);
    messages[from..].iter().filter(|m| m.role == "assistant" && !m.content.trim().is_empty()).collect()
}

/// May another voice speak after these messages? `Err` says why not (for the log, never shown).
pub fn turn_rule(messages: &[Message]) -> Result<(), &'static str> {
    if messages.last().is_none_or(|m| m.role != "assistant" || m.content.trim().is_empty()) {
        return Err("the last reply is not finished");
    }
    // The first reply answers the person; every one after it was not asked for.
    match replies_since_the_person(messages).len() {
        0 => Err("nobody has answered the person yet"),
        1 => Ok(()),
        _ => Err("one unasked reply for each message of the person"),
    }
}

/// The role who gave a reply, from the leader string saved on it (`persona:<id>`, `poiesis`). A reply
/// of the chat's own leader is saved as `team:<slug>`: that is the coordinator answering for the team.
pub fn role_of(team: &Team, author: &str) -> Option<String> {
    if team.coordinator == author || author.strip_prefix("team:") == Some(team.slug.as_str()) {
        return Some("coordinator".to_string());
    }
    team.members.iter().find(|m| m.persona == author).map(|m| m.role.clone())
}

/// Do these two talk to each other? The person has a `talks` line to both, or the team draws an `asks`
/// line between them (Part I.8).
pub fn may_chime(team: &Team, speaker: &str, other: &str) -> bool {
    let talks = |role: &str| team.lines.iter().any(|l| l.kind == LineKind::Talks && l.from == YOU && l.to == role);
    (talks(speaker) && talks(other))
        || team.lines.iter().any(|l| l.kind == LineKind::Asks && ((l.from == speaker && l.to == other) || (l.from == other && l.to == speaker)))
}

/// A persona that may speak up after `speaker`: its role, its name and what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub role: String,
    pub name: String,
    pub job: String,
}

pub fn candidates(db: &Db, team: &Team, speaker: &str) -> Vec<Candidate> {
    team.members
        .iter()
        .filter(|m| m.role != speaker && may_chime(team, speaker, &m.role))
        .filter_map(|m| {
            let name = team::member_named(db, team, &m.role)?.persona?.name;
            Some(Candidate { role: m.role.clone(), name, job: m.job.clone() })
        })
        .collect()
}

fn clip(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max { s } else { format!("{}…", s.chars().take(max).collect::<String>()) }
}

/// The whole of what the call is shown: the latest of the conversation, who may speak up, and the one
/// question, answered in one fixed line.
pub fn choose_messages(messages: &[Message], speaker_name: &str, who: &[Candidate]) -> Vec<Value> {
    let mut seen = String::new();
    for m in messages.iter().rev().take(SHOWN_MESSAGES).rev() {
        let by = if m.role == "user" { "The person".to_string() } else { speaker_name.to_string() };
        seen.push_str(&format!("{by}: {}\n", clip(&m.content, SHOWN_CHARS)));
    }
    let list: String = who
        .iter()
        .map(|c| format!("- {} ({}){}\n", c.role, c.name, if c.job.trim().is_empty() { String::new() } else { format!(": {}", clip(&c.job, 120)) }))
        .collect();
    vec![
        json!({ "role": "system", "content": "You decide whether anyone else in a group chat should speak. Most of the time nobody should: speak up only when a person in the list would add something the person asking needs that has not been said, and not to agree or repeat. Reply with exactly one line and nothing else: SPEAKER: followed by one role from the list, or SPEAKER: nobody." }),
        json!({ "role": "user", "content": format!("The conversation so far:\n{seen}\nWho could speak next:\n{list}\nWould one of them add something the person needs? Who, or nobody.") }),
    ]
}

/// Who the reply names, when it clearly names one of `who`. Anything else, including a reply that cannot
/// be read, is nobody: this fails closed.
pub fn parse_choice(reply: &str, who: &[Candidate]) -> Option<String> {
    let line = reply.lines().find_map(|l| {
        let l = l.trim().trim_start_matches(['*', '-', ' ']);
        l.get(..8).filter(|h| h.eq_ignore_ascii_case("speaker:")).map(|_| l[8..].to_string())
    })?;
    let said = line.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_').to_ascii_lowercase();
    who.iter().find(|c| c.role.to_ascii_lowercase() == said || c.name.to_ascii_lowercase() == said).map(|c| c.role.clone())
}

/// Who speaks next, if anyone, and what they are told. The rules come first, so a chat they close, or
/// one where nobody may chime in, never spends a call.
pub async fn next_voice<F, Fut>(db: &Db, mem: &MemoryStore, session: &str, ask: F) -> Option<(Addressable, &'static str)>
where
    F: FnOnce(Vec<Value>) -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let team = team::of_session(db, mem, session)?;
    let messages = db.list_messages(session).ok()?;
    turn_rule(&messages).ok()?;
    let last = messages.iter().rev().find(|m| m.role == "assistant")?;
    let speaker = role_of(&team, last.author.as_deref()?)?;
    let who = candidates(db, &team, &speaker);
    if who.is_empty() {
        return None;
    }
    let speaker_name = team::member_named(db, &team, &speaker).and_then(|a| a.persona).map_or_else(|| "The coordinator".to_string(), |p| p.name);
    let reply = ask(choose_messages(&messages, &speaker_name, &who)).await?;
    let role = parse_choice(&reply, &who)?;
    Some((team::voice(db, mem, session, &role)?, NUDGE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::ways::{Limits, Line, Member};

    fn msg(role: &str, text: &str, author: Option<&str>) -> Message {
        Message {
            id: format!("m-{text}"),
            conversation_id: "c".into(),
            role: role.into(),
            content: text.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            stop_reason: None,
            plan_json: None,
            spoken: false,
            author: author.map(str::to_string),
            addressed: None,
            created_at: 0,
            attachments: Vec::new(),
        }
    }

    fn team_of(lines: Vec<Line>) -> Team {
        let mut t = Team::blank();
        t.slug = "open".into();
        t.name = "Open team".into();
        t.coordinator = "persona:mira".into();
        t.limits = Limits { rounds: 3, ..Limits::default() };
        let member = |role: &str, persona: &str| Member { role: role.into(), persona: persona.into(), job: format!("does {role}"), owns: vec![] };
        t.members = vec![member("kai", "persona:kai"), member("juno", "persona:juno"), member("docs", "helper")];
        t.lines = lines;
        t
    }

    fn talks(to: &str) -> Line {
        Line::new(LineKind::Talks, YOU, to)
    }

    #[test]
    fn the_rule_lets_one_unasked_reply_follow_the_first_answer_and_no_more() {
        let asked = [msg("user", "q", None), msg("assistant", "a1", Some("persona:kai"))];
        assert!(turn_rule(&asked).is_ok());
        let after_one = [asked[0].clone(), asked[1].clone(), msg("assistant", "a2", Some("persona:juno"))];
        assert_eq!(turn_rule(&after_one), Err("one unasked reply for each message of the person"));
        // A new message of the person starts it again.
        let again = [after_one[0].clone(), after_one[1].clone(), after_one[2].clone(), msg("user", "q2", None), msg("assistant", "b1", Some("persona:kai"))];
        assert!(turn_rule(&again).is_ok());
    }

    #[test]
    fn the_rule_needs_a_finished_reply_to_follow() {
        assert!(turn_rule(&[]).is_err());
        assert!(turn_rule(&[msg("user", "q", None)]).is_err());
        assert!(turn_rule(&[msg("user", "q", None), msg("assistant", "  ", Some("persona:kai"))]).is_err());
    }

    #[test]
    fn only_personas_that_may_talk_with_the_speaker_are_asked_about() {
        // The person talks to the coordinator, kai and juno: they may all chime in after each other.
        let open = team_of(vec![talks("coordinator"), talks("kai"), talks("juno")]);
        assert!(may_chime(&open, "kai", "juno"));
        assert!(may_chime(&open, "juno", "kai"));
        // No line to juno: no unasked reply from juno after kai, however good it would be.
        let some = team_of(vec![talks("coordinator"), talks("kai")]);
        assert!(!may_chime(&some, "kai", "juno"));
        // An asks line between two members lets them, without the person talking to either.
        let asks = team_of(vec![talks("coordinator"), Line::new(LineKind::Asks, "kai", "juno")]);
        assert!(may_chime(&asks, "kai", "juno") && may_chime(&asks, "juno", "kai"));
        // A hand-over is not a line to talk along.
        assert!(!may_chime(&team_of(vec![Line::new(LineKind::HandsOver, "kai", "juno")]), "kai", "juno"));
    }

    #[test]
    fn a_reply_names_its_role_from_the_author_it_was_saved_with() {
        let t = team_of(vec![]);
        assert_eq!(role_of(&t, "persona:kai").as_deref(), Some("kai"));
        assert_eq!(role_of(&t, "persona:mira").as_deref(), Some("coordinator"));
        assert_eq!(role_of(&t, "persona:gone"), None);
        // The chat's own leader answering is the coordinator: an unasked reply may follow it.
        assert_eq!(role_of(&t, "team:open").as_deref(), Some("coordinator"));
        assert_eq!(role_of(&t, "team:another"), None);
    }

    fn who() -> Vec<Candidate> {
        vec![
            Candidate { role: "kai".into(), name: "Kai".into(), job: "Checks the copy".into() },
            Candidate { role: "juno".into(), name: "Juno".into(), job: String::new() },
        ]
    }

    #[test]
    fn the_answer_is_a_name_from_the_list_or_nobody_and_never_a_guess() {
        let w = who();
        assert_eq!(parse_choice("SPEAKER: kai", &w).as_deref(), Some("kai"));
        assert_eq!(parse_choice("speaker: Juno.", &w).as_deref(), Some("juno"), "by name, in any case");
        assert_eq!(parse_choice("Sure.\n**SPEAKER: \"kai\"**", &w).as_deref(), Some("kai"), "forgiving about the dressing");
        assert_eq!(parse_choice("SPEAKER: nobody", &w), None);
        // Fail closed: a name that is not on the list, no speaker line, an empty reply, a ramble.
        assert_eq!(parse_choice("SPEAKER: zed", &w), None);
        assert_eq!(parse_choice("I think kai should say something", &w), None);
        assert_eq!(parse_choice("", &w), None);
        assert_eq!(parse_choice("SPEAKER: kai and juno", &w), None);
    }

    #[test]
    fn the_call_is_shown_the_latest_words_the_short_list_and_one_fixed_form() {
        let messages = [msg("user", "Is the copy fine?", None), msg("assistant", "It reads well.", Some("persona:kai"))];
        let sent = choose_messages(&messages, "Kai", &who());
        let user = sent[1]["content"].as_str().unwrap();
        assert!(user.contains("The person: Is the copy fine?") && user.contains("Kai: It reads well."), "{user}");
        assert!(user.contains("- kai (Kai): Checks the copy") && user.contains("- juno (Juno)\n"), "{user}");
        assert!(sent[0]["content"].as_str().unwrap().contains("SPEAKER: nobody"));
    }
}
