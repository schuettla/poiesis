// Short spoken lines while a tool runs (VTN-5), so a pause is not silence. Built
// from the same step verbs the orb reads (`orbForStep`), never from the model.

import { orbForStep } from "../../components/Orb/orbState";

/** Say nothing if speech happened this recently. */
export const QUIET_BEFORE_NOTICE_MS = 1500;
/** At most one notice per this long. */
export const NOTICE_EVERY_MS = 10_000;

const LINES: Record<string, { en: string; de: string }> = {
  searching: { en: "Let me look that up.", de: "Ich schau kurz nach." },
  solving: { en: "Let me work that out.", de: "Ich rechne das kurz durch." },
  composing: { en: "I'll write that down.", de: "Ich schreibe das auf." },
};

/** The line for a step, or null when that kind of work needs none. `language`
 * is a code such as `de`; anything but German is read as English. */
export function noticeFor(verb: string, language: string | null | undefined): string | null {
  const line = LINES[orbForStep(verb)];
  if (!line) return null;
  return language?.toLowerCase().startsWith("de") ? line.de : line.en;
}

/** Decides whether a notice is due. Pure, so the timing is tested with numbers. */
export class NoticePacer {
  private lastNotice = -Infinity;

  /** `lastSpeechAt` is when anything last was said; `now` is the clock. */
  due(now: number, lastSpeechAt: number): boolean {
    return now - lastSpeechAt >= QUIET_BEFORE_NOTICE_MS && now - this.lastNotice >= NOTICE_EVERY_MS;
  }

  said(now: number): void {
    this.lastNotice = now;
  }
}
