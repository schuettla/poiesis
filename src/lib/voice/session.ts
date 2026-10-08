// A live voice conversation from the screen's side (TRN-5, VTN): mic frames go
// to Rust, Rust reports turns, the reply text goes back to be spoken, and what
// was actually heard decides what is saved (VTN-6). Everything it touches is
// passed in, so it is tested with fakes and no audio.

import type { VoiceEvent } from "../api";
import type { VoiceAudioEvents } from "./audio";
import type { SpeechBridge, SpeechOutcome } from "./bridge";
import { NoticePacer, noticeFor } from "./notices";
import { SpokenTurns } from "./turns";
import { setVoiceUi, useVoiceStore } from "./voiceStore";

/** 5 frames of 20 ms make one 100 ms batch (AUD-2). */
const FRAMES_PER_PUSH = 5;
/** How loud Poiesis stays while the user talks over it (TRN-2). */
const DUCK_GAIN = 0.18;
/** How long a spoken turn waits for the last reply to wind down. */
const BUSY_WAIT_MS = 5000;
/** Lost mic batches in a row before the user is told (each is 100 ms). */
const PUSH_FAILURES_BEFORE_WARNING = 10;

export interface VoiceApi {
  start(onEvent: (e: VoiceEvent) => void): Promise<void>;
  stop(): Promise<void>;
  push(counter: number, pcm: Uint8Array): Promise<void>;
  assistant(state: "started" | "finished", generation: number): Promise<void>;
  speak(generation: number, text: string, done: boolean, opts?: { notice?: boolean; language?: string }): Promise<void>;
  cancel(generation: number): Promise<void>;
}

export interface AudioPort {
  start(opts: { playback: boolean }): Promise<void>;
  stop(): void;
  enqueue(generationId: number, seq: number, pcm: Float32Array, sampleRate: number): void;
  cancel(generationId: number): void;
  setDuck(gain: number, seconds: number): void;
  setMuted(muted: boolean): void;
}

/** What a voice session needs from the rest of the app. */
export interface SessionHost {
  /** Sends the user's words as a spoken turn. Resolves when the turn is over. */
  sendSpoken(text: string): Promise<void>;
  /** Stops the run that is working on a reply. */
  stopRun(): void;
  busy(): boolean;
}

export interface SessionDeps {
  api: VoiceApi;
  makeAudio(events: VoiceAudioEvents): AudioPort;
  host: SessionHost;
  turns?: SpokenTurns;
  now?: () => number;
  /** For tests: how long to wait for the last reply to wind down. */
  busyWaitMs?: number;
}

/** 16-bit little-endian mono samples, base64, to floats. */
export function decodePcm16(base64: string): Float32Array {
  const raw = atob(base64);
  const out = new Float32Array(raw.length >> 1);
  for (let i = 0; i < out.length; i++) {
    let v = raw.charCodeAt(i * 2) | (raw.charCodeAt(i * 2 + 1) << 8);
    if (v >= 0x8000) v -= 0x10000;
    out[i] = v / 32768;
  }
  return out;
}

function plainReason(e: unknown): string {
  if (typeof e === "string" && e) return e;
  if (e instanceof Error && e.message) return e.message;
  return "something went wrong.";
}

function plain(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error && e.message) return e.message;
  return "Voice could not be started.";
}

export class VoiceSession implements SpeechBridge {
  private readonly turns: SpokenTurns;
  private readonly now: () => number;
  private audio: AudioPort | null = null;
  private running = false;
  private opening: Promise<void> | null = null;
  private frames: Int16Array[] = [];
  private counter = 1;
  private lastSpeechAt = 0;
  private userLanguage: string | null = null;
  private readonly pacer = new NoticePacer();
  private readonly started = new Set<number>();
  private readonly done = new Set<number>();
  private readonly finished = new Set<number>();
  private readonly waiters = new Map<number, ((o: SpeechOutcome) => void)[]>();
  /** Replies whose speech already failed once: say so once, not per token. */
  private readonly spokeBadly = new Set<number>();
  private pushFailures = 0;

  constructor(private readonly deps: SessionDeps) {
    this.turns = deps.turns ?? new SpokenTurns();
    this.now = deps.now ?? (() => Date.now());
  }

  get live(): boolean {
    return this.running;
  }

  /** Opens the session, then the mic. Throws a plain sentence if either fails.
   * Asking again while it opens waits for the same start. */
  start(): Promise<void> {
    if (this.running) return Promise.resolve();
    this.opening ??= this.open().finally(() => {
      this.opening = null;
    });
    return this.opening;
  }

  private async open(): Promise<void> {
    setVoiceUi({ floor: "starting", error: null, problem: null, hint: null, user: "", reply: "", muted: false });
    try {
      await this.deps.api.start((e) => this.onEvent(e));
      const audio = this.deps.makeAudio({
        onFrame: (pcm) => this.onFrame(pcm),
        onLevel: (input, output) => setVoiceUi({ input, output }),
        onPlayed: (generation, seq) => this.onPlayed(generation, seq),
      });
      this.audio = audio;
      await audio.start({ playback: true });
    } catch (e) {
      this.audio?.stop();
      this.audio = null;
      await this.deps.api.stop().catch(() => undefined);
      setVoiceUi({ floor: "off", error: plain(e) });
      throw e;
    }
    this.running = true;
    this.pushFailures = 0;
    setVoiceUi({ floor: "listening", floorSince: this.now(), startedAt: this.now() });
  }

  /** Closes the mic and the session. Whatever was being said stops (VXP-3). */
  async stop(): Promise<void> {
    if (!this.running && !this.audio) return;
    this.running = false;
    this.cutOff(this.turns.current, false);
    this.audio?.stop();
    this.audio = null;
    this.frames = [];
    await this.deps.api.stop().catch(() => undefined);
    setVoiceUi({ floor: "off", input: 0, output: 0, muted: false, problem: null });
  }

  setMuted(muted: boolean): void {
    this.audio?.setMuted(muted);
    this.frames = [];
    setVoiceUi({ muted });
  }

  /** Stop button or Escape: Poiesis stops talking at once, and its run stops. */
  stopSpeaking(): void {
    this.cutOff(this.turns.current, true);
  }

  // ---- speech bridge: what the chat store calls ----

  current(): number {
    return this.turns.current;
  }

  token(generation: number, text: string): void {
    if (!this.isLive(generation) || !text) return;
    this.lastSpeechAt = this.now();
    void this.deps.api.speak(generation, text, false).catch((e) => this.speechFailed(generation, e));
  }

  fail(message: string): void {
    this.trouble(message);
  }

  step(generation: number, verb: string): void {
    if (!this.isLive(generation)) return;
    const line = noticeFor(verb, this.userLanguage);
    const now = this.now();
    // Not while something is still being said or about to be.
    if (!line || this.turns.pending(generation) > 0 || !this.pacer.due(now, this.lastSpeechAt)) return;
    this.pacer.said(now);
    this.lastSpeechAt = now;
    void this.deps.api
      .speak(generation, line, false, { notice: true, language: this.userLanguage ?? undefined })
      .catch(() => undefined);
  }

  end(generation: number): Promise<SpeechOutcome> {
    if (this.turns.wasInterrupted(generation)) return Promise.resolve(this.outcome(generation));
    if (this.finished.has(generation)) return Promise.resolve(this.outcome(generation));
    if (!this.running) {
      this.turns.interrupt(generation);
      return Promise.resolve(this.outcome(generation));
    }
    return new Promise<SpeechOutcome>((resolve) => {
      const list = this.waiters.get(generation) ?? [];
      list.push(resolve);
      this.waiters.set(generation, list);
      void this.deps.api.speak(generation, "", true).catch((e) => {
        this.speechFailed(generation, e);
        this.cutOff(generation, false);
      });
    });
  }

  // ---- internals ----

  /** Something went wrong in a live conversation: say it in a line the user can
   * dismiss. Before the session is live the surface's own error is used. */
  private trouble(message: string): void {
    if (this.running) setVoiceUi({ problem: message });
    else setVoiceUi({ error: message });
  }

  private speechFailed(generation: number, e: unknown): void {
    if (this.spokeBadly.has(generation)) return;
    this.spokeBadly.add(generation);
    const why = typeof e === "string" && e ? ` ${e}` : "";
    this.trouble(`I could not speak that reply.${why} The answer is in the chat.`);
  }

  private isLive(generation: number): boolean {
    return this.running && generation === this.turns.current && !this.turns.wasInterrupted(generation);
  }

  private outcome(generation: number): SpeechOutcome {
    return { interrupted: this.turns.wasInterrupted(generation), heard: this.turns.heard(generation) };
  }

  private settle(generation: number): void {
    const list = this.waiters.get(generation);
    if (!list) return;
    this.waiters.delete(generation);
    const outcome = this.outcome(generation);
    for (const resolve of list) resolve(outcome);
  }

  /** Cuts the reply off: its voice stops and whatever was heard is what stays. */
  private cutOff(generation: number, stopRun: boolean): void {
    if (!generation || this.finished.has(generation)) return;
    this.finished.add(generation);
    this.turns.interrupt(generation);
    this.audio?.cancel(generation);
    void this.deps.api.cancel(generation).catch(() => undefined);
    void this.deps.api.assistant("finished", generation).catch(() => undefined);
    if (stopRun) this.deps.host.stopRun();
    this.settle(generation);
  }

  private maybeFinish(generation: number): void {
    if (!this.done.has(generation) || this.finished.has(generation) || this.turns.pending(generation) > 0) return;
    this.finished.add(generation);
    void this.deps.api.assistant("finished", generation).catch(() => undefined);
    this.settle(generation);
  }

  private onFrame(pcm: Int16Array): void {
    if (!this.running || useVoiceStore.getState().muted) return;
    this.frames.push(pcm);
    if (this.frames.length < FRAMES_PER_PUSH) return;
    const batch = this.frames;
    this.frames = [];
    const bytes = new Uint8Array(batch.reduce((n, f) => n + f.length * 2, 0));
    let at = 0;
    for (const f of batch) {
      bytes.set(new Uint8Array(f.buffer, f.byteOffset, f.byteLength), at);
      at += f.byteLength;
    }
    void this.deps.api
      .push(this.counter++, bytes)
      .then(() => {
        this.pushFailures = 0;
      })
      .catch(() => {
        // One lost batch is nothing; a run of them means nothing is being heard.
        if (++this.pushFailures === PUSH_FAILURES_BEFORE_WARNING) {
          this.trouble("Your voice is not reaching Poiesis. Try ending the voice conversation and starting it again.");
        }
      });
  }

  private onPlayed(generation: number, seq: number): void {
    this.turns.played(generation, seq);
    this.lastSpeechAt = this.now();
    if (generation === this.turns.current) setVoiceUi({ reply: this.turns.speaking(generation) });
    this.maybeFinish(generation);
  }

  private onEvent(e: VoiceEvent): void {
    switch (e.type) {
      case "floor":
        if (useVoiceStore.getState().floor !== e.state) setVoiceUi({ floor: e.state, floorSince: this.now() });
        break;
      case "partial":
        setVoiceUi({ user: e.text });
        break;
      case "transcript":
        this.userLanguage = e.language;
        setVoiceUi({ user: e.text });
        void this.beginTurn(e.text);
        break;
      case "duck":
        this.audio?.setDuck(DUCK_GAIN, 0.02);
        break;
      case "unduck":
        this.audio?.setDuck(1, 0.12);
        break;
      case "yield":
        // Speaking over Poiesis for long enough: it stops, and so does its run.
        this.audio?.setDuck(1, 0);
        this.cutOff(this.turns.current, true);
        break;
      case "audio": {
        const g = e.generation;
        if (g !== this.turns.current || this.turns.wasInterrupted(g) || !this.audio) break;
        this.turns.add(g, e.seq, e.text, e.notice);
        this.audio.enqueue(g, e.seq, decodePcm16(e.pcm), e.sample_rate);
        this.lastSpeechAt = this.now();
        if (!this.started.has(g)) {
          this.started.add(g);
          void this.deps.api.assistant("started", g).catch(() => undefined);
        }
        if (!e.notice) setVoiceUi({ reply: this.turns.speaking(g) });
        break;
      }
      case "speech_done":
        this.done.add(e.generation);
        this.maybeFinish(e.generation);
        break;
      case "hint":
        setVoiceUi({ hint: e.text });
        break;
      case "error":
        this.trouble(e.message);
        break;
    }
  }

  /** The user finished a turn: send it, and stay with it until it is over. */
  private async beginTurn(text: string): Promise<void> {
    const generation = this.turns.begin();
    setVoiceUi({ problem: null });
    const deadline = this.now() + (this.deps.busyWaitMs ?? BUSY_WAIT_MS);
    // A reply the user just cut in on needs a moment to wind down.
    while (this.deps.host.busy() && this.now() < deadline) {
      await new Promise((r) => setTimeout(r, 50));
    }
    if (this.deps.host.busy()) {
      // The store would drop the message without a word; the user must not
      // believe they were heard.
      this.trouble("Poiesis is still busy with the last answer, so I did not catch that. Say it again in a moment.");
    } else {
      try {
        await this.deps.host.sendSpoken(text);
      } catch (e) {
        // The chat shows its own error as the reply; the voice surface has no chat.
        this.trouble(`That did not work: ${plainReason(e)}`);
      }
    }
    // If the turn never reached speech (an error, no model), give the floor back.
    if (!this.finished.has(generation)) {
      this.finished.add(generation);
      void this.deps.api.assistant("finished", generation).catch(() => undefined);
      this.settle(generation);
    }
  }
}
