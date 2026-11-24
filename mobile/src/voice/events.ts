/** What a live voice session tells the screen. Mirrors `VoiceEvent` in the desktop's Rust, plus `ready`, which the
 * link adds when the desktop has opened the session. */
export type VoiceEvent =
  | { type: "floor"; state: "listening" | "user_speaking" | "thinking" | "speaking" }
  | { type: "partial"; text: string }
  | { type: "transcript"; text: string; language: string | null }
  | { type: "duck" }
  | { type: "unduck" }
  | { type: "yield" }
  | {
      type: "audio";
      generation: number;
      seq: number;
      sample_rate: number;
      /** 16-bit little-endian mono samples, base64. */
      pcm: string;
      /** What this piece says, as it was spoken. */
      text: string;
      /** A short spoken notice, not part of the reply. */
      notice: boolean;
    }
  | { type: "speech_done"; generation: number }
  | { type: "hint"; text: string }
  | { type: "error"; message: string };
