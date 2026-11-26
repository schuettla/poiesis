// Ported from oh-my-hermes (MIT), apps/omher/src/chat/parts/promptHistory.ts.
// ArrowUp/ArrowDown in the composer walk the person's earlier prompts of the open chat, like a
// shell's history. Only the box changes: the history is the chat's own messages, never edited here.
import type { Message } from "../../lib/types";
import { splitQuote } from "../../lib/mdQuote";

/** What the box shows while walking: `at` indexes the prompt shown (oldest first), `draft` is what was typed
 * before the first ArrowUp. `null` = the person's own text, not a recalled one. */
export type Recall = { at: number; draft: string } | null;
export type Step = { recall: Recall; text: string };

/** The person's prompts of a chat, oldest first, as they typed them: a `/skill` as `/name args`, not
 * the words it was sent as; a passage quoted from a document is a chip in the box, so it is left out.
 * A prompt sent twice in a row is one entry, as in a shell. */
export function promptsOf(messages: readonly Message[] | undefined): string[] {
  const out: string[] = [];
  for (const m of messages ?? []) {
    if (m.role !== "user") continue;
    // A block's or surface's action is a sentence the person never typed.
    if (m.text.includes("```poiesis-action")) continue;
    // A message held for "Stop the run?" was not sent.
    if (m.midRun === "held") continue;
    const typed = m.command ? `/${m.command.name}${m.command.args ? ` ${m.command.args}` : ""}` : splitQuote(m.text).rest;
    const text = typed.trim();
    if (!text) continue; // a files-only send has no words to recall
    if (out[out.length - 1] !== text) out.push(text);
  }
  return out;
}

/** ArrowUp: the next older prompt; null when there is none (the key then does what it always did). */
export function older(history: readonly string[], recall: Recall, draft: string): Step | null {
  // the chat can reload under a walk: never step past its end
  const at = Math.min(recall ? recall.at : history.length, history.length) - 1;
  if (at < 0) return null;
  return { recall: { at, draft: recall ? recall.draft : draft }, text: history[at] };
}

/** ArrowDown: the next newer prompt, and past the newest the draft typed before the walk began. */
export function newer(history: readonly string[], recall: Recall): Step | null {
  if (!recall) return null;
  const at = recall.at + 1;
  return at < history.length ? { recall: { at, draft: recall.draft }, text: history[at] } : { recall: null, text: recall.draft };
}
