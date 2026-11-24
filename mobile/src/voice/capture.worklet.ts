// Runs on the audio thread (AUD-1). Turns mic blocks into 16 kHz mono 20 ms
// frames and posts each one to the page. All the math is in `resample.ts`.

import { FrameBuilder } from "./resample";

// The audio worklet scope has these globals; the DOM typings do not.
declare const sampleRate: number;
declare class AudioWorkletProcessor {
  readonly port: MessagePort;
}
declare function registerProcessor(name: string, processor: new () => AudioWorkletProcessor): void;

class CaptureProcessor extends AudioWorkletProcessor {
  private readonly frames = new FrameBuilder(sampleRate);
  private seq = 0;

  process(inputs: Float32Array[][]): boolean {
    const mono = inputs[0]?.[0];
    if (mono) {
      this.frames.push(mono, (pcm) => this.port.postMessage({ pcm, seq: this.seq++ }, [pcm.buffer]));
    }
    return true;
  }
}

registerProcessor("poiesis-capture", CaptureProcessor);
