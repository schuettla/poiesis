// Dictation (VOC-UI-1): record one stretch of speech, then turn it into text.
// Frames are collected here and sent once, when the person stops. Streaming
// frames into a live session comes with turn-taking (TRN-5).

import { int16ToBytes, rmsInt16 } from "./resample";

/** The part of `VoiceAudio` dictation needs, so tests can stand in for the mic. */
export interface MicSource {
  start(opts?: { playback?: boolean }): Promise<void>;
  stop(): void;
}

export type MicFactory = (events: {
  onFrame: (pcm: Int16Array) => void;
  onLevel: (input: number) => void;
}) => MicSource;

/** Frames per second at 20 ms each. */
const FRAMES_PER_SECOND = 50;
/** The backend refuses more than 120 s; stop a little before. */
export const MAX_DICTATION_SECONDS = 115;

export class Dictation {
  private frames: Int16Array[] = [];
  private mic: MicSource | null = null;
  private limitHit = false;

  constructor(
    private readonly createMic: MicFactory,
    private readonly transcribe: (pcm: Uint8Array) => Promise<string>,
    private readonly hooks: { onLevel?: (level: number) => void; onLimit?: () => void } = {},
  ) {}

  get listening(): boolean {
    return this.mic !== null;
  }

  async start(): Promise<void> {
    if (this.mic) return;
    this.frames = [];
    this.limitHit = false;
    const mic = this.createMic({
      onFrame: (pcm) => this.onFrame(pcm),
      onLevel: (level) => this.hooks.onLevel?.(level),
    });
    this.mic = mic;
    try {
      await mic.start({ playback: false });
    } catch (e) {
      this.mic = null;
      throw e;
    }
  }

  private onFrame(pcm: Int16Array): void {
    if (!this.mic || this.limitHit) return;
    this.frames.push(pcm);
    if (this.frames.length >= MAX_DICTATION_SECONDS * FRAMES_PER_SECOND) {
      this.limitHit = true;
      this.hooks.onLimit?.();
    }
  }

  /** Close the mic and return what was said ("" when it was only silence). */
  async finish(): Promise<string> {
    const frames = this.frames;
    this.release();
    // Under a third of a second is a tap, not speech.
    if (frames.length < 16 || frames.every((f) => rmsInt16(f) < 0.003)) return "";
    return (await this.transcribe(int16ToBytes(frames))).trim();
  }

  /** Close the mic and throw the recording away. */
  cancel(): void {
    this.release();
  }

  private release(): void {
    this.mic?.stop();
    this.mic = null;
    this.frames = [];
  }
}

/** Add dictated words after what is already typed, with one space between. */
export function joinSpoken(typed: string, spoken: string): string {
  const base = typed.replace(/\s+$/, "");
  return base ? `${base} ${spoken}` : spoken;
}
