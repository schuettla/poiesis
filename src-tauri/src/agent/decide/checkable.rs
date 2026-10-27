//! `SEE-1`: is this sentence of an answer something that could be checked, and if so
//! is it a fact about the world or something the assistant says it did?
//!
//! One request, two questions about one sentence. It is asked once a sentence, for
//! every answer, so it is the cheapest question the check has: no passage is read
//! and no other model is called.
//!
//! - `checkable` (yes or no): a number, a name, a date, a quote, a result or an
//!   action taken, against advice, opinion, a greeting or a plan. The row's bar
//!   (0.85) is on this answer. Below it the sentence is left alone and nothing is
//!   marked: never a mark on a guess.
//! - `kind` (pick one): `fact` or `did`. A sentence is read as "I did this" only at
//!   [`DID_AT`], because a wrong "did" is a red mark on a true sentence, and a wrong
//!   "fact" is at worst an amber one.

use super::control::{self, Act, Gate};
use super::policy::Purpose;
use super::{Answer, Ask, Question, Verdict};

pub const CHECKABLE_ID: &str = "checkable";
pub const KIND_ID: &str = "kind";
pub const FACT: &str = "fact";
pub const DID: &str = "did";

/// How sure the model has to be that a sentence says what the assistant did.
pub const DID_AT: f32 = 0.85;

/// The state: the sentence, and nothing of the answer around it, so the model
/// judges the sentence and not the topic.
pub fn state(sentence: &str) -> String {
    format!("One sentence from an answer an assistant wrote:\n\n{}", sentence.trim())
}

pub fn checkable_question() -> Question {
    Question::Noul {
        instructions: "Does this sentence state something that could be checked against a source or a record: a number, a name, a date, a quote, a result, or something that was done?"
            .into(),
        when_true: "It states a number, a name, a date, a quote, a result or something that was done.".into(),
        when_false: "It is an opinion, advice, a greeting, an offer, a plan, a question or a remark about the conversation."
            .into(),
    }
}

pub fn kind_question() -> Question {
    Question::Choice {
        instructions: "Is this sentence a fact about the world, or does the assistant say it did something itself in this conversation?"
            .into(),
        options: vec![
            (FACT.into(), "A fact about the world, which a source could confirm or deny.".into()),
            (
                DID.into(),
                "Something the assistant says it did itself: it sent, wrote, created, ran, searched, changed or deleted something."
                    .into(),
            ),
        ],
    }
}

/// The request for one sentence: both questions, read together.
pub fn ask_of(state: &str) -> Ask<'_> {
    Ask {
        state,
        images: Vec::new(),
        questions: vec![(CHECKABLE_ID, checkable_question()), (KIND_ID, kind_question())],
    }
}

/// What the answer comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// Not something to check, or not sure it is. Nothing is marked.
    Skip,
    /// A fact about the world. `p` is how sure the model was that it can be checked.
    Fact { p: f32 },
    /// Something the assistant says it did.
    Did { p: f32 },
}

pub fn read(v: &Verdict) -> Reading {
    let Some(answer @ Answer::Noul { p_true }) = v.get(CHECKABLE_ID) else { return Reading::Skip };
    if control::evaluate(Purpose::ClaimCheckable, answer, v.calibrated, Gate::Open) != Act::Go {
        return Reading::Skip;
    }
    let did = match v.get(KIND_ID) {
        Some(Answer::Choice { probs, .. }) => probs.iter().find(|(k, _)| k == DID).map_or(0.0, |(_, p)| *p),
        _ => 0.0,
    };
    if did >= DID_AT {
        Reading::Did { p: did }
    } else {
        Reading::Fact { p: *p_true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::decide::{local, BackendId};
    use std::collections::BTreeMap;

    fn verdict(checkable: f32, did: f32) -> Verdict {
        Verdict {
            id: "d".into(),
            answers: BTreeMap::from([
                (CHECKABLE_ID.to_string(), Answer::Noul { p_true: checkable }),
                (
                    KIND_ID.to_string(),
                    Answer::Choice {
                        choice: if did >= 0.5 { DID.into() } else { FACT.into() },
                        probs: vec![(FACT.into(), 1.0 - did), (DID.into(), did)],
                        confidence: 0.5,
                    },
                ),
            ]),
            backend: BackendId::Local { model: "test".into() },
            calibrated: false,
            ms: 1,
        }
    }

    #[test]
    fn the_request_is_one_the_server_accepts_with_both_questions() {
        let s = state("The tower is 330 metres tall.");
        let ask = ask_of(&s);
        assert!(local::check_ask(&ask).is_ok());
        let ids: Vec<&str> = ask.questions.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [CHECKABLE_ID, KIND_ID]);
        assert!(s.ends_with("The tower is 330 metres tall."));
    }

    /// The row's bar is 0.85 on "can be checked"; below it nothing is marked.
    #[test]
    fn only_a_sure_checkable_sentence_is_read() {
        assert_eq!(read(&verdict(0.9, 0.1)), Reading::Fact { p: 0.9 });
        assert_eq!(read(&verdict(0.85, 0.1)), Reading::Fact { p: 0.85 });
        assert_eq!(read(&verdict(0.84, 0.1)), Reading::Skip);
        assert_eq!(read(&verdict(0.3, 0.99)), Reading::Skip, "an opinion that sounds like an action is still left alone");
    }

    /// A wrong "did" is a red mark on a true sentence, so it needs its own bar.
    #[test]
    fn a_sentence_counts_as_something_done_only_when_the_model_is_sure() {
        assert_eq!(read(&verdict(0.95, 0.9)), Reading::Did { p: 0.9 });
        assert_eq!(read(&verdict(0.95, 0.85)), Reading::Did { p: 0.85 });
        assert_eq!(read(&verdict(0.95, 0.84)), Reading::Fact { p: 0.95 });
    }

    #[test]
    fn an_answer_to_other_questions_is_skipped() {
        let mut v = verdict(0.9, 0.1);
        v.answers.clear();
        assert_eq!(read(&v), Reading::Skip);
        v.answers.insert(CHECKABLE_ID.into(), Answer::Choice { choice: "x".into(), probs: vec![("x".into(), 1.0)], confidence: 1.0 });
        assert_eq!(read(&v), Reading::Skip);
    }
}
