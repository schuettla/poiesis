//! A live voice session (TRN-5): mic audio in, turns out, speech back. See
//! plans/VOICE_PLAN.md.
//!
//! Two layers. [`TurnPipeline`] turns a stream of audio into turn events and
//! has no models or clock, so it is tested with a fake detector.
//! [`VoiceSession`] wires it to the speech engines and to the screen.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use base64::Engine as _;
use serde::Serialize;
use sherpa_onnx::VoiceActivityDetector;
use tauri::ipc::Channel;
use tauri::Manager;

use super::voice::{f32_to_pcm16, vad_config, VoiceManager, SAMPLE_RATE};
use super::voice_catalog::{language_name, voice_catalog, VoicePaths};
use super::voice_floor::{guess_language, Floor, FloorAction, FloorConfig, FloorMachine};
use super::voice_speech::{Job, SpeechQueue};

/// Speech detection works on 32 ms windows of 16 kHz audio.
pub const WINDOW: usize = 512;
/// Audio kept from before speech was noticed, so the first word is not cut.
const PREROLL: usize = 9_600;
/// While the user pauses: first look at the words after this much quiet...
const PARTIAL_AFTER_MS: u64 = 200;
/// ...and again this often (TRN-4).
const PARTIAL_EVERY_MS: u64 = 400;

/// What the screen is told. Audio travels as base64 of 16-bit little-endian
/// samples, which is much smaller than a JSON number list.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VoiceEvent {
    /// Who has the floor: `listening`, `user_speaking`, `thinking`, `speaking`.
    Floor { state: String },
    /// Words so far, while the user pauses.
    Partial { text: String },
    /// The finished turn, exactly as heard.
    Transcript { text: String, language: Option<String> },
    Duck,
    Unduck,
    /// The user cut in: stop speaking and stop the run.
    Yield,
    Audio { generation: u64, seq: u32, sample_rate: u32, pcm: String, text: String, notice: bool },
    /// Every piece of this reply has been made.
    SpeechDone { generation: u64 },
    /// One plain line for the voice surface (VXP-7).
    Hint { text: String },
    Error { message: String },
}

// ---------------------------------------------------------------------------
// Turn pipeline
// ---------------------------------------------------------------------------

/// Says whether a 32 ms window holds speech.
pub trait Detector: Send {
    fn detect(&mut self, window: &[f32]) -> bool;
}

/// Silero through sherpa. It only says yes or no (see Build notes).
pub struct SileroDetector(VoiceActivityDetector);

impl SileroDetector {
    pub fn new(model: &Path) -> Option<Self> {
        VoiceActivityDetector::create(&vad_config(model, 1), 30.0).map(Self)
    }
}

impl Detector for SileroDetector {
    fn detect(&mut self, window: &[f32]) -> bool {
        self.0.accept_waveform(window);
        let speech = self.0.detected();
        // The turn machine keeps its own audio; do not let sherpa pile up segments.
        if !self.0.is_empty() {
            self.0.clear();
        }
        speech
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// New floor name for the orb.
    Floor(&'static str),
    Duck,
    Unduck,
    Yield,
    /// The turn is over; this is all of its audio.
    EndOfTurn(Vec<f32>),
    /// A short sound was dropped (a cough); the floor is free again.
    Discard,
    /// Audio so far of a pause: hear it to learn how the turn reads.
    Partial { audio: Vec<f32>, turn: u64 },
}

pub struct TurnPipeline<D: Detector> {
    detector: D,
    floor: FloorMachine,
    /// Samples not yet a whole window.
    pending: Vec<f32>,
    windows_done: u64,
    /// Audio of the turn in progress (and a little before it).
    buffer: Vec<f32>,
    shown: &'static str,
    /// Counts turns, so a late partial for an old one is ignored.
    turn: u64,
    partial_at: Option<u64>,
    min_turn_ms: u64,
}

impl<D: Detector> TurnPipeline<D> {
    pub fn new(detector: D, config: FloorConfig) -> Self {
        Self {
            min_turn_ms: config.min_user_turn_ms,
            detector,
            floor: FloorMachine::new(config),
            pending: Vec::new(),
            windows_done: 0,
            buffer: Vec::new(),
            shown: "listening",
            turn: 0,
            partial_at: None,
        }
    }

    pub fn turn(&self) -> u64 {
        self.turn
    }

    /// Takes 16 kHz mono samples in any size.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<Out> {
        self.pending.extend_from_slice(samples);
        let mut outs = Vec::new();
        while self.pending.len() >= WINDOW {
            let window: Vec<f32> = self.pending.drain(..WINDOW).collect();
            self.windows_done += 1;
            let now_ms = self.windows_done * WINDOW as u64 * 1000 / SAMPLE_RATE as u64;
            let speech = self.detector.detect(&window);
            self.buffer.extend_from_slice(&window);
            match self.floor.observe(now_ms, speech) {
                FloorAction::None => {}
                FloorAction::EndOfTurn => {
                    self.turn += 1;
                    outs.push(Out::EndOfTurn(std::mem::take(&mut self.buffer)));
                }
                FloorAction::Discard => {
                    self.turn += 1;
                    outs.push(Out::Discard);
                }
                FloorAction::Duck => outs.push(Out::Duck),
                FloorAction::Unduck => outs.push(Out::Unduck),
                FloorAction::Yield => {
                    self.turn += 1;
                    outs.push(Out::Yield);
                }
            }
            self.settle(now_ms, &mut outs);
        }
        outs
    }

    /// After each window: keep only a little audio when nobody is mid-turn, say
    /// when the floor changed, and ask for a look at the words during a pause.
    fn settle(&mut self, now_ms: u64, outs: &mut Vec<Out>) {
        let floor = self.floor.floor();
        if matches!(floor, Floor::Listening | Floor::Thinking | Floor::Speaking { .. }) && self.buffer.len() > PREROLL {
            let extra = self.buffer.len() - PREROLL;
            self.buffer.drain(..extra);
        }
        self.announce(outs);
        match floor {
            Floor::UserPause { speech_since_ms, silence_since_ms } => {
                let quiet = now_ms.saturating_sub(silence_since_ms);
                // A cough is not worth a look.
                let long_enough = silence_since_ms.saturating_sub(speech_since_ms) >= self.min_turn_ms;
                let due = self.partial_at.is_none_or(|t| now_ms.saturating_sub(t) >= PARTIAL_EVERY_MS);
                if long_enough && quiet >= PARTIAL_AFTER_MS && due {
                    self.partial_at = Some(now_ms);
                    outs.push(Out::Partial { audio: self.buffer.clone(), turn: self.turn });
                }
            }
            _ => self.partial_at = None,
        }
    }

    fn announce(&mut self, outs: &mut Vec<Out>) {
        let name = self.floor.floor().name();
        if name != self.shown {
            self.shown = name;
            outs.push(Out::Floor(name));
        }
    }

    /// TRN-4. Ignored when it belongs to a turn that is already over.
    pub fn set_partial(&mut self, turn: u64, text: &str, language: &str) {
        if turn == self.turn {
            self.floor.set_partial_text(text, language);
        }
    }

    pub fn set_cut_in(&mut self, on: bool) {
        self.floor.set_cut_in(on);
    }

    /// True while the user is talking (or pausing mid-turn). A turn that was
    /// still being heard when this began is not the whole thought.
    pub fn user_has_floor(&self) -> bool {
        matches!(self.floor.floor(), Floor::UserSpeaking { .. } | Floor::UserPause { .. })
    }

    pub fn assistant_started(&mut self, generation: u64) -> Vec<Out> {
        self.floor.assistant_started(generation);
        let mut outs = Vec::new();
        self.announce(&mut outs);
        outs
    }

    pub fn assistant_finished(&mut self) -> Vec<Out> {
        self.floor.assistant_finished();
        let mut outs = Vec::new();
        self.announce(&mut outs);
        outs
    }
}

/// Words of a turn that was heard after the user had already started talking
/// again. They are not sent alone: the next turn carries them, so the two parts
/// read as one thought.
#[derive(Default)]
struct Carry(String);

impl Carry {
    fn hold(&mut self, text: &str) {
        self.0 = join_words(&self.0, text);
    }

    /// `text` after whatever was held; empties the carry.
    fn with(&mut self, text: &str) -> String {
        join_words(&std::mem::take(&mut self.0), text)
    }

    fn take(&mut self) -> Option<String> {
        let held = std::mem::take(&mut self.0);
        (!held.is_empty()).then_some(held)
    }
}

fn join_words(a: &str, b: &str) -> String {
    match (a.trim(), b.trim()) {
        ("", b) => b.to_string(),
        (a, "") => a.to_string(),
        (a, b) => format!("{a} {b}"),
    }
}

// ---------------------------------------------------------------------------
// Choosing a voice for a reply (VTN-4)
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub struct VoicePick {
    pub voice_id: String,
    /// Shown once when the reply's language has no installed voice.
    pub hint: Option<String>,
}

/// Keeps the chosen voice unless the reply is in a language it does not speak
/// and an installed voice does. `model_installed` takes a voice model id.
pub fn pick_voice(selected: &str, language: Option<&str>, model_installed: impl Fn(&str) -> bool) -> VoicePick {
    let catalog = voice_catalog();
    let keep = VoicePick { voice_id: selected.to_string(), hint: None };
    let selected_language = catalog.voices.iter().flat_map(|m| &m.voices).find(|v| v.id == selected).map(|v| v.language.as_str());
    let (Some(want), Some(have)) = (language, selected_language) else {
        return keep;
    };
    if want == have {
        return keep;
    }
    for model in catalog.voices.iter().filter(|m| model_installed(&m.id)) {
        if let Some(v) = model.voices.iter().find(|v| v.language == want) {
            return VoicePick { voice_id: v.id.clone(), hint: None };
        }
    }
    let hint = language_name(want).map(|name| {
        format!("This reply is in {name}, but no {name} voice is installed. You can get one in Voice settings.")
    });
    VoicePick { hint, ..keep }
}

// ---------------------------------------------------------------------------
// The session
// ---------------------------------------------------------------------------

/// What a session needs from the user's settings.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub hearing_id: String,
    pub voice_id: String,
    pub speed: f32,
    /// `auto` or a language code.
    pub language: String,
    pub cut_in: bool,
    pub threads: i32,
}

pub struct VoiceSession {
    app: tauri::AppHandle,
    events: Channel<VoiceEvent>,
    config: SessionConfig,
    paths: VoicePaths,
    pipeline: StdMutex<TurnPipeline<SileroDetector>>,
    queue: StdMutex<SpeechQueue>,
    closed: AtomicBool,
    last_frame: AtomicU64,
    partial_running: AtomicBool,
    /// Language of the user's last turn, when it could be told.
    user_language: StdMutex<Option<String>>,
    /// Language chosen for the reply being spoken: (generation, language).
    reply_language: StdMutex<Option<(u64, Option<String>)>>,
    hint_sent: AtomicBool,
    /// Heard words waiting for the turn the user is still speaking.
    carry: StdMutex<Carry>,
}

impl VoiceSession {
    pub fn start(
        app: tauri::AppHandle,
        events: Channel<VoiceEvent>,
        config: SessionConfig,
        paths: VoicePaths,
    ) -> Result<Arc<Self>, String> {
        let detector = SileroDetector::new(&paths.vad_file())
            .ok_or("Poiesis could not start listening. Try downloading its hearing again in Voice settings.")?;
        let floor = FloorConfig { cut_in: config.cut_in, ..FloorConfig::default() };
        let session = Arc::new(Self {
            app,
            events,
            config,
            paths,
            pipeline: StdMutex::new(TurnPipeline::new(detector, floor)),
            queue: StdMutex::new(SpeechQueue::new()),
            closed: AtomicBool::new(false),
            last_frame: AtomicU64::new(0),
            partial_running: AtomicBool::new(false),
            user_language: StdMutex::new(None),
            reply_language: StdMutex::new(None),
            hint_sent: AtomicBool::new(false),
            carry: StdMutex::new(Carry::default()),
        });
        // Load hearing and the voice now, so the first turn does not wait for them.
        let warm = session.clone();
        tauri::async_runtime::spawn(async move {
            let mgr = warm.app.state::<VoiceManager>();
            let _ = mgr.warm_hearing(&warm.paths, &warm.config.hearing_id, warm.config.threads).await;
            let _ = mgr.ensure_voice(&warm.paths, &warm.config.voice_id, warm.config.threads).await;
        });
        Ok(session)
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let mut q = self.queue.lock().unwrap();
        q.cancel(u64::MAX - 1);
    }

    fn emit(&self, event: VoiceEvent) {
        if !self.closed.load(Ordering::SeqCst) {
            let _ = self.events.send(event);
        }
    }

    fn language_of(&self, text: &str) -> Option<String> {
        if self.config.language != "auto" {
            return Some(self.config.language.clone());
        }
        guess_language(text).map(str::to_string)
    }

    /// 100 ms or so of 16 kHz mono samples. `counter` rises with every call;
    /// a repeated or older one is dropped.
    pub fn push_audio(self: &Arc<Self>, counter: u64, samples: &[f32]) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let before = self.last_frame.fetch_max(counter, Ordering::SeqCst);
        if counter != 0 && counter <= before {
            return;
        }
        self.app.state::<VoiceManager>().touch();
        let outs = self.pipeline.lock().unwrap().feed(samples);
        self.handle(outs);
    }

    fn handle(self: &Arc<Self>, outs: Vec<Out>) {
        for out in outs {
            match out {
                Out::Floor(name) => self.emit(VoiceEvent::Floor { state: name.into() }),
                Out::Duck => self.emit(VoiceEvent::Duck),
                Out::Unduck => self.emit(VoiceEvent::Unduck),
                Out::Yield => self.emit(VoiceEvent::Yield),
                Out::Discard => {
                    // The sound that took the floor back was nothing: send what was held.
                    let held = self.carry.lock().unwrap().take();
                    if let Some(text) = held {
                        self.send_transcript(text);
                    }
                }
                Out::EndOfTurn(audio) => {
                    let me = self.clone();
                    tauri::async_runtime::spawn(async move { me.hear_turn(audio).await });
                }
                Out::Partial { audio, turn } => {
                    if !self.partial_running.swap(true, Ordering::SeqCst) {
                        let me = self.clone();
                        tauri::async_runtime::spawn(async move { me.hear_partial(audio, turn).await });
                    }
                }
            }
        }
    }

    async fn hear_turn(self: Arc<Self>, audio: Vec<f32>) {
        let mgr = self.app.state::<VoiceManager>();
        let heard = mgr.hear(&self.paths, &self.config.hearing_id, self.config.threads, audio).await;
        match heard {
            Ok(text) if text.chars().any(char::is_alphanumeric) => {
                // The user may have started again while this was being heard
                // (a cut-in during the wait). Then it is the first half of a thought.
                if self.pipeline.lock().unwrap().user_has_floor() {
                    self.carry.lock().unwrap().hold(&text);
                    return;
                }
                let text = self.carry.lock().unwrap().with(&text);
                self.send_transcript(text);
            }
            other => {
                // Nothing to answer: give the floor back so the user can try again.
                if let Err(e) = other {
                    self.emit(VoiceEvent::Error { message: e.to_string() });
                }
                // Words held from the turn before are still a turn: send them now,
                // unless the user is talking again and they can wait for that.
                let held = if self.pipeline.lock().unwrap().user_has_floor() {
                    None
                } else {
                    self.carry.lock().unwrap().take()
                };
                if let Some(text) = held {
                    self.send_transcript(text);
                    return;
                }
                let outs = self.pipeline.lock().unwrap().assistant_finished();
                self.handle(outs);
            }
        }
    }

    fn send_transcript(&self, text: String) {
        let language = self.language_of(&text);
        *self.user_language.lock().unwrap() = language.clone();
        self.emit(VoiceEvent::Transcript { text, language });
    }

    async fn hear_partial(self: Arc<Self>, audio: Vec<f32>, turn: u64) {
        let mgr = self.app.state::<VoiceManager>();
        let heard = mgr.hear(&self.paths, &self.config.hearing_id, self.config.threads, audio).await;
        self.partial_running.store(false, Ordering::SeqCst);
        let Ok(text) = heard else { return };
        if text.is_empty() {
            return;
        }
        let language = self.language_of(&text).unwrap_or_else(|| "auto".into());
        {
            let mut pipeline = self.pipeline.lock().unwrap();
            if pipeline.turn() != turn {
                return;
            }
            pipeline.set_partial(turn, &text, &language);
        }
        self.emit(VoiceEvent::Partial { text });
    }

    /// `started`: Poiesis began to make sound. `finished`: it stopped, for any reason.
    pub fn assistant(self: &Arc<Self>, started: bool, generation: u64) {
        let outs = {
            let mut pipeline = self.pipeline.lock().unwrap();
            if started {
                pipeline.assistant_started(generation)
            } else {
                pipeline.assistant_finished()
            }
        };
        self.handle(outs);
    }

    /// Adds streamed reply text (or a notice) to what is spoken (VTN-3).
    pub fn speak(self: &Arc<Self>, generation: u64, text: &str, done: bool, notice: bool, language: Option<String>) {
        let start = {
            let mut q = self.queue.lock().unwrap();
            q.push(generation, text, done, notice, language);
            q.claim_worker()
        };
        if start {
            let me = self.clone();
            tauri::async_runtime::spawn(async move { me.run_worker().await });
        }
    }

    /// Stops speaking reply `generation` and anything older.
    pub fn cancel_speech(&self, generation: u64) {
        self.queue.lock().unwrap().cancel(generation);
    }

    async fn run_worker(self: Arc<Self>) {
        loop {
            let job = {
                let mut q = self.queue.lock().unwrap();
                match q.next_job() {
                    Some(job) => job,
                    None => {
                        let done = q.take_done();
                        q.release_worker();
                        drop(q);
                        if let Some((generation, _)) = done {
                            self.emit(VoiceEvent::SpeechDone { generation });
                        }
                        return;
                    }
                }
            };
            self.make(job).await;
            self.queue.lock().unwrap().job_finished();
        }
    }

    /// Language for this piece: what the caller said, else for a reply the
    /// first piece's guess, else the user's own language.
    fn language_for(&self, job: &Job) -> Option<String> {
        if let Some(l) = &job.language {
            return Some(l.clone());
        }
        let user = self.user_language.lock().unwrap().clone();
        if job.notice {
            return user;
        }
        let mut slot = self.reply_language.lock().unwrap();
        match slot.as_ref() {
            Some((g, l)) if *g == job.generation => l.clone(),
            _ => {
                let l = self.language_of(&job.text).or(user);
                *slot = Some((job.generation, l.clone()));
                l
            }
        }
    }

    async fn make(&self, job: Job) {
        let language = self.language_for(&job);
        let pick = pick_voice(&self.config.voice_id, language.as_deref(), |id| self.paths.is_installed(id));
        if let Some(hint) = pick.hint {
            if !self.hint_sent.swap(true, Ordering::SeqCst) {
                self.emit(VoiceEvent::Hint { text: hint });
            }
        }
        let mgr = self.app.state::<VoiceManager>();
        let made = mgr
            .speak(&self.paths, &pick.voice_id, self.config.threads, job.text.clone(), self.config.speed)
            .await;
        // The user may have cut in while this was being made.
        if !self.queue.lock().unwrap().is_current(job.generation) {
            return;
        }
        match made {
            Ok((samples, rate)) => {
                let pcm = base64::engine::general_purpose::STANDARD.encode(f32_to_pcm16(&samples));
                self.emit(VoiceEvent::Audio {
                    generation: job.generation,
                    seq: job.seq,
                    sample_rate: rate,
                    pcm,
                    text: job.text,
                    notice: job.notice,
                });
            }
            Err(e) => self.emit(VoiceEvent::Error { message: e.to_string() }),
        }
    }
}

/// The one open session, if any (TRN-5: starting one closes the last).
#[derive(Default)]
pub struct VoiceSessions {
    current: StdMutex<Option<Arc<VoiceSession>>>,
}

impl VoiceSessions {
    pub fn replace(&self, session: Option<Arc<VoiceSession>>) {
        let old = std::mem::replace(&mut *self.current.lock().unwrap(), session);
        if let Some(old) = old {
            old.close();
        }
    }

    pub fn get(&self) -> Option<Arc<VoiceSession>> {
        self.current.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Speech" is any loud sample.
    struct Loud;
    impl Detector for Loud {
        fn detect(&mut self, window: &[f32]) -> bool {
            window.iter().any(|s| s.abs() > 0.1)
        }
    }

    fn pipeline() -> TurnPipeline<Loud> {
        TurnPipeline::new(Loud, FloorConfig::default())
    }

    fn tone(ms: usize) -> Vec<f32> {
        vec![0.5; ms * 16]
    }

    fn quiet(ms: usize) -> Vec<f32> {
        vec![0.0; ms * 16]
    }

    /// Feeds in 20 ms frames, as the capture worklet does.
    fn feed(p: &mut TurnPipeline<Loud>, samples: &[f32]) -> Vec<Out> {
        samples.chunks(320).flat_map(|f| p.feed(f)).collect()
    }

    fn kinds(outs: &[Out]) -> Vec<&'static str> {
        outs.iter()
            .map(|o| match o {
                Out::Floor(n) => *n,
                Out::Duck => "duck",
                Out::Unduck => "unduck",
                Out::Yield => "yield",
                Out::EndOfTurn(_) => "end",
                Out::Discard => "discard",
                Out::Partial { .. } => "partial",
            })
            .collect()
    }

    #[test]
    fn a_spoken_turn_ends_with_all_of_its_audio() {
        let mut p = pipeline();
        let mut outs = feed(&mut p, &quiet(500));
        outs.extend(feed(&mut p, &tone(1000)));
        outs.extend(feed(&mut p, &quiet(1500)));
        let k = kinds(&outs);
        assert_eq!(k.iter().filter(|n| **n == "end").count(), 1);
        assert!(k.contains(&"user_speaking") && k.contains(&"thinking"));
        let Some(Out::EndOfTurn(audio)) = outs.iter().find(|o| matches!(o, Out::EndOfTurn(_))) else { unreachable!() };
        let tone_samples = 1000 * 16;
        assert!(audio.len() >= tone_samples, "audio has the whole turn: {}", audio.len());
        // And not minutes of old silence: preroll plus the turn plus the pause.
        assert!(audio.len() <= PREROLL + tone_samples + 700 * 16, "{}", audio.len());
        // The first word has its lead-in.
        assert!(audio[..PREROLL / 2].iter().all(|s| *s == 0.0) || audio.len() > tone_samples);
    }

    #[test]
    fn talking_again_while_the_last_turn_is_heard_takes_the_floor() {
        let mut p = pipeline();
        let mut outs = feed(&mut p, &quiet(500));
        outs.extend(feed(&mut p, &tone(1000)));
        outs.extend(feed(&mut p, &quiet(1500)));
        assert!(kinds(&outs).contains(&"end"));
        assert!(!p.user_has_floor(), "waiting for the answer");
        let again = feed(&mut p, &tone(600));
        assert!(kinds(&again).contains(&"yield"), "{:?}", kinds(&again));
        assert!(p.user_has_floor(), "the user went on talking");
    }

    #[test]
    fn held_words_come_before_the_next_turn() {
        let mut c = Carry::default();
        assert_eq!(c.take(), None);
        c.hold("Tell me about");
        assert_eq!(c.with("the weather in Berlin."), "Tell me about the weather in Berlin.");
        assert_eq!(c.take(), None, "used once");
        c.hold("Hello there.");
        c.hold(" And more.");
        assert_eq!(c.take().as_deref(), Some("Hello there. And more."));
        assert_eq!(c.with("  Plain.  "), "Plain.");
    }

    #[test]
    fn a_cough_is_forgotten() {
        let mut p = pipeline();
        let mut outs = feed(&mut p, &tone(160));
        outs.extend(feed(&mut p, &quiet(2000)));
        let k = kinds(&outs);
        assert!(!k.contains(&"end"), "{k:?}");
        assert_eq!(k, vec!["user_speaking", "discard", "listening"]);
    }

    #[test]
    fn a_pause_asks_for_a_look_at_the_words_every_so_often() {
        let mut p = pipeline();
        let mut outs = feed(&mut p, &tone(1000));
        outs.extend(feed(&mut p, &quiet(550)));
        let partials = kinds(&outs).iter().filter(|k| **k == "partial").count();
        assert!((1..=2).contains(&partials), "{partials}");
    }

    #[test]
    fn a_partial_for_another_turn_is_ignored() {
        let mut p = pipeline();
        feed(&mut p, &tone(1000));
        feed(&mut p, &quiet(100));
        // "und" belongs to some other turn, so the normal pause still applies.
        p.set_partial(p.turn() + 1, "ich moechte und", "de");
        let outs = feed(&mut p, &quiet(700));
        assert!(kinds(&outs).contains(&"end"), "{:?}", kinds(&outs));
    }

    #[test]
    fn und_at_the_end_holds_the_turn_open() {
        let mut p = pipeline();
        feed(&mut p, &tone(1000));
        let turn = p.turn();
        // Past the normal pause would end it; with "und" it must still be open.
        let before = feed(&mut p, &quiet(300));
        assert!(!kinds(&before).contains(&"end"));
        p.set_partial(turn, "Ich moechte ein Bild und", "de");
        let mid = feed(&mut p, &quiet(500));
        assert!(!kinds(&mid).contains(&"end"), "{:?}", kinds(&mid));
        let late = feed(&mut p, &quiet(600));
        assert!(kinds(&late).contains(&"end"));
    }

    #[test]
    fn cutting_in_ducks_then_yields_and_the_next_turn_has_the_words() {
        let mut p = pipeline();
        feed(&mut p, &tone(1000));
        feed(&mut p, &quiet(800));
        p.assistant_started(5);
        let mut outs = feed(&mut p, &tone(600));
        outs.extend(feed(&mut p, &quiet(1500)));
        let k = kinds(&outs);
        let at = |name: &str| k.iter().position(|n| *n == name);
        assert!(at("duck").unwrap() < at("yield").unwrap(), "{k:?}");
        assert!(k.contains(&"end"), "the cut-in becomes a turn: {k:?}");
        let Some(Out::EndOfTurn(audio)) = outs.iter().find(|o| matches!(o, Out::EndOfTurn(_))) else { unreachable!() };
        assert!(audio.len() >= 600 * 16, "the whole cut-in is kept: {}", audio.len());
    }

    #[test]
    fn the_end_of_the_reply_gives_the_floor_back() {
        let mut p = pipeline();
        feed(&mut p, &tone(1000));
        feed(&mut p, &quiet(800));
        assert_eq!(p.assistant_started(1), vec![Out::Floor("speaking")]);
        assert_eq!(p.assistant_finished(), vec![Out::Floor("listening")]);
    }

    #[test]
    fn frame_size_does_not_change_the_result() {
        let audio: Vec<f32> = [quiet(300), tone(900), quiet(1000)].concat();
        let mut a = pipeline();
        let by_frames = feed(&mut a, &audio);
        let mut b = pipeline();
        let at_once = b.feed(&audio);
        assert_eq!(kinds(&by_frames), kinds(&at_once));
    }

    #[test]
    fn a_german_reply_gets_the_german_voice_when_it_is_installed() {
        let installed = |id: &str| id == "piper-de-thorsten" || id == "kokoro-en";
        let pick = pick_voice("af_heart", Some("de"), installed);
        assert_eq!(pick, VoicePick { voice_id: "piper-de-thorsten".into(), hint: None });
        // Same language keeps the choice; unknown language keeps it too.
        assert_eq!(pick_voice("af_heart", Some("en"), installed).voice_id, "af_heart");
        assert_eq!(pick_voice("af_heart", None, installed).voice_id, "af_heart");
        assert_eq!(pick_voice("piper-de-thorsten", Some("en"), installed).voice_id, "af_heart");
    }

    #[test]
    fn without_a_german_voice_it_says_so_once_in_plain_words() {
        let pick = pick_voice("af_heart", Some("de"), |id| id == "kokoro-en");
        assert_eq!(pick.voice_id, "af_heart");
        let hint = pick.hint.expect("a hint");
        assert!(hint.contains("Voice settings"));
        let lower = hint.to_lowercase();
        for banned in crate::runtime::voice_catalog::BANNED_WORDS {
            assert!(!lower.contains(banned), "{hint}");
        }
        assert!(pick_voice("af_heart", Some("ja"), |_| false).hint.is_none());
    }

    /// VOC-T5 with the real speech detector and hearing: a recorded sentence
    /// padded with silence is one turn, and that turn reads back as speech.
    /// Needs `POIESIS_VOICE_MODELS` (see the spike test in `voice.rs`).
    #[test]
    #[ignore]
    fn real_audio_makes_one_turn_that_is_heard() {
        use super::super::voice::parakeet_config;
        use sherpa_onnx::{LinearResampler, OfflineRecognizer, Wave};
        let root = std::path::PathBuf::from(std::env::var("POIESIS_VOICE_MODELS").expect("set POIESIS_VOICE_MODELS"));
        let asr = OfflineRecognizer::create(&parakeet_config(&root.join("parakeet"), 4)).expect("hearing loads");
        for (name, word) in [("en_short.wav", "country"), ("de_short.wav", "Wurst")] {
            let wave = Wave::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/voice").join(name).to_str().unwrap(),
            )
            .expect("wav reads");
            let speech = LinearResampler::create(wave.sample_rate(), SAMPLE_RATE).unwrap().resample(wave.samples(), true);
            let audio: Vec<f32> = [quiet(500), speech, quiet(1500)].concat();

            let detector = SileroDetector::new(&root.join("silero_vad.onnx")).expect("detector loads");
            let mut p = TurnPipeline::new(detector, FloorConfig::default());
            let outs: Vec<Out> = audio.chunks(320).flat_map(|f| p.feed(f)).collect();
            println!("{name}: {:?}", outs.iter().map(|o| match o { Out::EndOfTurn(a) => format!("end({} samples)", a.len()), other => format!("{other:?}").chars().take(24).collect() }).collect::<Vec<_>>());
            let turns: Vec<&Vec<f32>> = outs.iter().filter_map(|o| if let Out::EndOfTurn(a) = o { Some(a) } else { None }).collect();
            assert_eq!(turns.len(), 1, "{name} should be one turn");

            let stream = asr.create_stream();
            stream.accept_waveform(SAMPLE_RATE, turns[0]);
            asr.decode(&stream);
            let text = stream.get_result().unwrap().text;
            println!("{name}: heard {text:?}");
            assert!(text.contains(word) && text.chars().any(char::is_alphanumeric));
        }
    }
}
