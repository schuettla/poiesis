import type { GoalCheck, StopReason } from "./api";

/**
 * `GOL`: keep working toward something the user set, until a check says it holds
 * or the rounds run out.
 *
 * It lives in this window, like the loop that drives it: nothing runs a goal once
 * the app is closed, so there is nothing to read back. (`GOL-1` kept it in the
 * session state, which the model can write to and reads on every turn; this is
 * the user's alone.)
 *
 * The user sets it (`/goal`), and only the user can stop it. Nothing the agent
 * does can start, extend or change one: the loop below is driven by the end of
 * the lead's own turn in this window, never by an agent event. Five rounds at
 * most, so a goal that cannot be reached ends instead of running all night.
 */
export const MAX_ROUNDS = 5;

export type GoalStatus = "active" | "met" | "stopped" | "exhausted";

export interface Goal {
  /** What the user wants. */
  text: string;
  /** What has to be true for it to be done. The same words as `text` when none was given. */
  until: string;
  maxRounds: number;
  /** The round the current or last turn belongs to. The first turn is round 1. */
  round: number;
  status: GoalStatus;
  /** The last check's verdict. */
  lastCheck: GoalCheck | null;
  /** A check is running: the turn is over, and I am deciding whether to go on. */
  checking: boolean;
}

/** `/goal make the tests pass until npm test exits 0`, or `/goal stop`. */
export function parseGoalArgs(args: string): { text: string; until: string } | "stop" | null {
  const t = args.trim();
  if (!t) return null;
  if (/^(stop|cancel|clear)$/i.test(t)) return "stop";
  const at = t.search(/\s+until\s+/i);
  if (at < 0) return { text: t, until: t };
  const text = t.slice(0, at).trim();
  const until = t.slice(at).replace(/^\s+until\s+/i, "").trim();
  // "until" with nothing on one side is not a split: the whole line is the goal.
  return text && until ? { text, until } : { text: t, until: t };
}

export function newGoal(text: string, until: string): Goal {
  return { text, until, maxRounds: MAX_ROUNDS, round: 1, status: "active", lastCheck: null, checking: false };
}

/** The first turn: the goal, and when it counts as done, said as a request. */
export function openingTurn(goal: Goal): string {
  const done = goal.until === goal.text ? "" : ` I will count it as done when: ${goal.until}.`;
  return (
    `Goal: ${goal.text}.${done} ` +
    "Work toward it, and check it yourself with your tools before you say it is done."
  );
}

/** Every later turn: the goal again, and what the last check said is left. */
export function continuingTurn(goal: Goal, check: GoalCheck): string {
  return `Keep going toward our goal: ${goal.text}. Last check: ${check.next}`;
}

/** The line in the transcript before a later round starts. */
export const continuingNote = (goal: Goal) =>
  `Continuing toward our goal (round ${goal.round} of ${goal.maxRounds})`;

/** How a run ended, as the loop needs to know it. */
export interface RunEnd {
  /** Absent means it completed. */
  stopReason?: StopReason;
  /** The plan it wrote is waiting for the user's yes, so nothing was done yet. */
  awaitingApproval?: boolean;
}

/** What the loop needs from the app. Passed in, so the rules can be tried
 * without a store. */
export interface GoalHost {
  get(convId: string): Goal | undefined;
  /** The conversation is the one on screen. A turn is only ever sent into the
   * active chat, so a goal cannot carry on somewhere the user is not looking. */
  isOpen(convId: string): boolean;
  /** Change the goal and keep it. */
  patch(convId: string, patch: Partial<Goal>): void;
  check(convId: string, goal: Goal): Promise<GoalCheck>;
  /** A line in the transcript, in my voice. */
  note(convId: string, text: string): void;
  /** Send an ordinary turn. */
  send(convId: string, text: string): Promise<void>;
}

/**
 * The run behind round `goal.round` has ended. Decide what happens next.
 *
 * - A run the user stopped, or that failed, ends the goal: I do not go on after
 *   being told to stop, and I do not judge work that broke off.
 * - A chat the user has left ends the goal too: the next turn would go to the
 *   wrong place, and the user is not watching it.
 * - A plan still waiting for approval has done nothing to judge, so the goal
 *   waits too; the run after the user's yes comes back here.
 * - Otherwise ask the check. Met ends it. Not met and rounds left starts the
 *   next round; not met and none left ends it as exhausted.
 */
export async function afterRun(host: GoalHost, convId: string, end: RunEnd): Promise<void> {
  const goal = host.get(convId);
  if (!goal || goal.status !== "active" || goal.checking) return;

  const reason = end.stopReason ?? "completed";
  if (reason === "aborted" || reason === "error" || reason === "timeout") {
    host.patch(convId, { status: "stopped" });
    return;
  }
  if (!host.isOpen(convId)) {
    host.patch(convId, { status: "stopped" });
    host.note(convId, "I stopped working toward the goal because you left this chat.");
    return;
  }
  if (end.awaitingApproval) return;

  host.patch(convId, { checking: true });
  let check: GoalCheck;
  try {
    check = await host.check(convId, goal);
  } catch {
    // A check that cannot run is not a verdict. The goal stops rather than guess.
    host.patch(convId, { checking: false, status: "stopped" });
    host.note(convId, "I couldn't check the goal just now, so I stopped working toward it.");
    return;
  }

  // The user pressed Stop goal while the check ran.
  if (host.get(convId)?.status !== "active") {
    host.patch(convId, { checking: false });
    return;
  }

  if (check.met) {
    host.patch(convId, { checking: false, status: "met", lastCheck: check });
    return;
  }
  if (!host.isOpen(convId)) {
    host.patch(convId, { checking: false, status: "stopped", lastCheck: check });
    host.note(convId, "I stopped working toward the goal because you left this chat.");
    return;
  }
  if (goal.round >= goal.maxRounds) {
    host.patch(convId, { checking: false, status: "exhausted", lastCheck: check });
    host.note(convId, `I stopped after ${goal.maxRounds} rounds. The goal is not met yet: ${check.next}`);
    return;
  }

  const next: Goal = { ...goal, round: goal.round + 1 };
  host.patch(convId, { checking: false, round: next.round, lastCheck: check });
  host.note(convId, continuingNote(next));
  await host.send(convId, continuingTurn(next, check));
}
