import { useEffect, useRef, useState } from "react";
import * as api from "../../lib/api";
import type { LastTime } from "../../lib/api";
import { hintAskable } from "./useComposerHint";

/** `EPI-UI-1`: how long typing has to pause before the message is read. Longer than the
 * other hints: this one embeds the message, and the first words rarely say what it is. */
export const LAST_TIME_DEBOUNCE_MS = 800;
/** How many times one conversation may ask. A hint that was not there after three
 * pauses is not coming, and each ask costs an embedding. */
export const LAST_TIME_MAX_ASKS = 3;

interface Ledger {
  asks: number;
  /** Taken up or put away: not offered again in this conversation. */
  closed: boolean;
}

/**
 * `EPI-UI-1`: "Last time", for the first message of a new conversation.
 *
 * - Asked after a pause, for text `hintAskable` allows, only while the conversation has
 *   no messages and `on` (a chat message is being written).
 * - At most `LAST_TIME_MAX_ASKS` times a conversation, and not again once taken up or
 *   put away.
 * - An answer from before the latest keystroke is ignored.
 * - An offer stays while the text stays askable; clearing the box drops it.
 */
export function useLastTimeHint({
  text,
  convId,
  fresh,
  on,
}: {
  text: string;
  convId: string | null;
  /** The conversation has no messages yet. */
  fresh: boolean;
  on: boolean;
}) {
  // The offer remembers which conversation it was found for, so it cannot follow the
  // person into another one.
  const [found, setFound] = useState<{ convId: string; offer: LastTime } | null>(null);
  const offer = found && found.convId === convId ? found.offer : null;
  const latest = useRef(0);
  const ledgers = useRef(new Map<string, Ledger>());

  useEffect(() => {
    const mine = ++latest.current;
    const ledger = convId ? ledgers.current.get(convId) ?? { asks: 0, closed: false } : null;
    if (!on || !fresh || !convId || !ledger || ledger.closed || !hintAskable(text)) {
      setFound(null);
      return;
    }
    // Already offered: the text moved on but the offer is still the answer.
    if (offer) return;
    if (ledger.asks >= LAST_TIME_MAX_ASKS) return;
    const timer = setTimeout(async () => {
      ledger.asks += 1;
      ledgers.current.set(convId, ledger);
      let answer: LastTime | null = null;
      try {
        answer = await api.lastTime(convId, text.trim());
      } catch {
        answer = null;
      }
      if (latest.current === mine && !ledger.closed) setFound(answer ? { convId, offer: answer } : null);
    }, LAST_TIME_DEBOUNCE_MS);
    return () => clearTimeout(timer);
    // `offer` is read to avoid asking again; it must not restart the wait.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text, convId, fresh, on]);

  const close = (): LastTime | null => {
    const taken = offer;
    if (convId) {
      const ledger = ledgers.current.get(convId) ?? { asks: 0, closed: false };
      ledger.closed = true;
      ledgers.current.set(convId, ledger);
    }
    setFound(null);
    return taken;
  };

  return {
    offer,
    /** "Do it the same way": the line goes and the offer comes back for the caller to use. */
    take: close,
    /** "Start fresh": the line goes and this conversation is not asked again. */
    dismiss: () => void close(),
  };
}
