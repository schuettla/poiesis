/**
 * `PAR-6`: any number of chats work at once, each with its own run. The store keeps them by
 * chat; every reader asks here, so "the run" always means the run of one chat.
 */
import type { PlanView } from "./api";
import type { AppState } from "./store";
import type { Model, ModelTier, PendingQuestion, Provenance } from "./types";

/** `HRN-1`: the run working in one chat. Its id is what mid-run steering addresses. */
export interface LiveRun {
  runId: string;
  convId: string;
  /** 1-based iteration, and the budget it is counted against (`HRN-UI-3`). */
  step: number;
  maxSteps: number;
  startedAt: number;
  /** `OBS-3`: an estimate of what the current turn is sending, and what the
   * model can hold. `contextWindow` is null when the provider does not say,
   * and the meter then shows no percentage rather than a made-up one. */
  contextTokens: number;
  contextWindow: number | null;
  /** `TIER-4`: the tier this run really ran at. */
  tier?: ModelTier;
  /** What the model has been thinking since the last thing it said out loud.
   * Cleared when prose starts arriving, so the indicator describes now and
   * not the whole run. Shown as thinking, never as the answer. */
  thinking: string;
  /** The model is writing a long tool call (a report into a document). It can take
   * minutes with nothing else arriving, so the live line says so and counts the words.
   * Cleared when the call is made or the model speaks. */
  writing?: { label: string | null; words: number };
  /** `PLN-UI-2`: the plan this run is working to, once it has written one.
   * The running item is a better label for the meter than the running tool,
   * because it says what the work is *for*. */
  plan?: PlanView;
}

/** Where a working chat's model runs. The engine of this PC takes one run at a time (`PAR-8`). */
export type Seat = "local" | "other";
export const seatOf = (provenance: Provenance): Seat => (provenance === "local" ? "local" : "other");

export const runOf = (s: Pick<AppState, "liveRuns">, convId: string | null | undefined): LiveRun | undefined =>
  convId ? s.liveRuns[convId] : undefined;

/** Is this chat working: from the moment a send starts until its stream ends. It covers the
 * gap before `run_started`, and picture jobs, which register no run. */
export const busyIn = (s: Pick<AppState, "busyChats">, convId: string | null | undefined): boolean =>
  !!convId && !!s.busyChats[convId];

export const anyBusy = (s: Pick<AppState, "busyChats">): boolean => Object.keys(s.busyChats).length > 0;

/** The run of the chat on screen. What the composer, meter and timeline mean by "the run". */
export const shownRun = (s: Pick<AppState, "liveRuns" | "activeConversationId">): LiveRun | undefined =>
  runOf(s, s.activeConversationId);

/** The questions open right now, in the order they were asked. */
export const openQuestions = (s: Pick<AppState, "pendingQuestion" | "queuedQuestions">): PendingQuestion[] =>
  s.pendingQuestion ? [s.pendingQuestion, ...s.queuedQuestions] : s.queuedQuestions;

/** `PAR-7`: the question this chat is waiting on, whatever other chats are asked. */
export const questionOf = (
  s: Pick<AppState, "pendingQuestion" | "queuedQuestions">,
  convId: string | null | undefined,
): PendingQuestion | undefined => openQuestions(s).find((q) => q.convId === convId);

/** `PAR-8`: why a send to this model has to wait: another chat has the local model. */
export function localModelNotice(
  s: Pick<AppState, "busyChats" | "conversations">,
  convId: string | null,
  model: Pick<Model, "provenance" | "modality"> | undefined,
): string | null {
  if (!model || model.provenance !== "local" || (model.modality && model.modality !== "chat")) return null;
  const other = Object.keys(s.busyChats).find((id) => id !== convId && s.busyChats[id] === "local");
  if (!other) return null;
  const title = s.conversations.find((c) => c.id === other)?.title ?? "another chat";
  return `The local model is working in "${title}". Your message is kept: send it when that one is done, or pick a cloud model.`;
}
