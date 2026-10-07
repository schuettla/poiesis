//! Turns streamed reply text into speakable sentences (VTN-3). Pure text
//! work, so it is tested without a voice. See plans/VOICE_PLAN.md.
//!
//! The model is told to answer without markdown (VTN-2), but it does not
//! always obey, so this strips what is left and drops code and tables, which
//! must never be read aloud (VXP-8).

/// Cut at a comma only for the first chunk, and only after this many words,
/// so speech starts sooner.
const EARLY_CUT_WORDS: usize = 8;
/// A later chunk shorter than this waits and joins the next one: a very short
/// piece of speech sounds choppy and costs a whole synthesis call.
const MIN_CHUNK_CHARS: usize = 12;
/// A run of text with no end of sentence is cut at a space past this length.
const MAX_CHUNK_CHARS: usize = 260;

/// Ends in a dot but not a sentence. Compared lower case.
const ABBREVIATIONS: &[&str] = &[
    "z.b.", "d.h.", "u.a.", "bzw.", "ca.", "nr.", "dr.", "vs.", "e.g.", "i.e.", "mr.", "mrs.", "ms.",
];

fn is_abbreviation(text_up_to_dot: &str) -> bool {
    let word = text_up_to_dot
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or("")
        .to_lowercase();
    ABBREVIATIONS.contains(&word.as_str())
}

/// Cuts streamed text into chunks one at a time. Push text as it arrives, take
/// the chunks it returns, and call [`finish`](Self::finish) at the end.
#[derive(Default)]
pub struct SentenceChunker {
    /// Text not yet looked at for code fences (it may end in a part of one).
    raw: String,
    /// Text outside code fences, not yet cut into chunks.
    text: String,
    in_fence: bool,
    /// A short chunk waiting for the next one.
    carry: String,
    /// True once a chunk has been given out.
    started: bool,
}

impl SentenceChunker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, more: &str) -> Vec<String> {
        self.raw.push_str(more);
        self.take_fences();
        self.cut(false)
    }

    /// The reply is over: whatever is left is speakable now. An unclosed code
    /// block is dropped.
    pub fn finish(&mut self) -> Vec<String> {
        if !self.in_fence {
            let rest = std::mem::take(&mut self.raw);
            self.text.push_str(&rest);
        }
        self.raw.clear();
        self.cut(true)
    }

    /// Moves text outside code fences from `raw` to `text`. Two trailing
    /// backticks stay in `raw`: they may be the start of a fence.
    fn take_fences(&mut self) {
        loop {
            if self.in_fence {
                match self.raw.find("```") {
                    Some(at) => {
                        self.raw.drain(..at + 3);
                        self.in_fence = false;
                    }
                    None => {
                        // Nothing in a fence is kept, except a possible closer.
                        let keep = trailing_backticks(&self.raw);
                        let cut = self.raw.len() - keep;
                        self.raw.drain(..cut);
                        return;
                    }
                }
            } else {
                match self.raw.find("```") {
                    Some(at) => {
                        let before: String = self.raw.drain(..at + 3).collect();
                        self.text.push_str(&before[..at]);
                        self.in_fence = true;
                    }
                    None => {
                        let keep = trailing_backticks(&self.raw);
                        let cut = self.raw.len() - keep;
                        let ready: String = self.raw.drain(..cut).collect();
                        self.text.push_str(&ready);
                        return;
                    }
                }
            }
        }
    }

    fn cut(&mut self, finishing: bool) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(end) = self.next_cut(finishing) {
            let piece: String = self.text.drain(..end).collect();
            if let Some(chunk) = self.accept(&piece, false) {
                out.push(chunk);
            }
        }
        if finishing {
            let rest = std::mem::take(&mut self.text);
            if let Some(chunk) = self.accept(&rest, true) {
                out.push(chunk);
            }
            // A last short piece that was still waiting.
            let carried = std::mem::take(&mut self.carry);
            if !carried.is_empty() {
                out.push(carried);
            }
        }
        out
    }

    /// Cleans a piece. Returns a chunk when it is ready to be spoken; a short
    /// one (after the first) is held back to join the next.
    fn accept(&mut self, piece: &str, finishing: bool) -> Option<String> {
        let cleaned = clean_markdown(piece);
        if !cleaned.chars().any(char::is_alphanumeric) {
            return None;
        }
        let joined = if self.carry.is_empty() { cleaned } else { format!("{} {}", std::mem::take(&mut self.carry), cleaned) };
        if self.started && !finishing && joined.chars().count() < MIN_CHUNK_CHARS {
            self.carry = joined;
            return None;
        }
        self.started = true;
        Some(joined)
    }

    /// Byte index just past the next place to cut, if there is one.
    fn next_cut(&self, finishing: bool) -> Option<usize> {
        let t = self.text.as_str();
        let mut words = 0;
        let mut in_word = false;
        let mut line_start = 0;
        for (i, c) in t.char_indices() {
            if c == '\n' {
                line_start = i + 1;
            }
            if c.is_whitespace() {
                in_word = false;
            } else if !in_word {
                in_word = true;
                words += 1;
            }
            let after = i + c.len_utf8();
            let next = t[after..].chars().next();
            match c {
                '\n' => return Some(after),
                '.' | '!' | '?' | ':' => {
                    // At the very end of the text more may still come ("3.5").
                    let ends = match next {
                        None => finishing,
                        Some(n) => n.is_whitespace(),
                    };
                    let not_an_end = c == '.' && (is_abbreviation(&t[..after]) || is_list_number(&t[line_start..i]));
                    if ends && !not_an_end {
                        return Some(after);
                    }
                }
                ',' if !self.started && words >= EARLY_CUT_WORDS && next.is_some_and(char::is_whitespace) => {
                    return Some(after);
                }
                _ => {}
            }
        }
        if !finishing && t.chars().count() > MAX_CHUNK_CHARS {
            let limit = t.char_indices().nth(MAX_CHUNK_CHARS).map(|(i, _)| i).unwrap_or(t.len());
            return t[..limit].rfind(char::is_whitespace).map(|i| i + 1);
        }
        None
    }
}

/// The `1` of a numbered list line (`1. First`): a marker, not a sentence.
fn is_list_number(line_so_far: &str) -> bool {
    let n = line_so_far.trim_start();
    !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit())
}

/// Number of backticks at the end of `s` that could begin a fence (0 to 2).
fn trailing_backticks(s: &str) -> usize {
    s.chars().rev().take_while(|&c| c == '`').count().min(2)
}

/// Plain speech from one piece of markdown: no emphasis marks, no heading or
/// list markers, link text without the address, no tables, no rules.
pub fn clean_markdown(piece: &str) -> String {
    let mut lines = Vec::new();
    for line in piece.lines() {
        let line = line.trim();
        if line.is_empty() || is_table_or_rule(line) {
            continue;
        }
        let line = line.trim_start_matches('#').trim_start_matches('>').trim_start();
        let line = strip_list_marker(line);
        lines.push(strip_inline(line));
    }
    lines.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_table_or_rule(line: &str) -> bool {
    if line.starts_with('|') {
        return true;
    }
    // `---`, `***`, `___`: a horizontal rule.
    line.len() >= 3 && line.chars().all(|c| matches!(c, '-' | '*' | '_' | ' '))
}

fn strip_list_marker(line: &str) -> &str {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    // `1. ` and `12) `
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let rest = &line[digits..];
        for marker in [". ", ") "] {
            if let Some(r) = rest.strip_prefix(marker) {
                return r.trim_start();
            }
        }
    }
    line
}

fn strip_inline(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            // `![alt](url)`: an image has nothing to say aloud.
            '!' if chars.get(i + 1) == Some(&'[') => {
                if let Some(end) = link_end(&chars, i + 1) {
                    i = end;
                    continue;
                }
                out.push(c);
            }
            // `[text](url)`: say the text.
            '[' => {
                if let Some(end) = link_end(&chars, i) {
                    let close = chars[i..end].iter().position(|&x| x == ']').unwrap_or(0) + i;
                    out.extend(&chars[i + 1..close]);
                    i = end;
                    continue;
                }
                out.push(c);
            }
            '*' | '`' | '~' => {}
            // Emphasis underscores sit at a word edge; the one in `snake_case` stays.
            '_' => {
                let before = i.checked_sub(1).map(|j| chars[j]);
                let after = chars.get(i + 1).copied();
                let inner = before.is_some_and(char::is_alphanumeric) && after.is_some_and(char::is_alphanumeric);
                if inner {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// For a `[` at `open`, the index just past the `)` of `[text](url)`, if the
/// text there is a link.
fn link_end(chars: &[char], open: usize) -> Option<usize> {
    let close = chars[open..].iter().position(|&c| c == ']')? + open;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let paren = chars[close + 2..].iter().position(|&c| c == ')')? + close + 2;
    Some(paren + 1)
}

/// One piece of speech to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub generation: u64,
    pub seq: u32,
    pub text: String,
    /// A short spoken notice (VTN-5). Not part of the reply, so not saved.
    pub notice: bool,
    /// Language the caller knows the text is in, if it said so.
    pub language: Option<String>,
}

/// What is waiting to be spoken, per reply (`generation`). Pure bookkeeping:
/// the session runs the jobs. A reply that was cancelled is forgotten, and
/// text that arrives late for it is ignored.
#[derive(Default)]
pub struct SpeechQueue {
    generation: u64,
    cancelled_upto: u64,
    chunker: SentenceChunker,
    jobs: std::collections::VecDeque<(String, bool, Option<String>)>,
    next_seq: u32,
    /// The reply has no more text coming.
    done: bool,
    done_reported: bool,
    /// A job is being made right now.
    busy: bool,
    /// A worker task is running (see [`claim_worker`](Self::claim_worker)).
    worker: bool,
}

impl SpeechQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds streamed text of reply `generation` (or a notice). `done` says no
    /// more text follows for this reply.
    pub fn push(&mut self, generation: u64, text: &str, done: bool, notice: bool, language: Option<String>) {
        if generation <= self.cancelled_upto {
            return;
        }
        if generation != self.generation {
            // A newer reply replaces whatever was left of the old one.
            *self = Self { cancelled_upto: self.cancelled_upto, worker: self.worker, busy: self.busy, generation, ..Self::default() };
        }
        if notice {
            if !text.trim().is_empty() {
                self.jobs.push_back((text.trim().to_string(), true, language));
            }
            return;
        }
        for chunk in self.chunker.push(text) {
            self.jobs.push_back((chunk, false, language.clone()));
        }
        if done {
            for chunk in self.chunker.finish() {
                self.jobs.push_back((chunk, false, language.clone()));
            }
            self.done = true;
        }
    }

    /// Forgets reply `generation` and everything older.
    pub fn cancel(&mut self, generation: u64) {
        self.cancelled_upto = self.cancelled_upto.max(generation);
        if self.generation <= self.cancelled_upto {
            *self = Self { cancelled_upto: self.cancelled_upto, worker: self.worker, busy: self.busy, ..Self::default() };
        }
    }

    /// True when text for `generation` is still wanted.
    pub fn is_current(&self, generation: u64) -> bool {
        generation > self.cancelled_upto && generation == self.generation
    }

    /// The next piece to make. The caller must call [`job_finished`](Self::job_finished).
    pub fn next_job(&mut self) -> Option<Job> {
        let (text, notice, language) = self.jobs.pop_front()?;
        self.busy = true;
        let seq = self.next_seq;
        self.next_seq += 1;
        Some(Job { generation: self.generation, seq, text, notice, language })
    }

    pub fn job_finished(&mut self) {
        self.busy = false;
    }

    /// For a reply that is complete and fully made: its generation and how many
    /// pieces were made, once.
    pub fn take_done(&mut self) -> Option<(u64, u32)> {
        if self.done && !self.done_reported && self.jobs.is_empty() && !self.busy {
            self.done_reported = true;
            return Some((self.generation, self.next_seq));
        }
        None
    }

    /// True for the caller that should start the worker task: there is work and
    /// no worker is running.
    pub fn claim_worker(&mut self) -> bool {
        let work = !self.jobs.is_empty() || (self.done && !self.done_reported && !self.busy);
        if work && !self.worker {
            self.worker = true;
            return true;
        }
        false
    }

    /// The worker found nothing to do and is stopping.
    pub fn release_worker(&mut self) {
        self.worker = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(chunker: &mut SentenceChunker, parts: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        for p in parts {
            out.extend(chunker.push(p));
        }
        out.extend(chunker.finish());
        out
    }

    #[test]
    fn cuts_at_sentence_ends() {
        let got = all(&mut SentenceChunker::new(), &["It is warm today. Do you want a coat? Yes!"]);
        assert_eq!(got, vec!["It is warm today.", "Do you want a coat?", "Yes!"]);
    }

    #[test]
    fn works_when_text_arrives_in_small_pieces() {
        let parts = ["It is wa", "rm today", ". Do you", " want a coat", "?"];
        let got = all(&mut SentenceChunker::new(), &parts);
        assert_eq!(got, vec!["It is warm today.", "Do you want a coat?"]);
    }

    #[test]
    fn waits_for_what_follows_a_dot_at_the_end() {
        let mut c = SentenceChunker::new();
        // "3." might still become "3.5".
        assert!(c.push("The value is 3.").is_empty());
        assert!(c.push("5 and that is fine").is_empty());
        assert_eq!(c.finish(), vec!["The value is 3.5 and that is fine"]);
    }

    #[test]
    fn does_not_cut_inside_numbers_or_abbreviations() {
        let got = all(&mut SentenceChunker::new(), &["Die Zahl ist 3.5 und das ist z.B. ein Wert. Dr. Meier sagt das auch."]);
        assert_eq!(got, vec!["Die Zahl ist 3.5 und das ist z.B. ein Wert.", "Dr. Meier sagt das auch."]);
    }

    #[test]
    fn the_first_chunk_may_end_at_an_early_comma() {
        let text = "Well, as far as I can tell from here, the answer is yes. And that is all.";
        let got = all(&mut SentenceChunker::new(), &[text]);
        assert_eq!(got, vec!["Well, as far as I can tell from here,", "the answer is yes.", "And that is all."]);
    }

    #[test]
    fn later_chunks_do_not_cut_at_commas() {
        let text = "First sentence is here. Well, as far as I can tell from here, the answer is yes.";
        let got = all(&mut SentenceChunker::new(), &[text]);
        assert_eq!(got, vec!["First sentence is here.", "Well, as far as I can tell from here, the answer is yes."]);
    }

    #[test]
    fn a_short_later_piece_joins_the_next() {
        let got = all(&mut SentenceChunker::new(), &["Here is the first answer. Yes. And here is the last one."]);
        assert_eq!(got, vec!["Here is the first answer.", "Yes. And here is the last one."]);
    }

    #[test]
    fn a_short_first_piece_is_spoken_at_once() {
        let mut c = SentenceChunker::new();
        assert_eq!(c.push("Sure. "), vec!["Sure."]);
    }

    #[test]
    fn markdown_is_taken_out() {
        let text = "## Heading\n**Bold** and _italic_ and `code` with a [link](https://x.y/z).\n- first item\n- second item\n1. numbered\n";
        let got = all(&mut SentenceChunker::new(), &[text]);
        let spoken = got.join(" ");
        assert_eq!(spoken, "Heading Bold and italic and code with a link. first item second item numbered");
        assert!(!spoken.contains(['*', '#', '`', '[', '(']));
    }

    #[test]
    fn snake_case_keeps_its_underscore() {
        assert_eq!(clean_markdown("use read_file now"), "use read_file now");
        assert_eq!(clean_markdown("_emphasis_ here"), "emphasis here");
    }

    #[test]
    fn code_blocks_are_dropped_even_when_split_across_pieces() {
        let parts = ["Here is a script.\n``", "`python\nprint('hi')\nx = 1.", "5\n``", "`\nIt prints a greeting."];
        let got = all(&mut SentenceChunker::new(), &parts);
        assert_eq!(got, vec!["Here is a script.", "It prints a greeting."]);
    }

    #[test]
    fn an_unclosed_code_block_is_dropped_at_the_end() {
        let got = all(&mut SentenceChunker::new(), &["Look at this.\n```js\nlet a = 1;"]);
        assert_eq!(got, vec!["Look at this."]);
    }

    #[test]
    fn tables_and_rules_are_dropped() {
        let text = "Results are in.\n| a | b |\n|---|---|\n| 1 | 2 |\n---\nThat is the table.";
        let got = all(&mut SentenceChunker::new(), &[text]);
        assert_eq!(got, vec!["Results are in.", "That is the table."]);
    }

    #[test]
    fn text_with_nothing_to_say_gives_no_chunk() {
        assert!(all(&mut SentenceChunker::new(), &["***\n```\ncode\n```\n"]).is_empty());
    }

    #[test]
    fn a_long_run_without_an_end_is_cut_at_a_space() {
        let long = "word ".repeat(80);
        let mut c = SentenceChunker::new();
        let got = c.push(&long);
        assert!(!got.is_empty());
        assert!(got.iter().all(|s| s.chars().count() <= MAX_CHUNK_CHARS), "{got:?}");
    }

    #[test]
    fn a_colon_ends_a_chunk_but_a_time_does_not() {
        let got = all(&mut SentenceChunker::new(), &["Here is the plan: first we wait. We meet at 12:30 sharp."]);
        assert_eq!(got, vec!["Here is the plan:", "first we wait.", "We meet at 12:30 sharp."]);
    }

    fn drain(q: &mut SpeechQueue) -> Vec<Job> {
        let mut out = Vec::new();
        while let Some(j) = q.next_job() {
            q.job_finished();
            out.push(j);
        }
        out
    }

    #[test]
    fn the_queue_numbers_pieces_and_reports_the_end_once() {
        let mut q = SpeechQueue::new();
        q.push(1, "First sentence here. Second sentence here", false, false, None);
        assert!(q.take_done().is_none());
        q.push(1, ". ", true, false, None);
        let jobs = drain(&mut q);
        assert_eq!(jobs.iter().map(|j| (j.seq, j.text.as_str())).collect::<Vec<_>>(), vec![(0, "First sentence here."), (1, "Second sentence here.")]);
        assert_eq!(q.take_done(), Some((1, 2)));
        assert_eq!(q.take_done(), None);
    }

    #[test]
    fn a_reply_with_nothing_to_say_still_ends() {
        let mut q = SpeechQueue::new();
        q.push(3, "```\ncode\n```", true, false, None);
        assert!(drain(&mut q).is_empty());
        assert_eq!(q.take_done(), Some((3, 0)));
    }

    #[test]
    fn a_notice_is_a_job_but_not_text_of_the_reply() {
        let mut q = SpeechQueue::new();
        q.push(1, "Let me look that up.", false, true, Some("en".into()));
        q.push(1, "Found it. That is all.", true, false, None);
        let jobs = drain(&mut q);
        assert_eq!(jobs.len(), 3);
        assert!(jobs[0].notice && !jobs[1].notice);
        assert_eq!(jobs[0].language.as_deref(), Some("en"));
        assert_eq!(q.take_done(), Some((1, 3)));
    }

    #[test]
    fn cancelling_forgets_the_reply_and_ignores_late_text() {
        let mut q = SpeechQueue::new();
        q.push(2, "One sentence is queued here. More comes", false, false, None);
        q.cancel(2);
        assert!(!q.is_current(2));
        assert!(drain(&mut q).is_empty());
        q.push(2, " late text. ", true, false, None);
        assert!(drain(&mut q).is_empty());
        assert!(q.take_done().is_none());
        // The next reply works.
        q.push(3, "A new reply starts here.", true, false, None);
        assert!(q.is_current(3));
        assert_eq!(drain(&mut q).len(), 1);
    }

    #[test]
    fn a_newer_reply_replaces_the_old_one() {
        let mut q = SpeechQueue::new();
        q.push(1, "Old reply sentence one. Old reply two", false, false, None);
        q.push(2, "New reply is here.", true, false, None);
        let jobs = drain(&mut q);
        assert_eq!(jobs.len(), 1);
        assert_eq!((jobs[0].generation, jobs[0].seq), (2, 0));
    }

    #[test]
    fn one_worker_at_a_time() {
        let mut q = SpeechQueue::new();
        assert!(!q.claim_worker());
        q.push(1, "Hello there, this is a test. ", false, false, None);
        assert!(q.claim_worker());
        assert!(!q.claim_worker());
        drain(&mut q);
        q.release_worker();
        assert!(!q.claim_worker());
    }
}
