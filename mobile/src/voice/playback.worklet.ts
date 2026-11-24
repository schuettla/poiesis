// Runs on the audio thread (AUD-3). A thin shell around `PlaybackQueue`: the
// page sends chunks and commands, this plays them and reports what happened.

import { PlaybackQueue } from "./playbackQueue";

declare const sampleRate: number;
declare class AudioWorkletProcessor {
  readonly port: MessagePort;
}
declare function registerProcessor(name: string, processor: new () => AudioWorkletProcessor): void;

type Command =
  | { type: "enqueue"; generationId: number; seq: number; pcm: Float32Array }
  | { type: "cancel"; generationId: number }
  | { type: "duck"; gain: number; seconds: number };

class PlaybackProcessor extends AudioWorkletProcessor {
  private readonly queue = new PlaybackQueue(sampleRate);

  constructor() {
    super();
    this.port.onmessage = (e: MessageEvent<Command>) => {
      const c = e.data;
      if (c.type === "enqueue") this.queue.enqueue(c.generationId, c.seq, c.pcm);
      else if (c.type === "cancel") this.queue.cancel(c.generationId);
      else this.queue.setDuck(c.gain, c.seconds);
    };
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const channels = outputs[0];
    if (!channels?.length) return true;
    const events = this.queue.process(channels[0]);
    for (let c = 1; c < channels.length; c++) channels[c].set(channels[0]);
    for (const event of events) this.port.postMessage(event);
    return true;
  }
}

registerProcessor("poiesis-playback", PlaybackProcessor);
