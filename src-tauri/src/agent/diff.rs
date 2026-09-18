//! `COD-11`: line diffs, for the agent's own patch and for the Changes view.
//!
//! Written here rather than pulled in as a crate because the job is small and
//! the inputs are bounded: common lines are trimmed off both ends first, which
//! reduces a typical edit to a few lines, and only what is left is compared
//! with a longest-common-subsequence table. A middle too large for the table
//! is shown as replaced whole — a correct diff, just not a minimal one.

use serde::Serialize;

/// Past this many cells, the middle is shown as one replacement.
const LCS_CELL_CAP: usize = 4_000_000;
/// Lines of unchanged context around each change.
const CONTEXT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub kind: LineKind,
    pub text: String,
    /// 1-based line number in the old file, for context and removed lines.
    pub old_no: Option<usize>,
    /// 1-based line number in the new file, for context and added lines.
    pub new_no: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub lines: Vec<DiffLine>,
}

/// Every line of both files, marked, in order.
fn edit_script(old: &[&str], new: &[&str]) -> Vec<DiffLine> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();

    let mut out = Vec::with_capacity(old.len().max(new.len()));
    for (i, line) in old.iter().take(prefix).enumerate() {
        out.push(DiffLine { kind: LineKind::Context, text: line.to_string(), old_no: Some(i + 1), new_no: Some(i + 1) });
    }

    let a = &old[prefix..old.len() - suffix];
    let b = &new[prefix..new.len() - suffix];
    let (mut i, mut j) = (0usize, 0usize);
    let removed = |out: &mut Vec<DiffLine>, i: usize| {
        out.push(DiffLine { kind: LineKind::Removed, text: a[i].to_string(), old_no: Some(prefix + i + 1), new_no: None });
    };
    let added = |out: &mut Vec<DiffLine>, j: usize| {
        out.push(DiffLine { kind: LineKind::Added, text: b[j].to_string(), old_no: None, new_no: Some(prefix + j + 1) });
    };
    if !a.is_empty() && !b.is_empty() && a.len().saturating_mul(b.len()) <= LCS_CELL_CAP {
        // lcs[i][j] = length of the LCS of a[i..] and b[j..], flattened.
        let w = b.len() + 1;
        let mut lcs = vec![0u32; (a.len() + 1) * w];
        for x in (0..a.len()).rev() {
            for y in (0..b.len()).rev() {
                lcs[x * w + y] = if a[x] == b[y] {
                    lcs[(x + 1) * w + y + 1] + 1
                } else {
                    lcs[(x + 1) * w + y].max(lcs[x * w + y + 1])
                };
            }
        }
        while i < a.len() && j < b.len() {
            if a[i] == b[j] {
                out.push(DiffLine {
                    kind: LineKind::Context,
                    text: a[i].to_string(),
                    old_no: Some(prefix + i + 1),
                    new_no: Some(prefix + j + 1),
                });
                i += 1;
                j += 1;
            } else if lcs[(i + 1) * w + j] >= lcs[i * w + j + 1] {
                removed(&mut out, i);
                i += 1;
            } else {
                added(&mut out, j);
                j += 1;
            }
        }
    }
    while i < a.len() {
        removed(&mut out, i);
        i += 1;
    }
    while j < b.len() {
        added(&mut out, j);
        j += 1;
    }

    for k in 0..suffix {
        let (oi, ni) = (old.len() - suffix + k, new.len() - suffix + k);
        out.push(DiffLine { kind: LineKind::Context, text: old[oi].to_string(), old_no: Some(oi + 1), new_no: Some(ni + 1) });
    }
    out
}

/// The changes between two texts, grouped into hunks with context.
pub fn hunks(old: &str, new: &str) -> Vec<Hunk> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let script = edit_script(&old_lines, &new_lines);

    let changed: Vec<usize> = script
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind != LineKind::Context)
        .map(|(i, _)| i)
        .collect();
    let mut out = Vec::new();
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(CONTEXT);
        let mut end = changed[k];
        // Changes closer than two contexts apart share one hunk.
        while k + 1 < changed.len() && changed[k + 1] <= end + 2 * CONTEXT + 1 {
            k += 1;
            end = changed[k];
        }
        let end = (end + CONTEXT + 1).min(script.len());
        let lines: Vec<DiffLine> = script[start..end].to_vec();
        let old_lines_n = lines.iter().filter(|l| l.kind != LineKind::Added).count();
        let new_lines_n = lines.iter().filter(|l| l.kind != LineKind::Removed).count();
        // A hunk that adds to an empty file starts at 0, as unified diff says.
        let old_start = lines.iter().find_map(|l| l.old_no).unwrap_or_else(|| {
            script[..start].iter().rev().find_map(|l| l.old_no).unwrap_or(0)
        });
        let new_start = lines.iter().find_map(|l| l.new_no).unwrap_or_else(|| {
            script[..start].iter().rev().find_map(|l| l.new_no).unwrap_or(0)
        });
        out.push(Hunk { old_start, old_lines: old_lines_n, new_start, new_lines: new_lines_n, lines });
        k += 1;
    }
    out
}

/// `(added, removed)` line counts.
pub fn counts(hunks: &[Hunk]) -> (usize, usize) {
    hunks.iter().flat_map(|h| &h.lines).fold((0, 0), |(a, r), l| match l.kind {
        LineKind::Added => (a + 1, r),
        LineKind::Removed => (a, r + 1),
        LineKind::Context => (a, r),
    })
}

/// A unified diff, as `git diff` would print it, for the model to read.
pub fn unified(label: &str, old_exists: bool, new_exists: bool, hunks: &[Hunk]) -> String {
    let mut out = format!(
        "--- {}\n+++ {}\n",
        if old_exists { format!("a/{label}") } else { "/dev/null".to_string() },
        if new_exists { format!("b/{label}") } else { "/dev/null".to_string() },
    );
    for h in hunks {
        out.push_str(&format!("@@ -{},{} +{},{} @@\n", h.old_start, h.old_lines, h.new_start, h.new_lines));
        for l in &h.lines {
            let mark = match l.kind {
                LineKind::Context => ' ',
                LineKind::Added => '+',
                LineKind::Removed => '-',
            };
            out.push(mark);
            out.push_str(&l.text);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_texts_have_no_hunks() {
        assert!(hunks("a\nb\n", "a\nb\n").is_empty());
    }

    #[test]
    fn one_changed_line_is_one_hunk_with_context_and_both_line_numbers() {
        let old = (1..=10).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
        let new = old.replace("line 5", "line five");
        let h = hunks(&old, &new);
        assert_eq!(h.len(), 1);
        assert_eq!((h[0].old_start, h[0].old_lines, h[0].new_start, h[0].new_lines), (2, 7, 2, 7));
        assert_eq!(counts(&h), (1, 1));
        let removed = h[0].lines.iter().find(|l| l.kind == LineKind::Removed).unwrap();
        assert_eq!((removed.text.as_str(), removed.old_no, removed.new_no), ("line 5", Some(5), None));
        let text = unified("a.txt", true, true, &h);
        assert!(text.starts_with("--- a/a.txt\n+++ b/a.txt\n@@ -2,7 +2,7 @@\n"), "{text}");
        assert!(text.contains("\n-line 5\n+line five\n"));
    }

    #[test]
    fn far_apart_changes_are_separate_hunks() {
        let old = (1..=40).map(|n| n.to_string()).collect::<Vec<_>>().join("\n");
        let new = old.replacen("\n3\n", "\nthree\n", 1).replacen("\n37\n", "\nthirty-seven\n", 1);
        assert_eq!(hunks(&old, &new).len(), 2);
    }

    #[test]
    fn a_created_file_is_all_additions() {
        let h = hunks("", "a\nb");
        assert_eq!(counts(&h), (2, 0));
        assert_eq!(h[0].old_start, 0);
        assert!(unified("n.txt", false, true, &h).starts_with("--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,2 @@"));
    }

    #[test]
    fn an_insertion_in_the_middle_keeps_the_lines_around_it_as_context() {
        let h = hunks("a\nb\nc", "a\nb\nx\nc");
        assert_eq!(counts(&h), (1, 0));
        let kinds: Vec<LineKind> = h[0].lines.iter().map(|l| l.kind).collect();
        assert_eq!(kinds, vec![LineKind::Context, LineKind::Context, LineKind::Added, LineKind::Context]);
    }
}
