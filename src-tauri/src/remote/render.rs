//! REM-5: a turn's events, as plain chat text for Telegram.
//!
//! Only these `type`s are read: `token` (kept for the answer), `error` (one plain line), `permission` and
//! `permission_gone` (a question and its end), `artifact` (a file to send), `remote_notice` (a line from
//! the gateway). Every other event is ignored. A question is built from the request's own fields, never
//! from the window's panel text. Nothing here talks to the network.

use serde_json::Value;

/// Telegram refuses a text longer than this.
pub const TELEGRAM_LIMIT: usize = 4096;
/// A question names its target in full, but never more than this many characters of it.
const CUT: usize = 300;
const SEE_DESKTOP: &str = "... see the desktop for the rest";

/// Telegram's HTML mode needs these three replaced and nothing else.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// At most `max` characters, and whether anything was cut.
fn clip(s: &str, max: usize) -> (String, bool) {
    let s = s.trim();
    match s.char_indices().nth(max) {
        Some((at, _)) => (s[..at].trim_end().to_string(), true),
        None => (s.to_string(), false),
    }
}

/// A piece of detail for a question: cut at 300 characters, escaped, with the pointer to the desktop when cut.
fn detail(raw: &str) -> String {
    let (text, cut) = clip(raw, CUT);
    let mut out = escape_html(&text);
    if cut {
        out.push_str(SEE_DESKTOP);
    }
    out
}

fn field<'a>(request: &'a Value, key: &str) -> &'a str {
    request[key].as_str().unwrap_or_default().trim()
}

/// The question a `permission` event's `request` asks, in plain words. HTML for Telegram: only `<code>` and
/// `<pre>` are used. The answer is yes once or no; "always" is for the desktop.
pub fn question(request: &Value) -> String {
    let summary = field(request, "summary");
    let path = field(request, "path");
    let diff = field(request, "diff");
    let argv = request["argv"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let engine = field(request, "engine");
    let tool = field(request, "tool");
    let block = |text: &str| if text.is_empty() { String::new() } else { format!("\n<pre>{}</pre>", detail(text)) };
    let place = |text: &str| if text.is_empty() { String::new() } else { format!("\nin <code>{}</code>", detail(text)) };

    match request["capability"].as_str() {
        None => {
            let target = if path.is_empty() { String::new() } else { format!("\n<code>{}</code>", detail(path)) };
            format!("{}{target}{}", detail(summary), block(diff))
        }
        Some("task" | "command") => format!("I want to run <code>{}</code>{}", detail(&argv), place(path)),
        Some("app") => {
            let verb = match field(request, "call_kind") {
                "send" => "send something with",
                "change" => "change something in",
                _ => "look something up in",
            };
            format!("I want to {verb} {}.{}", detail(path), block(diff))
        }
        Some("domain") => format!("I want to open the website <code>{}</code>.", detail(path)),
        Some("screen") => "I want to take a picture of your screen.".to_string(),
        Some("open-app") => format!("I want to open <code>{}</code> on your computer.", detail(path)),
        Some("engine") => {
            let who = if engine.is_empty() { "An agent app" } else { engine };
            if argv.is_empty() {
                format!("{} wants to use {}{}{}", escape_html(who), detail(tool), place(path), block(diff))
            } else {
                format!("{} wants to run <code>{}</code>{}", escape_html(who), detail(&argv), place(path))
            }
        }
        Some("handover") => {
            let who = if engine.is_empty() { "an agent app" } else { engine };
            format!("Hand this to {}?\n{}", escape_html(who), detail(diff))
        }
        Some("page") => format!(
            "{}\nAllow opens a window on the desktop for you. Not now skips this step.",
            detail(summary)
        ),
        Some("risk") => {
            let reason = field(request, "reason");
            let why = if reason.is_empty() { String::new() } else { format!("\n{}", detail(reason)) };
            format!("I want to use {}.{}{why}", detail(tool), block(diff))
        }
        Some(_) => detail(summary),
    }
}

/// The line that replaces a question once it is answered or gone.
pub fn answered_line(allow: bool) -> &'static str {
    if allow {
        "Allowed."
    } else {
        "Not allowed."
    }
}

/// The line for a question nobody needs to answer any more (answered at the desktop, or the run ended).
pub const GONE_LINE: &str = "No answer needed any more.";

/// Cut text into pieces of at most `limit` characters. A cut falls at a paragraph end when there is one in
/// reach, else at a line end, else at a space, else at the limit. Empty pieces are dropped.
pub fn split_text(text: &str, limit: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let Some((end, _)) = rest.char_indices().nth(limit) else {
            pieces.push(rest.to_string());
            break;
        };
        let window = &rest[..end];
        let cut = [window.rfind("\n\n"), window.rfind('\n'), window.rfind(' ')]
            .into_iter()
            .flatten()
            .find(|at| *at > 0)
            .unwrap_or(end);
        let piece = rest[..cut].trim();
        if !piece.is_empty() {
            pieces.push(piece.to_string());
        }
        rest = rest[cut..].trim_start();
    }
    pieces
}

/// One line of the model's markdown, as Telegram HTML: `code`, **bold**, headings as bold, bullets as dots.
/// Anything else stays plain text. A marker with no partner stays as typed.
fn inline(line: &str) -> String {
    let marks = line.trim_start().trim_start_matches('#');
    let (body, heading) = if line.trim_start().starts_with('#') && marks.starts_with(' ') {
        (marks.trim_start(), true)
    } else {
        (line, false)
    };
    let body = body
        .strip_prefix("- ")
        .or_else(|| body.strip_prefix("* "))
        .map(|rest| format!("\u{2022} {rest}"))
        .unwrap_or_else(|| body.to_string());
    let mut out = String::new();
    let mut chars = body.as_str();
    while let Some(c) = chars.chars().next() {
        if c == '`' {
            if let Some(end) = chars[1..].find('`').filter(|e| *e > 0) {
                out.push_str(&format!("<code>{}</code>", escape_html(&chars[1..1 + end])));
                chars = &chars[end + 2..];
                continue;
            }
        } else if let Some(after) = chars.strip_prefix("**") {
            if let Some(end) = after.find("**").filter(|e| *e > 0) {
                out.push_str(&format!("<b>{}</b>", escape_html(&after[..end])));
                chars = &after[end + 2..];
                continue;
            }
        }
        out.push_str(&escape_html(&c.to_string()));
        chars = &chars[c.len_utf8()..];
    }
    if heading {
        format!("<b>{out}</b>")
    } else {
        out
    }
}

/// The model's markdown as Telegram HTML. Fenced code becomes `<pre>`.
pub fn markdown_to_html(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<Vec<&str>> = None;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            match fence.take() {
                Some(code) => out.push(format!("<pre>{}</pre>", escape_html(&code.join("\n")))),
                None => fence = Some(Vec::new()),
            }
        } else if let Some(code) = fence.as_mut() {
            code.push(line);
        } else {
            out.push(inline(line));
        }
    }
    if let Some(code) = fence {
        out.push(format!("<pre>{}</pre>", escape_html(&code.join("\n"))));
    }
    out.join("\n")
}

/// An answer, ready to send: HTML in pieces that each fit Telegram's limit. The text is cut first and
/// converted second, so no tag is ever cut in two; a piece that grows past the limit when escaped is cut
/// again, smaller.
pub fn render_answer(text: &str) -> Vec<String> {
    let mut limit = TELEGRAM_LIMIT;
    loop {
        let pieces: Vec<String> = split_text(text, limit).iter().map(|p| markdown_to_html(p)).collect();
        if limit <= 50 || pieces.iter().all(|p| p.chars().count() <= TELEGRAM_LIMIT) {
            return pieces;
        }
        limit /= 2;
    }
}

/// A file the turn made, to send to the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOut {
    /// The artifact's id. A turn that changes an artifact twice sends it once, as it is at the end.
    pub id: String,
    pub title: String,
    /// `image` | `video` carry a path in `content`; every other kind carries its text.
    pub kind: String,
    pub content: String,
}

/// What one event asks of the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Question { request_id: String, html: String },
    Gone { request_id: String },
    /// One plain line.
    Line(String),
    File(FileOut),
}

/// A turn as it goes by: the answer is kept until the end, the rest is passed on at once.
#[derive(Default)]
pub struct Fold {
    answer: String,
    error: Option<String>,
}

impl Fold {
    pub fn event(&mut self, event: &Value) -> Option<Out> {
        match event["type"].as_str()? {
            "token" => {
                self.answer.push_str(event["text"].as_str().unwrap_or_default());
                None
            }
            "error" => {
                let line = event["message"].as_str().unwrap_or_default().trim();
                self.error = Some(if line.is_empty() { "Something went wrong.".into() } else { line.to_string() });
                None
            }
            "permission" => {
                let request = &event["request"];
                Some(Out::Question { request_id: request["id"].as_str()?.to_string(), html: question(request) })
            }
            "permission_gone" => Some(Out::Gone { request_id: event["id"].as_str()?.to_string() }),
            "remote_notice" => Some(Out::Line(event["text"].as_str()?.to_string())),
            "artifact" => Some(Out::File(FileOut {
                id: event["id"].as_str().unwrap_or_default().to_string(),
                title: event["title"].as_str().unwrap_or("file").to_string(),
                kind: event["kind"].as_str().unwrap_or("text").to_string(),
                content: event["content"].as_str().unwrap_or_default().to_string(),
            })),
            _ => None,
        }
    }

    /// The answer in sendable pieces, then the error line when the turn failed. An empty turn says so.
    pub fn finish(&self) -> Vec<String> {
        let mut pieces = render_answer(&self.answer);
        if let Some(error) = &self.error {
            pieces.push(escape_html(error));
        }
        if pieces.is_empty() {
            pieces.push("I have nothing to say to that.".to_string());
        }
        pieces
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{Mode, PageRef, PermissionRequest, RuleRef};
    use serde_json::json;

    fn q(request: PermissionRequest) -> String {
        question(&serde_json::to_value(request).unwrap())
    }

    fn id() -> String {
        "perm_x".into()
    }

    #[test]
    fn html_special_characters_are_escaped_and_nothing_else() {
        assert_eq!(escape_html("a < b && c > d \"q\" 'r'"), "a &lt; b &amp;&amp; c &gt; d \"q\" 'r'");
    }

    #[test]
    fn a_question_for_a_folder_names_the_folder() {
        let r = PermissionRequest::scope(id(), "I want to read the files in this folder.".into(), "C:\\work\\shop".into(), Mode::Read);
        assert_eq!(q(r), "I want to read the files in this folder.\n<code>C:\\work\\shop</code>");
    }

    #[test]
    fn a_question_for_a_file_change_shows_what_it_writes() {
        let r = PermissionRequest::operation(id(), "Change the report.".into(), "C:\\work\\report.md".into(), Mode::ReadWrite, Some("+ new line <b>".into()));
        assert_eq!(q(r), "Change the report.\n<code>C:\\work\\report.md</code>\n<pre>+ new line &lt;b&gt;</pre>");
    }

    #[test]
    fn a_question_for_a_command_shows_the_command_and_the_place() {
        let r = PermissionRequest::execution(
            id(),
            "command",
            "Run the tests.".into(),
            "C:\\work\\shop".into(),
            vec!["npm".into(), "test".into()],
            "shop".into(),
            60,
            "npm".into(),
        );
        assert_eq!(q(r), "I want to run <code>npm test</code>\nin <code>C:\\work\\shop</code>");
    }

    #[test]
    fn a_question_for_a_connected_app_says_what_kind_of_call_it_is() {
        let ask = |kind: &str| {
            q(PermissionRequest::app_call(id(), "s".into(), "Gmail".into(), kind, "{\"to\":\"a@b.c\"}".into(), None))
        };
        assert_eq!(ask("send"), "I want to send something with Gmail.\n<pre>{\"to\":\"a@b.c\"}</pre>");
        assert!(ask("change").starts_with("I want to change something in Gmail."));
        assert!(ask("read").starts_with("I want to look something up in Gmail."));
    }

    #[test]
    fn a_question_for_the_screen_a_site_or_an_app() {
        assert_eq!(q(PermissionRequest::capability(id(), "screen", "s".into(), String::new())), "I want to take a picture of your screen.");
        assert_eq!(
            q(PermissionRequest::capability(id(), "domain", "s".into(), "example.com".into())),
            "I want to open the website <code>example.com</code>."
        );
        assert_eq!(
            q(PermissionRequest::capability(id(), "open-app", "s".into(), "Notepad".into())),
            "I want to open <code>Notepad</code> on your computer."
        );
    }

    #[test]
    fn a_question_from_an_agent_app_names_it_and_what_it_wants() {
        let command = PermissionRequest::engine_ask(
            id(),
            "Claude Code".into(),
            "Bash".into(),
            "s".into(),
            "C:\\work".into(),
            Some(vec!["git".into(), "push".into()]),
            None,
        );
        assert_eq!(q(command), "Claude Code wants to run <code>git push</code>\nin <code>C:\\work</code>");
        let edit = PermissionRequest::engine_ask(id(), "Claude Code".into(), "Edit".into(), "s".into(), "C:\\work".into(), None, Some("x".into()));
        assert_eq!(q(edit), "Claude Code wants to use Edit\nin <code>C:\\work</code>\n<pre>x</pre>");
    }

    #[test]
    fn a_hand_over_asks_whether_to_pass_the_task_on() {
        let r = PermissionRequest::handover(id(), "Codex".into(), "Fix the build".into(), Some(RuleRef::cited("U2".into(), "x".into())));
        assert_eq!(q(r), "Hand this to Codex?\nFix the build");
    }

    #[test]
    fn a_page_stop_says_what_allow_does() {
        let page = PageRef { host: "shop.example".into(), wall: "login".into(), screenshot: None, note: "n".into() };
        let r = PermissionRequest::page_stop(id(), "shop.example wants a login.".into(), page);
        assert_eq!(
            q(r),
            "shop.example wants a login.\nAllow opens a window on the desktop for you. Not now skips this step."
        );
    }

    #[test]
    fn a_risky_call_shows_the_tool_its_input_and_why() {
        let r = PermissionRequest::risk_ask(id(), "s".into(), "send_mail".into(), "{}".into(), "Not what you asked for".into());
        assert_eq!(q(r), "I want to use send_mail.\n<pre>{}</pre>\nNot what you asked for");
    }

    #[test]
    fn a_long_target_is_cut_at_300_characters_with_a_pointer_to_the_desktop() {
        let r = PermissionRequest::scope(id(), "Open it.".into(), "p".repeat(500), Mode::Read);
        let text = q(r);
        let target = text.split("<code>").nth(1).unwrap().split("</code>").next().unwrap();
        assert_eq!(target, format!("{}{SEE_DESKTOP}", "p".repeat(300)));
    }

    #[test]
    fn an_unknown_kind_falls_back_to_the_summary() {
        let v = json!({ "id": "perm_x", "summary": "Do a <thing>.", "path": "", "capability": "newkind" });
        assert_eq!(question(&v), "Do a &lt;thing&gt;.");
    }

    #[test]
    fn a_callback_for_a_request_fits_telegrams_64_bytes() {
        // `p:<request id>:y`, with the id the gates make: `perm_` and a v4 uuid.
        let data = format!("p:perm_{}:y", uuid::Uuid::new_v4());
        assert!(data.len() <= 64, "{} bytes", data.len());
    }

    #[test]
    fn text_is_cut_at_paragraph_ends_and_never_past_the_limit() {
        let text = format!("{}\n\n{}\n\n{}", "a".repeat(40), "b".repeat(40), "c".repeat(40));
        let pieces = split_text(&text, 100);
        assert_eq!(pieces, vec![format!("{}\n\n{}", "a".repeat(40), "b".repeat(40)), "c".repeat(40)]);
        assert!(split_text(&"x".repeat(250), 100).iter().all(|p| p.chars().count() <= 100));
        assert_eq!(split_text(&"x".repeat(250), 100).concat().len(), 250, "nothing is lost");
        assert!(split_text("  \n ", 100).is_empty());
    }

    #[test]
    fn a_long_answer_splits_at_4096() {
        let long = format!("{}\n\n{}", "a".repeat(4000), "b".repeat(4000));
        let pieces = render_answer(&long);
        assert_eq!(pieces.len(), 2);
        assert!(pieces.iter().all(|p| p.chars().count() <= TELEGRAM_LIMIT));
    }

    #[test]
    fn an_answer_full_of_ampersands_still_fits_after_escaping() {
        let pieces = render_answer(&"&".repeat(6000));
        assert!(pieces.len() > 1);
        assert!(pieces.iter().all(|p| p.chars().count() <= TELEGRAM_LIMIT));
    }

    #[test]
    fn markdown_becomes_the_few_tags_telegram_takes() {
        assert_eq!(markdown_to_html("**Done** with `a < b`."), "<b>Done</b> with <code>a &lt; b</code>.");
        assert_eq!(markdown_to_html("## Plan\n- one\n* two"), "<b>Plan</b>\n\u{2022} one\n\u{2022} two");
        assert_eq!(markdown_to_html("```rust\nlet a = 1 < 2;\n```\nafter"), "<pre>let a = 1 &lt; 2;</pre>\nafter");
        assert_eq!(markdown_to_html("2 ** 3 and `open"), "2 ** 3 and `open", "a marker with no partner stays as typed");
        assert_eq!(markdown_to_html("<script>"), "&lt;script&gt;");
    }

    #[test]
    fn the_fold_keeps_the_answer_and_passes_the_rest_on() {
        let mut fold = Fold::default();
        assert_eq!(fold.event(&json!({ "type": "token", "text": "Hello " })), None);
        assert_eq!(fold.event(&json!({ "type": "token", "text": "there." })), None);
        assert_eq!(fold.event(&json!({ "type": "step_start", "id": "1", "verb": "Reading", "target": "x" })), None, "tool use is not narrated");
        assert_eq!(fold.event(&json!({ "type": "thinking", "run_id": "r", "text": "hmm" })), None);
        assert_eq!(
            fold.event(&json!({ "type": "permission_gone", "id": "perm_1" })),
            Some(Out::Gone { request_id: "perm_1".into() })
        );
        assert_eq!(
            fold.event(&json!({ "type": "remote_notice", "text": "No answer in 10 minutes, so I did not do that." })),
            Some(Out::Line("No answer in 10 minutes, so I did not do that.".into()))
        );
        assert_eq!(fold.finish(), vec!["Hello there."]);
    }

    #[test]
    fn a_failed_turn_ends_with_one_plain_line() {
        let mut fold = Fold::default();
        fold.event(&json!({ "type": "token", "text": "Part of it." }));
        fold.event(&json!({ "type": "error", "message": "The model stopped <early>." }));
        assert_eq!(fold.finish(), vec!["Part of it.", "The model stopped &lt;early&gt;."]);
        assert_eq!(Fold::default().finish(), vec!["I have nothing to say to that."]);
    }

    #[test]
    fn a_question_event_carries_its_request_id_and_text() {
        let request = PermissionRequest::capability("perm_7".into(), "screen", "s".into(), String::new());
        let event = json!({ "type": "permission", "request": serde_json::to_value(request).unwrap() });
        assert_eq!(
            Fold::default().event(&event),
            Some(Out::Question { request_id: "perm_7".into(), html: "I want to take a picture of your screen.".into() })
        );
    }

    #[test]
    fn an_artifact_event_is_a_file_to_send() {
        let event = json!({ "type": "artifact", "id": "a", "title": "Report", "kind": "markdown", "content": "# Hi" });
        assert_eq!(
            Fold::default().event(&event),
            Some(Out::File(FileOut { id: "a".into(), title: "Report".into(), kind: "markdown".into(), content: "# Hi".into() }))
        );
    }
}
