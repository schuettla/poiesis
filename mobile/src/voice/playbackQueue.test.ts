import { describe, expect, it } from "vitest";
import { PlaybackQueue, type PlaybackEvent } from "./playbackQueue";

const RATE = 1000; // small rate keeps the arithmetic readable

function ramp(n: number, v = 0.5): Float32Array {
  return new Float32Array(n).fill(v);
}

function run(q: PlaybackQueue, samples: number): { out: Float32Array; events: PlaybackEvent[] } {
  const out = new Float32Array(samples);
  const events: PlaybackEvent[] = [];
  for (let i = 0; i < samples; i += 100) {
    const block = out.subarray(i, Math.min(i + 100, samples));
    events.push(...q.process(block));
  }
  return { out, events };
}

const kinds = (events: PlaybackEvent[]) => events.filter((e) => e.type !== "level");

describe("PlaybackQueue (AUD-3)", () => {
  it("plays chunks in order, reports each as played, then goes idle once", () => {
    const q = new PlaybackQueue(RATE);
    q.enqueue(1, 0, ramp(150, 0.2));
    q.enqueue(1, 1, ramp(150, 0.4));
    const { out, events } = run(q, 400);
    expect(out[0]).toBeCloseTo(0.2);
    expect(out[200]).toBeCloseTo(0.4);
    expect(out[350]).toBe(0);
    expect(kinds(events)).toEqual([
      { type: "played", generationId: 1, seq: 0 },
      { type: "played", generationId: 1, seq: 1 },
      { type: "idle" },
    ]);
  });

  it("is silent and quiet when nothing is queued", () => {
    const q = new PlaybackQueue(RATE);
    const { out, events } = run(q, 300);
    expect(out.every((v) => v === 0)).toBe(true);
    expect(events).toEqual([]);
  });

  it("cancel fades the playing chunk out and never reports it as played", () => {
    const q = new PlaybackQueue(RATE, 30); // 30 samples of fade
    q.enqueue(1, 0, ramp(500));
    q.enqueue(1, 1, ramp(500));
    run(q, 100);
    q.cancel(1);
    const { out, events } = run(q, 200);
    expect(out[0]).toBeGreaterThan(0.4); // starts near full volume
    expect(out[28]).toBeLessThan(0.05); // nearly gone at the end of the fade
    expect(out[40]).toBe(0);
    expect(kinds(events)).toEqual([{ type: "idle" }]);
  });

  it("drops queued chunks of a cancelled reply and ignores late ones", () => {
    const q = new PlaybackQueue(RATE);
    q.enqueue(1, 0, ramp(100));
    q.enqueue(2, 0, ramp(100, 0.9));
    q.cancel(1);
    q.enqueue(1, 5, ramp(100)); // arrives after the cut
    const { events } = run(q, 400);
    const played = kinds(events).filter((e) => e.type === "played");
    expect(played).toEqual([{ type: "played", generationId: 2, seq: 0 }]);
  });

  it("lets the next reply play after the cancelled one fades", () => {
    const q = new PlaybackQueue(RATE, 20);
    q.enqueue(1, 0, ramp(300));
    q.cancel(1);
    q.enqueue(2, 0, ramp(100, 0.8));
    const { out } = run(q, 300);
    expect(out[19]).toBeLessThan(0.1);
    expect(out[40]).toBeCloseTo(0.8);
  });

  it("ducks to a lower volume over the given time and comes back", () => {
    const q = new PlaybackQueue(RATE);
    q.enqueue(1, 0, ramp(900, 1));
    q.setDuck(0.18, 0.02); // 20 samples
    const { out } = run(q, 300);
    expect(out[0]).toBeGreaterThan(0.9);
    expect(out[30]).toBeCloseTo(0.18, 2);
    q.setDuck(1, 0.02);
    const after = run(q, 100).out;
    expect(after[40]).toBeCloseTo(1, 2);
  });

  it("reports an output level about every 50 ms", () => {
    const q = new PlaybackQueue(RATE);
    q.enqueue(1, 0, ramp(200, 0.5));
    const { events } = run(q, 200);
    const levels = events.filter((e) => e.type === "level");
    expect(levels).toHaveLength(4);
    expect((levels[0] as { rms: number }).rms).toBeCloseTo(0.5);
  });
});
