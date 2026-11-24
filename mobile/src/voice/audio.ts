// Mic in and speakers out for voice (AUD-4). Capture and playback share one
// AudioContext on purpose: the browser's echo cancellation only removes sound
// that this same page played, which is what lets a person cut in without
// headphones.

import captureUrl from "./capture.worklet.ts?worker&url";
import playbackUrl from "./playback.worklet.ts?worker&url";
import type { PlaybackEvent } from "./playbackQueue";
import { resampleLinear, rmsInt16 } from "./resample";

/** A mic problem with words fit for the screen (VXP-6). */
export class MicError extends Error {}

export interface VoiceAudioEvents {
  /** One 20 ms frame of 16 kHz mono speech. */
  onFrame?(pcm: Int16Array, seq: number): void;
  /** Input and output loudness, 0 to 1, about every 50 ms. */
  onLevel?(input: number, output: number): void;
  onPlayed?(generationId: number, seq: number): void;
  onIdle?(): void;
}

function micMessage(e: unknown): string {
  const name = e instanceof DOMException ? e.name : "";
  if (name === "NotAllowedError" || name === "SecurityError") {
    return "Poiesis cannot use the microphone. Allow it in Windows settings, under Privacy and security, Microphone.";
  }
  if (name === "NotFoundError" || name === "OverconstrainedError") {
    return "No microphone was found. Plug one in and try again.";
  }
  if (name === "NotReadableError") return "The microphone is in use by another app.";
  return "The microphone could not be started.";
}

export class VoiceAudio {
  private context: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private playback: AudioWorkletNode | null = null;
  private inputLevel = 0;
  private outputLevel = 0;
  private lastLevelAt = 0;

  constructor(private readonly events: VoiceAudioEvents = {}) {}

  get active(): boolean {
    return this.context !== null;
  }

  /** Opens the mic (and, unless `playback` is false, the speakers). */
  async start(opts: { playback?: boolean } = {}): Promise<void> {
    if (this.context) return;
    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: false, autoGainControl: true },
      });
    } catch (e) {
      throw new MicError(micMessage(e));
    }
    try {
      const context = new AudioContext({ latencyHint: "interactive" });
      this.context = context;
      this.stream = stream;
      await context.audioWorklet.addModule(captureUrl);
      const capture = new AudioWorkletNode(context, "poiesis-capture", { numberOfInputs: 1, numberOfOutputs: 1 });
      capture.port.onmessage = (e: MessageEvent<{ pcm: Int16Array; seq: number }>) => this.onFrame(e.data.pcm, e.data.seq);
      context.createMediaStreamSource(stream).connect(capture);
      // A node nothing listens to may never be run; a muted gain keeps it pulled
      // without sending the mic to the speakers.
      const mute = context.createGain();
      mute.gain.value = 0;
      capture.connect(mute).connect(context.destination);

      if (opts.playback !== false) {
        await context.audioWorklet.addModule(playbackUrl);
        const node = new AudioWorkletNode(context, "poiesis-playback", { numberOfInputs: 0, outputChannelCount: [1] });
        node.port.onmessage = (e: MessageEvent<PlaybackEvent>) => this.onPlayback(e.data);
        node.connect(context.destination);
        this.playback = node;
      }
      if (context.state === "suspended") await context.resume();
    } catch (e) {
      this.stop();
      throw e instanceof MicError ? e : new MicError("Voice could not be started.");
    }
  }

  /** Turns the mic off for real, so the system mic light goes out (VXP-3). */
  stop(): void {
    this.stream?.getTracks().forEach((t) => t.stop());
    this.stream = null;
    this.playback?.disconnect();
    this.playback = null;
    void this.context?.close().catch(() => undefined);
    this.context = null;
    this.inputLevel = 0;
    this.outputLevel = 0;
  }

  /** Queue a chunk of a spoken reply. `pcm` is mono floats at `sampleRate`. */
  enqueue(generationId: number, seq: number, pcm: Float32Array, sampleRate: number): void {
    if (!this.context || !this.playback) return;
    const resampled = resampleLinear(pcm, sampleRate, this.context.sampleRate);
    this.playback.port.postMessage({ type: "enqueue", generationId, seq, pcm: resampled }, [resampled.buffer]);
  }

  cancel(generationId: number): void {
    this.playback?.port.postMessage({ type: "cancel", generationId });
  }

  /** Mutes or unmutes the mic itself: while muted the browser sends no sound at all. */
  setMuted(muted: boolean): void {
    this.stream?.getAudioTracks().forEach((t) => {
      t.enabled = !muted;
    });
  }

  /** Lower (or restore, with 1) the speaking volume, e.g. while the user talks. */
  setDuck(gain: number, seconds: number): void {
    this.playback?.port.postMessage({ type: "duck", gain, seconds });
  }

  private onFrame(pcm: Int16Array, seq: number): void {
    this.inputLevel = rmsInt16(pcm);
    this.events.onFrame?.(pcm, seq);
    this.emitLevel();
  }

  private onPlayback(e: PlaybackEvent): void {
    if (e.type === "level") {
      this.outputLevel = e.rms;
      this.emitLevel();
    } else if (e.type === "played") {
      this.events.onPlayed?.(e.generationId, e.seq);
    } else {
      this.outputLevel = 0;
      this.events.onIdle?.();
    }
  }

  private emitLevel(): void {
    const now = performance.now();
    if (now - this.lastLevelAt < 50) return;
    this.lastLevelAt = now;
    this.events.onLevel?.(this.inputLevel, this.outputLevel);
  }
}
