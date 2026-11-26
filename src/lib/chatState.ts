import { stillWorking } from "./api";
import { cutAtWord, stepWords } from "./liveWords";
import type { SubRun } from "./types";
import type { AppState } from "./store";

/** `UI-1`: what one chat is doing, as the sidebar says it. One function decides it, so the
 * chat row, a helper entry under it and its second line can never disagree. */
export type ChatState = "needs-you" | "running" | "failed" | "done-unseen" | "idle";

/** The helper jobs, looked up by chat. Every row of the rail asks about its chat on every store
 * update, so walking all jobs per row (a real history has thousands) froze the app. The index is
 * built once per `subRuns` object, which the store replaces whenever a job changes. */
interface RunIndex {
  byChat: Map<string, SubRun>;
  /** Chats that have a helper still working. */
  busyParents: Set<string>;
}
const indexes = new WeakMap<object, RunIndex>();
function indexOf(subRuns: Record<string, SubRun>): RunIndex {
  let index = indexes.get(subRuns);
  if (!index) {
    index = { byChat: new Map(), busyParents: new Set() };
    for (const r of Object.values(subRuns)) {
      index.byChat.set(r.conversationId, r);
      if (stillWorking(r.status)) index.busyParents.add(r.parentConversationId);
    }
    indexes.set(subRuns, index);
  }
  return index;
}

/** The helper job whose own conversation this is, if it is one. */
export function runOfChat(s: AppState, convId: string): SubRun | undefined {
  return indexOf(s.subRuns).byChat.get(convId);
}

/** First rule that is true wins. A helper's question carries the lead's conversation but the
 * helper's run id, so it counts for both the lead and the helper's own chat. */
export function chatState(s: AppState, convId: string): ChatState {
  const own = runOfChat(s, convId);
  const asks = (q: { convId: string; runId: string } | null | undefined) =>
    !!q && (q.convId === convId || (own !== undefined && q.runId === own.runId));
  if (s.waiting.some((p) => p.session_id === convId) || asks(s.pendingQuestion) || s.queuedQuestions.some(asks)) {
    return "needs-you";
  }
  if (
    s.runSummaries[convId]?.live ||
    (own && stillWorking(own.status)) ||
    indexOf(s.subRuns).busyParents.has(convId)
  ) {
    return "running";
  }
  const unread = s.unreadIds.includes(convId);
  if (unread && (s.runSummaries[convId]?.stopReason === "error" || own?.status === "error")) return "failed";
  if (unread) return "done-unseen";
  return "idle";
}

/** `UI-2`: the second line of a chat that is working or waiting, or null for any other state. */
export function chatNow(s: AppState, convId: string): string | null {
  const state = chatState(s, convId);
  if (state !== "running" && state !== "needs-you") return null;
  const own = runOfChat(s, convId);
  if (state === "needs-you") {
    const q = [s.pendingQuestion, ...s.queuedQuestions].find(
      (x) => !!x && (x.convId === convId || (own !== undefined && x.runId === own.runId))
    );
    return cutAtWord(q?.question ?? s.waiting.find((p) => p.session_id === convId)?.rationale ?? "");
  }
  // A helper's own chat is read from its run: its messages may never have been loaded.
  if (own && stillWorking(own.status)) {
    if (own.status === "queued" && own.queuedBecause) return own.queuedBecause;
    const step = own.steps.find((x) => x.status === "running");
    if (step) return stepWords(step);
    return own.text ? "Writing the answer" : "Thinking";
  }
  const messages = s.conversations.find((c) => c.id === convId)?.messages ?? [];
  let streaming: (typeof messages)[number] | undefined;
  for (let i = messages.length - 1; i >= 0 && !streaming; i--) {
    if (messages[i].role === "assistant" && messages[i].streaming) streaming = messages[i];
  }
  if (!streaming) return "Helpers working";
  const step = streaming.steps?.find((x) => x.status === "running");
  if (step) return stepWords(step);
  if (s.activeRun?.convId === convId && s.activeRun.thinking) return "Thinking";
  return streaming.text ? "Writing the answer" : "Thinking";
}
