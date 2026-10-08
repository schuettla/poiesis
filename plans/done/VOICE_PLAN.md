# Project Poiesis - Voice Plan

**You can talk to Poiesis, and it talks back. Everything runs on your machine.**

Every popular agent now has a voice mode: you speak, it answers out loud, you
cut in when you want, and it stops. Poiesis has none. Its parts are already
here: a supervised local runtime, an agent loop that streams tokens, a run
that can be cancelled, and an orb that already has a `listening` state. The
missing parts are a local speech engine, audio in and out, and turn-taking.

This plan adds one in-process speech engine (sherpa-onnx), audio capture and
playback in the WebView, a turn-taking state machine, and a voice mode surface
where the orb is the face of the conversation. No cloud model is needed at
any step.

> ID prefixes: **VOC** speech engine and models - **AUD** audio in and out -
> **TRN** turn-taking - **VTN** a voice turn through the agent - **VXP**
> experience rules - **-UI** frontend - **-T** tests.
>
> **Status: built, first hand check done on a laptop (2026-10-08), waiting for the full pass.** Built: all of
> Phase 0 except `VOC-0.4` (needs the real app); Phase 1 with its settings
> tab; Phase 2; Phase 3 (`TRN-1` to `TRN-5`); Phase 4 (`VTN-1` to `VTN-7`);
> Phase 5, all of `VOC-UI-1` to `VOC-UI-10`; `VOC-11` credits; the light
> English hearing (Moonshine). A full voice turn has been tested in pieces and
> with real audio, but not yet by a person in the running app (`VOC-T7`).
> Not built on purpose: `VOC-9` and `VOC-10` (see their rows).
> See "Build notes" for what changed from this plan and why.
> Written 2026-10-05.

---

## Status by item (2026-10-07)

Legend: **done** = built and tested. **partial** = built, part missing (noted).
**manual** = built, waits for a hand check in the real app. **open** = not started.

| ID | State | Note |
|---|---|---|
| `VOC-0.1` | done | Floor 1.90, toolchain 1.96.0. `tauri build --bundles nsis` works: installer 15.9 MB, program 43.4 MB, no extra DLL. |
| `VOC-0.2` | done | Static link works, also release with LTO. |
| `VOC-0.3` | done | Spike test and an end-to-end speak and hear test. Pocket dropped. |
| `VOC-0.4` | done | Checked by Erich in the real app (2026-10-08): WebView2 showed its own microphone box. Now answered by `mic_permission.rs` for the app's own pages, so the box is gone (needs one run to confirm). |
| `VOC-0.5` | done | Numbers in Build notes. |
| `VOC-1` | done | Hearing and voice engines, idle unload, one decode at a time, a live session keeps them loaded. |
| `VOC-2` | done | Catalog done: two hearings (standard, light English) and 41 voices in 18 languages, each with its license. Pocket is not in it (see Build notes). |
| `VOC-3` | done | Defaults and thread count. Under 8 GB of memory with an English interface: light hearing and the light English voice. Anything else: standard hearing. |
| `VOC-4` | done | Download, delete, status. Voice mode offers the download itself. |
| `VOC-5` | done | Keys and defaults read in Rust; the Voice tab changes hearing, voice, speed, language, cut-in and the shortcut. |
| `VOC-6` | manual | Preview command returns a WAV; "Hear it" in the Voice tab plays it. |
| `VOC-9` | not built | Smart Turn needs a second ONNX runtime next to sherpa's, which is linked in statically (`onnxruntime.lib`). The plan says to build it only if the pause feels wrong, and nobody has felt it yet. See Build notes. |
| `VOC-10` | not built | A measurement against a 6 GB chat model with audio input; it cannot give a result without that model and a person judging it. |
| `VOC-11` | done | `THIRD_PARTY_NOTICES.md` and About rows. |
| `AUD-1` | done | Capture worklet, 16 kHz, 20 ms frames. |
| `AUD-2` | done | `voice_push_audio_cmd`, raw body, 8-byte counter, 5 frames per call. |
| `AUD-3` | done | Playback queue, fade cancel, played and level events. |
| `AUD-4` | done | `VoiceAudio`, plus `setMuted`. |
| `TRN-1` | done | `FloorMachine` in `voice_floor.rs`. Takes a yes or no, not a probability (Build notes). |
| `TRN-2` | done | Duck, unduck, yield. `voice.cut_in` off turns it off. |
| `TRN-3` | done | End of turn after the pause; a cough is dropped; 30 s cap. |
| `TRN-4` | done | A look at the words every 400 ms during a pause; German and English word lists. |
| `TRN-5` | done | `VoiceSession`, six commands, `VoiceEvent`. |
| `VTN-1` | done | `sendMessage(..., { spoken: true })`. The prompt is built on the frontend, so `agent_chat_cmd` has no new parameter (Build notes). |
| `VTN-2` | done | `SPOKEN_GUIDANCE` in `context.rs` and `store.ts`, held together by `fixtures/voice/spoken-prompt.golden.txt`. |
| `VTN-3` | done | `SentenceChunker` and `SpeechQueue`, one piece at a time. No partial audio inside one piece (Build notes). |
| `VTN-4` | done | `pick_voice`: the installed voice for the reply's language, else one plain hint. |
| `VTN-5` | done | One short notice while a tool runs, English and German, not saved. |
| `VTN-6` | done | A reply that was cut off keeps only what finished playing. The whole reply is kept as `unspoken` in `steps_json`. |
| `VTN-7` | done | Schema v30, `messages.spoken`, `interrupted` stop reason. |
| `VOC-UI-1` | manual | Mic button: tap to dictate, hold to talk and send. |
| `VOC-UI-2` | manual | `/talk` (alias `/voice`) in the `/` menu, not a button (changed 2026-10-07, see Build notes). |
| `VOC-UI-3` | manual | `VoiceMode.tsx`. Orb is drawn at 64 and enlarged to 128 (Build notes). |
| `VOC-UI-4` | done | `orbForFloor`. |
| `VOC-UI-5` | manual | Wave glyph on spoken turns; "Stopped here" and "Show the full reply". |
| `VOC-UI-6` | manual | Runtime, tab "Voice" (second tab, after Images): hearing cards, one "Voices" box with a language filter (pick, hear, download, remove per voice), speed, language, shortcut, cut-in, microphone test. `openRuntime("voice")`. |
| `VOC-UI-7` | manual | "Want to talk to me?" in the onboarding guide: one button, optional. The guide only shows when no model is set up. |
| `VOC-UI-8` | manual | "Talk to Poiesis", "Stop talking", "Voice settings" in the palette. |
| `VOC-UI-9` | manual | Hotkey (works while Poiesis has focus), changed in the Voice tab and live at once. Escape stops speech first, a second Escape leaves. |
| `VOC-UI-10` | manual | Red mic in the top bar while the mic is open. |
| `VOC-UI-11` | manual | Problems are shown, not swallowed (2026-10-08). A failed reply, a voice that cannot speak, a mic that stops reaching the app, a send dropped because the last answer was still running, no model: each puts one dismissable line on the voice surface (`voiceStore.problem`). Startup failures keep the blocking note with "Try again". |
| `VOC-UI-12` | manual | "What I did" list under the orb (2026-10-08): the tools and agents of the current answer, running, done or failed, from `activity.ts`. |
| `VOC-UI-13` | manual | Way back to a live conversation (2026-10-08): a wave button left of the mic in the composer, only while a voice conversation is live and its surface is hidden ("Show chat"). Not there otherwise; starting voice stays `/talk`, the shortcut and the palette. The shortcut now switches between voice and chat instead of ending the conversation. |
| `VOC-T1` | done | `voice_floor.rs` tests. |
| `VOC-T2` | done | `voice_speech.rs` tests. |
| `VOC-T3` | done | Catalog and copy test, now with both hearings. |
| `VOC-T4` | done | Spoken sentence gate, Rust and vitest, on its own small golden. |
| `VOC-T5` | done | Ignored test `real_audio_makes_one_turn_that_is_heard`: real Silero and Parakeet on both fixtures. |
| `VOC-T6` | done | `orbForFloor`, `SpokenTurns`, `VoiceSession` with fakes, notices, hotkey, mic button. |
| `VOC-T7` | open | Manual pass. |

### Where things stand (2026-10-07)

| Area | State |
|---|---|
| Code | All phases built. Nothing is committed yet. |
| Automatic tests | Rust 715 passed (7 ignored: they need model files or a network). Vitest 659 of 660 passed; the one failure is an old inline icon in `WindowControls.tsx`, not from voice. `tsc` is clean. |
| Real audio | Silero, Parakeet and Moonshine tested on the fixture recordings. Kokoro and Piper tested for speed and sound. Every Piper voice in the list speaks and Parakeet hears its sentence back (ignored test `every_listed_voice_speaks`). |
| Installer | `tauri build --bundles nsis` works (15.9 MB). |
| Not yet done by a person | `VOC-0.4` (mic permission in WebView2) and `VOC-T7` (the 14-step pass in Build notes, on speakers). |
| Not built on purpose | `VOC-9` Smart Turn and `VOC-10` audio-model comparison. |
| Known limits | The hotkey works only while Poiesis has focus. Voice mode opens only in the chat view. The orb is drawn at 64 px and enlarged to 128 px. Leaving voice mode mid-reply saves only what was heard. |

Next: the manual pass (`VOC-T7`). Then `VOC-9` (Smart Turn) only if the pause timing feels wrong there.

---

## What this is not

- **Not a cloud feature.** No step sends audio or transcripts off the machine.
  A cloud chat model can still answer a voice turn if the user picked one, the
  same as a typed turn, but speech in and speech out are always local.
- **Not a second agent.** A voice turn is an ordinary turn through
  `agent_chat_cmd`. Tools, memory, personas, plans and permissions behave as
  they do for typed turns. Voice only changes how the text gets in and out.
- **Not a sidecar server.** The engine is a Rust library in the main process.
  We do not adopt Openlive's gateway-plus-browser design (see "Prior art").
- **Not voice cloning.** Pocket TTS can clone a voice from a short sample. We
  do not expose that. Voices are the curated ones in the catalog.
- **Not always listening.** The mic is open only while voice mode or dictation
  is on, and the UI shows that at all times (`VXP-3`).
- **Not a media artifact.** `media::Modality` stays image and video. Speech is
  a live conversation, not generated media in the Library.

---

## Where Poiesis stands

### Facts to build against

| Fact | Where |
|---|---|
| Chat, embed and rerank engines are supervised `llama-server` processes. Embed and rerank are CPU-only, with a curated catalog and a 5 minute idle stop. | `runtime/embedserver.rs` (`EmbedManager`, `embed_catalog`, `IDLE_STOP`, `spawn_idle_stop`), `runtime/rerankserver.rs` |
| Managers are registered with `app.manage(...)` in setup. | `src-tauri/src/lib.rs:111-120` |
| Model downloads report progress over `Channel<DownloadProgress>`. | `commands/embedgen.rs:87` |
| A turn streams `AgentEvent::Token { text }`, `StepStart`, `StepDone`, `Thinking` over a `Channel<AgentEvent>`. | `agent/mod.rs:56-85`, `commands/agent.rs:111` |
| The lead run is stopped with `stop_chat_cmd`; sub-runs with `stop_run_cmd` via `Fleet::cancel_tree`. | `commands/runtime.rs:394`, `commands/subagents.rs:37`, `agent/fleet.rs:299` |
| Frontend send and stop: `sendMessage(text, attachments)` and `stopGenerating()`. | `src/lib/store.ts:2763`, `:3192` |
| The system prompt is built by `compose_system_prompt(&PromptInputs)` in Rust, gated byte-for-byte against `fixtures/prompt-assembly.golden.txt`. `agent_chat_cmd` still assembles on the frontend, so a new prompt sentence must be mirrored in `store.ts`. | `agent/context.rs:174,226`, `agent/context_golden.rs` |
| The orb maps facts to animation states. `thinking-orbs` ships `listening`, `composing`, `breathing`, and others. | `src/components/Orb/orbState.ts`, `node_modules/thinking-orbs/dist/types.d.ts:18` |
| Runtime has tabs `chat | images | servers | recall`. | `src/lib/types.ts:431` |
| Composer: the send button is at `Composer.tsx:987`, the attach button at `:581`, the footer at `:996`. | `src/components/Composer/Composer.tsx` |
| `messages` has `content`, `model_name`, `model_provenance`, `steps_json`, `stop_reason`. Schema is v29. | `db/schema.sql:22`, `db/mod.rs:22` |
| CSP already allows `media-src blob:`. No `connect-src` change is needed, because audio goes over IPC. | `tauri.conf.json` |
| User-facing copy avoids engineering words (`SMP-8a`). | `plans/done/PERCEPTION_PLAN.md:1217` |
| `Cargo.toml` declares `rust-version = "1.77"`. It came from the project template; the installed compiler is 1.96. It is a minimum, not a reason, but deps have been pinned around it. | `src-tauri/Cargo.toml` |

### What is missing

- No speech-to-text or text-to-speech engine, local or cloud.
- No mic capture or audio playback path.
- No notion of a turn that starts from speech, or of speaking a reply.
- No way to save "what was actually heard" when a reply is cut off.

---

## Decisions (settled)

> Phase 0 changed some rows below (Parakeet size, Kokoro precision, Pocket,
> Moonshine, German voice). "Build notes, Changes from the plan" wins where
> they disagree.

| Layer | Choice | Why |
|---|---|---|
| Engine | `sherpa-onnx` crate 1.13.8, Apache-2.0, edition 2021, static link by default | One library for voice detection, speech-to-text and text-to-speech. Windows supported. CPU by default, so it never competes with the chat engine for VRAM (same reason as the embed engine). |
| Mic capture | WebView `getUserMedia` + AudioWorklet, 20 ms frames, resampled to 16 kHz mono, sent to Rust as raw bytes | WebView2 gives echo cancellation and gain control for free. Avoids a native audio crate. |
| Playback | WebView `AudioContext` + playback AudioWorklet | Echo cancellation only removes sound played from the same page. This is what makes cutting in work without headphones. |
| Voice detection | Silero VAD in sherpa (`VoiceActivityDetector`) | Small, fast, the standard. |
| Speech-to-text, default | Parakeet TDT 0.6B v3, int8, about 640 MB, CC-BY-4.0, via `OfflineRecognizer` (transducer) on each detected speech segment | Beats Whisper large-v3 at a quarter of the size, fast on CPU, 25 European languages including German, gives punctuation and capitals. |
| Speech-to-text, small | Moonshine v2 | For machines under 8 GB RAM. |
| Voice, English | Kokoro (Apache-2.0) | Best quality per size, many voices. No German. |
| Voice, German and other languages | Pocket TTS (Kyutai, MIT, about 100M, CPU-first) | Has German, Spanish, French, Italian, Portuguese bundles. |
| Voice, fallback | Piper VITS, Thorsten voice for German | Very small, works on any machine. |
| End of turn | Silence + falling energy + a trailing-word check (`TRN-4`). Smart Turn v3 later (`VOC-9`). | Ships without a second ONNX runtime. |
| Brain | Existing `agent_chat_cmd`, plus one prompt sentence when the turn is spoken | No new agent code path. |

### Rejected

- **Supertonic 3.** 31 languages and very fast, but the company dissolved in
  July 2026, the repo is being archived, and the weights are OpenRAIL-M.
- **whisper.cpp sidecar.** Matches the downloader pattern, but upstream ships
  no Windows Vulkan build and it has no text-to-speech. It is the fallback if
  `VOC-0.2` fails.
- **Qwen3-TTS, CosyVoice 3, Chatterbox.** Better voices, but GPU-sized. They
  would fight the chat model for VRAM. A possible later "best voice" option.
- **`cpal` for native capture.** Would lose WebView2 echo cancellation.
- **Speech-to-speech models as the base** (Gemma 4 E2B/E4B, Qwen3-Omni,
  Voxtral through llama-server's audio input). Upstream calls audio input
  "highly experimental". Kept as an experiment (`VOC-10`).

---

## Prior art: Openlive

`github.com/byte271/Openlive` (Apache-2.0, checked at `4ea922d`, 2026-08-28)
is an open "live voice" clone with a Tauri shell. Its design does not fit
Poiesis: the core is a separate HTTP/WebSocket gateway and a 5,000 line
browser client, the Tauri app only spawns that gateway, it has its own agent,
tools and memory, and it has no local speech models of its own (it calls an
external OpenAI-compatible server or a Piper install). Its "Silero VAD" and
"RNNoise" are hand-written JavaScript approximations, not the real models.

We take four ideas and one file. Ported code keeps an attribution line
(`VOC-11`).

| Take | Openlive source | Used in |
|---|---|---|
| Playback worklet with a generation id per reply, cancel with a short fade, and `played` reports per frame | `apps/openlive-gateway/web/audio-playback-worklet.js` | `AUD-3` (ported to TS, minus jitter and packet-loss code) |
| Floor-control states and default thresholds | `crates/openlive-runtime/src/lib.rs` (`FloorState`, `ChronosConfig`) | `TRN-1` |
| Gradual cut-in: duck to about 18%, then yield, then cancel | `web/audio-session.js` `applyLocalInterruption` | `TRN-2` |
| Trailing-word end-of-sentence check | `apps/openlive-gateway/src/session.rs` `check_semantic_completion` | `TRN-4` (plus a German list) |
| Short spoken notice while a tool runs | `web/speech-utils.js` `pickToolAck` | `VTN-5` (built from step verbs instead of regexes) |

Not taken: the gateway, WebRTC and WebSocket transport, jitter buffer, the
JavaScript VAD and noise filter, the custom NLMS echo canceller, the Bloub
face, and all agent, tool and memory code.

---

## Architecture

```
 WebView                                     Rust (main process)
 â”€â”€â”€â”€â”€â”€â”€                                     â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
 mic â”€ getUserMedia(echoCancellation)
     â”” capture worklet (20 ms, 16 kHz i16) â”€â”€â–º voice_push_audio_cmd â”€â”€â–º VoiceSession
                                                                         â”‚ Silero VAD
                                                                         â”‚ FloorMachine (TRN)
                                                                         â”‚ on end of turn:
                                                                         â”‚   Parakeet â†’ transcript
     â—„â”€â”€ Channel<VoiceEvent> â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
 store: VoiceEvent::Transcript â”€â–º sendMessage(text, { spoken: true })
                                    â””â–º agent_chat_cmd (unchanged loop)
 AgentEvent::Token â”€â–º voice_speak_cmd(text chunks, generation_id)
                                    â””â–º SpeechQueue: sentence chunker â”€â–º OfflineTts
     â—„â”€â”€ Channel<VoiceEvent::Audio {generation_id, seq, pcm}> â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
 playback worklet â”€â–º speakers
     â”” played {generation_id, seq} â”€â–º voice_played_cmd (what was heard)
```

The frontend owns playback and the agent turn, because both already live
there. Rust owns everything that needs a model or a clock: detecting speech,
deciding when a turn ends, transcribing, and making audio.

### One turn

1. Frames go to Rust every 20 ms. Silero reports speech start and end.
2. `FloorMachine` decides the turn is over (`TRN-3`, `TRN-4`). Rust runs
   Parakeet on the buffered segment and emits `VoiceEvent::Transcript`.
3. The store sends it as a normal user message, marked spoken.
4. `Token` events go to `voice_speak_cmd`. The speech queue cuts them into
   sentences, strips markdown, and synthesizes one sentence at a time. Audio
   chunks come back as `VoiceEvent::Audio`.
5. The playback worklet plays them and reports `played` per chunk.
6. If the user speaks during playback, `TRN-2` ducks, then yields and stops
   the run. The reply is saved up to the last played sentence (`VTN-6`).

---

## Experience rules (binding)

These are the deliverable, not decoration. A phase is not done until its
`VXP` rules hold in the real app.

- `VXP-1` **The orb is the face.** In voice mode the orb is large and centered,
  and its state always says who has the floor: `listening` while the user
  speaks, `connecting` between end of speech and first audio, step-mapped
  states (`orbForStep`) while tools run, `composing` while Poiesis speaks,
  `breathing` when the floor is open. The orb's size follows the live audio
  level (user's voice while listening, Poiesis's voice while speaking).
- `VXP-2` **Cutting in always works.** Speaking over Poiesis lowers its voice
  within 100 ms and stops it within about 300 ms if the user keeps talking.
  Esc and the stop button do the same at once. Nothing the user does makes it
  talk over them.
- `VXP-3` **The mic is never secretly open.** While capture is on, the
  composer and the voice surface show a live mic indicator. Leaving voice
  mode, switching conversation, or minimizing the window closes the mic.
- `VXP-4` **Nothing leaves the machine.** The voice surface shows a small
  "On this device" note. Copy never names models or engines (`VXP-6`).
- `VXP-5` **The transcript is honest.** The saved user message is exactly what
  was transcribed. The saved reply is exactly what was spoken, plus a quiet
  "stopped here" mark if it was cut off.
- `VXP-6` **Plain copy.** In addition to `SMP-8a`, user-facing text never says
  *VAD, ASR, STT, TTS, transcription model, ONNX, endpointing, barge-in*.
  Use: **voice** (the voice Poiesis speaks with), **listening**, **hearing**
  (speech recognition), **speak**, **cut in**.
- `VXP-7` **Silence is not an error.** If no voice model is installed, the mic
  button explains what to download in one sentence with one button. If the
  user says nothing for 30 s in voice mode, the orb breathes; no nagging.
- `VXP-8` **It sounds like speech, not a read-out document.** Spoken replies
  are short, have no markdown, no lists read as "bullet one", no code read
  aloud. Code and tables go to the screen with a short spoken pointer
  ("I put the script on screen").

---

## Phase 0: toolchain and spike

Goal: prove the stack builds and runs on this machine before any feature work.

- `VOC-0.1` **Raise the Rust floor.** Set `rust-version = "1.90"` in
  `src-tauri/Cargo.toml` and add `src-tauri/rust-toolchain.toml` pinning the
  toolchain in use (`channel = "1.96.0"`). Update the comment above the
  tree-sitter pins to say they are held for API stability, not the compiler.
  Unpinning tree-sitter is a separate change, not part of this plan.
  Accept: `cargo test` passes, `npm run tauri build` succeeds.
- `VOC-0.2` **sherpa-onnx builds.** Add `sherpa-onnx = "1.13.8"` behind a
  `voice` cargo feature (on by default). Build a release binary. Watch for:
  the build script downloads a prebuilt archive from GitHub (offline builds
  need `SHERPA_ONNX_LIB_DIR`); the static Windows build uses the static C
  runtime (`/MT`), which can clash with other C deps. If linking fails, use
  the `shared` feature and ship the DLL next to the exe via `bundle.resources`.
  Record which mode worked in "Build notes".
- `VOC-0.3` **Models load.** A `#[ignore]` test in `runtime/voice.rs` loads
  Silero, Parakeet v3 int8, Kokoro and Pocket (German) from a local folder,
  transcribes `fixtures/voice/de_short.wav` and `en_short.wav`, and
  synthesizes one sentence each. Pin the exact download URLs and sha256 for
  `VOC-2` from what loads here (sherpa-onnx publishes converted models as
  GitHub release assets; confirm the asset names, do not guess them).
- `VOC-0.4` **Mic in WebView2.** In a dev build, call `getUserMedia({ audio:
  { echoCancellation: true, noiseSuppression: false, autoGainControl: true }})`.
  Confirm whether WebView2 shows its own permission prompt or needs a
  `PermissionRequested` handler. If a handler is needed, allow microphone for
  the app's own origin only.
- `VOC-0.5` **Latency baseline.** Log, on this machine: Parakeet time for a
  5 s segment, Kokoro and Pocket time to first audio for a 15-word sentence,
  each on CPU with 4 threads. Write the numbers into "Build notes". These set
  the defaults in `VOC-3`.

Exit: all five recorded. If `VOC-0.2` fails both ways, stop and switch the
engine to a whisper.cpp + Piper sidecar pair before going on.

---

## Phase 1: speech engine (VOC)

New module `src-tauri/src/runtime/voice.rs`, registered in `runtime/mod.rs`
and managed in `lib.rs` next to `EmbedManager`.

- `VOC-1` **`VoiceManager`.** Holds lazily created engines:

  ```rust
  pub struct VoiceManager {
      vad: Mutex<Option<VoiceActivityDetector>>,
      hearing: Mutex<Option<OfflineRecognizer>>,   // Parakeet or Moonshine
      voice: Mutex<Option<(String, OfflineTts)>>,   // (voice id, engine)
      last_used: StdMutex<Instant>,
      session: Mutex<Option<VoiceSession>>,         // one live session at a time
  }
  impl VoiceManager {
      pub fn new() -> Self;
      pub async fn ensure_hearing(&self, app: &AppHandle) -> Result<(), VoiceError>;
      pub async fn ensure_voice(&self, app: &AppHandle, voice_id: &str) -> Result<(), VoiceError>;
      pub async fn unload(&self);
  }
  ```

  Engines run on `tokio::task::spawn_blocking`. Idle unload after 5 minutes
  with no session, same pattern as `embedserver::spawn_idle_stop`.
  `provider` is `"cpu"`, `num_threads` from `VOC-3`.
- `VOC-2` **Catalog.** `pub fn voice_catalog() -> VoiceCatalog` with two lists,
  shaped like `EmbedCatalogEntry` (name, note, size_label, url(s), sha256,
  languages). Hearing: Parakeet v3 (default), Moonshine v2 (small). Voices:
  Kokoro (English voices listed by name), Pocket per language bundle, Piper
  Thorsten. Notes follow `VXP-6`, for example "Understands German, English and
  23 more European languages. About 640 MB."
  Files go under `<app data>/voice/<model id>/`.
- `VOC-3` **Defaults from hardware.** RAM under 8 GB: Moonshine + Piper.
  Otherwise Parakeet + Kokoro for English UI locale, Parakeet + Pocket for
  German. `num_threads = min(4, physical cores / 2)`. Uses `HardwareProfile`.
- `VOC-4` **Download commands.** Reuse the embed download path:
  `voice_download_cmd(kind: "hearing" | "voice", id: String, on_progress:
  Channel<DownloadProgress>)`, `voice_delete_cmd`, `voice_status_cmd ->
  VoiceStatus { hearing: Option<String>, voices: Vec<String>, loaded: bool }`.
  Verify sha256 like `runtime/download.rs`.
- `VOC-5` **Settings keys.** `voice.hearing_model`, `voice.voice_id`,
  `voice.speed` (0.8 to 1.3, default 1.0), `voice.language` (`auto` or a code;
  passed to Parakeet only as a hint), `voice.cut_in` (`on` default, `off`
  means push-to-talk only), `voice.hotkey` (default `Ctrl+Shift+Space`).
- `VOC-6` **Preview.** `voice_preview_cmd(voice_id, text) -> Vec<u8>` returns
  16-bit PCM for the Runtime tab's play button.

Later:

- `VOC-9` **Smart Turn v3** end-of-turn model (ONNX, 23 languages incl.
  German). Not in sherpa, so it needs the `ort` crate; check its Rust floor
  against `VOC-0.1` first. Replaces the silence timer in `TRN-3` with a
  probability on the last 8 s of audio.
- `VOC-10` **Speech-in experiment.** Feed the segment straight to a
  llama-server model with audio input (Gemma 4 E4B) and compare quality and
  latency against Parakeet + chat. Behind a dev flag only.
- `VOC-11` **Attribution.** Add rows to the credits list in
  `src/routes/About.tsx` (the array at the top, `{ name, license, what }`),
  and a `THIRD_PARTY_NOTICES.md` at the repo root for the Apache-2.0 NOTICE
  text: sherpa-onnx (Apache-2.0), Openlive
  (Apache-2.0, for `AUD-3`, `TRN-1`, `TRN-2`, `TRN-4`), Parakeet (CC-BY-4.0,
  attribution required), Kokoro (Apache-2.0), Pocket TTS (MIT), Piper voice
  licenses per voice.

---

## Phase 2: audio in and out (AUD)

New folder `src/lib/voice/`.

- `AUD-1` **Capture worklet** `src/lib/voice/capture.worklet.ts`. Collects
  20 ms frames at the context rate, resamples to 16 kHz (windowed-sinc or
  linear; linear is fine for speech at this rate), converts to `Int16Array`,
  posts to the main thread with a frame counter. Loaded with Vite's
  `?worker&url` import so the CSP `script-src 'self'` holds.
- `AUD-2` **Frames to Rust.** `voice_push_audio_cmd` takes the raw body
  (`tauri::ipc::Request`, `InvokeBody::Raw`) so frames are not JSON-encoded.
  Batch 5 frames (100 ms) per call. Header: 8 bytes little-endian frame
  counter, then PCM.
- `AUD-3` **Playback worklet** `src/lib/voice/playback.worklet.ts`, ported
  from Openlive's playback worklet. Keeps:
  - a queue of chunks tagged `{ generationId, seq }`;
  - `cancel(generationId)` with a 30 ms fade-out;
  - a `played { generationId, seq }` message when a chunk finishes;
  - an `idle` message when the queue empties;
  - an output RMS message every 50 ms (for `VXP-1`).
  Drops: jitter controller, packet-loss concealment, echo reference frames.
- `AUD-4` **`VoiceAudio` class** `src/lib/voice/audio.ts`. Owns the
  `AudioContext`, the mic stream and both worklets. `start()`, `stop()`,
  `enqueue(generationId, seq, pcm, sampleRate)`, `cancel(generationId)`,
  `setDuck(gain, seconds)`, events `level(input, output)`, `played`, `idle`.
  `stop()` stops all mic tracks so the OS mic light goes off (`VXP-3`).

---

## Phase 3: turn-taking (TRN)

New file `src-tauri/src/runtime/voice_floor.rs`. Pure logic, no models, so
it is fully unit-testable with recorded probabilities.

- `TRN-1` **`FloorMachine`.** States from Openlive's `FloorState`:

  ```rust
  pub enum Floor {
      Listening,
      UserSpeaking { since_ms: u64 },
      UserPause { speech_since_ms: u64, silence_since_ms: u64 },
      Thinking,                                   // transcript sent, no audio yet
      Speaking { generation: u64 },
      Ducked { generation: u64, overlap_since_ms: u64 },
  }
  pub struct FloorConfig {
      pub speech_on: f32,          // 0.62
      pub speech_off: f32,         // 0.34
      pub cut_in_commit_ms: u32,   // 180
      pub min_user_turn_ms: u32,   // 280
      pub end_silence_ms: u32,     // 600, lowered to 300 when TRN-4 says "complete"
      pub max_turn_ms: u32,        // 30_000, then force end of turn
  }
  pub enum FloorAction { None, EndOfTurn, Duck, Unduck, Yield }
  impl FloorMachine {
      pub fn observe(&mut self, now_ms: u64, speech_prob: f32, rms: f32) -> FloorAction;
      pub fn assistant_started(&mut self, generation: u64);
      pub fn assistant_finished(&mut self, generation: u64);
      pub fn set_partial_text(&mut self, text: &str);  // feeds TRN-4
  }
  ```

  Speech probability comes from Silero per 32 ms window.
- `TRN-2` **Cut-in ladder.** While `Speaking`: probability above `speech_on`
  gives `Duck` (frontend sets gain 0.18 over 20 ms). Back under `speech_off`
  for 120 ms gives `Unduck`. Above `speech_on` for `cut_in_commit_ms` gives
  `Yield`: frontend cancels playback, calls `stopGenerating()`, and the
  machine moves to `UserSpeaking`. With `voice.cut_in = off`, only Esc and
  the button stop playback.
- `TRN-3` **End of turn.** `UserPause` lasting `end_silence_ms` after at least
  `min_user_turn_ms` of speech gives `EndOfTurn`. Shorter blips are dropped
  (coughs, "mhm").
- `TRN-4` **Trailing-word check.** Every 400 ms during a pause, run Parakeet on
  the buffered audio (cheap at this size) and pass the text to
  `set_partial_text`. If it ends in `.?!`, use the short silence (300 ms). If
  the last word is in the "not finished" list, use 1200 ms. Lists, lower case:
  - English: and, or, but, so, because, if, when, that, which, the, a, an,
    my, your, to, of, with.
  - German: und, oder, aber, weil, dass, wenn, ob, der, die, das, ein, eine,
    mein, dein, zu, mit, von, also.
  Language from Parakeet's detected language, else `voice.language`.
- `TRN-5` **`VoiceSession` and commands.**
  `voice_start_cmd(conversation_id, on_event: Channel<VoiceEvent>)`,
  `voice_stop_cmd()`, `voice_push_audio_cmd` (raw body),
  `voice_assistant_cmd(state: "started" | "finished", generation: u64)`.

  ```rust
  #[serde(tag = "type", rename_all = "snake_case")]
  pub enum VoiceEvent {
      Floor { state: String },                  // for the orb
      Level { input: f32 },
      Partial { text: String },                 // TRN-4 text, shown live
      Transcript { text: String, language: Option<String> },
      Duck, Unduck, Yield,
      Audio { generation: u64, seq: u32, sample_rate: u32, pcm: Vec<u8> },
      Error { message: String },
  }
  ```

  Starting a session closes any earlier one. Starting while hearing is not
  installed returns `Error` with a `VXP-7` message.

---

## Phase 4: a voice turn through the agent (VTN)

- `VTN-1` **Spoken flag on send.** Extend `sendMessage(text, attachments,
  opts?: { spoken?: boolean })` in `store.ts`. A spoken send stores
  `spoken: true` on the user `Message` and passes `spoken: true` to
  `agent_chat_cmd` (new optional param).
- `VTN-2` **Prompt sentence.** Add `pub spoken: bool` to `PromptInputs`. When
  true, `compose_system_prompt` appends one paragraph:
  "The user is talking to you by voice and will hear your reply spoken
  aloud. Answer in short spoken sentences. Do not use markdown, lists,
  tables or code in the reply; if something needs to be on screen, put it in
  an artifact and say so in one short sentence."
  Mirror the same text in the frontend assembly in `store.ts` and update the
  golden and the vitest in the same commit (`context_golden.rs` rule).
- `VTN-3` **Speech queue.** `voice_speak_cmd(generation: u64, text: String,
  done: bool)` appends streamed token text. Rust side `SpeechQueue`:
  - cuts at sentence ends (`. ! ? :` followed by space or end, and newlines),
    never inside a number like `3.5` or an abbreviation in a short list
    (`z.B., d.h., e.g., i.e., Dr., Nr.`);
  - the first chunk may be cut early at a comma after 8 words, so speech
    starts sooner;
  - strips markdown (`*`, `_`, `#`, backticks, link targets), drops fenced
    code blocks and tables entirely;
  - synthesizes one chunk at a time with `generate_with_config`. The callback
    must be `'static`, so it sends partial samples through a
    `std::sync::mpsc::Sender` that a task drains into `VoiceEvent::Audio`;
  - returning `false` from the callback stops synthesis when the generation
    is cancelled.
  The frontend calls it from the `Token` handler only when the turn is
  spoken. `Thinking` events are never spoken.
- `VTN-4` **Voice per language.** If Parakeet detected German and the selected
  voice is English-only, use the installed German voice for this reply. If
  none is installed, speak with the selected voice and show a one-line hint
  in the voice surface (`VXP-7`).
- `VTN-5` **Spoken notices during tools.** On `StepStart`, if nothing has been
  spoken for 1.5 s, speak one short notice picked from the step verb, using
  the same verb families as `orbForStep`: searching gives "Let me look that
  up." / "Ich schau kurz nach."; solving gives "Let me work that out." / "Ich
  rechne das kurz durch."; composing gives "I'll write that down." / "Ich
  schreibe das auf." At most one notice per 10 s. These are not saved to the
  transcript.
- `VTN-6` **Save what was heard.** The frontend tracks which `seq` per
  generation was `played`. The speech queue keeps the text of each `seq`. On
  `Yield` or stop, the saved assistant `content` is the joined text of played
  chunks, `stop_reason = "interrupted"`, and the full unspoken text is kept in
  `steps_json` under `unspoken` so nothing is lost. On normal finish, content
  is the full reply.
- `VTN-7` **Schema v30.** `ALTER TABLE messages ADD COLUMN spoken INTEGER NOT
  NULL DEFAULT 0`. Set for spoken user turns and spoken replies. Add
  `"interrupted"` to the documented `stop_reason` values in `schema.sql`.

---

## Phase 5: UI (VOC-UI)

### Integration map

| ID | Where | What the user sees |
|---|---|---|
| `VOC-UI-1` | `Composer.tsx`, left of the send button (`:987`) | Mic button. Click: dictate into the input (text appears, user edits and sends). Hold for more than 300 ms: push-to-talk, sends on release. A ring around the icon follows the input level while open. |
| `VOC-UI-2` | `/` menu, section "Conversation" (`shared/commands.json`, handler in `lib/commands.ts`) | `/talk` (alias `/voice`) opens voice mode for the current conversation, or brings the surface back after "Show chat". Same as the hotkey. Was a "Talk" button next to the mic; see Build notes. |
| `VOC-UI-3` | New `src/components/Voice/VoiceMode.tsx` + `.css`, rendered by `routes/Chat.tsx` over the conversation | The voice surface (spec below). |
| `VOC-UI-4` | `src/components/Orb/orbState.ts` | New `orbForFloor(state)`: `listening`, `user_speaking` â†’ `listening`; `thinking` â†’ `connecting`; `speaking` â†’ `composing`; `listening` with no speech for 2 s â†’ `breathing`. Tool steps override via `orbForStep` while a step runs. |
| `VOC-UI-5` | Conversation message rendering (`components/Conversation`) | Spoken messages show a small waveform glyph beside the time. An interrupted reply ends with a faint "stopped here" mark, and "Show the rest" expands the unspoken text from `steps_json`. |
| `VOC-UI-6` | `routes/Runtime.tsx`, new tab `voice` (extend `RuntimeTab`) | Hearing and Voice cards in the same card style as Recall: install, size, delete, a play button per voice, speed slider, language, "Let me cut in" toggle (`voice.cut_in`), and a mic test with a live level meter. `openRuntime("voice")` deep link. |
| `VOC-UI-7` | `components/Onboarding` | One optional step after the chat model: "Want to talk to me?" with one button that installs the defaults from `VOC-3`. Skippable, never blocking. |
| `VOC-UI-8` | `components/CommandPalette` | Commands "Talk to Poiesis", "Stop talking", "Voice settings". |
| `VOC-UI-9` | Global key handler (where other app hotkeys live) | `voice.hotkey` toggles voice mode. Esc in voice mode stops speech first; a second Esc leaves voice mode. |
| `VOC-UI-10` | `components/TopBar` | While the mic is open anywhere, a small red mic dot with "Listening" on hover; click leaves voice mode (`VXP-3`). |

### Voice surface spec (`VOC-UI-3`)

- Covers the conversation pane, not the whole window. The rail and top bar
  stay, so the user is still "in" the conversation.
- Center: the orb at 160 px, scaled 1.0 to 1.15 by the live level (`VXP-1`).
- Under the orb, one line of state text: "Listening", "Thinking", the current
  step's label while a tool runs, nothing while speaking.
- Below that, a two-line live caption area: the user's words (from
  `Partial`, then `Transcript`) in the secondary text color; then Poiesis's
  current sentence as it is spoken, in the primary color. Older lines fade.
- Bottom bar: mute mic toggle, stop (only while speaking), "Show chat"
  (leaves voice mode, keeps the session), and "On this device" in small
  tertiary text (`VXP-4`).
- Artifacts created during the turn appear as a small card above the bottom
  bar; clicking one leaves voice mode and opens it.
- Theme tokens from `DESIGN.md`; no new colors. Respect
  `prefers-reduced-motion` by fixing the orb size and keeping state changes.

---

## Tests (-T)

- `VOC-T1` `voice_floor.rs` unit tests with scripted probability sequences:
  cough under `min_user_turn_ms` is ignored; pause after "und" waits 1200 ms;
  pause after "?" ends at 300 ms; overlap under 180 ms ducks and unducks;
  overlap over 180 ms yields; `max_turn_ms` forces an end.
- `VOC-T2` `SpeechQueue` unit tests: sentence cuts, no cut in `3.5` or `z.B.`,
  early comma cut on the first chunk, markdown stripped, code fences dropped.
- `VOC-T3` Catalog test: every entry has url, sha256, size label, and its note
  passes the `VXP-6` banned-word check (extend the `SMP-8a` test).
- `VOC-T4` Golden: `UPDATE_PROMPT_GOLDEN` run with `spoken: true` and `false`;
  the vitest mirror matches.
- `VOC-T5` `#[ignore]` integration: `de_short.wav` and `en_short.wav` through
  VAD + Parakeet give the expected text within a small word error.
- `VOC-T6` Vitest: `orbForFloor` mapping; `VTN-6` saved content equals played
  chunks only; mic button hold vs click.
- `VOC-T7` Manual pass in the real app, recorded in "Build notes": speakers
  without headphones, cut in three times in one reply, German and English
  turns, a turn that calls web search, leave voice mode mid-reply.

---

## Order of work

1. Phase 0 (all of it). Decide sidecar fallback only if `VOC-0.2` fails.
2. `VOC-1` to `VOC-4`, `AUD-1` to `AUD-4`, `VOC-UI-1` dictation only.
   Dictation alone is useful and tests capture, hearing and the mic UI.
3. `TRN-1` to `TRN-5`, `VTN-1` to `VTN-3`, `VOC-UI-2` to `VOC-UI-4`.
   First full voice turn.
4. `VTN-4` to `VTN-7`, `VOC-UI-5` to `VOC-UI-10`, `VOC-6`, `VOC-11`.
5. Later: `VOC-9`, `VOC-10`.

## Risks and open questions

- **Build mode** (`VOC-0.2`): static vs shared sherpa-onnx library on
  Windows. Shared adds a DLL to the bundle and to the auto-updater payload.
- **CPU contention.** Parakeet and TTS on CPU while a CPU-offloaded chat
  model generates. Measure in `VOC-0.5`; if speech stutters, lower
  `num_threads` or give TTS priority while `Speaking`.
- **Pocket TTS German quality and load time** in sherpa are unverified until
  `VOC-0.3`. Fallback is Piper Thorsten.
- **Cloud chat models.** A voice turn with a cloud chat model sends the
  transcript text to that provider, exactly like a typed turn. Audio never
  leaves. The voice surface's "On this device" note must then read "Voice on
  this device", so it does not over-promise.
- **Echo cancellation on some headsets/drivers** may be weak. `voice.cut_in`
  off is the escape hatch; push-to-talk always works.

## Build notes

### Phase 0 results (2026-10-06)

Machine: AMD Ryzen 7 1800X (8 cores, AVX2 only, 2017). A weak CPU on purpose:
if it works here it works on most machines. All numbers are CPU only, 4
threads, models loaded from local files. Spike test:
`cargo test --lib voice::tests::spike -- --ignored --nocapture` with
`POIESIS_VOICE_MODELS` set (see the doc comment on the test for the layout).

| Item | Result |
|---|---|
| `VOC-0.1` | `rust-version = "1.90"`, `rust-toolchain.toml` pins 1.96.0. `cargo test --lib`: 661 passed. `cargo build --release`: ok (9 min, 39.5 MB exe). `npm run tauri build` (installer) not run. |
| `VOC-0.2` | **Static mode works**, including release with LTO. The crate downloads `sherpa-onnx-v1.13.8-win-x64-static-MT-Release-lib.tar.bz2` at build time, so an offline build needs `SHERPA_ONNX_LIB_DIR`. No DLL to ship. `cargo check --no-default-features` also builds. |
| `VOC-0.3` | Silero, Parakeet v3 int8, Kokoro and Piper Thorsten all load and work. Parakeet transcribed both fixtures exactly. Fixtures are the 24 kHz sample files from the Parakeet release (`fixtures/voice/`), resampled to 16 kHz in the test. |
| `VOC-0.4` | **Open.** wry (`0.55.1`) only handles the clipboard permission, so WebView2 should show its own microphone prompt on first use. Needs a manual check in the real app (`VOC-T7`). |
| `VOC-0.5` | See below. |

Timing (`VOC-0.5`):

| Step | Time |
|---|---|
| Silero load | 63 ms |
| Parakeet load | 3.0 s |
| Parakeet, 3.8 s of English | 0.48 s |
| Parakeet, 2.8 s of German | 0.34 s |
| Kokoro int8, 3.7 s sentence | **6.0 s** (slower than real time, no gain from 8 threads) |
| Kokoro fp32, same sentence | **1.9 s** (about 3x faster than int8 here) |
| Piper Thorsten (German) int8, 3.8 s sentence | 1.2 s |
| Kokoro fp32 load / Piper load | 2.2 s / 4.8 s |

TTS does not stream inside one call: the progress callback fires once at the
end, so "time to first audio" equals the time for the whole chunk. That is why
`VTN-3` cuts the first chunk early (8 words) and synthesizes one chunk at a
time.

Real loop test (ignored, needs the network): downloads the German voice and the
speech detection file with checksum, speaks "Heute ist das Wetter schoen und
ich gehe spazieren.", and Parakeet hears the same sentence back. Run:
`cargo test --lib voice::tests::install_speak -- --ignored --nocapture`.

### Changes from the plan, found while building

- **Parakeet size.** The int8 archive is 487 MB, not 640 MB. Copy says "About 490 MB".
- **Kokoro ships fp32, not int8.** int8 is 3x slower on this CPU. The catalog
  uses `kokoro-multi-lang-v1_0` (350 MB) and only the English lexicon.
  Speaker numbers were read from the model's own metadata.
- **Pocket TTS is out for now.** The sherpa export has no German bundle (the
  voice comes from a reference clip, nothing is bundled), and its release
  notes say the ONNX export is for non-commercial use. German is Piper
  Thorsten (CC0). Revisit only if a German bundle with a clear license appears.
- **Moonshine is in the catalog, English only.** The archive is
  `sherpa-onnx-moonshine-base-en-quantized-2026-02-27` (111 MB, checksum equals
  the release's own digest, MIT license for English). It holds two `.ort`
  files and `tokens.txt`: `moonshine_config` uses `encoder_model.ort` and
  `decoder_model_merged.ort`. It loads in 1.1 s and heard the English fixture
  in 0.17 s, with punctuation (ignored test `light_hearing_understands_english`).
  Its German sibling is non-commercial, so it is not offered. It is the
  default only for an English interface on a machine with under 8 GB of memory;
  German or any other language keeps Parakeet. Installing a second hearing
  does not switch to it: the Voice tab shows "Use this one".
  The light English voice is Piper Alba (en_GB, CC BY 4.0). Amy was dropped
  because its model card gives no license text; Ryan and the HFC voice are
  non-commercial.
- **Voice list, many languages.** The Voices box lists every voice of every
  model with one filter ("Installed voices", "All languages", or one language,
  starting on the language of the voice in use). Each row picks, plays a sample
  in its own language, downloads or removes; a set (Kokoro, 7 voices in one
  350 MB file) says so and is removed as a set. The list holds 41 voices in 18
  languages (English, German, French, Spanish, Italian, Dutch, Portuguese,
  Polish, Russian, Swedish, Danish, Norwegian, Finnish, Czech, Greek, Hungarian,
  Romanian, Vietnamese). Every archive name, size and sha256 comes from the
  sherpa-onnx `tts-models` release and was downloaded and checked.
- **Voice license rule.** Only voices whose data license is CC0, public domain,
  CC BY, Apache 2.0 or Unlicense are listed. Non-commercial, share-alike,
  AGPL and "see URL" licenses were left out (for example the German voices
  Pavoque, Karlsson and Ramona, Russian Ruslan, Turkish, Hindi). A Rust test
  checks that each voice is in `THIRD_PARTY_NOTICES.md` with an allowed license.
  Each row shows its license.
- **Left out because they did not work.** The Ukrainian voice loads but
  skips most sounds (its phoneme table does not match), so it is not offered.
  The Chinese voice needs a word list and number rules instead of the usual
  speech data, which `piper_config` does not set up. Both can be added later.
- **One shared speech-data folder.** The speech engine reads its data folder
  once per run and keeps the path. The data used to sit inside each voice, so
  removing the voice that was loaded first broke every other voice until a
  restart. All voices now use `<voice folder>/espeak-ng-data` (18 MB, copied from
  the first voice that has it, never deleted).
- **Default voice by language.** An interface language with a voice gets its
  first voice (French gets Siwis); a language with none gets English. The
  "this reply is in X but no X voice is installed" hint works for every named
  language. Reply language is still guessed for German and English only; for
  other languages the chosen voice is kept.
- **Which hearing is used.** `voice.hearing_model` is a preference. If that one
  is not installed but another is, the installed one is used, so nobody is
  stuck. `VoiceStatus` gained `hearings` (all installed ids).
- **`VOC-9` is not built, and why.** sherpa's static library already contains
  its own `onnxruntime.lib`. The `ort` crate would bring a second runtime into
  the same program: either a duplicate-symbol link error, or a loose
  `onnxruntime.dll` to ship and keep apart from the Windows system copy. That
  cost is only worth paying if the pause feels wrong in `VOC-T7`. The model
  would also need its own mel-spectrogram code.
- **`VOC-10` is not built.** It compares quality and speed against a large chat
  model with audio input. It needs that model on disk and a person to judge it.
- **Silero gives yes or no, not a probability.** sherpa's
  `VoiceActivityDetector` exposes only `detected()`. `FloorMachine.observe`
  (`TRN-1`) is fed 0.0 or 1.0 plus RMS. `speech_on` and `speech_off` then
  collapse to one threshold, and the Duck/Unduck ladder works on timers only.
  If that feels rough in `VOC-T7`, run Silero ourselves through `ort` (which
  `VOC-9` needs anyway).
- **Parakeet reports no language.** sherpa returns text and tokens only.
  `TRN-4` and `VTN-4` need a language guess: use `voice.language` when set,
  else a small stop-word check on the text (German vs English).
- **Dictation sends one buffer, not a stream.** `voice_transcribe_cmd` takes
  the whole stretch as one raw body (max 120 s). Frame streaming with
  `voice_push_audio_cmd` and the header counter (`AUD-2`) moves to `TRN-5`,
  where a live session needs it.
- **Preview returns a WAV file**, not bare PCM, so the sample rate travels
  with the bytes (`VOC-6`).

- **No `Level` event from Rust.** The screen already measures the mic level
  from the capture frames, so `VoiceEvent::Level` was dropped.
- **`observe` has no `rms`.** With a yes or no from the detector, energy has
  nothing to add. `FloorAction` gained `Discard` (a cough was dropped).
- **Speech detector pause.** `min_silence_duration` is 0.1 s (was 0.25), so
  the detector does not add its own pause on top of the turn machine's.
- **Audio over the channel is base64**, not a number list. `VoiceEvent::Audio`
  also carries the text of the piece and a `notice` flag, so the screen can
  keep what was heard without asking again. `VoiceEvent::SpeechDone` and
  `Hint` were added.
- **The prompt is built on the frontend**, so `VTN-1` needs no `spoken`
  parameter on `agent_chat_cmd`. Rust has `PromptInputs.spoken` so both
  sides stay equal.
- **The spoken sentence has its own small golden**
  (`fixtures/voice/spoken-prompt.golden.txt`), not a field in the shared
  prompt fixture, so that gate can move on its own.
- **`VTN-3` is one piece at a time.** The speech call gives no sound before
  it ends, so there is no stream to forward: a piece is made, then sent.
- **A turn stays open while Poiesis talks.** The chat store waits for the
  voice (`SpeechBridge.end`) before it saves the reply and frees the composer.
  Otherwise the reply would be saved long before it was heard, and "what was
  heard" could not be saved.
- **`unspoken` holds the whole written reply**, not only the unheard rest: the
  spoken text has its markdown removed, so the rest cannot be cut out exactly.
  The button says "Show the full reply".
- **`steps_json` can be an object.** A cut-off reply stores `{ steps, unspoken }`;
  `parseSteps` reads both shapes. Only the frontend reads this column.
- **`spoken` on a message is set by a second call** (`mark_spoken`), not a
  field of `NewMessage`, so the other 25 writers did not change.
- **Voice mode needs a voice as well as hearing.** The surface offers both
  downloads. Without it the mic could hear but never answer.
- **Orb size.** The orb library has 64, 32 and 20 px designs only. The voice
  surface draws the 64 px one and enlarges it to 128 px.
- **The hotkey works while Poiesis has focus**, not system wide. It is changed
  in the Voice tab ("Change", then press the keys; it needs Ctrl, Alt or Cmd,
  and Escape, Tab, Enter, Backspace and Delete are refused) and works at once.
- **Talking again while the last turn is still being heard.** If the user
  speaks for long enough to take the floor back before the words of the earlier
  turn were ready, those words are held (`Carry`) and sent in front of the next
  turn, so two halves of one thought become one message. If the new sound was a
  cough, or held no words, the held words are sent alone.
- **Removing the voice in use** switches to another installed voice, one in the
  same language first. With none left, voice mode offers the download again.
- **Voice mode opens from `/talk`, not a button (`VOC-UI-2`, 2026-10-07).** The
  composer rule `CMP-4` says `+` adds to the message and `/` does something;
  opening voice mode is an action, so it belongs in `/`. A Talk button next to
  the mic also crowded the composer and looked like a second mic. Voice mode
  opens four ways: `/talk` (alias `/voice`), the shortcut, "Talk to Poiesis" in
  the command palette, and the onboarding step. The mic button stays: hold to
  talk needs a press and a release, which a menu entry cannot give. `/talk`
  needs a conversation and says why not in workspace mode.
- **Dictation knows the interface language.** Its body is raw audio, so the
  language travels in an `x-ui-language` header; the default hearing then
  matches what the Voice tab shows.
- **Voice mode opens only in the chat view** (not in workspace mode or on
  another page), so the mic is never open where it cannot be seen.

### Where the code is

- `src-tauri/src/runtime/voice.rs`: model configs, `VoiceManager`, WAV helpers, spike tests.
- `src-tauri/src/runtime/voice_catalog.rs`: catalog, defaults, install (download, sha256, unpack).
- `src-tauri/src/runtime/voice_floor.rs`: `FloorMachine`, word lists, language guess (`TRN-1` to `TRN-4`).
- `src-tauri/src/runtime/voice_speech.rs`: `SentenceChunker`, `SpeechQueue` (`VTN-3`).
- `src-tauri/src/runtime/voice_session.rs`: `TurnPipeline`, `VoiceSession`, `VoiceEvent`, `pick_voice`.
- `src-tauri/src/commands/voice.rs`: `voice_*_cmd`, settings (`voice.*` keys).
- `src/lib/voice/`: `resample.ts`, `playbackQueue.ts`, two worklets, `audio.ts` (`VoiceAudio`), `dictation.ts`,
  `session.ts` (`VoiceSession`), `controller.ts` (the app's one session), `bridge.ts` (what the chat store calls),
  `turns.ts` (what was heard), `notices.ts`, `hotkey.ts`, `voiceStore.ts`.
- `src/components/Voice/`: `MicButton`, `VoiceMode`, `MicIndicator`.
- `shared/commands.json` and `src/lib/commands.ts`: the `/talk` command (`VOC-UI-2`).
- `src/components/Conversation/SpokenMark.tsx`: the wave glyph and "Stopped here".

### First hand check, 2026-10-08

Erich tried it on a laptop with a mic and no GPU. Overall experience good, and a
laptop without a GPU gives a proper experience. Two flaws, both fixed the same day:

- **No feedback in voice mode.** Errors only reached the chat, which voice mode
  covers: the agent's error was written as the reply and never spoken, the
  `speak` and `push` calls swallowed their failures, and a spoken turn sent while
  the last answer was still running was dropped without a word. Tool calls and
  agents were also invisible. Now `SpeechBridge.fail` carries a failed turn to
  the surface, the session reports speech and mic failures, and `activity.ts`
  lists what the answer did (`VOC-UI-11`, `VOC-UI-12`).
- **Hard to get back to voice.** After "Show chat" the only way back was typing
  `/talk`. `VOC-UI-13` adds a button that exists only while a voice conversation
  is live behind the chat. It is never a way to start one, so `CMP-4` and the
  decision that voice opens from `/` still hold.

- **No WebView2 microphone box.** `getUserMedia` made WebView2 ask "allow the
  microphone?". `src-tauri/src/mic_permission.rs` registers a `PermissionRequested`
  handler on the main window that allows the microphone, and only the microphone,
  for the app's own origins (`tauri.localhost`, the dev server). Artifact frames and
  every other permission keep WebView2's default. Windows' own privacy switch for
  desktop apps is separate and still applies.

### Manual pass (`VOC-T7`)

(Not done yet. Dictation first: tap, hold, Escape, leave the window, no model
installed, permission prompt.)

Then voice mode, in the real app, on speakers without headphones:

1. Open voice mode with nothing installed: it offers one download for hearing
   and a voice, with a size, and works after it.
2. Ask a question in English, then in German. The reply is spoken in a voice of
   that language when one is installed, else with a one-line hint.
3. Cut in three times in one reply: the voice drops at once, stops if you go on,
   and your words become the next turn. The saved reply says "Stopped here".
4. Ask something that needs web search: a short notice is spoken, and the answer
   follows.
5. Press Escape while it speaks (stops speech), then again (leaves).
6. Leave voice mode mid-reply, switch chat, minimize the window: the mic light
   goes out each time.
7. Mute the mic: the "Mic on" mark goes away and nothing is heard.
8. Say "mhm" or cough: nothing is sent.
9. Check CPU: speech must not stutter while a CPU-offloaded chat model answers.
10. Voice tab: download the light hearing, pick it, and talk in English. Switch back
    and talk in German.
11. Voice tab: change the shortcut, press it, then reset it.
12. Voice tab: download one voice in another language and press "Hear it".
13. Finish a sentence, then start a second one at once. The two parts must arrive as
    one message, and a cough in that gap must not lose the first part.
14. Type `/talk` (and `/voice`): voice mode opens. Press "Show chat", then `/talk`
    again: the same conversation comes back, without a second start.
15. Make something fail (stop the chat model, or pick none) and speak: the voice
    surface shows one line saying so, and Dismiss clears it.
16. Ask something with tools or an agent: the list under the orb shows each step
    running, then done; a failing step shows in red.
17. Press "Show chat": the wave button appears in the composer and brings voice
    back. End the conversation: the button is gone. The shortcut switches between
    chat and voice while it is live.

## Sources

- sherpa-onnx Rust crate: https://docs.rs/sherpa-onnx, https://github.com/k2-fsa/sherpa-onnx
- Parakeet vs Whisper (2026): https://openwhispr.com/blog/parakeet-vs-whisper-vs-nemotron
- Local STT comparison: https://www.onresonant.com/resources/local-stt-models-2026
- Parakeet TDT 0.6B v3 int8: https://huggingface.co/CoderViking/parakeet-tdt-0.6b-v3-onnx
- Local TTS overview: https://openvoxai.com/blog/best-free-local-tts-models-2026
- Pocket TTS ONNX: https://huggingface.co/xennonf4/pocket-tts-onnx
- Supertonic 3 status and license: https://huggingface.co/jinhwan000/supertonic-3-mirror
- Smart Turn v3: https://docs.pipecat.ai/server/utilities/smart-turn
- llama.cpp multimodal (audio input): https://github.com/ggml-org/llama.cpp/blob/master/docs/multimodal.md
- Openlive: https://github.com/byte271/Openlive
