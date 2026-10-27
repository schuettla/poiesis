//! `USE-7`: a scheduled task may carry a condition in plain words, "only run if
//! there is a new release since last time". Before the task runs, one yes-or-no
//! question asks whether the condition holds over what the task would work on. At
//! 0.5 or more the task runs; below it the run is skipped and the task's history
//! says so, with the probability.
//!
//! A task is never skipped by a model that was not asked. No engine, a cold or
//! busy one, an exhausted budget, an answer of the wrong shape, or an input that
//! could not be read at all: each runs the task as it always ran. What the model
//! reads is the *input*: the task's own folder, the pages the task's own text
//! names, and what the last run said. It is the same text the task would have read
//! itself, so nothing leaves the computer that was not going to, and the check is
//! a background purpose, so it stays on this computer.

use std::path::Path;

use super::control::{self, Act, Gate};
use super::policy::Purpose;
use super::words;
use super::sites::SiteAsk;
use super::{Answer, Opts, Question, Verdict};

/// How much input the check reads, in characters.
pub const INPUT_CHARS: usize = 5_000;
/// Folder entries listed. Newest first, so the cut drops the oldest.
pub const LISTED: usize = 60;
/// Pages read for one check.
pub const MAX_PAGES: usize = 2;
/// Characters of one page.
pub const PAGE_CHARS: usize = 2_400;
/// Characters of the last run's result.
const RESULT_CHARS: usize = 400;

/// The question for one task. The id is `run`.
pub fn condition_ask(condition: &str, input: &str) -> SiteAsk {
    SiteAsk {
        state: format!(
            "What a scheduled task would work on:\n{}",
            input.chars().take(INPUT_CHARS).collect::<String>()
        ),
        id: "run",
        question: Question::Noul {
            instructions: format!(
                "A task should run only when this is true: \"{}\". Judging only by the input, is it true right now?",
                condition.trim()
            ),
            when_true: "The condition holds, so the task has something to do.".into(),
            when_false: "The condition does not hold, so there is nothing for the task to do this time.".into(),
        },
    }
}

/// What to do with a task.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Due {
    Run,
    /// Skip it: the condition held with this probability, which is below the bar.
    Skip(f32),
}

/// Read one verdict. An answer that is not a probability for this question runs
/// the task.
pub fn due_from(v: &Verdict) -> Due {
    let Some(answer @ Answer::Noul { p_true }) = v.get("run") else { return Due::Run };
    if control::evaluate(Purpose::RunIf, answer, v.calibrated, Gate::Open) == Act::Go {
        Due::Run
    } else {
        Due::Skip(*p_true)
    }
}

/// The line the task's history and the log show for a skip (`SEE-4`). `p` is the
/// chance the condition held, so how sure the skip is, is what is left. The number is
/// the row's hover.
pub fn skipped_note(p: f32, calibrated: bool) -> String {
    format!("Skipped: the condition {} hold", words::say(1.0 - p, calibrated, "did not", "probably did not"))
}

/// Ask whether a task should run. `Run` whenever the answer cannot be had.
pub async fn check(condition: &str, input: &str) -> Due {
    if condition.trim().is_empty() || input.trim().is_empty() {
        return Due::Run;
    }
    let opts = Opts::default();
    let site = condition_ask(condition, input);
    let Ok(verdict) = super::ask(&site.as_ask(), Purpose::RunIf, opts).await else { return Due::Run };
    let due = due_from(&verdict);
    super::settle(&verdict, Purpose::RunIf, if due == Due::Run { Act::Go } else { Act::Below }, opts);
    due
}

// ---- the input a condition is judged over ----

/// How long ago, in the words a task's history uses.
pub fn ago(ms: i64) -> String {
    let minutes = (ms / 60_000).max(0);
    match minutes {
        0..=1 => "just now".into(),
        2..=119 => format!("{minutes} minutes ago"),
        120..=2_879 => format!("{} hours ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1_440),
    }
}

/// The top level of a folder, newest first, with what changed since the task last
/// ran marked. `None` when the folder cannot be read or is empty.
pub fn folder_listing(dir: &Path, since: Option<i64>) -> Option<String> {
    let mut rows: Vec<(i64, u64, String, bool)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let name = e.file_name().to_string_lossy().to_string();
            Some((modified, if meta.is_dir() { 0 } else { meta.len() }, name, meta.is_dir()))
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
    let total = rows.len();
    let changed = since.map_or(0, |s| rows.iter().filter(|r| r.0 > s).count());
    let mut out = match since {
        Some(_) => format!("{total} entries in the task's folder, {changed} changed since the last run. Newest first:\n"),
        None => format!("{total} entries in the task's folder. Newest first:\n"),
    };
    for (modified, size, name, is_dir) in rows.iter().take(LISTED) {
        let mark = if since.is_some_and(|s| *modified > s) { " [changed since the last run]" } else { "" };
        let kind = if *is_dir { "folder".to_string() } else { format!("{size} bytes") };
        out.push_str(&format!("- {name} ({kind}){mark}\n"));
    }
    if total > LISTED {
        out.push_str(&format!("... and {} older entries\n", total - LISTED));
    }
    Some(out)
}

/// The web addresses a task's own words name, at most [`MAX_PAGES`], in order.
/// Only what the user wrote into the task: nothing found in a page is followed.
pub fn urls_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_start_matches(['(', '[', '<', '"', '\'', '`']);
        let word = word.trim_end_matches([')', ']', '>', '"', '\'', '`', '.', ',', ';', ':', '!', '?']);
        if (word.starts_with("https://") || word.starts_with("http://")) && word.len() > 12 && !out.iter().any(|u| u == word) {
            out.push(word.to_string());
            if out.len() == MAX_PAGES {
                break;
            }
        }
    }
    out
}

/// Put the pieces together. `None` when there is nothing to judge, which runs the
/// task: a condition over no input is not a reason to skip anything.
pub fn render_input(listing: Option<&str>, pages: &[(String, String)], last: Option<(&str, &str)>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(listing) = listing {
        parts.push(listing.trim_end().to_string());
    }
    for (url, text) in pages {
        let text: String = text.chars().take(PAGE_CHARS).collect();
        parts.push(format!("The page {url}, read just now:\n{text}"));
    }
    let evidence = !parts.is_empty();
    if let Some((when, result)) = last {
        let result: String = result.chars().take(RESULT_CHARS).collect();
        parts.push(format!("The task last ran {when}. Its result then: {result}"));
    }
    // What the last run said is context for the other pieces, not an input of its
    // own: with no folder and no page there is nothing new to judge.
    evidence.then(|| parts.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::decide::{local, BackendId};
    use std::collections::BTreeMap;

    fn verdict(id: &str, p_true: f32, calibrated: bool) -> Verdict {
        Verdict {
            id: "d".into(),
            answers: BTreeMap::from([(id.to_string(), Answer::Noul { p_true })]),
            backend: BackendId::Local { model: "test".into() },
            calibrated,
            ms: 1,
        }
    }

    #[test]
    fn the_question_is_one_the_server_accepts_and_carries_the_condition() {
        let ask = condition_ask("there is a new release since last time", "v1.2 released today");
        assert!(local::check_ask(&ask.as_ask()).is_ok());
        let Question::Noul { instructions, .. } = &ask.question else { panic!("not a yes or no") };
        assert!(instructions.contains("there is a new release since last time"));
        assert!(ask.state.contains("v1.2 released today"));
        assert!(condition_ask("c", &"x".repeat(50_000)).state.chars().count() < INPUT_CHARS + 100);
    }

    /// The task runs at 0.5 and above and is skipped below it, with the number.
    #[test]
    fn a_task_runs_at_a_half_and_is_skipped_below_it() {
        assert_eq!(due_from(&verdict("run", 0.5, true)), Due::Run);
        assert_eq!(due_from(&verdict("run", 0.92, false)), Due::Run);
        assert_eq!(due_from(&verdict("run", 0.07, true)), Due::Skip(0.07));
        assert_eq!(due_from(&verdict("run", 0.49, false)), Due::Skip(0.49));
        // An answer to another question, or none, never skips a task.
        assert_eq!(due_from(&verdict("learn", 0.01, true)), Due::Run);
        assert_eq!(skipped_note(0.07, false), "Skipped: the condition did not hold");
        assert_eq!(skipped_note(0.4, false), "Skipped: the condition probably did not hold");
    }

    #[test]
    fn a_folder_listing_is_newest_first_and_marks_what_changed() {
        let dir = std::env::temp_dir().join(format!("poiesis_runif_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("old.txt"), "old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let since = crate::db::now_ms();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("new.txt"), "newer text").unwrap();

        let with = folder_listing(&dir, Some(since)).unwrap();
        assert!(with.starts_with("3 entries in the task's folder, 1 changed since the last run."), "{with}");
        assert!(with.contains("- new.txt (10 bytes) [changed since the last run]"), "{with}");
        assert!(with.contains("- old.txt (3 bytes)\n") && with.contains("- sub (folder)"), "{with}");
        assert!(with.find("new.txt").unwrap() < with.find("old.txt").unwrap(), "newest first");

        let without = folder_listing(&dir, None).unwrap();
        assert!(without.starts_with("3 entries in the task's folder. Newest first:"));
        assert!(!without.contains("changed since"));

        assert!(folder_listing(&dir.join("missing"), None).is_none());
        let empty = dir.join("sub");
        assert!(folder_listing(&empty, None).is_none(), "an empty folder is no input");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_folder_is_cut_to_its_newest_entries() {
        let dir = std::env::temp_dir().join(format!("poiesis_runif_many_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..(LISTED + 5) {
            std::fs::write(dir.join(format!("f{i:03}.txt")), "x").unwrap();
        }
        let listing = folder_listing(&dir, None).unwrap();
        assert_eq!(listing.lines().filter(|l| l.starts_with("- ")).count(), LISTED);
        assert!(listing.contains("... and 5 older entries"), "{listing}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_addresses_the_user_wrote_are_read_and_at_most_two() {
        let text = "Summarise new releases of (https://github.com/a/b/releases), then https://example.com/feed.xml. \
                    Also http://third.example/x and https://github.com/a/b/releases again, not ftp://x.y/z or www.nope.com.";
        assert_eq!(urls_in(text), vec!["https://github.com/a/b/releases", "https://example.com/feed.xml"]);
        assert!(urls_in("no links here, https:// alone is nothing").is_empty());
    }

    #[test]
    fn nothing_new_to_read_means_nothing_to_judge() {
        // The last run's result is context only: alone it is no input.
        assert_eq!(render_input(None, &[], Some(("2 days ago", "Found v1.1"))), None);
        assert_eq!(render_input(None, &[], None), None);
        let both = render_input(
            Some("2 entries in the task's folder."),
            &[("https://x.example".into(), "Release 1.2 is out".into())],
            Some(("2 days ago", "Found v1.1")),
        )
        .unwrap();
        assert!(both.contains("2 entries") && both.contains("Release 1.2 is out"));
        assert!(both.ends_with("The task last ran 2 days ago. Its result then: Found v1.1"), "{both}");
    }

    #[test]
    fn the_time_since_a_run_is_said_the_way_a_person_would() {
        assert_eq!(ago(10_000), "just now");
        assert_eq!(ago(45 * 60_000), "45 minutes ago");
        assert_eq!(ago(5 * 3_600_000), "5 hours ago");
        assert_eq!(ago(3 * 86_400_000), "3 days ago");
        assert_eq!(ago(-5), "just now");
    }

    /// With no app a task is never skipped.
    #[tokio::test]
    async fn without_an_engine_the_task_runs() {
        assert_eq!(check("a new release", "Release 1.2 is out").await, Due::Run);
        assert_eq!(check("", "anything").await, Due::Run);
        assert_eq!(check("a new release", "  ").await, Due::Run);
    }
}
