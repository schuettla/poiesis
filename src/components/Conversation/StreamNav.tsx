import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { Message } from "../../lib/types";
import { ChevronIcon } from "../Icons/Icons";

/** `UI-5`: the way back in a long chat, as omher has it (its `TimelineJump`, written again here).
 * A "Latest" button when the view is more than one screen above the bottom, with the number of
 * messages that came in since the person scrolled up; and one line with the question the answer
 * under the view belongs to, while that question has scrolled off the top. Both hang on
 * zero-height sticky anchors, so they take no room and never move a message. */

/** Within this many pixels of the bottom the stream follows what is written. */
const FOLLOW_PX = 80;

const reduced = () => typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/** The question line says the person's own words, flat. */
function asWords(text: string): string {
  const cut = text.indexOf("```poiesis-action");
  return (cut === -1 ? text : text.slice(0, cut)).replace(/\s+/g, " ").trim();
}

export interface StreamNav {
  /** More than one screen above the bottom. */
  away: boolean;
  /** Messages that arrived since the person scrolled away. */
  fresh: number;
  /** The newest question while its card is above the view, else null. */
  asked: { id: string; text: string } | null;
  /** True while the stream follows the end of the answer. */
  follow: { current: boolean };
  measure: () => void;
  jump: () => void;
  showAsked: () => void;
  /** The scroll box's own events: the person, not the page, decides when to let go. */
  handlers: {
    onScroll: () => void;
    onWheel: (e: { deltaY: number }) => void;
    onTouchMove: () => void;
    onPointerDown: (e: { target: EventTarget; currentTarget: EventTarget }) => void;
    onKeyDown: (e: { key: string }) => void;
  };
}

export function useStreamNav(ref: RefObject<HTMLDivElement | null>, messages: Message[], convId: string | undefined): StreamNav {
  const [away, setAway] = useState(false);
  const [asked, setAsked] = useState<{ id: string; text: string } | null>(null);
  // How many messages there were when the person let go of the bottom.
  const [since, setSince] = useState<number | null>(null);
  const follow = useRef(true);
  const count = useRef(messages.length);
  count.current = messages.length;
  const newest = [...messages].reverse().find((m) => m.role === "user");
  const askedId = newest?.id ?? null;
  const askedText = newest ? asWords(newest.text) : "";

  const measure = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const left = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (left <= FOLLOW_PX) follow.current = true;
    const nowAway = left > el.clientHeight;
    setAway(nowAway);
    setSince((s) => (nowAway ? (s ?? count.current) : null));
    let next: { id: string; text: string } | null = null;
    if (askedId && askedText) {
      const card = el.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(askedId)}"]`);
      // Only while its answer is what is on screen: the card is wholly above the top edge.
      if (card && card.getBoundingClientRect().bottom <= el.getBoundingClientRect().top + 4) {
        next = { id: askedId, text: askedText };
      }
    }
    setAsked((a) => (a?.id === next?.id && a?.text === next?.text ? a : next));
  }, [ref, askedId, askedText]);

  // Another chat: start over.
  useEffect(() => {
    follow.current = true;
    setAway(false);
    setSince(null);
    setAsked(null);
  }, [convId]);

  // New messages or new words can move the question card or the bottom.
  useEffect(() => measure(), [measure, messages.length]);

  const jump = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    follow.current = true;
    setSince(null);
    setAway(false);
    el.scrollTo?.({ top: el.scrollHeight, behavior: reduced() ? "auto" : "smooth" });
  }, [ref]);

  const showAsked = useCallback(() => {
    if (!asked) return;
    ref.current
      ?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(asked.id)}"]`)
      ?.scrollIntoView?.({ block: "start", behavior: reduced() ? "auto" : "smooth" });
  }, [ref, asked]);

  const letGo = () => {
    follow.current = false;
  };
  const handlers: StreamNav["handlers"] = {
    onScroll: measure,
    // Only the person's own scrolling lets go. A smooth scroll the page started passes
    // through places far from the bottom, and must not count as letting go.
    onWheel: (e) => {
      if (e.deltaY < 0) letGo();
    },
    onTouchMove: letGo,
    // A press on the scroll box itself is the scrollbar; a press inside is a click.
    onPointerDown: (e) => {
      if (e.target === e.currentTarget) letGo();
    },
    onKeyDown: (e) => {
      if (e.key === "PageUp" || e.key === "ArrowUp" || e.key === "Home") letGo();
    },
  };

  return {
    away,
    fresh: since === null ? 0 : Math.max(0, messages.length - since),
    asked,
    follow,
    measure,
    jump,
    showAsked,
    handlers,
  };
}

/** The anchor at the end of the stream, with the button on it. */
export function LatestButton({ nav }: { nav: StreamNav }) {
  return (
    <div className="stream-anchor bottom">
      {nav.away && (
        <button className="stream-latest" onClick={nav.jump}>
          <ChevronIcon dir="down" size={12} strokeWidth={1.6} />
          {nav.fresh > 0 ? `Latest · ${nav.fresh} new` : "Latest"}
        </button>
      )}
    </div>
  );
}

/** The anchor at the start of the stream, with the question line on it. */
export function AskedLine({ nav }: { nav: StreamNav }) {
  return (
    <div className="stream-anchor top">
      {nav.asked && (
        <button className="stream-asked" onClick={nav.showAsked} title={nav.asked.text}>
          <span className="stream-asked-label">You asked: </span>
          {nav.asked.text}
        </button>
      )}
    </div>
  );
}
