import { useEffect, useState, type ReactNode } from "react";
import type { ChatState } from "../../lib/chatState";
import { clockText } from "../../lib/liveWords";
import Ticker from "../Ticker/Ticker";
import { ArcIcon, CheckIcon, CloseIcon, DotIcon, PauseIcon } from "../Icons/Icons";

/** `UI-1`: the words for each state's glyph. One glyph per state, and ink for all but a failure. */
const WORDS: Record<Exclude<ChatState, "idle">, string> = {
  running: "Working",
  "needs-you": "Waiting for you",
  held: "Waiting on something else",
  failed: "Stopped with an error",
  "done-unseen": "Finished while you were away",
};

export function ChatStateMark({ state }: { state: ChatState }) {
  if (state === "idle") return null;
  const glyph =
    state === "running" ? (
      <ArcIcon size={12} />
    ) : state === "needs-you" ? (
      <DotIcon size={12} />
    ) : state === "held" ? (
      <PauseIcon size={12} />
    ) : state === "failed" ? (
      <CloseIcon size={12} strokeWidth={1.6} />
    ) : (
      <CheckIcon size={12} />
    );
  return (
    <span className="chat-state-glyph" role="img" aria-label={WORDS[state]} title={WORDS[state]}>
      {glyph}
    </span>
  );
}

/** The 14px slot after a row's title, there only while the chat has a state or helpers.
 * `twisty` is the chevron of a chat that has helpers (`UI-3`): it fills the slot while the chat
 * is idle, and takes the glyph's place on hover or focus otherwise (`Rail.css`). */
export function ChatSlot({ state, twisty }: { state: ChatState; twisty?: ReactNode }) {
  return (
    <span className={`chat-state ${state}${twisty ? " has-twisty" : ""}`}>
      <ChatStateMark state={state} />
      {twisty}
    </span>
  );
}

const GROW_MS = 360;

/** One timer for every clock in the list, running only while one is shown. */
const clocks = new Set<() => void>();
let clockTimer: ReturnType<typeof setInterval> | undefined;
function onSecond(listener: () => void): () => void {
  clocks.add(listener);
  clockTimer ??= setInterval(() => clocks.forEach((tick) => tick()), 1000);
  return () => {
    clocks.delete(listener);
    if (clocks.size) return;
    clearInterval(clockTimer);
    clockTimer = undefined;
  };
}

/** `PAR-9`: how long a working chat has worked. */
function RunClock({ since }: { since: number }) {
  const [, setTick] = useState(0);
  useEffect(() => onSecond(() => setTick((t) => t + 1)), []);
  return <span className="chat-now-time">{clockText(Date.now() - since)}</span>;
}

/** `UI-2`: the second line of a chat that works or waits. The row grows when it appears and
 * shrinks when it goes; its words change in place (`Ticker`). Under reduced motion all of it
 * is instant (the global rule). `live` is for a chat that works: the moving light passes
 * through its words. A question waiting for you stands still. `since` ends the line with
 * the time the run has taken (`PAR-9`). */
export function ChatNow({
  text,
  indent = 20,
  live = false,
  since = null,
}: {
  text: string | null;
  indent?: number;
  live?: boolean;
  since?: number | null;
}) {
  // The last words that were shown: what a closing line still says while it shrinks.
  const [held, setHeld] = useState<string | null>(text);

  useEffect(() => {
    if (text !== null) {
      setHeld(text);
      return;
    }
    if (held === null) return;
    const t = setTimeout(() => setHeld(null), GROW_MS);
    return () => clearTimeout(t);
    // `held` is state this effect sets; it must not re-run the effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text]);

  const shown = text ?? held;
  if (shown === null) return null;
  return (
    <span className={`chat-now ${text === null ? "closing" : ""}`} style={{ paddingLeft: indent }}>
      <span className="chat-now-clip">
        <span className={`chat-now-line${since !== null && text !== null ? " timed" : ""}`}>
          <Ticker text={shown} live={live && text !== null} />
          {since !== null && text !== null && <RunClock since={since} />}
        </span>
      </span>
    </span>
  );
}
