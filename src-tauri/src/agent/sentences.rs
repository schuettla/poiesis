//! `SEE-1`: an answer's sentences, split in code, for the check that runs on every
//! answer. No model is asked anything here.
//!
//! What counts: the sentences of a paragraph, and a list item as one sentence.
//! What does not: code blocks (an open one too), headings, tables, quoted text
//! (the user's words), rules, questions, lead-ins that end in a colon and anything
//! under three words.
//!
//! **Final.** The answer is still being written when the check starts, so a
//! sentence is final only once the text has moved past its end: another sentence
//! began after it, or a line break followed it. A sentence that is still being
//! typed is returned with `is_final: false` and is not checked yet. Text only
//! ever grows, so the sentences already final never change, and a caller can
//! count how many it has taken.

use super::verify::plain_text;

/// The most sentences one answer has checked. Past this the check says so.
pub const MAX_SENTENCES: usize = 40;

/// Fewer words than this are not a statement (`Sure.`, `Done.`).
const MIN_WORDS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    /// As the screen draws it: emphasis marks, ticks and link targets removed.
    pub text: String,
    pub is_final: bool,
}

/// Words that end in a dot without ending the sentence.
const ABBREVIATIONS: [&str; 22] = [
    "e.g", "i.e", "vs", "dr", "mr", "mrs", "ms", "prof", "st", "no", "fig", "approx", "inc", "ltd", "jr", "sr", "u.s",
    "a.m", "p.m", "cf", "est", "ca",
];

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && t.chars().all(|c| matches!(c, '-' | '*' | '_' | ' ')) && t.chars().filter(|c| !c.is_whitespace()).count() >= 3
}

/// The text of a list item when the line is one.
fn item_of(line: &str) -> Option<&str> {
    let l = line.trim_start();
    if let Some(rest) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).or_else(|| l.strip_prefix("+ ")) {
        return Some(rest);
    }
    let (n, rest) = l.split_once(". ")?;
    (!n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit())).then_some(rest)
}

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Whether the dot at `at` (a byte index into `chars`' string) ends a sentence.
fn ends_sentence(chars: &[(usize, char)], i: usize, text: &str) -> bool {
    let (at, c) = chars[i];
    if !matches!(c, '.' | '!' | '?') {
        return false;
    }
    // Closing marks stay with the sentence: `He said "stop."` and `(see above.)`.
    let mut j = i + 1;
    while j < chars.len() && matches!(chars[j].1, '"' | '\'' | ')' | ']' | '\u{201d}' | '\u{2019}') {
        j += 1;
    }
    match chars.get(j) {
        None => return true,
        Some((_, n)) if !n.is_whitespace() => return false,
        _ => {}
    }
    if c != '.' {
        return true;
    }
    // The word before the dot.
    let head = &text[..at];
    let word: String = head.chars().rev().take_while(|c| c.is_alphabetic() || *c == '.').collect::<Vec<_>>().into_iter().rev().collect();
    let lower = word.to_lowercase();
    if ABBREVIATIONS.contains(&lower.as_str()) {
        return false;
    }
    // An initial: `J. Smith`.
    if word.chars().count() == 1 && word.chars().all(|c| c.is_uppercase()) {
        return false;
    }
    // What comes next has to look like the start of a sentence.
    let next = chars[j..].iter().map(|(_, c)| *c).find(|c| !c.is_whitespace());
    next.is_none_or(|n| !n.is_lowercase())
}

/// The sentences of one paragraph's text. The last one is `final` when `closed`.
fn paragraph_sentences(text: &str, closed: bool) -> Vec<Sentence> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if ends_sentence(&chars, i, text) {
            let mut j = i + 1;
            while j < chars.len() && matches!(chars[j].1, '"' | '\'' | ')' | ']' | '\u{201d}' | '\u{2019}') {
                j += 1;
            }
            let end = chars.get(j).map_or(text.len(), |(b, _)| *b);
            let piece = text[start..end].trim();
            if !piece.is_empty() {
                // Followed by more text, so the stream has moved past it.
                out.push(Sentence { text: piece.to_string(), is_final: end < text.trim_end().len() || closed });
            }
            start = end;
            i = j;
        } else {
            i += 1;
        }
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        out.push(Sentence { text: rest.to_string(), is_final: closed });
    }
    out
}

/// Split an answer. `complete` says the answer is over, so every sentence is final.
pub fn split(markdown: &str, complete: bool) -> Vec<Sentence> {
    let mut out: Vec<Sentence> = Vec::new();
    let mut para = String::new();
    let mut in_fence = false;
    let lines: Vec<&str> = markdown.split('\n').collect();
    let last = lines.len().saturating_sub(1);

    // A paragraph that something followed (a blank line, another block) is over.
    let flush = |para: &mut String, out: &mut Vec<Sentence>, closed: bool| {
        if !para.trim().is_empty() {
            out.extend(paragraph_sentences(para.trim(), closed));
        }
        para.clear();
    };

    for (n, raw) in lines.iter().enumerate() {
        let line = raw.trim_end_matches('\r');
        // The last piece has no line break after it yet, unless the answer is over.
        let open = n == last && !complete;
        let t = line.trim_start();

        if t.starts_with("```") || t.starts_with("~~~") {
            flush(&mut para, &mut out, true);
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if t.is_empty() {
            // The text ending in a line break is not a blank line yet: a paragraph
            // is over only when something came after it.
            if n < last || complete {
                flush(&mut para, &mut out, true);
            }
            continue;
        }
        if t.starts_with('#') || t.starts_with('>') || t.starts_with('|') || is_rule(t) {
            flush(&mut para, &mut out, true);
            continue;
        }
        if let Some(item) = item_of(line) {
            flush(&mut para, &mut out, true);
            let text = plain_text(item).trim().to_string();
            if !text.is_empty() {
                out.push(Sentence { text, is_final: !open });
            }
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(plain_text(t).trim());
    }
    // The answer ends in this paragraph. Its last sentence is final only when its
    // line is over and a terminator closes it.
    if !para.trim().is_empty() {
        let line_over = markdown.ends_with('\n');
        let mut sentences = paragraph_sentences(para.trim(), complete);
        if let Some(last) = sentences.last_mut().filter(|_| !complete) {
            last.is_final = line_over && last.text.ends_with(['.', '!', '?', '"', ')', '\u{201d}']);
        }
        out.extend(sentences);
    }

    out.retain(|s| words(&s.text) >= MIN_WORDS && !s.text.trim_end().ends_with(['?', ':']));
    out
}

/// The sentences that are final now, in order, cut to [`MAX_SENTENCES`], and how many
/// more there were than that.
pub fn finals(markdown: &str, complete: bool) -> (Vec<Sentence>, usize) {
    let all: Vec<Sentence> = split(markdown, complete).into_iter().take_while(|s| s.is_final).collect();
    let over = all.len().saturating_sub(MAX_SENTENCES);
    (all.into_iter().take(MAX_SENTENCES).collect(), over)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(md: &str, complete: bool) -> Vec<String> {
        split(md, complete).into_iter().map(|s| s.text).collect()
    }

    /// `SEE-T-1`: code, headings, tables and quoted text are not sentences.
    #[test]
    fn code_headings_tables_and_quotes_are_skipped() {
        let md = "# A heading about Paris\n\nParis is the capital of France.\n\n```rust\nlet capital = \"Paris is in France.\";\n```\n\n| City | Country |\n|---|---|\n| Paris | France |\n\n> You said Rome is the capital of Italy.\n\nIt lies on the Seine.\n";
        assert_eq!(texts(md, true), ["Paris is the capital of France.", "It lies on the Seine."]);
    }

    #[test]
    fn a_list_item_is_one_sentence_and_marks_are_drawn_away() {
        let md = "- The **Eiffel Tower** is 330 metres tall, and it opened in 1889\n2. See [the guide](https://x.org) for more detail\n";
        assert_eq!(
            texts(md, true),
            ["The Eiffel Tower is 330 metres tall, and it opened in 1889", "See the guide for more detail"]
        );
    }

    #[test]
    fn questions_and_fragments_are_not_statements() {
        assert_eq!(texts("Is it big? Sure. The tower is very tall.", true), ["The tower is very tall."]);
    }

    #[test]
    fn decimals_abbreviations_initials_and_addresses_do_not_end_a_sentence() {
        let md = "The tower is 3.5 times taller than it is wide, e.g. at the base. Dr. Smith and J. Jones measured it in 2019. See https://example.org/a.b for details.";
        assert_eq!(
            texts(md, true),
            [
                "The tower is 3.5 times taller than it is wide, e.g. at the base.",
                "Dr. Smith and J. Jones measured it in 2019.",
                "See https://example.org/a.b for details."
            ]
        );
    }

    /// `SEE-T-1`: a sentence is not final until the stream has moved past it.
    #[test]
    fn a_sentence_is_final_once_the_text_has_moved_past_it() {
        let flags = |md: &str| split(md, false).into_iter().map(|s| (s.text, s.is_final)).collect::<Vec<_>>();
        assert_eq!(flags("Paris is the capital of France."), [("Paris is the capital of France.".to_string(), false)]);
        assert_eq!(
            flags("Paris is the capital of France. It has"),
            [("Paris is the capital of France.".to_string(), true)],
            "the short second piece is not a statement yet"
        );
        assert_eq!(
            flags("Paris is the capital of France. It lies on the Seine"),
            [("Paris is the capital of France.".to_string(), true), ("It lies on the Seine".to_string(), false)]
        );
        // A line break after a terminator is movement too.
        assert_eq!(flags("Paris is the capital of France.\n"), [("Paris is the capital of France.".to_string(), true)]);
        // A line break in the middle of a sentence is not.
        assert_eq!(flags("Paris is the capital of\n"), [("Paris is the capital of".to_string(), false)]);
        // A list item is final when its line is over.
        assert_eq!(flags("- The tower is tall and wide"), [("The tower is tall and wide".to_string(), false)]);
        assert_eq!(flags("- The tower is tall and wide\n"), [("The tower is tall and wide".to_string(), true)]);
        // Everything is final when the answer is over.
        assert!(split("Paris is the capital of France", true).iter().all(|s| s.is_final));
    }

    /// An answer only grows, so what was final stays what it was.
    #[test]
    fn growing_text_never_changes_a_final_sentence() {
        let full = "Paris is the capital of France. It lies on the Seine, and 2.2 million people live there. The tower opened in 1889.\n\n- It is 330 metres tall\n- It has three levels\n";
        let mut seen: Vec<String> = Vec::new();
        for end in (0..=full.len()).filter(|e| full.is_char_boundary(*e)) {
            let now: Vec<String> = finals(&full[..end], false).0.into_iter().map(|s| s.text).collect();
            assert!(now.len() >= seen.len(), "{end}");
            assert_eq!(&now[..seen.len()], &seen[..], "at {end}");
            seen = now;
        }
        assert_eq!(seen.len(), 5);
        assert_eq!(finals(full, true).0.len(), 5);
    }

    #[test]
    fn an_open_code_block_holds_no_sentences() {
        assert_eq!(texts("Here it is:\n\n```\nthis is not a sentence at all", false), Vec::<String>::new());
        assert_eq!(texts("The code is below now.\n\n```\nthis is not a sentence at all", false), ["The code is below now."]);
    }

    /// `SEE-T-1`: at most 40 sentences, and the caller is told how many were left out.
    #[test]
    fn at_most_forty_sentences_are_taken() {
        let md: String = (0..45).map(|i| format!("- Fact number {i} holds true here\n")).collect();
        let (taken, over) = finals(&md, true);
        assert_eq!((taken.len(), over), (MAX_SENTENCES, 5));
        assert_eq!(taken[0].text, "Fact number 0 holds true here");
        assert_eq!(finals("A short answer with three words.", true).1, 0);
    }
}
