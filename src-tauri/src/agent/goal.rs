//! `GOL`: what is left of the old `/goal` check.
//!
//! `/goal` is now the Mission Command way of working (`ORG_LOOP_PLAN` `WRK-12`):
//! the goal's condition is an agreed criterion, checked by `aside::criteria_messages`.
//! The `goal-met` decision purpose keeps its seed rows and calibration, and they
//! are written in the state this builds.

/// How much of my last answer the judge reads. A goal is judged on how the work
/// ended, and the end of a long answer is where that is said.
const ANSWER_CHARS: usize = 4000;

/// The state the `goal-met` decision reads.
pub fn decision_state(objective: &str, until: &str, last_answer: &str, changes: &str) -> String {
    format!(
        "Goal: {objective}\nDone when: {until}\nFiles the assistant changed: {changes}\n\
         The assistant's last answer:\n{}",
        tail(last_answer.trim(), ANSWER_CHARS)
    )
}

/// The tail of `text`, so the end of a long answer is what survives the cut.
fn tail(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    format!("…{}", chars[chars.len() - max..].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decision_reads_the_end_of_a_long_answer() {
        let long = format!("{}THE END", "x".repeat(10_000));
        let state = decision_state("g", "u", &long, "2 file(s): a.rs, b.rs");
        assert!(state.contains("Goal: g") && state.contains("Done when: u"));
        assert!(state.contains("a.rs, b.rs"));
        assert!(state.contains("THE END"), "the end of a long answer is what is read");
    }
}
