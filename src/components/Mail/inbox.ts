import type { InboxLastLook, InboxRow } from "../../lib/api";
import { says } from "../../lib/decisions";

/** `USE-UI-4`: what the Inbox shows, kept apart from the component so the words
 * and the cuts can be tested without drawing anything. */

export type InboxFilter = "all" | "reply" | "urgent";

/** Urgency 3 and 4 count as urgent. */
export const URGENT_FROM = 3;

export const wantsReply = (r: InboxRow) => r.reply !== null;
export const isUrgent = (r: InboxRow) => (r.urgency ?? 0) >= URGENT_FROM;

export function filterRows(rows: InboxRow[], filter: InboxFilter): InboxRow[] {
  if (filter === "reply") return rows.filter(wantsReply);
  if (filter === "urgent") return rows.filter(isUrgent);
  return rows;
}

export function countRows(rows: InboxRow[]): Record<InboxFilter, number> {
  return { all: rows.length, reply: rows.filter(wantsReply).length, urgent: rows.filter(isUrgent).length };
}

/** Each level as it reads when sure, then when only probable. */
const URGENCY_WORDS: Record<number, [string, string]> = {
  1: ["Looks not urgent", "Probably not urgent"],
  2: ["Looks low in urgency", "Probably low in urgency"],
  3: ["Looks like it needs attention soon", "Probably needs attention soon"],
  4: ["Looks urgent", "Probably urgent"],
};

/** `SEE-4`: a judgment in words. Sure is said plainly, middling with "probably", and a
 * number that is not sure enough to have acted says nothing. A kind of mail has no
 * fitted number to this screen, so the stricter bar is the one used. */
function judged(p: number | null, plain: string, hedged: string): string | null {
  return p === null ? plain : says(p, false, plain, hedged);
}

/** The hover text of the urgency bar: "Looks urgent". Null when no urgency was
 * judged, and then there is no bar. */
export function urgencyHint(r: InboxRow): string | null {
  if (r.urgency === null) return null;
  const [plain, hedged] = URGENCY_WORDS[r.urgency] ?? URGENCY_WORDS[4];
  return judged(r.urgency_p, plain, hedged) ?? hedged;
}

export const replyHint = (r: InboxRow) => judged(r.reply, "Looks like it wants a reply", "Probably wants a reply") ?? "Probably wants a reply";

/** The hover text of the kind tag. A kind the person set says so. */
export function kindHint(r: InboxRow): string {
  if (r.retagged) return "Set by you. Click to change it.";
  const kind = r.category ?? "unsure";
  const said = judged(r.category_p, `Looks like ${kind}`, `Probably ${kind}`) ?? `Probably ${kind}`;
  return `${said}. Click to change it.`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const pad = (n: number) => String(n).padStart(2, "0");

/** "08:52" for today, "23 Oct" for earlier this year, "23 Oct 2025" before that.
 * Written by hand so the same message reads the same in every locale. */
export function whenText(ms: number | null, now: number): string {
  if (ms === null) return "";
  const d = new Date(ms);
  const n = new Date(now);
  const sameDay = d.getFullYear() === n.getFullYear() && d.getMonth() === n.getMonth() && d.getDate() === n.getDate();
  if (sameDay) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  const day = `${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return d.getFullYear() === n.getFullYear() ? day : `${day} ${d.getFullYear()}`;
}

/** The line under the title: when I last looked, how often, and where the text goes. */
export function statusLine(last: InboxLastLook | null, everyMinutes: number, now: number): string {
  const fetched = last ? `Fetched ${whenText(last.at, now)}` : "Not fetched yet";
  return `${fetched} · every ${everyMinutes} minutes · nothing leaves this computer`;
}
