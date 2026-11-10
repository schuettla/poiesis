//! `WRK-4`/`WRK-5`/`WRK-6`: the calls a way of working makes to the model on the
//! side of the run: the second look, the handover note, the pick between two
//! ways.
//!
//! Each of these is a **fresh context on purpose**. The whole point of a second
//! look is that it never saw the path, so it is given the outcome (the patch, the
//! checks, what was done in outline) and not the transcript. The whole point of a
//! handover note is that the next instance reads only the note. A model that is
//! shown how the work went will be talked into it, the way a person is.
//!
//! This file is the part that needs no run to test: the messages that are sent
//! and how the replies are read. Where a call is made, and what the loop does
//! with the answer, is in `run.rs` and `work.rs`.
//!
//! Every reply is read forgivingly and every failure is a plain `None`: a side
//! call that cannot be made or read must never stop the run it is a side of.

use serde_json::{json, Value};

use crate::cloud::{ChatEndpoint, Effort};
use crate::runtime::proxy::{CancelFlag, TurnOutcome, Usage};

/// What the second look is shown of the item's patch, in characters.
const PATCH_CHARS: usize = 14_000;
/// What a handover note may hold, in characters. A note that grows past a page is
/// the transcript again.
pub const NOTE_CHARS: usize = 3_000;
/// How much of the old transcript the note's author is shown, in characters.
const DIGEST_CHARS: usize = 28_000;
/// One tool result in that digest.
const DIGEST_RESULT: usize = 500;
/// One message of the person's or the model's in that digest.
const DIGEST_TURN: usize = 1_500;

/// One call to the model, no tools, cold. `Err` carries something to log, never
/// something to show.
pub async fn ask(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    cancel: &CancelFlag,
    messages: Vec<Value>,
) -> Result<(String, Option<Usage>), String> {
    let outcome = crate::cloud::drive_turn(client, endpoint, &messages, &[], 0.0, Effort::Off, cancel, |_| {})
        .await
        .map_err(|e| e.provider_message().to_string())?;
    match outcome {
        TurnOutcome::Final { content, usage, .. } => Ok((strip_think(&content), usage)),
        TurnOutcome::ToolCalls { .. } => Err("the model tried a tool instead of answering".into()),
        TurnOutcome::Cancelled => Err("cancelled".into()),
    }
}

/// A `<think>…</think>` preamble some models stream as content is not the reply.
fn strip_think(s: &str) -> String {
    let t = s.trim_start();
    if let Some(rest) = t.strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return rest[end + "</think>".len()..].trim().to_string();
        }
    }
    s.trim().to_string()
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// The same, one line.
fn line(s: &str, max: usize) -> String {
    clip(&s.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

// ---- the second look ----

/// What the second look decided. A look that sends an item back always says why:
/// `parse_look` does not produce one that does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    Checked(Option<String>),
    SentBack(String),
}

const LOOK_RULES: &str = "You are giving a second look. You did not do this work and you have not seen how it was done: \
you are given only what it left behind. Judge one thing: is the item below really done, as its \"done when\" says? \
Be fair and be concrete. Send it back only for something that is missing, wrong or unproven, and say exactly what. \
If nothing you were shown contradicts it and the evidence fits, it is checked. \
Reply in exactly this form and nothing else:\n\
VERDICT: CHECKED or SENT BACK\n\
REASON: one sentence (required when you send it back)";

/// The whole of what the second look is given.
pub fn look_messages(
    task: &str,
    item: &str,
    done_when: Option<&str>,
    trail: &[String],
    patch: &str,
    checks: &[String],
    made: &str,
) -> Vec<Value> {
    let mut user = format!("The person asked: {}\n\nThe item: {}\n", line(task, 600), line(item, 300));
    match done_when {
        Some(d) => user.push_str(&format!("Done when: {}\n", line(d, 300))),
        None => user.push_str("Done when: (not written; judge by the item itself)\n"),
    }
    user.push_str("\nWhat was done, in outline:\n");
    if trail.is_empty() {
        user.push_str("(nothing was recorded)\n");
    }
    for t in trail {
        user.push_str(&format!("- {}\n", line(t, 160)));
    }
    user.push_str("\nHow the checks stand:\n");
    if checks.is_empty() {
        user.push_str("(no check was run)\n");
    }
    for c in checks {
        user.push_str(&format!("- {}\n", line(c, 200)));
    }
    user.push_str("\nWhat changed on disk during this item:\n");
    let patch = patch.trim();
    if patch.is_empty() || patch.starts_with("You have not changed any files") {
        user.push_str("(no files were changed)\n");
    } else {
        user.push_str(&clip(patch, PATCH_CHARS));
        user.push('\n');
    }
    // A report or a page is made as an artifact, not as a file: without this the
    // look found "no files were changed" and sent back every piece of writing.
    let made = made.trim();
    if !made.is_empty() {
        user.push_str("\nWhat was made in the chat during this item (artifacts):\n");
        user.push_str(&clip(made, PATCH_CHARS));
        user.push('\n');
    }
    vec![json!({ "role": "system", "content": LOOK_RULES }), json!({ "role": "user", "content": user })]
}

/// Read the verdict. Anything that is not clearly a verdict, and a send-back with
/// no reason, is `None`: the caller lets the item stand rather than block it on a
/// reply nobody can use.
pub fn parse_look(reply: &str) -> Option<Look> {
    let mut verdict: Option<bool> = None; // Some(true) = sent back
    let mut reason = String::new();
    let mut loose = String::new();
    for l in reply.lines() {
        let t = l.trim().trim_start_matches(['*', '#', '-', ' ']);
        let lower = t.to_lowercase();
        if let Some(rest) = lower.strip_prefix("verdict") {
            let rest = rest.trim_start_matches(['*', ':', ' ']);
            if rest.starts_with("sent back") || rest.starts_with("send back") || rest.starts_with("back") || rest.starts_with("not ") || rest.starts_with("rejected") {
                verdict = Some(true);
            } else if rest.starts_with("checked") || rest.starts_with("ok") || rest.starts_with("pass") || rest.starts_with("approved") {
                verdict = Some(false);
            }
        } else if lower.starts_with("reason") {
            let after = t.split_once(':').map(|x| x.1).unwrap_or("");
            reason = after.trim().trim_matches('*').trim().to_string();
        } else if !t.is_empty() {
            loose.push_str(t);
            loose.push(' ');
        }
    }
    let reason = if reason.is_empty() { None } else { Some(line(&reason, 240)) };
    match verdict? {
        true => reason.map(Look::SentBack),
        false => Some(Look::Checked(reason)),
    }
}

// ---- the handover note ----

const HANDOVER_RULES: &str = "You are about to be replaced by a fresh copy of yourself that has not seen any of this. \
Write the handover note it will start from. It will have the original request, the plan and this note, and nothing else. \
Say, plainly and briefly: what the person wants; what is done and where it is (file paths, names); \
what you were in the middle of; what you found out that cost effort (decisions, dead ends, how things are named); \
what is left to do; anything that is failing and why. No greeting, no headings, no more than about 25 lines. \
Write it as notes to yourself.";

/// Boil the transcript down to something a note can be written from. Tool
/// results are cut hard (the next instance can read the files again); the
/// person's words and the model's own are kept longer. Past the cap the middle
/// goes, because the start (what was asked) and the end (where it stands) matter.
pub fn digest(transcript: &[Value]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for m in transcript {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let content = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
        match role {
            "system" => {}
            "user" => parts.push(format!("PERSON: {}", clip(content, DIGEST_TURN))),
            "assistant" => {
                let calls: Vec<String> = m
                    .get("tool_calls")
                    .and_then(|c| c.as_array())
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        let name = c.pointer("/function/name").and_then(|n| n.as_str()).unwrap_or("?");
                        let args = c.pointer("/function/arguments").and_then(|n| n.as_str()).unwrap_or("");
                        format!("{name}({})", line(args, 140))
                    })
                    .collect();
                let said = line(content, DIGEST_TURN);
                if !said.is_empty() {
                    parts.push(format!("YOU: {said}"));
                }
                if !calls.is_empty() {
                    parts.push(format!("YOU CALLED: {}", calls.join(", ")));
                }
            }
            "tool" => parts.push(format!("RESULT: {}", line(content, DIGEST_RESULT))),
            _ => {}
        }
    }
    let all = parts.join("\n");
    if all.chars().count() <= DIGEST_CHARS {
        return all;
    }
    let head: String = all.chars().take(DIGEST_CHARS / 4).collect();
    let tail: String = all.chars().rev().take(DIGEST_CHARS * 3 / 4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}\n[…the middle of the work is left out…]\n{tail}")
}

pub fn handover_messages(transcript: &[Value]) -> Vec<Value> {
    vec![
        json!({ "role": "system", "content": HANDOVER_RULES }),
        json!({ "role": "user", "content": format!("Here is the work so far:\n\n{}\n\nWrite the handover note now.", digest(transcript)) }),
    ]
}

/// A note worth starting from: not empty, not a refusal, cut to a page.
pub fn clean_note(reply: &str) -> Option<String> {
    let note = reply.trim();
    if note.chars().count() < 40 {
        return None;
    }
    Some(clip(note, NOTE_CHARS))
}

/// What the new start begins from: the system messages the run began with, the
/// request it was started on, anything the person said since, and the note. Not
/// one tool call or result, and none of how it went.
pub fn fresh_transcript(old: &[Value], opening: usize, note: &str) -> Vec<Value> {
    let role = |m: &Value| m.get("role").and_then(|r| r.as_str()).unwrap_or("").to_string();
    let lead = old.iter().take_while(|m| role(m) == "system").count();
    let end = opening.clamp(lead, old.len());
    let mut out: Vec<Value> = old[..lead].to_vec();
    if let Some(i) = old[lead..end].iter().rposition(|m| role(m) == "user") {
        out.push(old[lead + i].clone());
    }
    for m in &old[end..] {
        if role(m) == "user" {
            out.push(m.clone());
        }
    }
    out.push(json!({
        "role": "system",
        "content": format!(
            "You are carrying on this work after a fresh start: you forgot how it went on purpose. \
             This is the handover note you wrote to yourself:\n\n{note}\n\n\
             Your plan, if you have one, is shown to you as it stands. Anything you read before is not in front \
             of you any more: read a file again when you need it."
        ),
    }));
    out
}

// ---- two ways ----

/// One way, as the judge reads it.
pub struct WayReport {
    pub name: &'static str,
    /// What the helper said it did.
    pub report: String,
    /// What changed in its copy, as a patch.
    pub patch: String,
    pub files: usize,
}

const JUDGE_RULES: &str = "Two ways of doing the same item were tried, each in its own copy of the folder. \
Pick the one to keep. Prefer the one that really does what \"done when\" says, then the smaller and safer change, \
then the one that adds nothing new to depend on. If neither is any good, still pick the lesser and say so. \
Reply in exactly this form and nothing else:\n\
KEEP: A or B\n\
REASON: one short sentence the person will read (for example: no new dependency)";

pub fn judge_messages(task: &str, item: &str, done_when: Option<&str>, a: &WayReport, b: &WayReport) -> Vec<Value> {
    let mut user = format!("The person asked: {}\n\nThe item: {}\n", line(task, 600), line(item, 300));
    if let Some(d) = done_when {
        user.push_str(&format!("Done when: {}\n", line(d, 300)));
    }
    for w in [a, b] {
        user.push_str(&format!(
            "\n=== Way {} ({} file{} changed) ===\nIt said: {}\n",
            w.name,
            w.files,
            if w.files == 1 { "" } else { "s" },
            line(&w.report, 600)
        ));
        let patch = w.patch.trim();
        if patch.is_empty() {
            user.push_str("(no files changed)\n");
        } else {
            user.push_str(&clip(patch, PATCH_CHARS / 2));
            user.push('\n');
        }
    }
    vec![json!({ "role": "system", "content": JUDGE_RULES }), json!({ "role": "user", "content": user })]
}

/// `("a" | "b", reason)`. The reason is required here too.
pub fn parse_judge(reply: &str) -> Option<(String, String)> {
    let mut keep: Option<String> = None;
    let mut reason = String::new();
    for l in reply.lines() {
        let t = l.trim().trim_start_matches(['*', '#', '-', ' ']);
        let lower = t.to_lowercase();
        if let Some(rest) = lower.strip_prefix("keep") {
            let rest = rest.trim_start_matches(['*', ':', ' ']);
            if rest.starts_with('a') {
                keep = Some("a".into());
            } else if rest.starts_with('b') {
                keep = Some("b".into());
            }
        } else if lower.starts_with("reason") {
            reason = t.split_once(':').map(|x| x.1).unwrap_or("").trim().trim_matches('*').trim().to_string();
        }
    }
    let reason = line(&reason, 200);
    (!reason.is_empty()).then_some(()).and(keep).map(|k| (k, reason))
}

// ---- what done means ----

const CRITERIA_RULES: &str = "You are checking whether work meets what the person agreed. You did not do the work and \
you have not seen how it was done: you are given the criteria, the answer that is about to go out, what changed on disk \
and how the checks stand. For each criterion decide whether it is met, going by what you were shown. Be fair and \
concrete. A criterion about files, tests or behaviour is met only when something you were shown supports it: the \
assistant saying so is not enough. Reply with one line per criterion, in order, in exactly this form and nothing \
else:\n1: MET - one short sentence\n2: NOT MET - one short sentence saying what is missing (required when not met)";

/// What the final check is given. The answer is the end of it, where a long one
/// says how the work ended.
pub fn criteria_messages(task: &str, criteria: &[String], answer: &str, patch: &str, checks: &[String], made: &str) -> Vec<Value> {
    let mut user = format!("The person asked: {}\n\nWhat they agreed done means:\n", line(task, 600));
    for (n, c) in criteria.iter().enumerate() {
        user.push_str(&format!("{}. {}\n", n + 1, line(c, 300)));
    }
    user.push_str("\nThe answer that is about to go out:\n");
    let answer = answer.trim();
    let tail: String = if answer.chars().count() > 4000 {
        format!("…{}", answer.chars().rev().take(4000).collect::<Vec<_>>().into_iter().rev().collect::<String>())
    } else {
        answer.to_string()
    };
    user.push_str(if tail.is_empty() { "(nothing yet)" } else { &tail });
    user.push_str("\n\nHow the checks stand:\n");
    if checks.is_empty() {
        user.push_str("(no check was run)\n");
    }
    for c in checks {
        user.push_str(&format!("- {}\n", line(c, 200)));
    }
    user.push_str("\nWhat changed on disk during this task:\n");
    let patch = patch.trim();
    if patch.is_empty() || patch.starts_with("You have not changed any files") {
        user.push_str("(no files were changed)\n");
    } else {
        user.push_str(&clip(patch, PATCH_CHARS));
        user.push('\n');
    }
    // A report or a page is made as an artifact, not as a file: without this the
    // look found "no files were changed" and sent back every piece of writing.
    let made = made.trim();
    if !made.is_empty() {
        user.push_str("\nWhat was made in the chat (artifacts):\n");
        user.push_str(&clip(made, PATCH_CHARS));
        user.push('\n');
    }
    vec![json!({ "role": "system", "content": CRITERIA_RULES }), json!({ "role": "user", "content": user })]
}

/// One `(met, why)` for each of `n` criteria, or `None`. All of them or none: a
/// reply that skips one has not settled it, and ticking the rest on that would
/// say more than was checked. A criterion that is not met has its reason, or it is
/// no verdict, as for the second look.
pub fn parse_criteria(reply: &str, n: usize) -> Option<Vec<(bool, Option<String>)>> {
    let mut found: Vec<Option<(bool, Option<String>)>> = vec![None; n];
    for l in reply.lines() {
        let t = l.trim().trim_start_matches(['*', '#', '-', ' ']);
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(k) = digits.parse::<usize>() else { continue };
        if k == 0 || k > n {
            continue;
        }
        // ASCII lowercase keeps byte lengths, so `after` lines up with the original line.
        let rest = t[digits.len()..].trim_start_matches(['.', ':', ')', '*', ' ']).to_ascii_lowercase();
        let (met, after) = if let Some(a) = rest.strip_prefix("not met").or_else(|| rest.strip_prefix("unmet")) {
            (false, a)
        } else if let Some(a) = rest.strip_prefix("met") {
            (true, a)
        } else {
            continue;
        };
        // The reason keeps its own case: take it from the original line.
        let reason_at = t.len() - after.len();
        let reason = line(t[reason_at..].trim_start_matches(['-', '\u{2013}', '\u{2014}', ':', '*', ' ']), 200);
        let reason = (!reason.is_empty()).then_some(reason);
        if !met && reason.is_none() {
            return None;
        }
        found[k - 1] = Some((met, reason));
    }
    found.into_iter().collect()
}

// ---- advice ----

const ADVICE_RULES: &str = "You are asked for advice before an action that is hard to take back. You have not seen \
how the work went. You are given what the person asked, the action about to be made, and notes the person keeps that \
may bear on it. Say in one sentence, in the first person, what the notes say about it and whether anything speaks \
against it. If nothing in the notes bears on it, say you found nothing against it. Begin with \"I asked first:\". \
Reply with that one sentence and nothing else.";

/// The notes (`(description, body)` of each fact or lesson) that share words with
/// what is about to be done, best match first, at most `ADVICE_NOTES`. A word
/// counts when it is three letters or more and is not one of the act's own verbs.
/// Cheap on purpose: advice is one more call before a risky step, and it may not
/// cost a search of its own.
pub fn relevant_notes(action: &str, notes: &[(String, String)]) -> Vec<String> {
    const SKIP: &[&str] = &[
        "the", "and", "for", "with", "from", "into", "delete", "deleting", "send", "sending", "mail", "file", "files",
        "change", "changing", "something", "using", "use", "connected", "app", "reply", "replying", "message",
    ];
    let words = |s: &str| -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 3 && !SKIP.contains(w))
            .map(str::to_string)
            .collect()
    };
    let wanted = words(action);
    let mut scored: Vec<(usize, String)> = notes
        .iter()
        .filter_map(|(description, body)| {
            let have = words(&format!("{description} {body}"));
            let hits = wanted.intersection(&have).count();
            (hits > 0).then(|| {
                let first: String = body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
                (hits, if first.is_empty() { description.clone() } else { format!("{description}: {first}") })
            })
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(ADVICE_NOTES).map(|(_, n)| n).collect()
}

/// How many notes advice is shown.
const ADVICE_NOTES: usize = 4;

pub fn advice_messages(task: &str, action: &str, notes: &[String]) -> Vec<Value> {
    let mut user = format!("The person asked: {}\n\nThe action about to be made: {}\n\nNotes that may bear on it:\n", line(task, 600), line(action, 300));
    if notes.is_empty() {
        user.push_str("(no note mentions anything like this)\n");
    }
    for n in notes {
        user.push_str(&format!("- {}\n", line(n, 300)));
    }
    vec![json!({ "role": "system", "content": ADVICE_RULES }), json!({ "role": "user", "content": user })]
}

/// The advice as one line that begins the way it is shown. Empty is no advice.
pub fn clean_advice(reply: &str) -> Option<String> {
    let one = line(reply.lines().find(|l| !l.trim().is_empty())?, 240);
    let one = one.trim_matches(['"', '*', ' ']).to_string();
    if one.is_empty() {
        return None;
    }
    Some(if one.to_lowercase().starts_with("i asked first") { one } else { format!("I asked first: {one}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn criteria_are_read_one_line_each_and_all_or_nothing() {
        let ok = parse_criteria("1: MET - the button is there\n2: NOT MET - Excel shows one column", 2);
        assert_eq!(
            ok,
            Some(vec![(true, Some("the button is there".into())), (false, Some("Excel shows one column".into()))])
        );
        // The ways models bend it.
        assert_eq!(parse_criteria("- 1. Met\n- 2) not met: no test covers it", 2), Some(vec![(true, None), (false, Some("no test covers it".into()))]));
        // One skipped, one unmet without a reason, noise: no verdict, so nothing is ticked.
        assert_eq!(parse_criteria("1: MET - fine", 2), None);
        assert_eq!(parse_criteria("1: MET\n2: NOT MET", 2), None, "a verdict that sends back always says why");
        assert_eq!(parse_criteria("Looks good to me.", 1), None);
        assert_eq!(parse_criteria("1: MET - ok\n7: MET - extra line", 1), Some(vec![(true, Some("ok".into()))]));
    }

    #[test]
    fn the_criteria_check_is_given_the_outcome_and_never_the_path() {
        let m = criteria_messages(
            "Export the runs",
            &["Fleet has an Export button".into(), "Excel opens it".into()],
            "Done. I added the button.",
            "You have not changed any files in this conversation.",
            &["cargo test: passed".into()],
            "",
        );
        let user = m[1]["content"].as_str().unwrap();
        assert!(user.contains("1. Fleet has an Export button") && user.contains("2. Excel opens it"));
        assert!(user.contains("(no files were changed)") && user.contains("cargo test: passed"));
        assert!(m[0]["content"].as_str().unwrap().contains("did not do the work"));
    }

    #[test]
    fn advice_is_one_line_that_begins_the_way_it_is_shown() {
        assert_eq!(clean_advice("I asked first: your notes say /old is a backup.\nMore."), Some("I asked first: your notes say /old is a backup.".into()));
        assert_eq!(clean_advice("\"Nothing in the notes speaks against it.\""), Some("I asked first: Nothing in the notes speaks against it.".into()));
        assert_eq!(clean_advice("   \n"), None);
        let notes = vec![
            ("The /old folder is a backup".to_string(), "Keep everything under /old.".to_string()),
            ("Prefers short answers".to_string(), "Tone and length.".to_string()),
            ("Backup drive".to_string(), "The backup drive is labelled E.".to_string()),
        ];
        let picked = relevant_notes("deleting /old/notes.md", &notes);
        assert_eq!(picked.len(), 1, "only the note that shares a word with the act: {picked:?}");
        assert!(picked[0].starts_with("The /old folder is a backup: Keep everything under /old."));
        assert!(relevant_notes("sending mail to ann@example.com", &notes).is_empty(), "the verbs alone match nothing");
        let m = advice_messages("clean up", "delete 3 files in /old", &["/old is a backup folder".into()]);
        assert!(m[1]["content"].as_str().unwrap().contains("- /old is a backup folder"));
        assert!(advice_messages("x", "y", &[])[1]["content"].as_str().unwrap().contains("no note mentions"));
    }

    #[test]
    fn a_verdict_is_read_in_the_form_it_was_asked_for_and_in_the_ways_models_bend_it() {
        assert_eq!(
            parse_look("VERDICT: SENT BACK\nREASON: The button is not wired to anything."),
            Some(Look::SentBack("The button is not wired to anything.".into()))
        );
        assert_eq!(parse_look("VERDICT: CHECKED\nREASON: The file is written and the test passes."), Some(Look::Checked(Some("The file is written and the test passes.".into()))));
        assert_eq!(parse_look("**Verdict:** Checked"), Some(Look::Checked(None)));
        assert_eq!(parse_look("- verdict: sent back\n- reason: no test covers it"), Some(Look::SentBack("no test covers it".into())));
    }

    #[test]
    fn a_send_back_without_a_reason_is_no_verdict_and_so_is_noise() {
        assert_eq!(parse_look("VERDICT: SENT BACK"), None, "a verdict without a reason does not exist");
        assert_eq!(parse_look("VERDICT: SENT BACK\nREASON:   "), None);
        assert_eq!(parse_look("Looks fine to me, I think."), None);
        assert_eq!(parse_look(""), None);
    }

    #[test]
    fn the_second_look_is_given_the_outcome_and_never_the_path() {
        let m = look_messages(
            "Add an export button",
            "Button",
            Some("the button is there"),
            &["read_file src/App.tsx".into(), "edit_file src/App.tsx".into()],
            "--- a/src/App.tsx\n+++ b/src/App.tsx\n+<button>Export</button>",
            &["cargo test: passed".into()],
            "",
        );
        assert_eq!(m.len(), 2, "two messages: the rules and the evidence, no transcript");
        let user = m[1]["content"].as_str().unwrap();
        for needle in ["Add an export button", "Done when: the button is there", "edit_file src/App.tsx", "cargo test: passed", "<button>Export</button>"] {
            assert!(user.contains(needle), "{needle} missing: {user}");
        }
        let none = look_messages("t", "i", None, &[], "", &[], "");
        let user = none[1]["content"].as_str().unwrap();
        assert!(user.contains("(no files were changed)") && user.contains("(no check was run)") && user.contains("not written"));
    }

    #[test]
    fn writing_made_as_an_artifact_is_shown_to_the_look() {
        let m = look_messages("Write a report", "Draft section 1", None, &[], "", &[], "## Briefing (markdown)\n# Executive summary");
        let user = m[1]["content"].as_str().unwrap();
        assert!(user.contains("artifacts") && user.contains("# Executive summary"), "{user}");
    }

    #[test]
    fn a_long_patch_is_cut_so_the_look_stays_cheap() {
        let patch = "+x\n".repeat(20_000);
        let m = look_messages("t", "i", None, &[], &patch, &[], "");
        assert!(m[1]["content"].as_str().unwrap().chars().count() < PATCH_CHARS + 2_000);
    }

    #[test]
    fn the_digest_keeps_the_start_and_the_end_and_cuts_the_results() {
        let mut transcript = vec![json!({ "role": "system", "content": "SYSTEM PROMPT" }), json!({ "role": "user", "content": "Build the export" })];
        for i in 0..400 {
            transcript.push(json!({ "role": "assistant", "content": format!("step {i} thinking about things at some length"),
                "tool_calls": [{ "id": format!("c{i}"), "function": { "name": "read_file", "arguments": "{\"path\":\"a.txt\"}" } }] }));
            transcript.push(json!({ "role": "tool", "tool_call_id": format!("c{i}"), "content": "x".repeat(4000) }));
        }
        let d = digest(&transcript);
        assert!(!d.contains("SYSTEM PROMPT"), "the system prompt is the next instance's own");
        assert!(d.starts_with("PERSON: Build the export"));
        assert!(d.contains("step 399"), "the end is what the note needs most");
        assert!(d.contains("the middle of the work is left out"));
        assert!(d.chars().count() <= DIGEST_CHARS + 100);
        assert!(!d.contains(&"x".repeat(DIGEST_RESULT + 5)), "a result is cut hard");
    }

    #[test]
    fn a_fresh_start_keeps_the_request_and_the_persons_later_words_and_nothing_of_the_path() {
        let old = vec![
            json!({ "role": "system", "content": "SYS" }),
            json!({ "role": "user", "content": "earlier question" }),
            json!({ "role": "assistant", "content": "earlier answer" }),
            json!({ "role": "user", "content": "Build the export" }),
            json!({ "role": "assistant", "content": "", "tool_calls": [{ "id": "c1", "function": { "name": "read_file", "arguments": "{}" } }] }),
            json!({ "role": "tool", "tool_call_id": "c1", "content": "file contents" }),
            json!({ "role": "user", "content": "and make it blue" }),
            json!({ "role": "system", "content": "a nudge" }),
        ];
        let new = fresh_transcript(&old, 4, "I wrote the export in src/export.rs.");
        let roles: Vec<&str> = new.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, vec!["system", "user", "user", "system"]);
        assert_eq!(new[1]["content"], "Build the export", "the request, not the chat before it");
        assert_eq!(new[2]["content"], "and make it blue", "the person's later words are kept");
        let last = new[3]["content"].as_str().unwrap();
        assert!(last.contains("src/export.rs") && last.contains("read a file again"));
        assert!(!new.iter().any(|m| m.get("tool_calls").is_some() || m["role"] == "tool"));
    }

    #[test]
    fn a_note_is_kept_when_it_says_something_and_cut_to_a_page() {
        assert_eq!(clean_note("ok"), None);
        assert_eq!(clean_note(""), None);
        let long = "The export writes to out.csv in src/export.rs, and the button is not done. ".repeat(100);
        assert!(clean_note(&long).unwrap().chars().count() <= NOTE_CHARS + 1);
    }

    #[test]
    fn the_judge_names_one_way_and_gives_a_reason() {
        assert_eq!(parse_judge("KEEP: B\nREASON: no new dependency"), Some(("b".into(), "no new dependency".into())));
        assert_eq!(parse_judge("**Keep:** A\n**Reason:** it is smaller"), Some(("a".into(), "it is smaller".into())));
        assert_eq!(parse_judge("KEEP: A"), None, "no reason, no verdict");
        assert_eq!(parse_judge("I like both"), None);
        let a = WayReport { name: "A", report: "did x".into(), patch: "+a".into(), files: 1 };
        let b = WayReport { name: "B", report: "did y".into(), patch: String::new(), files: 0 };
        let m = judge_messages("t", "i", Some("d"), &a, &b);
        let u = m[1]["content"].as_str().unwrap();
        assert!(u.contains("Way A (1 file changed)") && u.contains("Way B (0 files changed)") && u.contains("(no files changed)"));
    }
}
