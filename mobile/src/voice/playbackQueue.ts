// The playback side of voice (AUD-3): a queue of audio chunks tagged with the
// reply (`generationId`) and position (`seq`) they belong to. Pure logic, so
// the worklet is a thin shell and the rules run under vitest.
//
// Idea taken from Openlive's playback worklet (Apache-2.0, see
// THIRD_PARTY_NOTICES.md): a generation id per reply, cancel with a short
// fade instead of a click, and a `played` report per chunk so the app knows
// what was actually heard. Its jitter and packet-loss code is not needed here.

export type PlaybackEvent =
  | { type: "played"; generationId: number; seq: number }
  | { type: "idle" }
  | { type: "level"; rms: number };

interface Chunk {
  generationId: number;
  seq: number;
  pcm: Float32Array;
  pos: number;
}

/** How often `level` is reported, in seconds. */
const LEVEL_EVERY_S = 0.05;

export class PlaybackQueue {
  private queue: Chunk[] = [];
  private cancelled = new Set<number>();
  private gain = 1;
  private targetGain = 1;
  private gainStep = 0;
  private fade: { generationId: number; remaining: number } | null = null;
  private wasPlaying = false;
  private levelSum = 0;
  private levelCount = 0;
  private readonly fadeSamples: number;
  private readonly levelEvery: number;

  constructor(
    private readonly sampleRate: number,
    fadeMs = 30,
  ) {
    this.fadeSamples = Math.max(1, Math.round((sampleRate * fadeMs) / 1000));
    this.levelEvery = Math.round(sampleRate * LEVEL_EVERY_S);
  }

  /** `pcm` must already be at this queue's sample rate. */
  enqueue(generationId: number, seq: number, pcm: Float32Array): void {
    // A late chunk of a reply that was already cut off must stay silent.
    if (this.cancelled.has(generationId)) return;
    this.queue.push({ generationId, seq, pcm, pos: 0 });
  }

  /** Stop one reply: queued chunks go now, the one playing fades out. */
  cancel(generationId: number): void {
    this.cancelled.add(generationId);
    // Keep the head (it is playing and will fade); drop the rest of this reply.
    this.queue = this.queue.filter((c, i) => c.generationId !== generationId || i === 0);
    if (this.queue[0]?.generationId === generationId) this.fade = { generationId, remaining: this.fadeSamples };
  }

  /** Lower or restore the volume, reaching `gain` after `seconds`. */
  setDuck(gain: number, seconds: number): void {
    this.targetGain = gain;
    if (seconds <= 0) {
      this.gain = gain;
      this.gainStep = 0;
    } else {
      this.gainStep = (gain - this.gain) / (seconds * this.sampleRate);
    }
  }

  /** Fill `out` (silence when nothing is queued) and say what happened. */
  process(out: Float32Array): PlaybackEvent[] {
    const events: PlaybackEvent[] = [];
    for (let i = 0; i < out.length; i++) {
      const chunk = this.queue[0];
      if (!chunk) {
        out[i] = 0;
        continue;
      }
      this.stepGain();
      let s = chunk.pcm[chunk.pos++] * this.gain;
      if (this.fade && this.fade.generationId === chunk.generationId) {
        s *= this.fade.remaining / this.fadeSamples;
        if (--this.fade.remaining <= 0) {
          this.fade = null;
          this.queue.shift(); // cut off: not reported as played
          out[i] = s;
          this.accumulate(s, events);
          continue;
        }
      }
      out[i] = s;
      this.accumulate(s, events);
      if (chunk.pos >= chunk.pcm.length) {
        this.queue.shift();
        events.push({ type: "played", generationId: chunk.generationId, seq: chunk.seq });
      }
    }
    const playing = this.queue.length > 0;
    if (this.wasPlaying && !playing) events.push({ type: "idle" });
    this.wasPlaying = playing;
    return events;
  }

  private stepGain(): void {
    if (this.gain === this.targetGain) return;
    this.gain += this.gainStep;
    const passed = this.gainStep >= 0 ? this.gain >= this.targetGain : this.gain <= this.targetGain;
    if (passed || this.gainStep === 0) this.gain = this.targetGain;
  }

  private accumulate(s: number, events: PlaybackEvent[]): void {
    this.levelSum += s * s;
    if (++this.levelCount >= this.levelEvery) {
      events.push({ type: "level", rms: Math.sqrt(this.levelSum / this.levelCount) });
      this.levelSum = 0;
      this.levelCount = 0;
    }
  }
}
