// Audio math with no browser in it, so the worklets stay thin and these parts
// run under vitest. Linear interpolation is enough for speech at 16 kHz.

/** What the speech engine hears: 16 kHz mono. */
export const CAPTURE_RATE = 16_000;

/**
 * Turns a stream of mic blocks at any rate into 16 kHz mono frames of
 * `frameMs` (AUD-1). Feed it the worklet's 128-sample blocks; it calls `emit`
 * with a fresh `Int16Array` every time a frame fills.
 */
export class FrameBuilder {
  private readonly step: number;
  private readonly frameSize: number;
  private frame: Int16Array;
  private filled = 0;
  /** Where the next output sample sits between `prev` (0) and the new input (1). */
  private t = 1;
  private prev = 0;

  constructor(inRate: number, outRate = CAPTURE_RATE, frameMs = 20) {
    this.step = inRate / outRate;
    this.frameSize = Math.round((outRate * frameMs) / 1000);
    this.frame = new Int16Array(this.frameSize);
  }

  push(input: Float32Array, emit: (frame: Int16Array) => void): void {
    for (let i = 0; i < input.length; i++) {
      const x = input[i];
      while (this.t < 1) {
        this.put(this.prev + (x - this.prev) * this.t, emit);
        this.t += this.step;
      }
      this.t -= 1;
      this.prev = x;
    }
  }

  private put(v: number, emit: (frame: Int16Array) => void): void {
    const clamped = v > 1 ? 1 : v < -1 ? -1 : v;
    this.frame[this.filled++] = Math.round(clamped * 32767);
    if (this.filled === this.frameSize) {
      const done = this.frame;
      this.frame = new Int16Array(this.frameSize);
      this.filled = 0;
      emit(done);
    }
  }
}

/** Resample a whole buffer (used for spoken replies, which arrive at 22 to 24 kHz). */
export function resampleLinear(input: Float32Array, inRate: number, outRate: number): Float32Array {
  if (inRate === outRate || input.length === 0) return input;
  const outLength = Math.max(1, Math.floor((input.length * outRate) / inRate));
  const out = new Float32Array(outLength);
  const ratio = inRate / outRate;
  for (let i = 0; i < outLength; i++) {
    const pos = i * ratio;
    const i0 = Math.floor(pos);
    const i1 = Math.min(i0 + 1, input.length - 1);
    const frac = pos - i0;
    out[i] = input[i0] + (input[i1] - input[i0]) * frac;
  }
  return out;
}

/** Root mean square of 16-bit samples, 0 to 1. */
export function rmsInt16(pcm: Int16Array): number {
  if (pcm.length === 0) return 0;
  let sum = 0;
  for (let i = 0; i < pcm.length; i++) {
    const v = pcm[i] / 32768;
    sum += v * v;
  }
  return Math.sqrt(sum / pcm.length);
}

/** Little-endian bytes of 16-bit samples, the shape `voice_transcribe_cmd` takes. */
export function int16ToBytes(chunks: Int16Array[]): Uint8Array {
  const total = chunks.reduce((n, c) => n + c.length, 0);
  const out = new Uint8Array(total * 2);
  const view = new DataView(out.buffer);
  let offset = 0;
  for (const chunk of chunks) {
    for (let i = 0; i < chunk.length; i++, offset += 2) view.setInt16(offset, chunk[i], true);
  }
  return out;
}

/** 16-bit mono WAV bytes decoded to floats (header read for rate and length). */
export function decodeWav(bytes: ArrayBuffer): { samples: Float32Array; sampleRate: number } {
  const view = new DataView(bytes);
  const sampleRate = view.getUint32(24, true);
  const dataLength = view.getUint32(40, true);
  const count = Math.min(dataLength, bytes.byteLength - 44) >> 1;
  const samples = new Float32Array(count);
  for (let i = 0; i < count; i++) samples[i] = view.getInt16(44 + i * 2, true) / 32768;
  return { samples, sampleRate };
}
