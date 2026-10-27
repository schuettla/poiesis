//! The wire format of `llama-server`'s `POST /v1/systemone` (`DEC-5`, local).
//!
//! Checked against a real Clef-flash Q4_K_M on build `b11418` (2026-10-23), not
//! only against the docs:
//!
//! * `noul` answers `{ "type": "noul", "noul": 0.96 }`.
//! * `choice` answers `choice`, `probabilities` (a map keyed by option id) and
//!   a `confidence` that is *not* the entropy measure we use, so it is ignored.
//! * `score` answers a float `score` (an expected level, not an index),
//!   `legend` and `probabilities` keyed by the level index as a string.
//! * `usage.input_tokens` counts the prompt; `output_tokens` is 0.
//! * A model that is not a decision model answers HTTP 501.
//! * A state longer than `--ubatch-size` answers HTTP 500.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::{confidence, Answer, Ask, DecideError, Question};

/// The most options one `choice` may have. The server's own limit.
pub const MAX_OPTIONS: usize = 255;

pub(super) fn question_body(q: &Question) -> Value {
    match q {
        Question::Noul { instructions, when_true, when_false } => json!({
            "type": "noul",
            "instructions": instructions,
            "criteria": { "true": when_true, "false": when_false },
        }),
        Question::Choice { instructions, options } => {
            let mut criteria = Map::new();
            for (id, text) in options {
                criteria.insert(id.clone(), Value::String(text.clone()));
            }
            json!({ "type": "choice", "instructions": instructions, "criteria": criteria })
        }
        Question::Score { instructions, levels } => json!({
            "type": "score",
            "instructions": instructions,
            "criteria": levels,
        }),
    }
}

/// The request body for `ask`, with `state` already cut to fit.
pub fn build_body(ask: &Ask<'_>, state: &str) -> Value {
    let mut questions = Map::new();
    for (id, q) in &ask.questions {
        questions.insert((*id).to_string(), question_body(q));
    }
    let mut body = json!({ "state": state, "questions": questions });
    if !ask.images.is_empty() {
        body["images"] = Value::Array(ask.images.iter().map(|i| Value::String(i.data_url.clone())).collect());
    }
    body
}

/// A question the server could never answer, found before sending it.
pub fn check_ask(ask: &Ask<'_>) -> Result<(), String> {
    if ask.questions.is_empty() {
        return Err("a decision needs at least one question".into());
    }
    for (id, q) in &ask.questions {
        match q {
            Question::Noul { .. } => {}
            Question::Choice { options, .. } => {
                if options.len() < 2 || options.len() > MAX_OPTIONS {
                    return Err(format!("question {id} needs 2 to {MAX_OPTIONS} options"));
                }
            }
            Question::Score { levels, .. } => {
                if !(2..=10).contains(&levels.len()) {
                    return Err(format!("question {id} needs 2 to 10 levels"));
                }
            }
        }
    }
    Ok(())
}

fn number(v: &Value) -> Option<f32> {
    v.as_f64().map(|n| n as f32)
}

/// Read the response of a request built from `ask`. Returns the answers and
/// the prompt's token count. Anything missing or out of range is an error: a
/// half-read decision is worse than none.
pub fn parse_response(ask: &Ask<'_>, reply: &Value) -> Result<(BTreeMap<String, Answer>, u32), String> {
    let answers = reply
        .get("answers")
        .and_then(Value::as_object)
        .ok_or("the reply has no answers")?;
    let mut out = BTreeMap::new();
    for (id, q) in &ask.questions {
        let a = answers.get(*id).ok_or_else(|| format!("no answer for {id}"))?;
        let parsed = match q {
            Question::Noul { .. } => {
                let p = a.get("noul").and_then(number).ok_or_else(|| format!("{id}: no noul value"))?;
                if !(0.0..=1.0).contains(&p) {
                    return Err(format!("{id}: probability {p} is outside 0 to 1"));
                }
                Answer::Noul { p_true: p }
            }
            Question::Choice { options, .. } => {
                let choice = a
                    .get("choice")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("{id}: no choice"))?;
                if !options.iter().any(|(o, _)| o == choice) {
                    return Err(format!("{id}: \"{choice}\" is not one of the options"));
                }
                let map = a
                    .get("probabilities")
                    .and_then(Value::as_object)
                    .ok_or_else(|| format!("{id}: no probabilities"))?;
                // In the order the question lists its options, not the map's.
                let probs: Vec<(String, f32)> = options
                    .iter()
                    .map(|(o, _)| (o.clone(), map.get(o).and_then(number).unwrap_or(0.0)))
                    .collect();
                let confidence = confidence(&probs.iter().map(|(_, p)| *p).collect::<Vec<_>>());
                Answer::Choice { choice: choice.to_string(), probs, confidence }
            }
            Question::Score { levels, .. } => {
                let score = a.get("score").and_then(number).ok_or_else(|| format!("{id}: no score"))?;
                let map = a
                    .get("probabilities")
                    .and_then(Value::as_object)
                    .ok_or_else(|| format!("{id}: no probabilities"))?;
                let probs: Vec<f32> = (0..levels.len())
                    .map(|i| map.get(&i.to_string()).and_then(number).unwrap_or(0.0))
                    .collect();
                let confidence = confidence(&probs);
                Answer::Score { score, probs, confidence }
            }
        };
        out.insert((*id).to_string(), parsed);
    }
    let tokens = reply
        .get("usage")
        .and_then(|u| u.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    Ok((out, tokens))
}

/// Post a request to a running decision engine.
pub async fn post(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    body: &Value,
    deadline: Duration,
) -> Result<Value, DecideError> {
    let resp = client
        .post(format!("{base_url}/v1/systemone"))
        .bearer_auth(token)
        .timeout(deadline)
        .json(body)
        .send()
        .await
        .map_err(|e| if e.is_timeout() { DecideError::Deadline } else { DecideError::Failed(e.to_string()) })?;
    let status = resp.status();
    if status.as_u16() == 501 {
        // The server's way of saying "this file has no decision head".
        return Err(DecideError::Failed("This file can't make decisions.".into()));
    }
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(DecideError::Failed(format!("HTTP {status}: {}", text.chars().take(300).collect::<String>())));
    }
    resp.json::<Value>()
        .await
        .map_err(|e| if e.is_timeout() { DecideError::Deadline } else { DecideError::Failed(e.to_string()) })
}

/// The fixed request of the head check (`DEC-3`): one question of each kind
/// with an obvious answer. A decision model answers all three well formed; a
/// model that only generates text answers HTTP 501.
pub fn head_check_ask() -> Ask<'static> {
    Ask {
        state: "Question: What is the capital of France?\n\nExcerpt: Paris is the capital and largest city of France.",
        images: Vec::new(),
        questions: vec![
            (
                "a_enough",
                Question::Noul {
                    instructions: "Does the excerpt answer the question?".into(),
                    when_true: "The excerpt contains the answer.".into(),
                    when_false: "The excerpt does not contain the answer.".into(),
                },
            ),
            (
                "b_topic",
                Question::Choice {
                    instructions: "Which topic is the excerpt about?".into(),
                    options: vec![
                        ("geography".into(), "Places and countries.".into()),
                        ("cooking".into(), "Food and recipes.".into()),
                        ("unsure".into(), "It cannot be told.".into()),
                    ],
                },
            ),
            (
                "c_direct",
                Question::Score {
                    instructions: "How directly does the excerpt answer the question?".into(),
                    levels: vec!["not at all".into(), "partly".into(), "directly".into()],
                },
            ),
        ],
    }
}

/// Did the head check pass? It does when the reply parses, and the obvious
/// question got the obvious answer (not asserted for the other two: only that
/// they are well formed).
pub fn head_check_passes(reply: &Value) -> bool {
    let ask = head_check_ask();
    match parse_response(&ask, reply) {
        Ok((answers, _)) => matches!(answers.get("a_enough"), Some(Answer::Noul { p_true }) if *p_true > 0.5),
        Err(_) => false,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent::decide::ImageRef;

    /// What the real server returned for `head_check_ask`-shaped questions
    /// (Clef-flash Q4_K_M, `b11418`), shortened.
    pub fn real_reply() -> Value {
        json!({
            "model": "clef-flash/Clef-Flash-Q4_K_M.gguf",
            "answers": {
                "a_enough": { "type": "noul", "noul": 0.9600926226624481 },
                "b_topic": {
                    "type": "choice", "choice": "geography",
                    "probabilities": { "cooking": 0.011853862980071985, "geography": 0.9743639064484216, "unsure": 0.013782230571506314 },
                    "confidence": 0.9615458596726323
                },
                "c_direct": {
                    "type": "score", "score": 1.91184698112007,
                    "legend": { "0": "not at all", "1": "partly", "2": "directly" },
                    "probabilities": { "0": 0.02254079408947159, "1": 0.04307143070098679, "2": 0.9343877752095416 },
                    "confidence": 0.867770471680105
                }
            },
            "usage": { "input_tokens": 334, "output_tokens": 0 }
        })
    }

    #[test]
    fn the_request_matches_the_wire_format() {
        let ask = head_check_ask();
        let body = build_body(&ask, ask.state);
        assert_eq!(body["state"], ask.state);
        assert!(body.get("images").is_none(), "no images, no field");
        assert_eq!(body["questions"]["a_enough"]["type"], "noul");
        // `noul` always carries both sides.
        assert_eq!(body["questions"]["a_enough"]["criteria"]["true"], "The excerpt contains the answer.");
        assert_eq!(body["questions"]["a_enough"]["criteria"]["false"], "The excerpt does not contain the answer.");
        // `choice` criteria are a map, `score` criteria an array.
        assert_eq!(body["questions"]["b_topic"]["criteria"]["cooking"], "Food and recipes.");
        assert!(body["questions"]["c_direct"]["criteria"].is_array());
        assert_eq!(body["questions"]["c_direct"]["criteria"][2], "directly");
    }

    #[test]
    fn images_go_in_as_data_urls_only_when_there_are_some() {
        let mut ask = head_check_ask();
        ask.images.push(ImageRef { data_url: "data:image/png;base64,AAAA".into() });
        let body = build_body(&ask, "s");
        assert_eq!(body["images"][0], "data:image/png;base64,AAAA");
        assert_eq!(body["state"], "s");
    }

    #[test]
    fn a_real_reply_is_read_into_answers() {
        let ask = head_check_ask();
        let (answers, tokens) = parse_response(&ask, &real_reply()).unwrap();
        assert_eq!(tokens, 334);
        assert!(matches!(answers["a_enough"], Answer::Noul { p_true } if (p_true - 0.96).abs() < 0.001));
        let Answer::Choice { choice, probs, confidence } = &answers["b_topic"] else { panic!("not a choice") };
        assert_eq!(choice, "geography");
        // In the question's own option order, not the map's.
        assert_eq!(probs.iter().map(|(o, _)| o.as_str()).collect::<Vec<_>>(), ["geography", "cooking", "unsure"]);
        // Our measure, not the server's 0.96.
        assert!((*confidence - 0.875).abs() < 0.01, "got {confidence}");
        let Answer::Score { score, probs, .. } = &answers["c_direct"] else { panic!("not a score") };
        assert!((score - 1.9118).abs() < 0.001);
        assert_eq!(probs.len(), 3);
        assert!(probs[2] > 0.9);
    }

    #[test]
    fn a_reply_that_misses_a_question_is_refused() {
        let ask = head_check_ask();
        let mut reply = real_reply();
        reply["answers"].as_object_mut().unwrap().remove("b_topic");
        assert!(parse_response(&ask, &reply).unwrap_err().contains("b_topic"));
    }

    #[test]
    fn a_choice_outside_the_options_is_refused() {
        let ask = head_check_ask();
        let mut reply = real_reply();
        reply["answers"]["b_topic"]["choice"] = json!("astronomy");
        assert!(parse_response(&ask, &reply).is_err());
    }

    #[test]
    fn a_probability_outside_zero_to_one_is_refused() {
        let ask = head_check_ask();
        let mut reply = real_reply();
        reply["answers"]["a_enough"]["noul"] = json!(1.4);
        assert!(parse_response(&ask, &reply).is_err());
    }

    #[test]
    fn generated_text_is_not_a_decision() {
        let reply = json!({ "choices": [{ "message": { "content": "Paris" } }] });
        assert!(!head_check_passes(&reply));
        assert!(head_check_passes(&real_reply()));
    }

    #[test]
    fn a_question_the_server_could_never_answer_is_found_early() {
        let mut ask = head_check_ask();
        assert!(check_ask(&ask).is_ok());
        ask.questions.push((
            "d",
            Question::Choice { instructions: "x".into(), options: vec![("only".into(), "one".into())] },
        ));
        assert!(check_ask(&ask).unwrap_err().contains("2 to 255"));
        ask.questions.pop();
        ask.questions.push(("e", Question::Score { instructions: "x".into(), levels: vec!["a".into(); 11] }));
        assert!(check_ask(&ask).unwrap_err().contains("2 to 10"));
        ask.questions.clear();
        assert!(check_ask(&ask).is_err());
    }
}
