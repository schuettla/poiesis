import { useEffect, useRef, useState } from "react";
import * as api from "../../lib/api";
import type { ComposerHint } from "../../lib/api";

/** `SEE-3`: how long typing has to pause before the message is read, and how long a
 * different answer has to hold before it replaces the one on screen. */
export const HINT_DEBOUNCE_MS = 400;
/** The fewest characters worth reading. */
export const HINT_MIN_CHARS = 12;

/** Long enough, and not a `/` command: a command is run, not read. */
export function hintAskable(text: string): boolean {
  const t = text.trim();
  return [...t].length >= HINT_MIN_CHARS && !t.startsWith("/");
}

const keyOf = (h: ComposerHint | null) => (h ? `${h.kind}|${h.text}` : "");

/**
 * `SEE-3`: what the composer says about the message being typed. At most one hint.
 *
 * - Asked 400 ms after the last keystroke, for text that `hintAskable` allows, and only
 *   when `on` (the fast-and-slow experiment is on and the person is writing a chat message).
 * - An answer that is the same as the one on screen leaves it alone, so edits that do not
 *   flip the answer do not flicker it.
 * - A different answer replaces it only after holding for a second pause; typing again
 *   drops it, and the next answer takes its place.
 * - An answer from before the latest keystroke is ignored.
 * - A hint that was put away, or taken up, does not come back for the same message.
 */
export function useComposerHint({
  text,
  convId,
  runId,
  on,
}: {
  text: string;
  convId: string | null;
  /** The run that is working in this chat, if any: the message would go to it. */
  runId?: string;
  on: boolean;
}) {
  const [hint, setHint] = useState<ComposerHint | null>(null);
  const shown = useRef<ComposerHint | null>(null);
  const latest = useRef(0);
  const settle = useRef<ReturnType<typeof setTimeout>>();
  const away = useRef(new Set<string>());

  const show = (h: ComposerHint | null) => {
    shown.current = h;
    setHint(h);
  };

  useEffect(() => {
    const mine = ++latest.current;
    clearTimeout(settle.current);
    if (!on || !convId || !hintAskable(text)) {
      away.current.clear();
      show(null);
      return;
    }
    const timer = setTimeout(async () => {
      let answer: ComposerHint | null = null;
      try {
        answer = await api.composerHint(convId, text.trim(), runId);
      } catch {
        answer = null;
      }
      if (latest.current !== mine) return;
      if (answer && away.current.has(keyOf(answer))) answer = null;
      if (keyOf(answer) === keyOf(shown.current)) {
        // The same line; only the decision behind it is new.
        if (answer) show(answer);
        return;
      }
      settle.current = setTimeout(() => {
        if (latest.current === mine) show(answer);
      }, HINT_DEBOUNCE_MS);
    }, HINT_DEBOUNCE_MS);
    return () => clearTimeout(timer);
    // `show` only sets state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text, convId, runId, on]);

  useEffect(() => () => clearTimeout(settle.current), []);

  const leave = (taken: boolean): ComposerHint | null => {
    const h = shown.current;
    if (!h) return null;
    away.current.add(keyOf(h));
    clearTimeout(settle.current);
    show(null);
    api.composerHintLabel(h.decision, h.kind, taken).catch(() => {});
    return h;
  };

  return {
    hint,
    /** The action was taken: the line goes, the decision is labelled, and the hint comes back
     * so the caller can do what it says. */
    take: () => leave(true),
    /** Put away without doing anything. */
    dismiss: () => void leave(false),
  };
}
