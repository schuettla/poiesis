/**
 * Context homeostasis (CTX-2). What a turn sends, and the summary of what it
 * leaves out, is decided in Rust (`agent::turn`, `agent::context`); the cut that
 * `/compact` makes now has to agree with the one a turn makes on overflow.
 */

/** Rough token estimate: 1 token ≈ 4 chars. Deliberately conservative. The meter
 * under the composer uses it; what a turn actually sends is cut in Rust. */
export function estimateTokens(text: string): number {
  return Math.ceil(text.length / 4);
}

/** Recent turns that are never dropped — the thread of the live exchange. */
export const KEEP_RECENT = 6;

/** Fewer in workspace mode: the surface and session state carry the task state. */
export const KEEP_RECENT_WORKSPACE = 3;
