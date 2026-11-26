//! `PRF-3`: the unfinished-work scan.
//!
//! A pure function over the lines a change added and removed: no model, no file
//! reads. It finds what a change leaves open (a new TODO, a test switched off, a
//! body that only says "not done") and one kind of quiet loss (a refusal that was
//! removed with no test that it still refuses). The categories and the word lists
//! are the ones oh-my-hermes' `completion_integrity.py` uses, written fresh here.

/// What one file of a change did, as lines.
pub struct Changed<'a> {
    pub path: &'a str,
    pub added: &'a [String],
    pub removed: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// TODO, FIXME, XXX or HACK in a new comment.
    Unfinished { path: String, line: String },
    /// A test switched off.
    TestOff { path: String, line: String },
    /// A body that only says "not done".
    Stub { path: String, line: String },
    /// A refusal line was removed, and no test was added that it still refuses.
    GuardRemoved { path: String, line: String },
}

impl Finding {
    /// The short item the person reads (Part VI of the plan).
    pub fn words(&self) -> String {
        match self {
            Finding::Unfinished { path, .. } => format!("a new TODO in {path}"),
            Finding::TestOff { path, .. } => format!("a test switched off in {path}"),
            Finding::Stub { path, .. } => format!("a stub in {path}"),
            Finding::GuardRemoved { path, .. } => format!("a removed check in {path} with no test that it still refuses"),
        }
    }

    /// The line that was found, for a tooltip.
    pub fn line(&self) -> &str {
        match self {
            Finding::Unfinished { line, .. }
            | Finding::TestOff { line, .. }
            | Finding::Stub { line, .. }
            | Finding::GuardRemoved { line, .. } => line,
        }
    }
}

/// The items of `findings`, each said once, in order.
pub fn items(findings: &[Finding]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in findings {
        let w = f.words();
        if !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

const CODE: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "swift", "c", "cc", "cpp", "h", "hpp", "cs", "rb",
    "php", "scala", "sh", "ps1",
];
/// Where a `#` starts a comment.
const HASH_COMMENT: &[&str] = &["py", "rb", "sh", "ps1"];

const MARKERS: &[&str] = &["TODO", "FIXME", "XXX", "HACK"];
/// Not a bare `.skip(`: that is also every iterator's `.skip(n)`.
const TEST_OFF: &[&str] =
    &["it.skip", "describe.skip", "test.skip", "xit(", "xdescribe(", "#[ignore", "@pytest.mark.skip", "@unittest.skip", "t.Skip("];
const GUARD_WORDS: &[&str] = &["refuse", "reject", "deny", "forbid", "invalid", "unauthorized", "not allowed", "permission"];
const GUARD_SHAPES: &[&str] = &["if ", "return Err", "Err(", "throw ", "raise ", "assert", "bail!"];
/// A test of a refusal says one of these in its name.
const TEST_NAME_WORDS: &[&str] = &["refuse", "reject", "deny", "block", "invalid"];
/// Lines that start a test, for the added-test check.
const TEST_ATTRS: &[&str] = &["#[test]", "#[tokio::test]", "@Test", "@pytest", "def test", "func Test"];
const DECLARES: &[&str] = &["fn ", "def ", "func ", "it(", "test(", "describe("];

fn extension(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().and_then(|f| f.rsplit_once('.')).map(|(_, e)| e).unwrap_or("")
}

fn is_code(path: &str) -> bool {
    let e = extension(path).to_ascii_lowercase();
    CODE.contains(&e.as_str())
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Does `line` hold `pat`, not as the end of a longer name (`exit(` is not `xit(`)?
fn has(line: &str, pat: &str) -> bool {
    let starts_name = pat.chars().next().is_some_and(is_word_char);
    line.match_indices(pat).any(|(i, _)| !starts_name || !line[..i].chars().next_back().is_some_and(is_word_char))
}

/// The text of a comment on this line, if it has one.
fn comment_part<'a>(line: &'a str, ext: &str) -> Option<&'a str> {
    let mut found: Option<usize> = None;
    let mut see = |at: Option<usize>, skip: usize| {
        if let Some(i) = at {
            if found.map_or(true, |f| i + skip < f) {
                found = Some(i + skip);
            }
        }
    };
    see(line.find("//"), 2);
    see(line.find("/*"), 2);
    if HASH_COMMENT.contains(&ext) {
        see(line.find('#'), 1);
    }
    let trimmed = line.trim_start();
    if found.is_none() && trimmed.starts_with('*') {
        return Some(&trimmed[1..]);
    }
    found.map(|i| &line[i..])
}

/// An issue number (`#123`) or a link on the line: someone tracks it.
fn tracked(line: &str) -> bool {
    line.contains("http://")
        || line.contains("https://")
        || line.match_indices('#').any(|(i, _)| line[i + 1..].chars().next().is_some_and(|c| c.is_ascii_digit()))
}

fn has_marker(comment: &str) -> bool {
    comment.split(|c: char| !is_word_char(c)).any(|w| MARKERS.contains(&w))
}

/// Our own real-agent tests: `#[ignore = "needs Codex installed and signed in"]`.
fn ignored_for_a_missing_program(line: &str) -> bool {
    let Some(at) = line.find("#[ignore") else { return false };
    let rest = line[at + "#[ignore".len()..].trim_start();
    rest.strip_prefix('=').is_some_and(|r| r.trim_start().starts_with("\"needs"))
}

fn clipped(line: &str) -> String {
    let t = line.trim();
    if t.chars().count() > 120 {
        t.chars().take(120).collect::<String>() + "…"
    } else {
        t.to_string()
    }
}

fn is_stub(line: &str) -> bool {
    if line.contains("todo!(") || line.contains("unimplemented!(") || line.contains("raise NotImplementedError") {
        return true;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("not implemented") && (lower.contains("throw") || lower.contains("panic!("))
}

fn is_guard(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    GUARD_WORDS.iter().any(|w| lower.contains(w)) && GUARD_SHAPES.iter().any(|s| line.contains(s))
}

/// Did the change add a test whose name says it refuses, rejects, denies, blocks or
/// catches the invalid? A line that declares something with such a word in it, in a
/// test file or next to a test attribute.
fn adds_a_refusal_test(changes: &[Changed<'_>]) -> bool {
    changes.iter().any(|c| {
        let test_file = {
            let p = c.path.to_ascii_lowercase();
            p.contains("test") || p.contains("spec")
        };
        let test_here = test_file || c.added.iter().any(|l| TEST_ATTRS.iter().any(|a| l.contains(a)));
        test_here
            && c.added.iter().any(|l| {
                let lower = l.to_ascii_lowercase();
                DECLARES.iter().any(|d| l.contains(d)) && TEST_NAME_WORDS.iter().any(|w| lower.contains(w))
            })
    })
}

/// What a run left open: the scan over the lines its changes added and removed
/// (`PRF-4`). A file the run deleted is not scanned: nothing of it is left.
pub fn of_change_set(set: &super::changes::ChangeSet) -> Vec<Finding> {
    use super::diff::LineKind;
    let lines = |file: &super::changes::FileChange, kind: LineKind| -> Vec<String> {
        file.hunks.iter().flat_map(|h| &h.lines).filter(|l| l.kind == kind).map(|l| l.text.clone()).collect()
    };
    let owned: Vec<(String, Vec<String>, Vec<String>)> = set
        .files
        .iter()
        .filter(|f| f.status != "deleted")
        .map(|f| (f.display.clone(), lines(f, LineKind::Added), lines(f, LineKind::Removed)))
        .collect();
    let changed: Vec<Changed<'_>> = owned.iter().map(|(path, added, removed)| Changed { path, added, removed }).collect();
    scan(&changed)
}

/// The note a run is given before its answer when its changes left something open
/// (`PRF-4`): at most five items, then "and N more". `None` when there are none.
pub fn note(items: &[String]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let mut list = items.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    if items.len() > 5 {
        list.push_str(&format!(" and {} more", items.len() - 5));
    }
    Some(format!("Before you answer: {list}. Finish them, or say plainly in your answer that they are left open."))
}

/// What a change leaves open. Code files only: Markdown, text and data are skipped.
pub fn scan(changes: &[Changed<'_>]) -> Vec<Finding> {
    let mut out = Vec::new();
    let code: Vec<&Changed<'_>> = changes.iter().filter(|c| is_code(c.path)).collect();
    // A guard that moved to another place in the change was not removed.
    let came_back = |line: &str| {
        let t = line.trim();
        code.iter().any(|c| c.added.iter().any(|a| a.trim() == t))
    };
    let test_added = adds_a_refusal_test(changes);

    for c in &code {
        let ext = extension(c.path).to_ascii_lowercase();
        for line in c.added {
            let path = c.path.to_string();
            let found = || clipped(line);
            if !tracked(line) && comment_part(line, &ext).is_some_and(has_marker) {
                out.push(Finding::Unfinished { path: path.clone(), line: found() });
            }
            if TEST_OFF.iter().any(|p| has(line, p)) && !tracked(line) && !ignored_for_a_missing_program(line) {
                out.push(Finding::TestOff { path: path.clone(), line: found() });
            }
            if is_stub(line) {
                out.push(Finding::Stub { path: path.clone(), line: found() });
            }
        }
        if !test_added {
            for line in c.removed {
                if is_guard(line) && !came_back(line) {
                    out.push(Finding::GuardRemoved { path: c.path.to_string(), line: clipped(line) });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().map(|l| l.to_string()).collect()
    }

    fn added(path: &str, a: &[&str]) -> Vec<Finding> {
        let a = lines(a);
        scan(&[Changed { path, added: &a, removed: &[] }])
    }

    #[test]
    fn a_new_todo_in_a_comment_is_found_and_the_item_names_the_file() {
        let f = added("src/a.rs", &["    // TODO: handle the empty case"]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].words(), "a new TODO in src/a.rs");
        assert_eq!(f[0].line(), "// TODO: handle the empty case");
        assert_eq!(added("a.py", &["x = 1  # FIXME later"]).len(), 1);
        assert_eq!(added("a.ts", &["/* HACK */", " * XXX remove"]).len(), 2);
    }

    #[test]
    fn a_todo_that_is_not_a_comment_or_not_the_whole_word_is_not_found() {
        assert!(added("a.rs", &["let todo = 1;", "// the TODOS are done", "// todo lower case"]).is_empty());
        // `#` is a comment only in the languages that say so.
        assert!(added("a.rs", &["let s = \"# TODO\";"]).is_empty());
        assert!(added("a.rs", &["#[derive(Debug)] struct TODO;"]).is_empty());
    }

    #[test]
    fn a_todo_with_an_issue_number_or_a_link_is_tracked_and_not_a_finding() {
        assert!(added("a.rs", &["// TODO(#123): later"]).is_empty());
        assert!(added("a.ts", &["// TODO see https://example.com/issue/4"]).is_empty());
        assert!(added("a.py", &["# FIXME http://x.test"]).is_empty());
    }

    #[test]
    fn a_todo_in_a_markdown_file_or_a_folder_of_docs_is_skipped() {
        assert!(added("README.md", &["- TODO: write this"]).is_empty());
        assert!(added("docs/plan.txt", &["// TODO"]).is_empty());
        assert!(added("data.json", &["\"// TODO\""]).is_empty());
    }

    #[test]
    fn a_test_switched_off_is_found() {
        for line in [
            "  it.skip('works', () => {})",
            "describe.skip('x', () => {})",
            "xit('y', () => {})",
            "#[ignore]",
            "@pytest.mark.skip(reason='later')",
            "    t.Skip(\"later\")",
            "test.skip('z', () => {})",
        ] {
            let ext = if line.contains("pytest") { "a.py" } else if line.contains("t.Skip") { "a_test.go" } else if line.contains("ignore") { "a.rs" } else { "a.test.ts" };
            let f = added(ext, &[line]);
            assert_eq!(f.len(), 1, "{line}");
            assert_eq!(f[0].words(), format!("a test switched off in {ext}"));
        }
    }

    #[test]
    fn a_test_turned_off_for_a_tracked_reason_or_a_missing_program_is_not_found() {
        assert!(added("a.test.ts", &["it.skip('x', f) // #42"]).is_empty());
        assert!(added("a.rs", &["#[ignore = \"needs Codex installed and signed in\"]"]).is_empty());
        assert!(added("a.rs", &["#[ignore=\"needs the real agent\"]"]).is_empty());
        // Any other reason is a finding.
        assert_eq!(added("a.rs", &["#[ignore = \"flaky\"]"]).len(), 1);
        // `exit(` is not `xit(`, and an iterator's `.skip(n)` is not a test.
        assert!(added("a.c", &["exit(1);"]).is_empty());
        assert!(added("a.rs", &["for w in words.iter().skip(1) {"]).is_empty());
        assert!(added("a.ts", &["const rest = rows.skip(2);"]).is_empty());
    }

    #[test]
    fn a_body_that_only_says_not_done_is_a_stub() {
        for (path, line) in [
            ("a.rs", "    todo!()"),
            ("a.rs", "unimplemented!(\"later\")"),
            ("a.py", "    raise NotImplementedError"),
            ("a.ts", "throw new Error('Not implemented')"),
            ("a.rs", "panic!(\"not implemented yet\")"),
        ] {
            let f = added(path, &[line]);
            assert_eq!(f.len(), 1, "{line}");
            assert_eq!(f[0].words(), format!("a stub in {path}"));
        }
        // The words alone, outside a throw or a panic, are only words.
        assert!(added("a.ts", &["const note = 'not implemented in v1';"]).is_empty());
    }

    #[test]
    fn a_removed_refusal_is_found_when_nothing_replaces_it() {
        let removed = lines(&["    if path.contains(\"..\") { return Err(\"invalid path\".into()); }"]);
        let f = scan(&[Changed { path: "src/files.rs", added: &[], removed: &removed }]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].words(), "a removed check in src/files.rs with no test that it still refuses");
        // A line that is not a guard (no guard word, or no guard shape) is not found.
        let plain = lines(&["let invalid = 3;", "if x > 1 { y() }"]);
        assert!(scan(&[Changed { path: "a.rs", added: &[], removed: &plain }]).is_empty());
    }

    #[test]
    fn a_guard_line_that_moved_to_another_file_is_not_removed() {
        let line = "if user.is_none() { return Err(\"unauthorized\".into()); }";
        let gone = lines(&[&format!("    {line}")]);
        let there = lines(&[line]);
        let f = scan(&[
            Changed { path: "src/old.rs", added: &[], removed: &gone },
            Changed { path: "src/new.rs", added: &there, removed: &[] },
        ]);
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn a_removed_guard_with_a_test_that_it_still_refuses_is_not_a_finding() {
        let gone = lines(&["if bad { return Err(\"rejected\".into()); }"]);
        let test = lines(&["#[test]", "fn it_still_rejects_bad_input() {"]);
        let f = scan(&[
            Changed { path: "src/a.rs", added: &[], removed: &gone },
            Changed { path: "src/b.rs", added: &test, removed: &[] },
        ]);
        assert!(f.is_empty(), "{f:?}");
        // A function with such a name that is not a test does not count.
        let plain = lines(&["fn reject_everything() {"]);
        let f = scan(&[
            Changed { path: "src/a.rs", added: &[], removed: &gone },
            Changed { path: "src/b.rs", added: &plain, removed: &[] },
        ]);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn the_note_lists_five_items_then_says_how_many_more() {
        assert_eq!(note(&[]), None);
        let one = note(&["a new TODO in src/a.rs".to_string()]).unwrap();
        assert_eq!(one, "Before you answer: a new TODO in src/a.rs. Finish them, or say plainly in your answer that they are left open.");
        let seven: Vec<String> = (1..=7).map(|n| format!("a stub in f{n}.rs")).collect();
        let text = note(&seven).unwrap();
        assert!(text.contains("a stub in f5.rs and 2 more."), "{text}");
        assert!(!text.contains("f6.rs"));
    }

    /// A run's own edits, read back from its undo snapshots, are scanned like an agent app's.
    #[test]
    fn a_runs_own_changes_are_scanned_and_a_deleted_file_is_not() {
        use crate::db::Db;
        let db = Db::open_in_memory().unwrap();
        let root = std::env::temp_dir().join(format!("poiesis_integrity_{}", uuid::Uuid::new_v4().simple()));
        let data = std::env::temp_dir().join(format!("poiesis_integrity_data_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let root = crate::permissions::canonicalize_lenient(&root);
        let conv = db.create_conversation("c", None, false).unwrap();
        db.set_conversation_folder(&conv.id, Some(&root.to_string_lossy())).unwrap();
        let write = |name: &str, body: &str| {
            let path = root.join(name);
            let op = if path.exists() { "edit" } else { "write" };
            crate::agent::trash::record(&db, &data, &conv.id, op, &path, None);
            std::fs::write(&path, body).unwrap();
        };
        std::fs::write(root.join("old.rs"), "fn a() {}\n").unwrap();
        write("old.rs", "fn a() {}\n// TODO: handle errors\n");
        write("notes.md", "TODO\n");
        // Every line of a deleted file reads as removed; its guard is not "a removed check".
        std::fs::write(root.join("gone.rs"), "fn b() -> R { if bad { return Err(\"invalid\".into()); } }\n").unwrap();
        crate::agent::trash::record(&db, &data, &conv.id, "delete", &root.join("gone.rs"), None);
        std::fs::remove_file(root.join("gone.rs")).unwrap();
        let items = items(&of_change_set(&crate::agent::changes::change_set(&db, &conv.id, 0)));
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&data).ok();
        assert_eq!(items, vec!["a new TODO in old.rs".to_string()]);
    }

    #[test]
    fn the_items_say_each_thing_once() {
        let f = added("src/a.rs", &["// TODO one", "// TODO two", "todo!()"]);
        assert_eq!(items(&f), vec!["a new TODO in src/a.rs".to_string(), "a stub in src/a.rs".to_string()]);
    }
}
