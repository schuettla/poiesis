//! `USE-11`: a look at the page the browser agent has just reached. After a
//! browse, a click or a key press the agent asks, with a picture of the page and
//! the start of its text, whether it shows ordinary content, a login, a captcha or
//! an error. A login or a captcha stops the agent and hands the browser to the
//! person; so does a page the model cannot place. Nothing else changes: with no
//! picture reader, no engine, no budget or an answer that does not come, the
//! agent carries on exactly as it did before there was a check.
//!
//! The picture is the screenshot the Browser panel already takes. The question is
//! a pick between four fixed options, so a page that talks to the model cannot make
//! it do anything but pick one of them, and the only thing a pick can do is stop
//! the agent for a click.

use std::path::Path;

use super::control::{self, Act, Action, Gate};
use super::policy::Purpose;
use super::{Answer, Ask, ImageRef, Opts, Question, Verdict};

/// The id of the one question.
pub const PAGE_ID: &str = "page";
/// How much of the page's text the model reads, in characters.
const TEXT_CHARS: usize = 700;
/// A longer path is cut: it only helps the model tell a login address from a
/// product one.
const URL_CHARS: usize = 160;
/// A screenshot larger than this is not sent. A viewport PNG is a few hundred KB.
const MAX_SHOT_BYTES: u64 = 8 * 1024 * 1024;
/// A page the model calls an error is passed on with a note at this probability.
const ERROR_NOTE_AT: f32 = 0.6;
/// A winner under this probability is not named: the card says the model cannot tell.
const NAME_AT: f32 = 0.5;

/// The four things a page can be, with the sentence the model reads for each.
pub const OPTIONS: [(&str, &str); 4] = [
    (
        "fine",
        "The page shows ordinary content the assistant can read and use: an article, a list, a form to fill, a product, a search result or a document.",
    ),
    (
        "login",
        "The page asks the visitor to sign in or log in before it shows what was wanted: a login form, a sign-in prompt or a notice that members only can see the content.",
    ),
    (
        "captcha",
        "The page asks the visitor to prove they are a person: a captcha, a verify-you-are-human check, a checkbox challenge or a bot-detection page.",
    ),
    (
        "error",
        "The page is an error or has nothing on it: not found, access denied, a server error, a blocked page or a blank page.",
    ),
];

/// What stopped the agent, in the words the card uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wall {
    Login,
    Captcha,
    /// Neither was the likeliest, and `fine` fell short of the bar.
    Unsure,
}

impl Wall {
    pub fn code(self) -> &'static str {
        match self {
            Wall::Login => "login",
            Wall::Captcha => "captcha",
            Wall::Unsure => "unsure",
        }
    }

    /// The question the card asks.
    pub fn question(self) -> &'static str {
        match self {
            Wall::Login => "This page wants a login. Take over?",
            Wall::Captcha => "This page wants to know you are a person. Take over?",
            Wall::Unsure => "I can't tell what this page is showing. Want to look?",
        }
    }

    fn words(self) -> &'static str {
        match self {
            Wall::Login => "login page",
            Wall::Captcha => "captcha",
            Wall::Unsure => "not sure",
        }
    }
}

/// What the model made of a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageRead {
    /// Sure enough that it is ordinary: go on.
    Fine,
    /// An error or blank page, and the model is fairly sure: go on, and say so.
    Error(f32),
    /// Stop and hand over. `p` is the probability of what the card names.
    Stop { wall: Wall, p: f32 },
}

impl PageRead {
    /// What the controller counts it as: stopping is the below-the-bar default.
    pub fn act(self) -> Act {
        match self {
            PageRead::Stop { .. } => Act::Below,
            _ => Act::Go,
        }
    }
}

/// The question for one page. `host` and `path` are the address without its query,
/// which can carry tokens; `text` is what the page shows.
pub fn ask_state(host: &str, path: &str, title: &str, text: &str) -> String {
    let address: String = format!("{host}{path}").chars().take(URL_CHARS).collect();
    let title: String = title.chars().take(160).collect();
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(TEXT_CHARS).collect();
    format!(
        "An AI assistant is browsing the web for a user and has just reached a page.\nAddress: {address}\nTitle: {title}\nThe start of the text on the page:\n{text}"
    )
}

pub fn question() -> Question {
    Question::Choice {
        instructions: "What is this page showing?".into(),
        options: OPTIONS.iter().map(|(id, text)| ((*id).into(), (*text).into())).collect(),
    }
}

/// Read one verdict. `None` when the answer is not the one asked for.
pub fn read_page(v: &Verdict) -> Option<PageRead> {
    let answer = v.get(PAGE_ID)?;
    let Answer::Choice { choice, probs, .. } = answer else { return None };
    let p_of = |id: &str| probs.iter().find(|(k, _)| k == id).map_or(0.0, |(_, p)| *p);
    if control::evaluate(Purpose::BrowserCheck, answer, v.calibrated, Gate::Open) == Act::Go {
        return Some(PageRead::Fine);
    }
    let p = p_of(choice);
    Some(match choice.as_str() {
        "login" if p >= NAME_AT => PageRead::Stop { wall: Wall::Login, p },
        "captcha" if p >= NAME_AT => PageRead::Stop { wall: Wall::Captcha, p },
        "error" if p >= ERROR_NOTE_AT => PageRead::Error(p),
        // Under the bar for `fine`, and nothing else is clear: say the model cannot tell.
        _ => PageRead::Stop { wall: Wall::Unsure, p: p_of("fine") },
    })
}

/// The line under the card's question: what was seen and how sure, in the words
/// the handover card uses for a quick decision.
pub fn card_line(wall: Wall, p: f32) -> String {
    match wall {
        Wall::Unsure => format!("{} \u{b7} quick decision (page looks fine {p:.2})", wall.words()),
        _ => format!("{} \u{b7} quick decision ({p:.2})", wall.words()),
    }
}

/// The note added to a tool result for an error page.
pub fn error_note(p: f32) -> String {
    format!("[A quick look at the page: it seems to be an error or an empty page ({p:.2}).]")
}

/// What the model is told when the person chose not to take over.
pub fn skipped_text(wall: Wall) -> &'static str {
    match wall {
        Wall::Login => "The page needs a sign-in and the person chose to skip this step. Do not try to get past it. Carry on without this page, or say what you could not do.",
        Wall::Captcha => "The page asks to prove the visitor is a person and the person chose to skip this step. Do not try to get past it. Carry on without this page, or say what you could not do.",
        Wall::Unsure => "A quick look could not tell what this page shows and the person chose to skip this step. Carry on without this page, or say what you could not do.",
    }
}

/// What one look came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub read: PageRead,
    /// For the label the card's answer writes.
    pub decision: String,
}

/// A screenshot as a PNG data URL. A file that cannot be read means no check.
fn data_url(path: &Path) -> Result<String, String> {
    use base64::Engine as _;
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_SHOT_BYTES {
        return Err("the picture is too large".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Ask about one page. `None` means carry on as before, for any reason: the
/// experiment is off, the engine is cold, on the processor or has no picture
/// reader, the turn is out of decisions, or the answer did not come. A stop that
/// the turn has no escalation left for is also `None`: three cards a turn is all
/// a person is asked for.
pub async fn look(state: &str, shot: &Path, conversation_id: &str) -> Option<Look> {
    let image = data_url(shot).ok()?;
    let opts = Opts { conversation_id: Some(conversation_id), run_id: None };
    let ask = Ask { state, images: vec![ImageRef { data_url: image }], questions: vec![(PAGE_ID, question())] };
    // Every error is the old path: no card and nothing said. The seam has already
    // logged what it was asked to.
    let verdict = super::ask(&ask, Purpose::BrowserCheck, opts).await.ok()?;
    judge(&verdict, conversation_id)
}

/// Read a verdict and say what was done with it (`CTL-2`).
fn judge(verdict: &Verdict, conversation_id: &str) -> Option<Look> {
    let read = read_page(verdict)?;
    let opts = Opts { conversation_id: Some(conversation_id), run_id: None };
    let action = super::settle(verdict, Purpose::BrowserCheck, read.act(), opts);
    let read = match (read, action) {
        // The turn has shown its three cards: go on, as it did before the check.
        (PageRead::Stop { .. }, Action::Budget(_)) => PageRead::Fine,
        (r, _) => r,
    };
    if let PageRead::Stop { wall, .. } = read {
        super::note(
            verdict,
            match wall {
                Wall::Login => "Stopped the browser at a login",
                Wall::Captcha => "Stopped the browser at a check that wants a person",
                Wall::Unsure => "Stopped the browser on a page I could not read",
            },
        );
    }
    Some(Look { read, decision: verdict.id.clone() })
}

/// What the person did with the card. `Open` took over, `Fine` said the page was
/// fine, `Skip` said nothing about the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAnswer {
    Open,
    Fine,
    Skip,
}

/// `CTL-3`: taking over says the page needed a person, "the page is fine" says
/// the stop was wrong. Skipping a step says nothing about the page, so it writes
/// nothing: a label that can only ever be `right` would teach a fit that every
/// stop was.
pub fn label_card(db: &crate::db::Db, decision: &str, answer: CardAnswer) {
    let label = match answer {
        CardAnswer::Open => "right",
        CardAnswer::Fine => "wrong",
        CardAnswer::Skip => return,
    };
    let _ = db.add_outcome(decision, label, None, "browser-card");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::decide::{BackendId, Verdict};
    use std::collections::BTreeMap;

    fn verdict(calibrated: bool, probs: [(&str, f32); 4]) -> Verdict {
        let winner = probs.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap().0.to_string();
        let list: Vec<(String, f32)> = probs.iter().map(|(k, p)| ((*k).to_string(), *p)).collect();
        let confidence = crate::agent::decide::confidence(&list.iter().map(|(_, p)| *p).collect::<Vec<_>>());
        let mut answers = BTreeMap::new();
        answers.insert(PAGE_ID.to_string(), Answer::Choice { choice: winner, probs: list, confidence });
        Verdict { id: "d1".into(), answers, backend: BackendId::Local { model: "m".into() }, calibrated, ms: 10 }
    }

    fn page(fine: f32, login: f32, captcha: f32, error: f32) -> Verdict {
        verdict(false, [("fine", fine), ("login", login), ("captcha", captcha), ("error", error)])
    }

    #[test]
    fn the_question_names_the_four_options_the_row_reads() {
        let Question::Choice { options, .. } = question() else { panic!("a pick-one question") };
        let ids: Vec<&str> = options.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["fine", "login", "captcha", "error"]);
        assert_eq!(Purpose::BrowserCheck.policy().act_at.measure, super::super::policy::Measure::Option("fine"));
    }

    #[test]
    fn the_state_keeps_the_address_without_its_query_and_cuts_the_text() {
        let state = ask_state("shop.example", "/account/login", "Sign in", &"word ".repeat(400));
        assert!(state.contains("Address: shop.example/account/login\n"));
        assert!(!state.contains('?'), "no query string reaches the model");
        let text = state.split("page:\n").nth(1).unwrap();
        assert_eq!(text.chars().count(), TEXT_CHARS);
    }

    #[test]
    fn a_page_changing_its_text_changes_the_state_so_it_is_asked_again() {
        let a = ask_state("a.example", "/", "Home", "Welcome back");
        let b = ask_state("a.example", "/", "Home", "Please sign in to continue");
        assert_ne!(a, b);
    }

    #[test]
    fn a_page_that_is_sure_to_be_ordinary_goes_on() {
        assert_eq!(read_page(&page(0.9, 0.05, 0.03, 0.02)), Some(PageRead::Fine));
        // Exactly the bar goes on, as the row says "proceed at 0.8".
        assert_eq!(read_page(&page(0.8, 0.1, 0.05, 0.05)), Some(PageRead::Fine));
    }

    #[test]
    fn a_login_or_a_captcha_stops_the_agent_and_names_which() {
        assert_eq!(read_page(&page(0.1, 0.8, 0.05, 0.05)), Some(PageRead::Stop { wall: Wall::Login, p: 0.8 }));
        assert_eq!(read_page(&page(0.1, 0.05, 0.8, 0.05)), Some(PageRead::Stop { wall: Wall::Captcha, p: 0.8 }));
    }

    /// Under the bar, with nothing clear to name, the agent is stopped all the
    /// same: that is the row's below-threshold default.
    #[test]
    fn a_page_that_is_not_clearly_anything_stops_the_agent_as_unsure() {
        let got = read_page(&page(0.45, 0.3, 0.15, 0.1)).unwrap();
        assert_eq!(got, PageRead::Stop { wall: Wall::Unsure, p: 0.45 });
        // A winner under a half is not named even when it leads.
        let got = read_page(&page(0.2, 0.4, 0.3, 0.1)).unwrap();
        assert_eq!(got, PageRead::Stop { wall: Wall::Unsure, p: 0.2 });
        assert_eq!(got.act(), Act::Below);
    }

    #[test]
    fn an_error_page_goes_on_with_a_note_when_the_model_is_fairly_sure() {
        assert_eq!(read_page(&page(0.1, 0.1, 0.05, 0.75)), Some(PageRead::Error(0.75)));
        assert_eq!(PageRead::Error(0.75).act(), Act::Go);
        // Not fairly sure: the person is asked to look instead of the model guessing.
        let got = read_page(&page(0.3, 0.1, 0.05, 0.55)).unwrap();
        assert_eq!(got, PageRead::Stop { wall: Wall::Unsure, p: 0.3 });
        assert!(error_note(0.75).contains("0.75"));
    }

    /// An answer without the question's id, or of another kind, reads as nothing.
    #[test]
    fn an_answer_that_is_not_the_one_asked_for_reads_as_nothing() {
        let mut v = page(0.9, 0.05, 0.03, 0.02);
        v.answers.clear();
        assert_eq!(read_page(&v), None);
        v.answers.insert(PAGE_ID.into(), Answer::Noul { p_true: 0.9 });
        assert_eq!(read_page(&v), None);
    }

    #[test]
    fn the_card_says_what_was_seen_and_how_sure() {
        assert_eq!(card_line(Wall::Login, 0.91), "login page \u{b7} quick decision (0.91)");
        assert_eq!(card_line(Wall::Unsure, 0.45), "not sure \u{b7} quick decision (page looks fine 0.45)");
        assert_eq!(Wall::Login.question(), "This page wants a login. Take over?");
        for w in [Wall::Login, Wall::Captcha, Wall::Unsure] {
            assert!(skipped_text(w).contains("skip"), "{}", w.code());
        }
    }

    /// `CTL-3`: taking over is right, "the page is fine" is wrong, a skip is nothing.
    #[test]
    fn the_cards_answers_write_labels_and_a_skip_writes_none() {
        let db = crate::db::Db::open_in_memory().unwrap();
        for id in ["open", "fine", "skip"] {
            db.add_decision(&crate::db::decisions::NewDecision {
                id,
                conversation_id: None,
                run_id: None,
                purpose: "browser-check",
                backend: "local:m",
                model: "m",
                ms: 1,
                calibrated: false,
                answers: &serde_json::json!({}),
                input_tokens: 0,
                state_preview: "",
                state_hash: id,
                action: None,
            })
            .unwrap();
        }
        label_card(&db, "open", CardAnswer::Open);
        label_card(&db, "fine", CardAnswer::Fine);
        label_card(&db, "skip", CardAnswer::Skip);
        let labels = |id: &str| db.decision_by_id(id).unwrap().unwrap().outcomes;
        assert_eq!(labels("open").iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["right"]);
        assert_eq!(labels("fine").iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["wrong"]);
        assert!(labels("skip").is_empty());
    }

    /// `CTL-2`: a stop is shown as a card, three times a turn. The fourth is let
    /// through, as the browser went on before the check; a turn of its own is
    /// not touched by another's.
    #[test]
    fn a_turn_is_stopped_for_three_cards_and_then_the_agent_goes_on() {
        let stop = page(0.05, 0.9, 0.03, 0.02);
        let key = "pagecheck-budget-turn";
        for _ in 0..3 {
            let got = judge(&stop, key).unwrap();
            assert_eq!(got.read, PageRead::Stop { wall: Wall::Login, p: 0.9 });
        }
        assert_eq!(judge(&stop, key).unwrap().read, PageRead::Fine, "the fourth card is not shown");
        // A page that is fine costs no escalation, and another turn starts afresh.
        assert_eq!(judge(&page(0.95, 0.02, 0.02, 0.01), key).unwrap().read, PageRead::Fine);
        assert!(matches!(judge(&stop, "pagecheck-other-turn").unwrap().read, PageRead::Stop { .. }));
    }

    #[test]
    fn a_screenshot_is_sent_as_a_png_data_url_and_a_missing_one_is_no_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, [0x89, b'P', b'N', b'G']).unwrap();
        assert!(data_url(&path).unwrap().starts_with("data:image/png;base64,"));
        assert!(data_url(&dir.path().join("nope.png")).is_err());
    }
}
