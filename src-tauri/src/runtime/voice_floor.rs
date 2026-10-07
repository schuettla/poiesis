//! Turn-taking (TRN-1 to TRN-4): who has the floor, when a turn is over, and
//! when the user cutting in should stop Poiesis. Pure logic with no models and
//! no clock of its own, so it is tested with scripted input. See
//! plans/VOICE_PLAN.md.
//!
//! The states and the cut-in ladder follow Openlive's `FloorState` (Apache-2.0,
//! ideas only, no source).
//!
//! The speech detector gives yes or no per 32 ms window (see Build notes), so
//! `observe` takes a bool. `speech_on` and `speech_off` from the plan would be
//! one threshold here; the timers do the smoothing instead.

/// Who has the floor right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Floor {
    /// Nobody is talking.
    Listening,
    UserSpeaking { since_ms: u64 },
    /// The user went quiet. A turn ends when the pause is long enough.
    UserPause { speech_since_ms: u64, silence_since_ms: u64 },
    /// The turn was sent; no sound from Poiesis yet.
    Thinking,
    Speaking { generation: u64 },
    /// The user is talking over Poiesis. Its voice is turned down.
    Ducked { generation: u64, overlap_since_ms: u64 },
}

impl Floor {
    /// Name sent to the screen (the orb follows it).
    pub fn name(&self) -> &'static str {
        match self {
            Floor::Listening => "listening",
            Floor::UserSpeaking { .. } | Floor::UserPause { .. } => "user_speaking",
            Floor::Thinking => "thinking",
            Floor::Speaking { .. } | Floor::Ducked { .. } => "speaking",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FloorConfig {
    /// Speech over Poiesis for this long stops it.
    pub cut_in_commit_ms: u64,
    /// Quiet again for this long turns Poiesis back up.
    pub unduck_ms: u64,
    /// Less speech than this is a cough or a "mhm" and is dropped.
    pub min_user_turn_ms: u64,
    /// Pause that ends a turn when nothing else is known.
    pub end_silence_ms: u64,
    /// Pause that ends a turn when the words so far read as finished.
    pub complete_silence_ms: u64,
    /// Pause that ends a turn when the last word asks for more ("and").
    pub incomplete_silence_ms: u64,
    /// A turn this long is cut off.
    pub max_turn_ms: u64,
    /// False means speaking over Poiesis does nothing (`voice.cut_in` off).
    pub cut_in: bool,
}

impl Default for FloorConfig {
    fn default() -> Self {
        Self {
            cut_in_commit_ms: 180,
            unduck_ms: 120,
            min_user_turn_ms: 280,
            end_silence_ms: 600,
            complete_silence_ms: 300,
            incomplete_silence_ms: 1200,
            max_turn_ms: 30_000,
            cut_in: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorAction {
    None,
    /// The user finished a turn: hear it and answer.
    EndOfTurn,
    /// The user made a sound that was too short to be a turn: forget the audio.
    Discard,
    /// The user started talking over Poiesis: turn its voice down.
    Duck,
    Unduck,
    /// The user cut in: stop speaking and stop the run.
    Yield,
}

/// What the last words say about whether the user is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Complete,
    Unknown,
    /// The last word is one a sentence does not end on.
    Incomplete,
}

const OPEN_EN: &[&str] = &[
    "and", "or", "but", "so", "because", "if", "when", "that", "which", "the", "a", "an", "my", "your", "to", "of",
    "with",
];
const OPEN_DE: &[&str] = &[
    "und", "oder", "aber", "weil", "dass", "wenn", "ob", "der", "die", "das", "ein", "eine", "mein", "dein", "zu",
    "mit", "von", "also",
];

/// Reads the end of what was said so far (TRN-4). `language` is a code like
/// `de` or `en`; anything else checks both lists.
pub fn ending_of(text: &str, language: &str) -> Ending {
    let text = text.trim();
    if text.ends_with(['.', '?', '!', '\u{2026}']) {
        return Ending::Complete;
    }
    let last = text
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    if last.is_empty() {
        return Ending::Unknown;
    }
    let open = match language {
        "de" => OPEN_DE.contains(&last.as_str()),
        "en" => OPEN_EN.contains(&last.as_str()),
        _ => OPEN_DE.contains(&last.as_str()) || OPEN_EN.contains(&last.as_str()),
    };
    if open {
        Ending::Incomplete
    } else {
        Ending::Unknown
    }
}

const WORDS_DE: &[&str] = &[
    "ich", "du", "und", "der", "die", "das", "ist", "nicht", "ein", "eine", "mit", "für", "auf", "wie", "was", "kannst",
    "bitte", "mir", "ja", "nein", "wir", "sie", "es", "zu", "von", "den", "dem", "auch", "aber", "wenn",
];
const WORDS_EN: &[&str] = &[
    "i", "you", "and", "the", "is", "not", "a", "an", "with", "for", "on", "how", "what", "can", "please", "me", "yes",
    "no", "we", "it", "to", "of", "that", "this", "are", "do", "have", "but", "if", "my",
];

/// Parakeet gives no language, so a small stop-word count stands in. `None`
/// when the text is too short or too even to tell.
pub fn guess_language(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    let (mut de, mut en) = (0, 0);
    for w in lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if WORDS_DE.contains(&w) {
            de += 1;
        }
        if WORDS_EN.contains(&w) {
            en += 1;
        }
    }
    if de + en < 2 || de == en {
        None
    } else if de > en {
        Some("de")
    } else {
        Some("en")
    }
}

pub struct FloorMachine {
    config: FloorConfig,
    floor: Floor,
    /// When the user's sound over Poiesis (or while it thinks) began.
    overlap_since: Option<u64>,
    /// When the user last went quiet while Poiesis was ducked.
    quiet_since: Option<u64>,
    ending: Ending,
}

impl FloorMachine {
    pub fn new(config: FloorConfig) -> Self {
        Self { config, floor: Floor::Listening, overlap_since: None, quiet_since: None, ending: Ending::Unknown }
    }

    pub fn floor(&self) -> Floor {
        self.floor
    }

    pub fn set_cut_in(&mut self, on: bool) {
        self.config.cut_in = on;
    }

    /// Feed one speech-detection window. Returns what the caller must do.
    pub fn observe(&mut self, now_ms: u64, speech: bool) -> FloorAction {
        match self.floor {
            Floor::Listening => {
                if speech {
                    self.floor = Floor::UserSpeaking { since_ms: now_ms };
                    self.ending = Ending::Unknown;
                }
                FloorAction::None
            }
            Floor::UserSpeaking { since_ms } => {
                if now_ms.saturating_sub(since_ms) >= self.config.max_turn_ms {
                    return self.end_turn();
                }
                if !speech {
                    self.floor = Floor::UserPause { speech_since_ms: since_ms, silence_since_ms: now_ms };
                }
                FloorAction::None
            }
            Floor::UserPause { speech_since_ms, silence_since_ms } => {
                if speech {
                    // Same turn goes on; what the words said so far is stale.
                    self.floor = Floor::UserSpeaking { since_ms: speech_since_ms };
                    self.ending = Ending::Unknown;
                    return FloorAction::None;
                }
                let silence = now_ms.saturating_sub(silence_since_ms);
                let spoke = silence_since_ms.saturating_sub(speech_since_ms);
                if spoke < self.config.min_user_turn_ms {
                    // A blip. Wait out the normal pause so a slow start is not cut.
                    if silence >= self.config.end_silence_ms {
                        self.floor = Floor::Listening;
                        return FloorAction::Discard;
                    }
                    return FloorAction::None;
                }
                if now_ms.saturating_sub(speech_since_ms) >= self.config.max_turn_ms || silence >= self.needed_silence() {
                    return self.end_turn();
                }
                FloorAction::None
            }
            Floor::Thinking => {
                if !self.config.cut_in {
                    return FloorAction::None;
                }
                if !speech {
                    self.overlap_since = None;
                    return FloorAction::None;
                }
                let since = *self.overlap_since.get_or_insert(now_ms);
                if now_ms.saturating_sub(since) >= self.config.cut_in_commit_ms {
                    self.overlap_since = None;
                    self.floor = Floor::UserSpeaking { since_ms: since };
                    self.ending = Ending::Unknown;
                    return FloorAction::Yield;
                }
                FloorAction::None
            }
            Floor::Speaking { generation } => {
                if self.config.cut_in && speech {
                    self.floor = Floor::Ducked { generation, overlap_since_ms: now_ms };
                    self.quiet_since = None;
                    return FloorAction::Duck;
                }
                FloorAction::None
            }
            Floor::Ducked { generation, overlap_since_ms } => {
                if speech {
                    self.quiet_since = None;
                    if now_ms.saturating_sub(overlap_since_ms) >= self.config.cut_in_commit_ms {
                        self.floor = Floor::UserSpeaking { since_ms: overlap_since_ms };
                        self.ending = Ending::Unknown;
                        return FloorAction::Yield;
                    }
                    return FloorAction::None;
                }
                let quiet = *self.quiet_since.get_or_insert(now_ms);
                if now_ms.saturating_sub(quiet) >= self.config.unduck_ms {
                    self.quiet_since = None;
                    self.floor = Floor::Speaking { generation };
                    return FloorAction::Unduck;
                }
                FloorAction::None
            }
        }
    }

    fn end_turn(&mut self) -> FloorAction {
        self.floor = Floor::Thinking;
        self.overlap_since = None;
        FloorAction::EndOfTurn
    }

    fn needed_silence(&self) -> u64 {
        match self.ending {
            Ending::Complete => self.config.complete_silence_ms,
            Ending::Unknown => self.config.end_silence_ms,
            Ending::Incomplete => self.config.incomplete_silence_ms,
        }
    }

    /// TRN-4: what the words so far say about the end of the turn.
    pub fn set_partial_text(&mut self, text: &str, language: &str) {
        if matches!(self.floor, Floor::UserPause { .. }) {
            self.ending = ending_of(text, language);
        }
    }

    /// Poiesis began to make sound.
    pub fn assistant_started(&mut self, generation: u64) {
        if matches!(self.floor, Floor::Thinking | Floor::Listening) {
            self.floor = Floor::Speaking { generation };
            self.overlap_since = None;
        }
    }

    /// Poiesis is silent again: it finished, it was stopped, or it failed.
    pub fn assistant_finished(&mut self) {
        if matches!(self.floor, Floor::Thinking | Floor::Speaking { .. } | Floor::Ducked { .. }) {
            self.floor = Floor::Listening;
            self.overlap_since = None;
            self.quiet_since = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: u64 = 32;

    /// Runs `ms` of speech or silence from `*now`, returns the actions that
    /// were not `None`, each with its time.
    fn run(m: &mut FloorMachine, now: &mut u64, ms: u64, speech: bool) -> Vec<(u64, FloorAction)> {
        let end = *now + ms;
        let mut out = Vec::new();
        while *now < end {
            let a = m.observe(*now, speech);
            if a != FloorAction::None {
                out.push((*now, a));
            }
            *now += WINDOW;
        }
        out
    }

    fn actions(v: &[(u64, FloorAction)]) -> Vec<FloorAction> {
        v.iter().map(|(_, a)| *a).collect()
    }

    #[test]
    fn a_cough_is_dropped_and_never_ends_a_turn() {
        let mut m = FloorMachine::new(FloorConfig::default());
        let mut now = 0;
        run(&mut m, &mut now, 160, true);
        let after = run(&mut m, &mut now, 1500, false);
        assert_eq!(actions(&after), vec![FloorAction::Discard]);
        assert_eq!(m.floor(), Floor::Listening);
    }

    #[test]
    fn a_turn_ends_after_the_normal_pause() {
        let mut m = FloorMachine::new(FloorConfig::default());
        let mut now = 0;
        run(&mut m, &mut now, 1000, true);
        let silence_from = now;
        let after = run(&mut m, &mut now, 1500, false);
        assert_eq!(actions(&after), vec![FloorAction::EndOfTurn]);
        let at = after[0].0 - silence_from;
        assert!((600..600 + WINDOW).contains(&at), "ended after {at} ms");
        assert_eq!(m.floor(), Floor::Thinking);
    }

    #[test]
    fn a_finished_sentence_ends_sooner() {
        let mut m = FloorMachine::new(FloorConfig::default());
        let mut now = 0;
        run(&mut m, &mut now, 1000, true);
        let silence_from = now;
        run(&mut m, &mut now, 200, false);
        m.set_partial_text("Wie spaet ist es?", "de");
        let after = run(&mut m, &mut now, 1500, false);
        let at = after[0].0 - silence_from;
        assert!((300..300 + WINDOW).contains(&at), "ended after {at} ms");
    }

    #[test]
    fn a_pause_after_und_waits_longer() {
        let mut m = FloorMachine::new(FloorConfig::default());
        let mut now = 0;
        run(&mut m, &mut now, 1000, true);
        let silence_from = now;
        run(&mut m, &mut now, 200, false);
        m.set_partial_text("Ich moechte ein Bild und", "de");
        let after = run(&mut m, &mut now, 2000, false);
        let at = after[0].0 - silence_from;
        assert!((1200..1200 + WINDOW).contains(&at), "ended after {at} ms");
    }

    #[test]
    fn speaking_again_keeps_the_turn_and_clears_the_hint() {
        let mut m = FloorMachine::new(FloorConfig::default());
        let mut now = 0;
        run(&mut m, &mut now, 1000, true);
        run(&mut m, &mut now, 200, false);
        m.set_partial_text("Das war es.", "de");
        run(&mut m, &mut now, 300, true);
        // The old hint must not shorten the next pause.
        let silence_from = now;
        let after = run(&mut m, &mut now, 1500, false);
        let at = after[0].0 - silence_from;
        assert!(at >= 600, "ended after {at} ms");
    }

    #[test]
    fn a_very_long_turn_is_cut_off() {
        let mut m = FloorMachine::new(FloorConfig { max_turn_ms: 5000, ..Default::default() });
        let mut now = 0;
        // Stop just after the cut so the run does not go on to a cut-in.
        let a = run(&mut m, &mut now, 5100, true);
        assert_eq!(actions(&a), vec![FloorAction::EndOfTurn]);
        assert!(a[0].0 >= 5000 && a[0].0 < 5000 + WINDOW);
    }

    fn speaking() -> (FloorMachine, u64) {
        let mut m = FloorMachine::new(FloorConfig::default());
        let mut now = 0;
        run(&mut m, &mut now, 1000, true);
        run(&mut m, &mut now, 700, false);
        assert_eq!(m.floor(), Floor::Thinking);
        m.assistant_started(7);
        (m, now)
    }

    #[test]
    fn a_short_overlap_ducks_and_then_unducks() {
        let (mut m, mut now) = speaking();
        let a = run(&mut m, &mut now, 100, true);
        assert_eq!(actions(&a), vec![FloorAction::Duck]);
        let b = run(&mut m, &mut now, 400, false);
        assert_eq!(actions(&b), vec![FloorAction::Unduck]);
        assert_eq!(m.floor(), Floor::Speaking { generation: 7 });
    }

    #[test]
    fn a_long_overlap_yields_and_the_user_has_the_floor() {
        let (mut m, mut now) = speaking();
        let a = run(&mut m, &mut now, 400, true);
        assert_eq!(actions(&a), vec![FloorAction::Duck, FloorAction::Yield]);
        assert!(matches!(m.floor(), Floor::UserSpeaking { .. }));
        // The cut-in words become the next turn.
        let b = run(&mut m, &mut now, 1500, false);
        assert_eq!(actions(&b), vec![FloorAction::EndOfTurn]);
    }

    #[test]
    fn with_cut_in_off_nothing_happens_over_poiesis() {
        let (mut m, mut now) = speaking();
        m.set_cut_in(false);
        assert!(run(&mut m, &mut now, 2000, true).is_empty());
        assert_eq!(m.floor(), Floor::Speaking { generation: 7 });
    }

    #[test]
    fn talking_while_it_thinks_stops_the_run() {
        let mut m = FloorMachine::new(FloorConfig::default());
        let mut now = 0;
        run(&mut m, &mut now, 1000, true);
        run(&mut m, &mut now, 700, false);
        assert_eq!(m.floor(), Floor::Thinking);
        let a = run(&mut m, &mut now, 400, true);
        assert_eq!(actions(&a), vec![FloorAction::Yield]);
    }

    #[test]
    fn finishing_gives_the_floor_back() {
        let (mut m, _) = speaking();
        m.assistant_finished();
        assert_eq!(m.floor(), Floor::Listening);
        assert_eq!(m.floor().name(), "listening");
    }

    #[test]
    fn endings_follow_the_word_lists() {
        assert_eq!(ending_of("Is it done?", "en"), Ending::Complete);
        assert_eq!(ending_of("Das ist gut.", "de"), Ending::Complete);
        assert_eq!(ending_of("I want tea and", "en"), Ending::Incomplete);
        assert_eq!(ending_of("Ich will Tee und,", "de"), Ending::Incomplete);
        assert_eq!(ending_of("I want tea", "en"), Ending::Unknown);
        // The other language's list does not apply.
        assert_eq!(ending_of("Das ist die", "en"), Ending::Unknown);
        // Unknown language checks both.
        assert_eq!(ending_of("Das ist die", "auto"), Ending::Incomplete);
        assert_eq!(ending_of("", "en"), Ending::Unknown);
    }

    #[test]
    fn the_language_guess_needs_evidence() {
        assert_eq!(guess_language("Ich habe eine Frage und bitte um Hilfe"), Some("de"));
        assert_eq!(guess_language("Can you help me with this please"), Some("en"));
        assert_eq!(guess_language("Hmm"), None);
        assert_eq!(guess_language(""), None);
    }
}
