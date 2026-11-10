import type { PlanItem } from "./api";

/**
 * `WRK-8`/`WRK-9`: what the organization is doing, read from real events and put
 * into words. The header mark and the strip above a running answer both draw from
 * this, so the picture can never claim something that is not happening
 * (`ORG_LOOP_PLAN` IX.6): every shape is also a sentence here.
 */
export interface OrgFacts {
  /** The preset's name while a run works under one other than Ad hoc. */
  preset: string | null;
  live: boolean;
  /** Helpers that were started and have not finished. */
  helpers: number;
  /** Someone who never saw the work is looking at an item. */
  looking: boolean;
  branching: boolean;
  stopped: boolean;
}

/** The pieces of the mark's tooltip, in the order they read: "working the Lean way
 * · 2 helpers out · one thing is being checked". Empty when there is nothing to say
 * beyond the mark's own state. */
export function orgWords(f: OrgFacts): string[] {
  if (!f.live) return [];
  const out: string[] = [];
  if (f.preset) out.push(`working the ${f.preset} way`);
  if (f.helpers > 0) out.push(f.helpers === 1 ? "a helper is out" : `${f.helpers} helpers out`);
  if (f.branching) out.push("two ways are being tried");
  if (f.looking) out.push("one thing is being checked");
  if (f.stopped) out.push("the line is stopped");
  return out;
}

/** One item as a dot, and the word for it: `○` waiting, `◐` in hand, `◔` being
 * checked, `●` done, `↺` sent back, `×` dropped. Shape, never colour. */
export function itemDot(item: PlanItem): { glyph: string; word: string } {
  if (item.review?.state === "checking") return { glyph: "◔", word: "being checked" };
  if (item.review?.state === "sent_back" && item.status === "doing") return { glyph: "↺", word: "sent back" };
  switch (item.status) {
    case "done":
      return { glyph: "●", word: "done" };
    case "doing":
      return { glyph: "◐", word: "in hand" };
    case "dropped":
      return { glyph: "×", word: "dropped" };
    default:
      return { glyph: "○", word: "waiting" };
  }
}
