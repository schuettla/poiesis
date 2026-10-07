// The seam between the chat store and a live voice session. The store streams
// a reply and knows nothing about audio; while voice is on, it hands each piece
// of a spoken turn to whoever registered here. Kept apart from the session so
// the store imports a few lines, not the whole audio stack.

export interface SpeechOutcome {
  /** The user cut the reply off, or voice was closed while it was spoken. */
  interrupted: boolean;
  /** The words they heard. Only meaningful when `interrupted`. */
  heard: string;
}

export interface SpeechBridge {
  /** The reply being made for the turn that was just sent. */
  current(): number;
  /** Streamed reply text. */
  token(generation: number, text: string): void;
  /** A tool step began, with the verb the timeline shows. */
  step(generation: number, verb: string): void;
  /** The reply text is complete. Resolves once it has been heard or cut off,
   * so the turn is not over while Poiesis is still talking. */
  end(generation: number): Promise<SpeechOutcome>;
  /** The user pressed Stop: Poiesis stops talking at once. */
  stopSpeaking(): void;
}

let bridge: SpeechBridge | null = null;

export function setSpeechBridge(next: SpeechBridge | null): void {
  bridge = next;
}

export function getSpeechBridge(): SpeechBridge | null {
  return bridge;
}
