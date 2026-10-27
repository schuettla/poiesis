//! The five sites that already asked a small model a small question (`DEC-6`),
//! written as decisions. Each has two halves kept apart so each can be tested
//! without an engine: the question it asks, and how the answer maps back to
//! what the site did with the prose reply before.
//!
//! The goal check's mapping lives in `agent::goal` (`combine`), and the rule
//! preview's in `engines::rules` (`preview_from_option`), next to the types they
//! produce. This file builds their questions too.
//!
//! Every site keeps its old path as the fallback. When [`super::ask`] says no,
//! the site does what it did before there was a decision engine.

use super::policy::Purpose;
use super::{Answer, Ask, Question, Verdict};

/// A request with one question, which is all the five sites need. Owns the
/// state so the site can build it and hand out a borrowed [`Ask`].
#[derive(Debug, Clone)]
pub struct SiteAsk {
    pub state: String,
    pub id: &'static str,
    pub question: Question,
}

impl SiteAsk {
    pub fn as_ask(&self) -> Ask<'_> {
        Ask { state: &self.state, images: Vec::new(), questions: vec![(self.id, self.question.clone())] }
    }
}

// ---- RET-4: do the closest excerpts answer the question? ----

pub struct Sufficiency {
    pub enough: bool,
    /// The probability that they do. It goes into the warning text.
    pub p: f32,
}

pub fn sufficiency(query: &str, excerpts: &str) -> SiteAsk {
    SiteAsk {
        state: format!("A user asked: \"{query}\"\n\nThe closest excerpts found in their files:\n{excerpts}"),
        id: "enough",
        question: Question::Noul {
            instructions: "Do these excerpts actually answer the question, or do they only share its general subject without answering it?"
                .into(),
            when_true: "The excerpts answer the question.".into(),
            when_false: "The excerpts only share the subject, or say nothing useful about it.".into(),
        },
    }
}

/// At the row's bar (0.5, where the one-word answer "yes" or "no" sat).
pub fn sufficiency_from(v: &Verdict) -> Option<Sufficiency> {
    let answer = v.get("enough")?;
    match answer {
        Answer::Noul { p_true } => Some(Sufficiency {
            enough: Purpose::RetSufficiency.policy().act_at.reaches(answer, v.calibrated),
            p: *p_true,
        }),
        _ => None,
    }
}

// ---- SCP-1: does a saved note apply always, or only on its subject? ----

pub fn scope(name: &str, description: &str, body: &str) -> SiteAsk {
    SiteAsk {
        state: format!("A personal note was just saved:\nname: {name}\ndescription: {description}\ntext: {body}"),
        id: "scope",
        question: Question::Choice {
            instructions: "Does this note apply to every response regardless of subject, or only when a specific subject comes up?"
                .into(),
            options: vec![
                ("global".into(), "It applies to every response, whatever the subject is.".into()),
                ("topical".into(), "It applies only when its own subject comes up.".into()),
                ("unsure".into(), "It cannot be told from the note.".into()),
            ],
        },
    }
}

/// `global` or `topical`; `None` for `unsure` or an answer the model was not
/// sure of, which leaves the note unclassified exactly as a failed call did.
pub fn scope_from(v: &Verdict) -> Option<String> {
    let answer = v.get("scope")?;
    let Answer::Choice { choice, .. } = answer else { return None };
    // Below the row's bar a scope is left unclassified (the backfill retries).
    if choice == "unsure" || !Purpose::MemScope.policy().act_at.reaches(answer, v.calibrated) {
        return None;
    }
    matches!(choice.as_str(), "global" | "topical").then(|| choice.clone())
}

// ---- CRT-1: does a drafted lesson hold up against its conversation? ----

/// The options of the critic's question, with the sentence shown as the
/// objection when one wins. The `ok` text is written so that it is the default:
/// a lesson with imprecise wording or a narrow scope still passes (`CRT-4`).
pub const CRITIC_OPTIONS: [(&str, &str); 4] = [
    (
        "ok",
        "Nothing below applies. The lesson is supported, would change what the assistant does, and would not make it worse. Imprecise wording or a narrower scope than you would have chosen is fine.",
    ),
    ("unsupported", "The conversation does not show the mistake this lesson claims."),
    ("unactionable", "Following this lesson would not change what the assistant does."),
    ("wrong", "Acting on this lesson would make the assistant worse."),
];

pub struct Critic {
    pub ok: bool,
    /// The winning objection, in words. Empty when `ok`.
    pub reason: String,
}

pub fn critic(transcript: &str, name: &str, body: &str) -> SiteAsk {
    SiteAsk {
        // The lesson comes first, so cutting the transcript from the middle
        // never cuts the thing being judged.
        state: format!("A reflection pass drew this lesson from the conversation below.\nLesson \"{name}\": {body}\n\nConversation:\n{transcript}"),
        id: "verdict",
        question: Question::Choice {
            instructions: "Review another process's self-drafted lesson. Be fair, not adversarial: reject only what is unsupported, unactionable, or wrong, and pass the rest."
                .into(),
            options: CRITIC_OPTIONS.iter().map(|(id, text)| ((*id).to_string(), (*text).to_string())).collect(),
        },
    }
}

/// The winning option decides: `ok` clears the lesson, any other demotes it to a
/// proposal with that option's sentence as the reason. No JSON is parsed.
pub fn critic_from(v: &Verdict) -> Option<Critic> {
    let answer = v.get("verdict")?;
    let Answer::Choice { choice, .. } = answer else { return None };
    // A critic that cannot pick a winner is the old critic's job.
    if !Purpose::ReflectCritic.policy().act_at.reaches(answer, v.calibrated) {
        return None;
    }
    if choice == "ok" {
        return Some(Critic { ok: true, reason: String::new() });
    }
    let reason = CRITIC_OPTIONS.iter().find(|(id, _)| id == choice).map(|(_, text)| (*text).to_string())?;
    Some(Critic { ok: false, reason })
}

// ---- GOL-2: the goal check ----

pub fn goal(state: String) -> SiteAsk {
    SiteAsk {
        state,
        id: "met",
        question: Question::Noul {
            instructions: "Does the condition now hold, judged from the assistant's last answer and the changes it made? Say true only if the answer itself shows it holds, for example by reporting a command's real result. A plan, a promise or \"I think so\" is not enough."
                .into(),
            when_true: "The answer shows that the condition holds.".into(),
            when_false: "The condition is not shown to hold yet.".into(),
        },
    }
}

pub fn goal_from(v: &Verdict) -> Option<crate::agent::goal::Decision> {
    match v.get("met")? {
        Answer::Noul { p_true } => Some(crate::agent::goal::Decision { p: *p_true, calibrated: v.calibrated }),
        _ => None,
    }
}

// ---- RTE-7: what would the lead do with this request? ----

/// What a routing choice says to do.
pub enum Route {
    /// An agent and the rule that sends the task to it (`claude-code:U2`).
    Agent(String),
    /// A confident "no rule fits": the lead would do it itself.
    Nobody,
    /// Not sure enough: ask the model the old way.
    Unsure,
}

pub fn route(
    request: &str,
    options: Vec<(String, String)>,
    avoid_lines: &[String],
) -> SiteAsk {
    route_after(request, "", options, avoid_lines)
}

/// The same question in a live turn (`USE-13`), where the assistant's last reply
/// is part of what the message means ("do that in the other module too"). The
/// request comes first, so a later look at the logged preview still finds it.
pub fn route_after(
    request: &str,
    previous: &str,
    options: Vec<(String, String)>,
    avoid_lines: &[String],
) -> SiteAsk {
    let mut instructions =
        "A lead assistant may hand a task to one of the agent apps below, under the user's own rules. \
         Which rule applies to the request?"
            .to_string();
    for line in avoid_lines {
        instructions.push_str("\n- ");
        instructions.push_str(line);
    }
    let mut state = format!("The user's request:\n{}", request.trim());
    if !previous.trim().is_empty() {
        state.push_str(&format!("\n\nThe assistant's last reply:\n{}", previous.trim()));
    }
    SiteAsk {
        state,
        id: "route",
        question: Question::Choice { instructions, options },
    }
}

pub fn route_from(v: &Verdict) -> Route {
    let Some(answer @ Answer::Choice { choice, .. }) = v.get("route") else { return Route::Unsure };
    // Below the row's bar the preview asks the model the way it used to.
    if choice == crate::engines::rules::ROUTE_UNSURE || !Purpose::HandoverRoute.policy().act_at.reaches(answer, v.calibrated) {
        return Route::Unsure;
    }
    if choice == crate::engines::rules::ROUTE_NONE {
        return Route::Nobody;
    }
    Route::Agent(choice.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::decide::{local, BackendId};
    use std::collections::BTreeMap;

    fn verdict(id: &str, answer: Answer) -> Verdict {
        Verdict {
            id: "d".into(),
            answers: BTreeMap::from([(id.to_string(), answer)]),
            backend: BackendId::Local { model: "test".into() },
            calibrated: false,
            ms: 1,
        }
    }

    fn choice(winner: &str, probs: &[(&str, f32)]) -> Answer {
        let p: Vec<(String, f32)> = probs.iter().map(|(k, p)| ((*k).to_string(), *p)).collect();
        let confidence = crate::agent::decide::confidence(&p.iter().map(|(_, p)| *p).collect::<Vec<_>>());
        Answer::Choice { choice: winner.into(), probs: p, confidence }
    }

    #[test]
    fn every_site_builds_a_question_the_server_accepts() {
        let asks = [
            sufficiency("q", "e"),
            scope("n", "d", "b"),
            critic("t", "n", "b"),
            goal("Goal: g".into()),
            route("do it", vec![("a:U1".into(), "x".into()), ("none".into(), "y".into())], &[]),
        ];
        for a in asks {
            assert!(local::check_ask(&a.as_ask()).is_ok(), "{} is not valid", a.id);
        }
    }

    // ---- DEC-T-2: parity at 0.5 ----

    #[test]
    fn sufficiency_decides_the_same_at_one_half_as_yes_and_no_did() {
        let yes = sufficiency_from(&verdict("enough", Answer::Noul { p_true: 0.62 })).unwrap();
        assert!(yes.enough);
        let no = sufficiency_from(&verdict("enough", Answer::Noul { p_true: 0.31 })).unwrap();
        assert!(!no.enough);
        assert!((no.p - 0.31).abs() < 1e-6, "the probability goes into the warning");
        assert!(sufficiency_from(&verdict("enough", Answer::Noul { p_true: 0.5 })).unwrap().enough);
    }

    #[test]
    fn a_scope_of_global_or_topical_is_kept_when_the_model_is_sure() {
        let sure = choice("topical", &[("global", 0.02), ("topical", 0.97), ("unsure", 0.01)]);
        assert_eq!(scope_from(&verdict("scope", sure)).as_deref(), Some("topical"));
        let sure = choice("global", &[("global", 0.96), ("topical", 0.03), ("unsure", 0.01)]);
        assert_eq!(scope_from(&verdict("scope", sure)).as_deref(), Some("global"));
    }

    /// `unsure`, or confidence below 0.6, stores nothing (the backfill retries).
    #[test]
    fn an_unsure_or_wavering_scope_is_left_unclassified() {
        let unsure = choice("unsure", &[("global", 0.1), ("topical", 0.1), ("unsure", 0.8)]);
        assert_eq!(scope_from(&verdict("scope", unsure)), None);
        let wavering = choice("global", &[("global", 0.45), ("topical", 0.4), ("unsure", 0.15)]);
        assert_eq!(scope_from(&verdict("scope", wavering)), None);
    }

    #[test]
    fn the_critic_clears_a_lesson_when_ok_wins_and_says_why_when_another_does() {
        let ok = choice("ok", &[("ok", 0.9), ("unsupported", 0.04), ("unactionable", 0.03), ("wrong", 0.03)]);
        let c = critic_from(&verdict("verdict", ok)).unwrap();
        assert!(c.ok && c.reason.is_empty());

        let bad = choice("unactionable", &[("ok", 0.1), ("unsupported", 0.1), ("unactionable", 0.7), ("wrong", 0.1)]);
        let c = critic_from(&verdict("verdict", bad)).unwrap();
        assert!(!c.ok);
        assert_eq!(c.reason, "Following this lesson would not change what the assistant does.");
    }

    /// Imprecise wording is the default pass (`CRT-4`): `ok` is described so.
    #[test]
    fn the_ok_option_is_the_lenient_default() {
        assert!(CRITIC_OPTIONS[0].1.contains("Imprecise wording"));
        assert_eq!(CRITIC_OPTIONS[0].0, "ok");
    }

    #[test]
    fn the_critic_reads_lesson_first_so_a_long_transcript_cuts_from_its_own_middle() {
        let ask = critic(&"x".repeat(200_000), "be-brief", "Answer in one line.");
        let (state, cut) = crate::agent::decide::truncate_middle(&ask.state, 12_000);
        assert!(cut);
        assert!(state.contains("be-brief") && state.contains("Answer in one line."));
    }

    #[test]
    fn the_goal_decision_carries_its_probability_and_calibration() {
        let d = goal_from(&verdict("met", Answer::Noul { p_true: 0.83 })).unwrap();
        assert!((d.p - 0.83).abs() < 1e-6);
        assert!(!d.calibrated);
    }

    #[test]
    fn a_route_names_an_agent_unless_none_or_unsure_wins_or_the_model_wavers() {
        let opts = ["claude-code:U2", "none", "unsure"];
        let p = |a: f32, b: f32, c: f32| [(opts[0], a), (opts[1], b), (opts[2], c)];
        let agent = choice("claude-code:U2", &p(0.95, 0.03, 0.02));
        assert!(matches!(route_from(&verdict("route", agent)), Route::Agent(o) if o == "claude-code:U2"));
        let none = choice("none", &p(0.03, 0.95, 0.02));
        assert!(matches!(route_from(&verdict("route", none)), Route::Nobody));
        let unsure = choice("unsure", &p(0.1, 0.1, 0.8));
        assert!(matches!(route_from(&verdict("route", unsure)), Route::Unsure));
        let wavering = choice("claude-code:U2", &p(0.4, 0.35, 0.25));
        assert!(matches!(route_from(&verdict("route", wavering)), Route::Unsure));
    }

    #[test]
    fn the_avoid_rules_are_quoted_in_the_routing_question() {
        let ask = route("fix it", vec![("a:U1".into(), "x".into()), ("none".into(), "y".into())], &["Do not hand it to Codex for: quick questions".into()]);
        let Question::Choice { instructions, .. } = &ask.question else { panic!("not a choice") };
        assert!(instructions.contains("quick questions"));
    }

    /// `DEC-T-2` with the transport: build the question, run it against a mock
    /// server that speaks the real reply shape, and map the answer back.
    #[tokio::test]
    async fn a_site_round_trips_through_a_server_that_speaks_the_real_format() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 65_536];
            let n = sock.read(&mut buf).await.unwrap();
            let seen = String::from_utf8_lossy(&buf[..n]).to_string();
            assert!(seen.starts_with("POST /v1/systemone"), "wrong path: {}", &seen[..40.min(seen.len())]);
            assert!(seen.to_lowercase().contains("authorization: bearer tok"));
            let body = serde_json::json!({
                "answers": { "enough": { "type": "noul", "noul": 0.2 } },
                "usage": { "input_tokens": 90, "output_tokens": 0 }
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
        });

        let site = sufficiency("what is the refund window?", "Our office is open on weekdays.");
        let ask = site.as_ask();
        let body = local::build_body(&ask, &site.state);
        let reply = local::post(
            &reqwest::Client::new(),
            &format!("http://{addr}"),
            "tok",
            &body,
            std::time::Duration::from_secs(5),
        )
        .await
        .unwrap();
        let (answers, tokens) = local::parse_response(&ask, &reply).unwrap();
        assert_eq!(tokens, 90);
        let v = Verdict {
            id: "d".into(),
            answers,
            backend: BackendId::Local { model: "mock".into() },
            calibrated: false,
            ms: 1,
        };
        let s = sufficiency_from(&v).unwrap();
        assert!(!s.enough, "the excerpts do not answer it, so the retrieval warning stays");
    }

    /// `DEC-T-3`: a model that answers HTTP 501 is not a decision model.
    #[tokio::test]
    async fn a_server_that_answers_501_fails_the_head_check() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 65_536];
            let _ = sock.read(&mut buf).await.unwrap();
            let body = r#"{"error":{"code":501,"message":"This model is not a decision model","type":"not_supported_error"}}"#;
            let resp = format!(
                "HTTP/1.1 501 Not Implemented\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
        });
        let ask = local::head_check_ask();
        let err = local::post(
            &reqwest::Client::new(),
            &format!("http://{addr}"),
            "tok",
            &local::build_body(&ask, ask.state),
            std::time::Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert_eq!(err, crate::agent::decide::DecideError::Failed("This file can't make decisions.".into()));
    }
}
