//! SMP-8c — a vocabulary rule enforced by a test, not just a code review
//! comment, because a comment doesn't survive a year of edits. Scans the
//! frontend's own `.ts`/`.tsx` sources (not `node_modules`, not the built
//! bundle) for the engineering words `SMP-8a` bans from user-facing copy.
//!
//! Scanning source rather than the built bundle is a deliberate deviation
//! from the plan's literal wording. Calibrating this against a real
//! `vite build` output turned up the actual failure mode: a minified bundle
//! is full of *true* whole-word hits that are not copy at all — SVG
//! attributes (`vector-effect`), CSS class names (`wb-index-status`),
//! settings keys (`"index.explained"`), and React's own internal `.index`
//! property accesses — and telling those apart from real prose reliably
//! would need an actual JS parser, not a grep. Source is far cleaner: once
//! comments are stripped, a banned word only survives inside a same-line
//! quoted string, and requiring that string to contain a space (the one
//! thing an identifier, a class name, or a dotted settings key never has) is
//! precise enough in practice — confirmed against every false positive this
//! check turned up during development.
//!
//! This only proves the *word* is gone from an authored string; the two real
//! hits found while writing this test (`store.ts`'s memory-index prompt
//! strings, sent to the model and shown verbatim by `WHY-3` in Everything
//! mode) were fixed alongside it.

use std::fs;
use std::path::{Path, PathBuf};

/// `SMP-8a`. `index` is banned "as a noun" in the plan text, but this test
/// doesn't try to distinguish noun from verb use — simpler and stricter to
/// keep the concept out of user-facing copy entirely, matching `SMP-4a/4b`'s
/// "read / has read" replacement, which uses neither.
const BANNED_WORDS: &[&str] = &[
    "embedding",
    "vector",
    "index",
    "reranker",
    "cross-encoder",
    "bi-encoder",
    "chunk",
    "rag",
    "semantic",
    "cosine",
    "threshold",
    "corpus",
    "ocr",
];

fn frontend_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("src")
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, out);
            continue;
        }
        let is_ts = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e == "ts" || e == "tsx");
        if is_ts {
            out.push(path);
        }
    }
}

/// Strip `//` line comments and `/* … */` block comments (including `/** */`
/// doc comments), which are full of these words by design (every amendment
/// in this codebase explains itself) but never ship to a user. Best-effort,
/// not a real tokenizer: it doesn't know a `//` inside a string from a real
/// comment, so worst case a line is truncated early — a missed check, never
/// a false alarm.
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_block = false;
    for line in source.lines() {
        let mut line = line;
        if in_block {
            match line.find("*/") {
                Some(end) => {
                    line = &line[end + 2..];
                    in_block = false;
                }
                None => {
                    out.push('\n');
                    continue;
                }
            }
        }
        // A line may open a block comment after some real code, and even
        // close+reopen one; loop until neither remains.
        loop {
            let line_comment = line.find("//");
            let block_start = line.find("/*");
            match (line_comment, block_start) {
                (Some(lc), Some(bs)) if lc < bs => {
                    line = &line[..lc];
                    break;
                }
                (Some(lc), None) => {
                    line = &line[..lc];
                    break;
                }
                (_, Some(bs)) => {
                    match line[bs..].find("*/") {
                        Some(rel_end) => {
                            let end = bs + rel_end + 2;
                            out.push_str(&line[..bs]);
                            line = &line[end..];
                            continue;
                        }
                        None => {
                            out.push_str(&line[..bs]);
                            in_block = true;
                            line = "";
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// One violation: a banned word found inside a same-line quoted string that
/// also contains a space — the signal that separates prose from an
/// identifier, a CSS class, or a settings key (see the module doc).
struct Hit {
    file: String,
    line_no: usize,
    line: String,
    word: &'static str,
}

fn scan_line(file: &str, line: &str, line_no: usize, hits: &mut Vec<Hit>) {
    let lower = line.to_lowercase();
    let bytes = lower.as_bytes();
    for &word in BANNED_WORDS {
        let mut start = 0;
        while let Some(rel) = lower[start..].find(word) {
            let idx = start + rel;
            let end = idx + word.len();
            start = end;
            let before_ok = idx == 0 || !bytes[idx - 1].is_ascii_alphanumeric();
            let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if !before_ok || !after_ok {
                continue; // part of a longer word, e.g. "indexed" for "index"
            }
            if enclosed_in_spaced_string(line, idx, end) {
                hits.push(Hit {
                    file: file.to_string(),
                    line_no,
                    line: line.trim().to_string(),
                    word,
                });
                break; // one report per word per line is plenty
            }
        }
    }
}

/// Does the match sit inside a same-line `"…"`, `'…'` or `` `…` `` span that
/// itself contains a space? Tried against all three quote characters since a
/// template literal, a plain string, and JSX text all use different ones.
///
/// One more false-positive class turned up beyond the module doc's list: a
/// multi-class `className="foo bar-index"` value is a space-separated
/// string, but it's a token list, not a sentence — so a string whose
/// immediately preceding attribute is `className=`/`class=` is never prose,
/// however many spaces it has.
fn enclosed_in_spaced_string(line: &str, start: usize, end: usize) -> bool {
    for quote in ['"', '\'', '`'] {
        let left = line[..start].rfind(quote);
        let right = line[end..].find(quote);
        if let (Some(l), Some(r)) = (left, right) {
            let span = &line[l..end + r];
            if !span.contains(' ') {
                continue;
            }
            let prefix = line[..l].trim_end();
            if prefix.ends_with("className=") || prefix.ends_with("class=") {
                continue;
            }
            return true;
        }
    }
    false
}

#[test]
fn banned_words_stay_out_of_user_facing_copy() {
    let src = frontend_src_dir();
    assert!(src.is_dir(), "expected the frontend source tree at {}", src.display());

    let mut files = Vec::new();
    collect_source_files(&src, &mut files);
    assert!(!files.is_empty(), "found no .ts/.tsx files under {}", src.display());

    let mut hits = Vec::new();
    for path in &files {
        let Ok(raw) = fs::read_to_string(path) else { continue };
        let stripped = strip_comments(&raw);
        let display = path.strip_prefix(&src).unwrap_or(path).display().to_string();
        for (i, line) in stripped.lines().enumerate() {
            scan_line(&display, line, i + 1, &mut hits);
        }
    }

    if !hits.is_empty() {
        let report = hits
            .iter()
            .map(|h| format!("  {}:{} [{}] {}", h.file, h.line_no, h.word, h.line))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "SMP-8a banned word(s) found in user-facing copy:\n{report}\n\
             Replace with the SMP-8b vocabulary (recall, read/has read, my eyes, notes, \
             what I learned, how I work), or move the text to a code comment if it isn't \
             actually shown to the user."
        );
    }
}

#[test]
fn comment_stripping_removes_both_styles_without_truncating_code() {
    let source = "const a = 1; // a trailing comment about vector math\nconst b = /* inline */ 2;\n/**\n * a doc comment about the embedding model\n */\nconst c = 3;\n";
    let stripped = strip_comments(source);
    assert!(stripped.contains("const a = 1;"));
    assert!(stripped.contains("const b =  2;") || stripped.contains("const b = 2;"));
    assert!(stripped.contains("const c = 3;"));
    assert!(!stripped.to_lowercase().contains("vector"));
    assert!(!stripped.to_lowercase().contains("embedding"));
}

#[test]
fn only_a_spaced_same_line_string_counts_as_prose() {
    let mut hits = Vec::new();
    scan_line("f", r#"const KEY = "index.explained";"#, 1, &mut hits);
    assert!(hits.is_empty(), "a dotted settings key is not copy");

    hits.clear();
    scan_line("f", r#"className="wb-index-status""#, 2, &mut hits);
    assert!(hits.is_empty(), "a CSS class name is not copy");

    hits.clear();
    scan_line("f", r#"<div className="wb-head-row wb-index-row">"#, 2, &mut hits);
    assert!(hits.is_empty(), "a multi-class className value has spaces but still isn't copy");

    hits.clear();
    scan_line("f", r#"const s = "I haven't read this folder yet";"#, 3, &mut hits);
    assert!(hits.is_empty(), "no banned word here at all");

    hits.clear();
    scan_line("f", r#"return `## Your memory index (durable facts)`;"#, 4, &mut hits);
    assert_eq!(hits.len(), 1, "a real sentence containing the word must be caught");
    assert_eq!(hits[0].word, "index");
}
