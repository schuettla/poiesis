import type { LastTime } from "./api";
import type { TurnModifiers } from "./types";

/** `EPI-UI-1`: the words of "Last time" (Cognition plan, IV.1). */

/** 1760270400000 -> "12 Oct". */
export function shortDate(ms: number, locale?: string): string {
  return new Date(ms).toLocaleDateString(locale, { day: "numeric", month: "short" });
}

/** The line in the composer, before its two actions. */
export function lastTimeLine(offer: Pick<LastTime, "at" | "line">): string {
  return `Last time (${shortDate(offer.at)}) ${offer.line}`;
}

/** What "Do it the same way" adds to this one message, in the copy the model gets.
 * It names the conversation so the model can read the whole of it, and says nothing
 * about the user that the digest did not. */
export function lastTimeNote(taken: NonNullable<TurnModifiers["lastTime"]>): string {
  const skill = taken.skill ? ` The skill I used was "${taken.skill}".` : "";
  return (
    `\n\n[Context: last time (${shortDate(taken.at)}) we did something like this (conversation id ${taken.conversationId}). ` +
    `What I recorded about it: ${taken.digest.trim()}${skill} ` +
    `Do it the same way unless this message says otherwise. Read that conversation with read_conversation if you need more.]`
  );
}
