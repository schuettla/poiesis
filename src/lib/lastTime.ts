import type { LastTime } from "./api";

/** `EPI-UI-1`: the words of "Last time" (Cognition plan, IV.1). */

/** 1760270400000 -> "12 Oct". */
export function shortDate(ms: number, locale?: string): string {
  return new Date(ms).toLocaleDateString(locale, { day: "numeric", month: "short" });
}

/** The line in the composer, before its two actions. */
export function lastTimeLine(offer: Pick<LastTime, "at" | "line">): string {
  return `Last time (${shortDate(offer.at)}) ${offer.line}`;
}
