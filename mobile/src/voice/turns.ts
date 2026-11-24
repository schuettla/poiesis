// What was actually heard of each spoken reply (VTN-6, VXP-5). A reply is saved
// as the pieces that finished playing, not as everything the model wrote.

interface Piece {
  text: string;
  notice: boolean;
  played: boolean;
}

interface Reply {
  pieces: Map<number, Piece>;
  /** The user cut it off, or voice was closed while it was spoken. */
  interrupted: boolean;
}

export class SpokenTurns {
  private last = 0;
  private replies = new Map<number, Reply>();

  /** Starts a reply. Ids rise, so a late piece of an old reply is easy to tell. */
  begin(): number {
    this.last += 1;
    this.replies.set(this.last, { pieces: new Map(), interrupted: false });
    return this.last;
  }

  /** The reply most recently started, or 0. */
  get current(): number {
    return this.last;
  }

  /** A piece was made. */
  add(generation: number, seq: number, text: string, notice: boolean): void {
    this.replies.get(generation)?.pieces.set(seq, { text, notice, played: false });
  }

  /** A piece finished playing. */
  played(generation: number, seq: number): void {
    const piece = this.replies.get(generation)?.pieces.get(seq);
    if (piece) piece.played = true;
  }

  interrupt(generation: number): void {
    const reply = this.replies.get(generation);
    if (reply) reply.interrupted = true;
  }

  wasInterrupted(generation: number): boolean {
    return this.replies.get(generation)?.interrupted ?? false;
  }

  /** Pieces made and not yet played, notices included: playback is not over
   * while this is above zero. */
  pending(generation: number): number {
    let n = 0;
    for (const p of this.replies.get(generation)?.pieces.values() ?? []) if (!p.played) n += 1;
    return n;
  }

  /** The words the user heard, in order. A spoken notice is not part of the reply. */
  heard(generation: number): string {
    const reply = this.replies.get(generation);
    if (!reply) return "";
    return [...reply.pieces.entries()]
      .sort((a, b) => a[0] - b[0])
      .filter(([, p]) => p.played && !p.notice)
      .map(([, p]) => p.text)
      .join(" ");
  }

  /** What is being said now: the first reply piece that has not finished playing. */
  speaking(generation: number): string {
    const reply = this.replies.get(generation);
    if (!reply) return "";
    const next = [...reply.pieces.entries()].sort((a, b) => a[0] - b[0]).find(([, p]) => !p.played && !p.notice);
    return next ? next[1].text : "";
  }

  /** Drops what is no longer needed. */
  forget(generation: number): void {
    this.replies.delete(generation);
  }
}
