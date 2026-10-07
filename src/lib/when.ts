import type { Cadence } from "./api";

/**
 * `UCM-8`: the schedule field of a task, filled in from words.
 *
 * `/schedule every weekday at 9 summarise my mail` is a time and a task in one
 * line. This reads the time off the front and leaves the rest as the task.
 *
 * The scheduler only knows four rhythms (every hour, every 6 hours, daily,
 * weekly) and counts them from the first run. It has no time of day and no
 * weekdays. So this never claims more than that: a phrase it can only roughly
 * honour comes back with a `note` saying what is different, and one it cannot
 * honour at all comes back with no cadence, which leaves the field empty for the
 * user to choose. Nothing here saves anything.
 */
export interface When {
  /** The rhythm the scheduler can run, or `null` when none of them is right. */
  cadence: Cadence | null;
  /** What differs from what was asked, in my own words. `null` when nothing does. */
  note: string | null;
  /** Everything after the time: the task. */
  rest: string;
}

const DAYS = "monday|tuesday|wednesday|thursday|friday|saturday|sunday";
const TIME = String.raw`(?:\s+(?:at|@)\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?)?`;
const FILLER = /^[\s,:;.-]*(?:to\s+)?/i;

const SHORTEST = "I can only run every hour, every 6 hours, daily or weekly.";

/** A clock time that is a real one, or `null`. */
function realTime(h: string | undefined, m: string | undefined, ap: string | undefined): boolean {
  if (h === undefined) return false;
  let hour = Number(h);
  const minute = m === undefined ? 0 : Number(m);
  if (ap) {
    if (hour < 1 || hour > 12) return false;
    hour = hour % 12;
  }
  return hour >= 0 && hour <= 23 && minute >= 0 && minute <= 59;
}

/** Read a time off the front of `text`, or `null` when it does not start with one. */
export function parseWhen(text: string): When | null {
  const t = text.trim();
  if (!t) return null;

  // Every N minutes or hours.
  const every = new RegExp(String.raw`^every\s+(\d+)\s*(minutes?|mins?|hours?|hrs?)\b`, "i").exec(t);
  if (every) {
    const n = Number(every[1]);
    const hours = /^h/i.test(every[2]) ? n : n / 60;
    const rest = t.slice(every[0].length).replace(FILLER, "").trim();
    if (hours === 1) return { cadence: "hourly", note: null, rest };
    if (hours === 6) return { cadence: "six-hourly", note: null, rest };
    if (hours === 24) return { cadence: "daily", note: null, rest };
    return { cadence: null, note: SHORTEST, rest };
  }

  // Once, tomorrow.
  const once = new RegExp(String.raw`^tomorrow${TIME}\b`, "i").exec(t);
  if (once) {
    const rest = t.slice(once[0].length).replace(FILLER, "").trim();
    return { cadence: null, note: "I can't run something just once yet. Pick how often.", rest };
  }

  // Hourly, daily, weekly, nightly, weekdays, a named day: each with an optional time.
  const phrase = new RegExp(
    String.raw`^(?:(hourly|every\s+hour)|(daily|nightly|every\s*day|every\s+night)|(weekly|every\s+week)|(every\s+weekday|weekdays|on\s+weekdays)|(?:every|on)\s+(${DAYS})s?)${TIME}\b`,
    "i"
  ).exec(t);
  if (!phrase) return null;

  const [whole, hourly, , weekly, weekdays, named, h, m, ap] = phrase;
  const rest = t.slice(whole.length).replace(FILLER, "").trim();
  const notes: string[] = [];

  let cadence: Cadence;
  if (hourly) {
    cadence = "hourly";
  } else if (weekly || named) {
    cadence = "weekly";
    if (named) notes.push("I can't pick the day yet, so I'll run it every 7 days from the first run.");
  } else {
    cadence = "daily";
    if (weekdays) notes.push("I can't skip weekends yet, so I'll run it every day.");
  }
  if (h !== undefined) {
    // Every time that was typed is answered for, never dropped without a word.
    if (!realTime(h, m, ap)) notes.push("I couldn't read that time, so I left it out.");
    else if (hourly) notes.push("An hourly task has no set time, so I left the time out.");
    else notes.push("I can't run at a set time yet, so I'll count from the first run.");
  }
  return { cadence, note: notes.length ? notes.join(" ") : null, rest };
}

/** What the Tasks editor opens with: filled in, and saved only when the user says so. */
export interface TaskDraft {
  name: string;
  prompt: string;
  conversationId: string;
  /** The rhythm to preselect. `undefined` leaves the editor's own default;
   * `null` leaves the field empty for the user to choose. */
  cadence?: Cadence | null;
  /** Where my reading of the time differs from what was asked. */
  whenNote?: string;
}

/** `/schedule [when] [task]`: split the words into a time and a task. */
export function scheduleDraft(name: string, conversationId: string, words: string): TaskDraft {
  const when = parseWhen(words);
  if (!when) return { name, prompt: words.trim(), conversationId, cadence: null };
  return {
    name,
    prompt: when.rest,
    conversationId,
    cadence: when.cadence,
    ...(when.note ? { whenNote: when.note } : {}),
  };
}

/** A schedule I proposed myself: the time and the task arrive separately. */
export function proposedDraft(name: string, conversationId: string, when: string | undefined, task: string | undefined): TaskDraft {
  const prompt = (task ?? "").trim();
  const said = (when ?? "").trim();
  if (!said) return { name, prompt, conversationId, cadence: null };
  const parsed = parseWhen(said);
  if (!parsed || parsed.cadence === null) {
    // What I suggested is not something the scheduler can run as it is said, so
    // the field stays empty and the words stay on screen.
    const note = parsed?.note ? ` ${parsed.note}` : "";
    return { name, prompt, conversationId, cadence: null, whenNote: `I suggested “${said}”.${note}` };
  }
  return {
    name,
    prompt,
    conversationId,
    cadence: parsed.cadence,
    ...(parsed.note ? { whenNote: parsed.note } : {}),
  };
}
