//! `COD-10`: a build or test log, read the way a person reads it.
//!
//! The model gets the structured list and a short tail rather than the whole
//! log, and the user gets the same list as rows they can click. Each parser
//! recognises one tool's shape and ignores everything else, so a log that
//! mixes two tools (a `tsc` run inside `npm run build`) is read by both.
//! Output nobody recognises degrades to the tail, never to nothing.

use serde::Serialize;

/// How many diagnostics are kept. A build with four hundred errors is a build
/// with one error and its consequences; the first ones are the ones to read.
const MAX_ITEMS: usize = 60;
/// Lines of raw output kept beside the list.
const TAIL_LINES: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    /// A failed test: not a compiler error, but the thing the run is about.
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// As printed, relative to wherever the tool ran. May be empty for a
    /// failure that names no file.
    pub file: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub severity: Severity,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub items: Vec<Diagnostic>,
    pub errors: usize,
    pub warnings: usize,
    /// Failed tests, from the tool's own summary when it printed one.
    pub failed: usize,
    pub passed: Option<usize>,
    /// The outcome in a word or two: `passed`, `3 failed`, `12 errors`.
    pub outcome: String,
    pub tail: String,
}

fn num(s: &str) -> Option<u32> {
    s.trim().parse().ok()
}

/// `path:line:col` or `path:line`, with a Windows drive letter allowed.
fn split_location(loc: &str) -> Option<(String, Option<u32>, Option<u32>)> {
    let loc = loc.trim();
    let (drive, rest) = match loc.as_bytes() {
        [d, b':', ..] if d.is_ascii_alphabetic() && loc.len() > 2 && matches!(loc.as_bytes()[2], b'\\' | b'/') => {
            (&loc[..2], &loc[2..])
        }
        _ => ("", loc),
    };
    let mut parts = rest.rsplitn(3, ':');
    let last = parts.next()?;
    let mid = parts.next();
    let first = parts.next();
    match (first, mid, num(last)) {
        (Some(file), Some(line), Some(col)) if num(line).is_some() => {
            Some((format!("{drive}{file}"), num(line), Some(col)))
        }
        (_, Some(_), Some(line)) => {
            let file = rest[..rest.len() - last.len() - 1].to_string();
            Some((format!("{drive}{file}"), Some(line), None))
        }
        _ => None,
    }
}

fn push(items: &mut Vec<Diagnostic>, d: Diagnostic) {
    if items.len() < MAX_ITEMS && !items.contains(&d) {
        items.push(d);
    }
}

/// rustc and cargo: `error[E0308]: message` then ` --> file:line:col`.
fn rustc(lines: &[&str], items: &mut Vec<Diagnostic>) {
    for (i, line) in lines.iter().enumerate() {
        let (severity, rest) = if let Some(r) = line.strip_prefix("error") {
            (Severity::Error, r)
        } else if let Some(r) = line.strip_prefix("warning") {
            (Severity::Warning, r)
        } else {
            continue;
        };
        let (code, message) = if let Some(r) = rest.strip_prefix('[') {
            match r.split_once("]: ") {
                Some((code, msg)) => (Some(code.to_string()), msg),
                None => continue,
            }
        } else if let Some(msg) = rest.strip_prefix(": ") {
            (None, msg)
        } else {
            continue;
        };
        // The summary lines cargo prints are not diagnostics.
        if message.starts_with("could not compile") || message.contains("generated") && message.contains("warning") {
            continue;
        }
        let Some(loc) = lines.iter().skip(i + 1).take(3).find_map(|l| l.trim_start().strip_prefix("--> ")) else {
            continue;
        };
        if let Some((file, line_no, col)) = split_location(loc) {
            push(items, Diagnostic { file, line: line_no, col, severity, message: message.to_string(), code });
        }
    }
}

/// tsc, both shapes: `file(12,5): error TS2322: msg` and
/// `file:12:5 - error TS2322: msg`. msbuild uses the first shape too.
fn tsc_and_msbuild(lines: &[&str], items: &mut Vec<Diagnostic>) {
    for line in lines {
        let line = line.trim();
        // file(12,5): error CODE: message
        if let Some(open) = line.find('(') {
            if let Some(close) = line[open..].find("): ").map(|c| open + c) {
                let coords: Vec<&str> = line[open + 1..close].split(',').collect();
                let rest = &line[close + 3..];
                let (severity, rest) = if let Some(r) = rest.strip_prefix("error ") {
                    (Severity::Error, r)
                } else if let Some(r) = rest.strip_prefix("warning ") {
                    (Severity::Warning, r)
                } else {
                    continue;
                };
                if let (Some(l), Some((code, msg))) = (coords.first().and_then(|c| num(c)), rest.split_once(": ")) {
                    let message = msg.rsplit_once(" [").map(|(m, _)| m).unwrap_or(msg);
                    push(items, Diagnostic {
                        file: line[..open].to_string(),
                        line: Some(l),
                        col: coords.get(1).and_then(|c| num(c)),
                        severity,
                        message: message.to_string(),
                        code: Some(code.to_string()),
                    });
                }
                continue;
            }
        }
        // file:12:5 - error TS2322: message
        if let Some((loc, rest)) = line.split_once(" - ") {
            let (severity, rest) = if let Some(r) = rest.strip_prefix("error ") {
                (Severity::Error, r)
            } else if let Some(r) = rest.strip_prefix("warning ") {
                (Severity::Warning, r)
            } else {
                continue;
            };
            if let (Some((file, l, c)), Some((code, msg))) = (split_location(loc), rest.split_once(": ")) {
                if code.starts_with("TS") {
                    push(items, Diagnostic { file, line: l, col: c, severity, message: msg.to_string(), code: Some(code.to_string()) });
                }
            }
        }
    }
}

/// eslint's default formatter: a file path on its own line, then indented
/// `line:col  error  message  rule` rows under it.
fn eslint(lines: &[&str], items: &mut Vec<Diagnostic>) {
    fn columns(line: &str) -> Vec<&str> {
        line.split("  ").map(str::trim).filter(|s| !s.is_empty()).collect()
    }
    fn is_row(line: &str) -> bool {
        line.starts_with(char::is_whitespace)
            && columns(line)
                .first()
                .and_then(|loc| loc.split_once(':'))
                .is_some_and(|(l, c)| num(l).is_some() && num(c).is_some())
    }
    let mut file: Option<&str> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            file = None;
            continue;
        }
        // A heading is an unindented line with a row directly under it.
        if !line.starts_with(char::is_whitespace) {
            file = lines.get(i + 1).is_some_and(|next| is_row(next)).then(|| line.trim());
            continue;
        }
        let Some(current) = file else { continue };
        if !is_row(line) {
            continue;
        }
        let cols = columns(line);
        if cols.len() < 3 {
            continue;
        }
        let Some((l, c)) = cols[0].split_once(':') else { continue };
        let severity = match cols[1] {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => continue,
        };
        let (message, code) = if cols.len() >= 4 {
            (cols[2..cols.len() - 1].join("  "), Some(cols[cols.len() - 1].to_string()))
        } else {
            (cols[2].to_string(), None)
        };
        push(items, Diagnostic { file: current.to_string(), line: num(l), col: num(c), severity, message, code });
    }
}

/// pytest: `FAILED tests/test_x.py::test_name - AssertionError: msg`, located by
/// the `tests/test_x.py:12: AssertionError` line in the traceback when present.
fn pytest(lines: &[&str], items: &mut Vec<Diagnostic>) {
    for line in lines {
        let Some(rest) = line.strip_prefix("FAILED ").or_else(|| line.strip_prefix("ERROR ")) else { continue };
        let (id, message) = rest.split_once(" - ").unwrap_or((rest, ""));
        let file = id.split("::").next().unwrap_or(id).to_string();
        let located = lines.iter().find_map(|l| {
            let (loc, _) = l.split_once(": ")?;
            let (f, n) = loc.rsplit_once(':')?;
            (f == file).then(|| num(n)).flatten()
        });
        push(items, Diagnostic {
            file,
            line: located,
            col: None,
            severity: Severity::Failure,
            message: if message.is_empty() { id.to_string() } else { format!("{id}: {message}") },
            code: None,
        });
    }
}

/// A Python traceback: the deepest `File "x", line N` and the exception line.
fn python_traceback(lines: &[&str], items: &mut Vec<Diagnostic>) {
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].starts_with("Traceback (most recent call last)") {
            i += 1;
            continue;
        }
        let mut last: Option<(String, u32)> = None;
        let mut j = i + 1;
        while j < lines.len() && (lines[j].starts_with(' ') || lines[j].is_empty()) {
            let t = lines[j].trim();
            if let Some(rest) = t.strip_prefix("File \"") {
                if let Some((file, after)) = rest.split_once('"') {
                    if let Some(n) = after.trim_start_matches(", line ").split(',').next().and_then(num) {
                        last = Some((file.to_string(), n));
                    }
                }
            }
            j += 1;
        }
        if let (Some((file, line)), Some(exc)) = (last, lines.get(j)) {
            let (code, message) = match exc.split_once(": ") {
                Some((c, m)) => (Some(c.to_string()), m.to_string()),
                None => (Some(exc.trim().to_string()), exc.trim().to_string()),
            };
            push(items, Diagnostic { file, line: Some(line), col: None, severity: Severity::Error, message, code });
        }
        i = j + 1;
    }
}

/// go build and go vet: `./main.go:12:5: message`. go test: `--- FAIL: TestX`
/// followed by an indented `x_test.go:12: message`.
fn go(lines: &[&str], items: &mut Vec<Diagnostic>) {
    for (i, line) in lines.iter().enumerate() {
        if let Some(name) = line.trim().strip_prefix("--- FAIL: ") {
            let located = lines.get(i + 1).and_then(|next| {
                let t = next.trim();
                let (loc, msg) = t.split_once(": ")?;
                let (file, l, _) = split_location(loc)?;
                file.ends_with(".go").then(|| (file, l, msg.to_string()))
            });
            let (file, line_no, msg) = located.unwrap_or_default();
            push(items, Diagnostic {
                file,
                line: line_no,
                col: None,
                severity: Severity::Failure,
                message: if msg.is_empty() { name.to_string() } else { format!("{}: {msg}", name.split(' ').next().unwrap_or(name)) },
                code: None,
            });
            continue;
        }
        let t = line.trim_start_matches("# ");
        let Some((loc, msg)) = t.split_once(": ") else { continue };
        let Some((file, l, c)) = split_location(loc) else { continue };
        if file.ends_with(".go") && c.is_some() && !line.starts_with(' ') {
            push(items, Diagnostic { file: file.trim_start_matches("./").to_string(), line: l, col: c, severity: Severity::Error, message: msg.to_string(), code: None });
        }
    }
}

/// TAP, as `node --test` prints it: `not ok 6 - name`, then a YAML block with
/// `location:`, `error: |-`, `expected:`/`actual:` and a `stack:` whose first
/// `file:///` frame is the failing assertion. A suite that failed only because
/// a test inside it did (`failureType: 'subtestsFailed'`) is not listed twice.
fn tap(lines: &[&str], items: &mut Vec<Diagnostic>) {
    let unquote = |v: &str| v.trim().trim_matches('\'').replace("\\\\", "\\");
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("not ok ") else { continue };
        let name = rest.split_once(" - ").map(|(_, n)| n).unwrap_or(rest).trim();
        let mut block = Vec::new();
        for l in lines.iter().skip(i + 1) {
            let t = l.trim();
            if t == "..." || t.starts_with("ok ") || t.starts_with("not ok ") {
                break;
            }
            block.push(*l);
        }
        let field = |key: &str| block.iter().find_map(|l| l.trim().strip_prefix(key).map(unquote));
        if field("failureType:").as_deref() == Some("subtestsFailed") {
            continue;
        }
        let frame = block.iter().find_map(|l| {
            let at = l.find("file:///")?;
            let loc = l[at + "file:///".len()..].trim_end_matches(')');
            split_location(loc)
        });
        let (file, line_no, col) = frame.or_else(|| field("location:").and_then(|l| split_location(&l))).unwrap_or_default();
        let detail = match (field("expected:"), field("actual:")) {
            (Some(e), Some(a)) => format!("expected {e}, got {a}"),
            _ => block
                .iter()
                .skip_while(|l| !l.trim().starts_with("error:"))
                .skip(1)
                .map(|l| l.trim())
                .find(|l| !l.is_empty())
                .unwrap_or("")
                .to_string(),
        };
        push(items, Diagnostic {
            file,
            line: line_no,
            col,
            severity: Severity::Failure,
            message: if detail.is_empty() { name.to_string() } else { format!("{name}: {detail}") },
            code: None,
        });
    }
}

/// Test summaries, for the counts a list of failures cannot give.
fn summary_counts(text: &str) -> (Option<usize>, Option<usize>) {
    let mut failed = None;
    let mut passed = None;
    for line in text.lines() {
        let l = line.trim().trim_matches('=').trim();
        // node --test (TAP): "# pass 6" and "# fail 2"
        if let Some(n) = l.strip_prefix("# pass ").and_then(|n| n.trim().parse::<usize>().ok()) {
            passed = Some(n);
            continue;
        }
        if let Some(n) = l.strip_prefix("# fail ").and_then(|n| n.trim().parse::<usize>().ok()) {
            failed = Some(n);
            continue;
        }
        // cargo: "test result: FAILED. 3 passed; 1 failed; 0 ignored"
        if let Some(rest) = l.strip_prefix("test result:") {
            for part in rest.split([';', '.']) {
                let part = part.trim();
                if let Some(n) = part.strip_suffix(" passed").and_then(|n| n.trim().parse::<usize>().ok()) {
                    passed = Some(passed.unwrap_or(0) + n);
                }
                if let Some(n) = part.strip_suffix(" failed").and_then(|n| n.trim().parse::<usize>().ok()) {
                    failed = Some(failed.unwrap_or(0) + n);
                }
            }
            continue;
        }
        // pytest: "1 failed, 3 passed in 0.12s"; vitest/jest: "Tests  1 failed | 3 passed (4)"
        if l.contains(" failed") || l.contains(" passed") {
            let words: Vec<&str> = l.split(|c: char| c.is_whitespace() || c == ',' || c == '|').filter(|w| !w.is_empty()).collect();
            for w in words.windows(2) {
                if let Ok(n) = w[0].parse::<usize>() {
                    if w[1] == "failed" {
                        failed = Some(n);
                    } else if w[1] == "passed" {
                        passed = Some(n);
                    }
                }
            }
        }
    }
    (failed, passed)
}

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    lines[start..].join("\n")
}

/// Read a finished task's output. `exit_code` decides between `passed` and a
/// failure when the log itself says nothing countable.
pub fn parse(output: &str, exit_code: Option<i32>) -> Report {
    let lines: Vec<&str> = output.lines().collect();
    let mut items = Vec::new();
    rustc(&lines, &mut items);
    tsc_and_msbuild(&lines, &mut items);
    eslint(&lines, &mut items);
    pytest(&lines, &mut items);
    python_traceback(&lines, &mut items);
    go(&lines, &mut items);
    tap(&lines, &mut items);

    let errors = items.iter().filter(|d| d.severity == Severity::Error).count();
    let warnings = items.iter().filter(|d| d.severity == Severity::Warning).count();
    let listed_failures = items.iter().filter(|d| d.severity == Severity::Failure).count();
    let (summary_failed, passed) = summary_counts(output);
    let failed = summary_failed.unwrap_or(listed_failures).max(listed_failures);

    let plural = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    let outcome = match exit_code {
        Some(0) if warnings > 0 => format!("passed, {}", plural(warnings, "warning")),
        Some(0) => "passed".to_string(),
        _ if failed > 0 => format!("{failed} failed"),
        _ if errors > 0 => plural(errors, "error"),
        Some(code) => format!("failed (exit code {code})"),
        None => "did not finish".to_string(),
    };
    Report { items, errors, warnings, failed, passed, outcome, tail: tail(output) }
}

/// The report as the model reads it: outcome, the list, then a short tail.
pub fn render_for_model(task: &str, report: &Report, exit_code: Option<i32>) -> String {
    // The outcome already names the code when it has nothing better to say.
    let code = exit_code
        .filter(|_| !report.outcome.contains("exit code"))
        .map(|c| format!(" (exit code {c})"))
        .unwrap_or_default();
    let mut out = format!("`{task}` {}{code}.\n", report.outcome);
    if !report.items.is_empty() {
        out.push_str("\nDiagnostics:\n");
        for d in &report.items {
            let loc = match (d.line, d.col) {
                (Some(l), Some(c)) => format!("{}:{l}:{c}", d.file),
                (Some(l), None) => format!("{}:{l}", d.file),
                _ => d.file.clone(),
            };
            let sev = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Failure => "failed",
            };
            let code = d.code.as_deref().map(|c| format!(" [{c}]")).unwrap_or_default();
            out.push_str(&format!("- {sev} {loc}: {}{code}\n", d.message));
        }
    }
    if !report.tail.trim().is_empty() {
        out.push_str(&format!("\nLast lines of output:\n{}\n", report.tail));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustc_errors_and_warnings_with_their_codes() {
        let log = "   Compiling x v0.1.0\nwarning: unused variable: `a`\n --> src/lib.rs:3:9\n  |\nerror[E0308]: mismatched types\n  --> src\\main.rs:12:18\n   |\nerror: could not compile `x` due to 1 previous error\n";
        let r = parse(log, Some(101));
        assert_eq!(r.items.len(), 2, "{:?}", r.items);
        assert_eq!(r.items[1], Diagnostic { file: "src\\main.rs".into(), line: Some(12), col: Some(18), severity: Severity::Error, message: "mismatched types".into(), code: Some("E0308".into()) });
        assert_eq!(r.outcome, "1 error");
    }

    #[test]
    fn node_test_runner_tap_failures_with_the_assertion_line() {
        let log = r#"TAP version 13
# Subtest: prices under a dollar
ok 5 - prices under a dollar
  ---
  duration_ms: 1.3537
  ...
# Subtest: prices with a leading zero in the cents
not ok 6 - prices with a leading zero in the cents
  ---
  duration_ms: 1.1988
  location: 'C:\\Users\\Erich\\coding-testbed\\tests\\format_check.ts:9:1'
  failureType: 'testCodeFailure'
  error: |-
    Expected values to be strictly equal:

    '$12.5' !== '$12.05'

  code: 'ERR_ASSERTION'
  name: 'AssertionError'
  expected: '$12.05'
  actual: '$12.5'
  operator: 'strictEqual'
  stack: |-
    TestContext.<anonymous> (file:///C:/Users/Erich/coding-testbed/tests/format_check.ts:10:10)
    Test.runInAsyncScope (node:async_hooks:211:14)
  ...
# Subtest: a suite
not ok 7 - a suite
  ---
  location: 'C:\\Users\\Erich\\coding-testbed\\tests\\suite.ts:1:1'
  failureType: 'subtestsFailed'
  error: '1 subtest failed'
  ...
# Subtest: throws
not ok 8 - throws
  ---
  location: 'C:\\Users\\Erich\\coding-testbed\\tests\\format_check.ts:20:1'
  failureType: 'testCodeFailure'
  error: |-
    boom
  ...
1..8
# tests 8
# pass 6
# fail 2
"#;
        let r = parse(log, Some(1));
        assert_eq!(r.outcome, "2 failed");
        assert_eq!(r.passed, Some(6));
        assert_eq!(r.items.len(), 2, "the suite that only failed through its child is not listed: {:?}", r.items);
        assert_eq!(
            r.items[0],
            Diagnostic {
                file: "C:/Users/Erich/coding-testbed/tests/format_check.ts".into(),
                line: Some(10),
                col: Some(10),
                severity: Severity::Failure,
                message: "prices with a leading zero in the cents: expected $12.05, got $12.5".into(),
                code: None,
            }
        );
        assert_eq!(r.items[1].file, "C:\\Users\\Erich\\coding-testbed\\tests\\format_check.ts");
        assert_eq!(r.items[1].line, Some(20));
        assert_eq!(r.items[1].message, "throws: boom");
    }

    #[test]
    fn the_exit_code_is_said_once() {
        let r = parse("something went wrong\n", Some(1));
        let text = render_for_model("npm run test", &r, Some(1));
        assert!(text.starts_with("`npm run test` failed (exit code 1).\n"), "{text}");
    }

    #[test]
    fn cargo_test_failures_are_counted_from_the_summary() {
        let log = "test a ... ok\ntest b ... FAILED\n\ntest result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured\n";
        let r = parse(log, Some(101));
        assert_eq!((r.failed, r.passed), (1, Some(3)));
        assert_eq!(r.outcome, "1 failed");
    }

    #[test]
    fn tsc_in_both_of_its_shapes() {
        let log = "src/a.ts(4,7): error TS2322: Type 'string' is not assignable to type 'number'.\nsrc/b.tsx:10:3 - error TS2304: Cannot find name 'x'.\n";
        let r = parse(log, Some(2));
        assert_eq!(r.items.len(), 2);
        assert_eq!((r.items[0].file.as_str(), r.items[0].line, r.items[0].col), ("src/a.ts", Some(4), Some(7)));
        assert_eq!(r.items[1].code.as_deref(), Some("TS2304"));
        assert_eq!(r.outcome, "2 errors");
    }

    #[test]
    fn msbuild_drops_the_project_suffix() {
        let r = parse("Program.cs(12,5): error CS1002: ; expected [C:\\x\\app.csproj]\n", Some(1));
        assert_eq!(r.items[0].message, "; expected");
        assert_eq!(r.items[0].code.as_deref(), Some("CS1002"));
    }

    #[test]
    fn eslint_rows_take_their_file_from_the_heading_line() {
        let log = "\n/home/u/app/src/x.js\n  1:10  error    'a' is defined but never used  no-unused-vars\n  4:1   warning  Unexpected console statement   no-console\n\n✖ 2 problems (1 error, 1 warning)\n";
        let r = parse(log, Some(1));
        assert_eq!(r.items.len(), 2, "{:?}", r.items);
        assert_eq!(r.items[0].file, "/home/u/app/src/x.js");
        assert_eq!(r.items[0].code.as_deref(), Some("no-unused-vars"));
        assert_eq!(r.items[1].severity, Severity::Warning);
    }

    #[test]
    fn pytest_failures_find_their_line() {
        let log = "tests/test_x.py:12: AssertionError\n=== short test summary info ===\nFAILED tests/test_x.py::test_add - assert 1 == 2\n=== 1 failed, 3 passed in 0.12s ===\n";
        let r = parse(log, Some(1));
        assert_eq!(r.items[0].line, Some(12));
        assert_eq!(r.items[0].severity, Severity::Failure);
        assert_eq!(r.outcome, "1 failed");
    }

    #[test]
    fn a_python_traceback_points_at_its_deepest_frame() {
        let log = "Traceback (most recent call last):\n  File \"main.py\", line 3, in <module>\n    f()\n  File \"lib.py\", line 9, in f\n    1/0\nZeroDivisionError: division by zero\n";
        let r = parse(log, Some(1));
        assert_eq!(r.items[0].file, "lib.py");
        assert_eq!(r.items[0].line, Some(9));
        assert_eq!(r.items[0].code.as_deref(), Some("ZeroDivisionError"));
    }

    #[test]
    fn go_build_and_go_test() {
        let r = parse("# example\n./main.go:12:5: undefined: foo\n", Some(1));
        assert_eq!(r.items[0].file, "main.go");
        let r = parse("--- FAIL: TestAdd (0.00s)\n    add_test.go:9: got 3, want 4\nFAIL\n", Some(1));
        assert_eq!(r.items[0].file, "add_test.go");
        assert_eq!(r.items[0].line, Some(9));
    }

    /// `COD-10-T`: output nobody recognises degrades to a tail.
    #[test]
    fn unknown_output_degrades_to_the_tail() {
        let log = (1..=50).map(|n| format!("step {n}")).collect::<Vec<_>>().join("\n");
        let r = parse(&log, Some(3));
        assert!(r.items.is_empty());
        assert_eq!(r.outcome, "failed (exit code 3)");
        assert!(r.tail.starts_with("step 21") && r.tail.ends_with("step 50"));
        assert_eq!(parse("all good", Some(0)).outcome, "passed");
    }
}
