//! Lessons that fire (`HAB-1`, `HAB-2`, `COGNITION_PLAN` III.5).
//!
//! A lesson with `fires_on: <tool>` is not in the memory index. When the loop
//! runs that tool, and the call's arguments carry the lesson's words, the
//! lesson's body is added to the tool result the model reads next. A lesson may
//! also name one check from a small fixed set. It runs after the tool returns,
//! on the file the call named. A failed check is one more line on the result.
//!
//! No model-written code runs here. A check is one of the five below, and the
//! lesson only picks which. Changing what a call is *given* (a rewrite rule) is
//! left to the person, on the lesson card, and is not in this build.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::memory::Fact;

/// The whole set. `Fires::check` holds one of these or nothing.
pub const CHECKS: &[&str] = &["header_row", "path_exists", "file_nonempty", "json_parses", "row_count"];

pub fn is_check(id: &str) -> bool {
    CHECKS.contains(&id)
}

/// The most of a lesson body that goes into one result.
const NOTE_CAP: usize = 600;
/// A file bigger than this is not read by a check.
const READ_CAP: u64 = 5_000_000;
/// Argument names the file tools use for the file they act on.
const PATH_KEYS: &[&str] = &["path", "file_path", "file", "filename"];

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail(String),
    /// The call did not give the check what it needs (no file, no folder).
    Skip,
}

/// What firing added to one call.
#[derive(Debug, Default)]
pub struct Spoken {
    /// Appended to the tool result the model reads.
    pub text: String,
    /// `(lesson name, one plain line)` for the step's expanded detail (`HAB-UI-1`).
    pub lines: Vec<(String, String)>,
}

/// Only the lessons that fire, once per run.
pub fn firing(lessons: Vec<Fact>) -> Vec<Fact> {
    lessons.into_iter().filter(|l| l.fires.is_some() && !l.is_superseded()).collect()
}

/// Everything the firing lessons have to say about one call. `output` is the tool's
/// result when it worked and `None` when it failed: a check has nothing to look at
/// after a failure, but the lesson itself may still help the retry.
pub fn speak(lessons: &[Fact], tool: &str, args: &Value, output: Option<&str>, folder: Option<&Path>) -> Spoken {
    let mut out = Spoken::default();
    let haystack = args.to_string().to_lowercase();
    for lesson in lessons {
        let Some(fires) = &lesson.fires else { continue };
        if !fires.on.eq_ignore_ascii_case(tool) {
            continue;
        }
        if let Some(words) = fires.when.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
            if !haystack.contains(&words.to_lowercase()) {
                continue;
            }
        }
        out.text.push_str(&format!(
            "\n\n[A lesson from earlier applies here ({}): {}]",
            lesson.name,
            clip(lesson.body.trim(), NOTE_CAP)
        ));
        let when = format!("You showed me on {}.", day_month(&lesson.created));
        let verdict = match (fires.check.as_deref().filter(|c| is_check(c)), output) {
            (Some(id), Some(output)) => Some((id, run_check(id, args, output, folder))),
            _ => None,
        };
        let line = match verdict {
            Some((id, Verdict::Pass)) => format!("Checked {}. {when}", label(id)),
            Some((id, Verdict::Fail(why))) => {
                out.text.push_str(&format!("\n[Check failed: {why}. Fix this before you go on.]"));
                format!("Checked {}: {why}. {when}", label(id))
            }
            _ => format!("{} {when}", sentence(&lesson.description)),
        };
        out.lines.push((lesson.name.clone(), line));
    }
    out
}

/// Run one check of the fixed set against what a call did.
pub fn run_check(id: &str, args: &Value, output: &str, folder: Option<&Path>) -> Verdict {
    let Some(raw) = path_arg(args) else {
        // `json_parses` can still read the tool's own result.
        return if id == "json_parses" { json_verdict(output) } else { Verdict::Skip };
    };
    let Some(path) = anchored(&raw, folder) else { return Verdict::Skip };
    match id {
        "path_exists" => {
            if path.exists() { Verdict::Pass } else { Verdict::Fail(format!("{raw} does not exist")) }
        }
        "file_nonempty" => match std::fs::metadata(&path) {
            Ok(m) if m.len() > 0 => Verdict::Pass,
            Ok(_) => Verdict::Fail(format!("{raw} is empty")),
            Err(_) => Verdict::Fail(format!("{raw} does not exist")),
        },
        "json_parses" => match read_text(&path) {
            Ok(text) => json_verdict(&text),
            Err(why) => Verdict::Fail(why),
        },
        "header_row" => match read_text(&path) {
            Ok(text) => header_verdict(&text),
            Err(why) => Verdict::Fail(why),
        },
        "row_count" => {
            // Only a write has an expected count: the `content` the call carried.
            let Some(sent) = args.get("content").and_then(Value::as_str) else { return Verdict::Skip };
            match read_text(&path) {
                Ok(text) => {
                    let (want, have) = (rows(sent), rows(&text));
                    if want == have {
                        Verdict::Pass
                    } else {
                        Verdict::Fail(format!("{raw} has {have} rows, the call wrote {want}"))
                    }
                }
                Err(why) => Verdict::Fail(why),
            }
        }
        _ => Verdict::Skip,
    }
}

fn json_verdict(text: &str) -> Verdict {
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(_) => Verdict::Pass,
        Err(e) => Verdict::Fail(format!("the JSON does not read ({e})")),
    }
}

/// A header row is a first row of words: no cell empty, none a bare number.
fn header_verdict(text: &str) -> Verdict {
    let Some(first) = text.lines().find(|l| !l.trim().is_empty()) else {
        return Verdict::Fail("the file has no rows".into());
    };
    let delimiter = [',', ';', '\t'].into_iter().max_by_key(|d| first.matches(*d).count()).unwrap_or(',');
    let cells: Vec<&str> = first.split(delimiter).map(|c| c.trim().trim_matches('"')).collect();
    if cells.iter().any(|c| c.is_empty()) || cells.iter().any(|c| c.parse::<f64>().is_ok()) {
        Verdict::Fail("the first row looks like data, not a header".into())
    } else {
        Verdict::Pass
    }
}

fn rows(text: &str) -> usize {
    text.lines().filter(|l| !l.trim().is_empty()).count()
}

fn path_arg(args: &Value) -> Option<String> {
    PATH_KEYS.iter().find_map(|k| args.get(*k).and_then(Value::as_str)).map(str::to_string)
}

/// The same rule the file tools use: a relative path hangs off the working folder.
/// With no folder there is nothing to measure it against, so the check is skipped.
fn anchored(raw: &str, folder: Option<&Path>) -> Option<PathBuf> {
    let p = Path::new(raw);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    folder.map(|root| root.join(p))
}

fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|_| format!("{} does not exist", path.display()))?;
    if meta.len() > READ_CAP {
        return Err(format!("{} is too big to check", path.display()));
    }
    std::fs::read_to_string(path).map_err(|_| format!("{} cannot be read as text", path.display()))
}

/// The check, if any, that would have caught a failure with this error text. Plain
/// words in the error decide; nothing here asks a model.
pub fn check_for(error: &str) -> Option<&'static str> {
    let e = error.to_lowercase();
    if e.contains("header") {
        Some("header_row")
    } else if e.contains("json") {
        Some("json_parses")
    } else if e.contains("no such file") || e.contains("not found") || e.contains("does not exist") {
        Some("path_exists")
    } else if e.contains("empty") {
        Some("file_nonempty")
    } else {
        None
    }
}

/// What a check looks at, in words, for the line the person reads.
pub fn label(id: &str) -> &'static str {
    match id {
        "header_row" => "the header row",
        "path_exists" => "that the file is there",
        "file_nonempty" => "that the file is not empty",
        "json_parses" => "that the JSON reads",
        "row_count" => "the row count",
        _ => "the result",
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max).collect();
    s.push('\u{2026}');
    s
}

/// A description as a sentence: first letter up, one full stop at the end.
fn sentence(description: &str) -> String {
    let d = description.trim().trim_end_matches('.');
    let mut chars = d.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => "I remembered something.".into(),
    }
}

/// `2026-10-04` as `4 Oct`. A date that does not read comes back as it is.
pub fn day_month(date: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let parts: Vec<&str> = date.trim().split('-').collect();
    if let [_, m, d] = parts[..] {
        if let (Ok(m), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>()) {
            if (1..=12).contains(&m) {
                return format!("{d} {}", MONTHS[m - 1]);
            }
        }
    }
    date.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fires;
    use serde_json::json;

    fn lesson(on: &str, when: Option<&str>, check: Option<&str>) -> Fact {
        Fact {
            name: "check-header".into(),
            description: "check the header row before you read a CSV".into(),
            kind: "lesson".into(),
            created: "2026-10-04".into(),
            body: "Look at the first row before you use the columns.".into(),
            fires: Some(Fires { on: on.into(), when: when.map(str::to_string), check: check.map(str::to_string) }),
            ..Default::default()
        }
    }

    #[test]
    fn a_lesson_speaks_at_its_tool_and_nowhere_else() {
        let lessons = vec![lesson("read_file", None, None)];
        let at = speak(&lessons, "read_file", &json!({"path": "a.csv"}), Some("x"), None);
        assert!(at.text.contains("Look at the first row"));
        assert_eq!(at.lines.len(), 1);
        assert_eq!(at.lines[0].1, "Check the header row before you read a CSV. You showed me on 4 Oct.");
        let elsewhere = speak(&lessons, "write_file", &json!({"path": "a.csv"}), Some("x"), None);
        assert!(elsewhere.text.is_empty() && elsewhere.lines.is_empty());
    }

    #[test]
    fn the_words_must_be_in_the_arguments() {
        let lessons = vec![lesson("read_file", Some(".CSV"), None)];
        assert!(!speak(&lessons, "read_file", &json!({"path": "notes.md"}), Some("x"), None).text.contains("lesson"));
        assert!(speak(&lessons, "read_file", &json!({"path": "data.csv"}), Some("x"), None).text.contains("lesson"));
    }

    #[test]
    fn a_firing_lesson_is_left_out_of_the_index() {
        let l = lesson("read_file", None, None);
        assert!(!l.in_prompt());
        let mut plain = l.clone();
        plain.fires = None;
        assert!(plain.in_prompt());
    }

    #[test]
    fn the_header_check_tells_a_header_from_data() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("good.csv"), "name,city\nAnna,Hamburg\n").unwrap();
        std::fs::write(dir.path().join("bad.csv"), "1,2,3\n4,5,6\n").unwrap();
        let ok = run_check("header_row", &json!({"path": "good.csv"}), "", Some(dir.path()));
        assert_eq!(ok, Verdict::Pass);
        let bad = run_check("header_row", &json!({"path": "bad.csv"}), "", Some(dir.path()));
        assert!(matches!(bad, Verdict::Fail(_)));
    }

    #[test]
    fn a_failed_check_is_a_line_on_the_result() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.csv"), "1,2,3\n").unwrap();
        let lessons = vec![lesson("write_file", None, Some("header_row"))];
        let said = speak(&lessons, "write_file", &json!({"path": "bad.csv"}), Some("wrote 1 line"), Some(dir.path()));
        assert!(said.text.contains("[Check failed: the first row looks like data"));
        assert!(said.lines[0].1.starts_with("Checked the header row: the first row looks like data"));
        // A tool that failed has nothing to check; the lesson still speaks.
        let failed = speak(&lessons, "write_file", &json!({"path": "bad.csv"}), None, Some(dir.path()));
        assert!(failed.text.contains("lesson") && !failed.text.contains("Check failed"));
    }

    #[test]
    fn the_other_checks() {
        let dir = tempfile::tempdir().unwrap();
        let at = Some(dir.path());
        std::fs::write(dir.path().join("empty.txt"), "").unwrap();
        std::fs::write(dir.path().join("a.json"), r#"{"a":1}"#).unwrap();
        std::fs::write(dir.path().join("b.json"), "{oops").unwrap();
        std::fs::write(dir.path().join("rows.csv"), "h\n1\n2\n").unwrap();
        assert!(matches!(run_check("path_exists", &json!({"path": "gone"}), "", at), Verdict::Fail(_)));
        assert_eq!(run_check("path_exists", &json!({"path": "empty.txt"}), "", at), Verdict::Pass);
        assert!(matches!(run_check("file_nonempty", &json!({"path": "empty.txt"}), "", at), Verdict::Fail(_)));
        assert_eq!(run_check("json_parses", &json!({"path": "a.json"}), "", at), Verdict::Pass);
        assert!(matches!(run_check("json_parses", &json!({"path": "b.json"}), "", at), Verdict::Fail(_)));
        assert_eq!(run_check("json_parses", &json!({}), r#"[1,2]"#, at), Verdict::Pass);
        assert_eq!(run_check("row_count", &json!({"path": "rows.csv", "content": "h\n1\n2\n"}), "", at), Verdict::Pass);
        assert!(matches!(run_check("row_count", &json!({"path": "rows.csv", "content": "h\n1\n"}), "", at), Verdict::Fail(_)));
        // No folder and a relative path: nothing to measure against.
        assert_eq!(run_check("path_exists", &json!({"path": "rows.csv"}), "", None), Verdict::Skip);
    }

    #[test]
    fn an_error_names_the_check_that_would_have_caught_it() {
        assert_eq!(check_for("Missing header row in data.csv"), Some("header_row"));
        assert_eq!(check_for("No such file or directory"), Some("path_exists"));
        assert_eq!(check_for("expected JSON at line 1"), Some("json_parses"));
        assert_eq!(check_for("permission denied"), None);
    }

    #[test]
    fn dates_read_as_a_person_says_them() {
        assert_eq!(day_month("2026-10-04"), "4 Oct");
        assert_eq!(day_month("nonsense"), "nonsense");
    }
}
