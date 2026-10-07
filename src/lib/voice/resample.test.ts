import { describe, expect, it } from "vitest";
import { CAPTURE_RATE, FrameBuilder, decodeWav, int16ToBytes, resampleLinear, rmsInt16 } from "./resample";

function sine(freq: number, rate: number, seconds: number, amp = 0.5): Float32Array {
  const out = new Float32Array(Math.round(rate * seconds));
  for (let i = 0; i < out.length; i++) out[i] = amp * Math.sin((2 * Math.PI * freq * i) / rate);
  return out;
}

function collect(inRate: number, input: Float32Array, block = 128): Int16Array[] {
  const frames: Int16Array[] = [];
  const b = new FrameBuilder(inRate);
  for (let i = 0; i < input.length; i += block) b.push(input.subarray(i, i + block), (f) => frames.push(f));
  return frames;
}

describe("FrameBuilder (AUD-1)", () => {
  it("makes 20 ms frames of 320 samples at 16 kHz", () => {
    const frames = collect(48_000, sine(440, 48_000, 1));
    // 1 s of audio is 50 frames; the last partial frame stays pending.
    expect(frames.length).toBeGreaterThanOrEqual(49);
    expect(frames.length).toBeLessThanOrEqual(50);
    for (const f of frames) expect(f.length).toBe(320);
  });

  it("keeps the level of a tone when it changes the rate", () => {
    const frames = collect(48_000, sine(440, 48_000, 0.5, 0.5));
    const level = rmsInt16(frames[frames.length - 1]);
    // RMS of a 0.5 amplitude sine is 0.5 / sqrt(2).
    expect(level).toBeCloseTo(0.3535, 1);
  });

  it("passes 16 kHz audio through without changing a sample", () => {
    const input = sine(300, CAPTURE_RATE, 0.11); // one sample of lag, so a little extra
    const frames = collect(CAPTURE_RATE, input);
    expect(frames).toHaveLength(5);
    const flat = Array.from(frames[0]);
    for (let i = 0; i < 20; i++) expect(flat[i]).toBe(Math.round(input[i] * 32767));
  });

  it("clamps samples that are too loud", () => {
    const frames = collect(CAPTURE_RATE, new Float32Array(400).fill(3));
    expect(frames[0][100]).toBe(32767);
  });

  it("gives each frame its own buffer so it can be transferred", () => {
    const frames = collect(44_100, sine(200, 44_100, 0.2));
    expect(new Set(frames.map((f) => f.buffer)).size).toBe(frames.length);
  });
});

describe("resampleLinear", () => {
  it("scales length by the rate ratio", () => {
    expect(resampleLinear(new Float32Array(2400), 24_000, 48_000).length).toBe(4800);
    expect(resampleLinear(new Float32Array(2205), 22_050, 48_000).length).toBe(4800);
  });
  it("returns the same buffer when the rates match", () => {
    const x = new Float32Array(10);
    expect(resampleLinear(x, 16_000, 16_000)).toBe(x);
  });
  it("interpolates between samples", () => {
    const out = resampleLinear(Float32Array.from([0, 1]), 1, 2);
    expect(Array.from(out)).toEqual([0, 0.5, 1, 1]);
  });
});

describe("byte helpers", () => {
  it("writes little-endian 16-bit samples", () => {
    const bytes = int16ToBytes([Int16Array.from([1, -2]), Int16Array.from([256])]);
    expect(Array.from(bytes)).toEqual([1, 0, 0xfe, 0xff, 0, 1]);
  });

  it("reads back a WAV written the way the backend writes it", () => {
    const pcm = Int16Array.from([0, 16384, -16384]);
    const buf = new ArrayBuffer(44 + 6);
    const v = new DataView(buf);
    v.setUint32(24, 22_050, true);
    v.setUint32(40, 6, true);
    pcm.forEach((s, i) => v.setInt16(44 + i * 2, s, true));
    const { samples, sampleRate } = decodeWav(buf);
    expect(sampleRate).toBe(22_050);
    expect(Array.from(samples)).toEqual([0, 0.5, -0.5]);
  });
});
