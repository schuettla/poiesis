/**
 * Context homeostasis (CTX-2). llama.cpp truncates from the *front* when a
 * request overflows its window, which quietly eats the system prompt — the
 * surface tree, session state, and standing guidance. So we budget turns here,
 * before sending, and never hand the engine more than it can hold.
 *
 * Nothing here deletes or hides anything: this decides only what is *sent*.
 */

import type { ChatTurnMessage } from "./api";

/** Flat per-image estimate; real cost is model-specific, this is a safe floor. */
const IMAGE_TOKEN_COST = 800;

/** Rough token estimate: 1 token ≈ 4 chars. Deliberately conservative. */
export function estimateTokens(text: string): number {
  return Math.ceil(text.length / 4);
}

/**
 * Estimated tokens for one turn. Text is charged by length; an image is charged
 * a flat `IMAGE_TOKEN_COST` rather than the meaningless character count of its
 * data: URI.
 */
function turnTokens(turn: ChatTurnMessage): number {
  if (typeof turn.content === "string") return estimateTokens(turn.content);
  let total = 0;
  for (const part of turn.content) {
    total += part.type === "text" ? estimateTokens(part.text) : IMAGE_TOKEN_COST;
  }
  return total;
}

/** Share of the budget reserved for the response plus tool traffic. */
const RESPONSE_RESERVE = 0.25;

/** Recent turns that are never dropped — the thread of the live exchange. */
export const KEEP_RECENT = 6;

/** Fewer in workspace mode: the surface and session state carry the task state. */
export const KEEP_RECENT_WORKSPACE = 3;

export interface BudgetedTurns {
  /** What to send, in order: system, kept prior turns, current. */
  turns: ChatTurnMessage[];
  /** Prior turns that did not fit, oldest first — candidates for summarizing. */
  overflow: ChatTurnMessage[];
  usedTokens: number;
  budget: number;
  /** History alone exceeds the threshold; the caller should compact first. */
  needsCompaction: boolean;
}

/**
 * Fit `prior` into `budget`, newest first. Never drops the system turn, the
 * last `keepRecent` turns, or the current user turn — if those alone overflow,
 * we send them anyway and let the engine deal with it, because dropping the
 * user's actual question is never the right answer.
 */
export function budgetTurns(
  system: string,
  prior: ChatTurnMessage[],
  current: ChatTurnMessage,
  budget: number,
  keepRecent: number = KEEP_RECENT
): BudgetedTurns {
  const ceiling = budget * (1 - RESPONSE_RESERVE);
  const fixed = estimateTokens(system) + turnTokens(current);

  const kept: ChatTurnMessage[] = [];
  let acc = 0;

  for (let i = prior.length - 1; i >= 0; i--) {
    const turn = prior[i];
    const cost = turnTokens(turn);
    const mustKeep = prior.length - i <= keepRecent;
    if (!mustKeep && fixed + acc + cost > ceiling) break;
    acc += cost;
    kept.unshift(turn);
  }

  const overflow = prior.slice(0, prior.length - kept.length);
  const systemTurn: ChatTurnMessage = { role: "system", content: system };

  return {
    turns: [systemTurn, ...kept, current],
    overflow,
    usedTokens: fixed + acc,
    budget,
    needsCompaction: overflow.length > 0,
  };
}

/** Append a conversation summary to the system prompt (CTX-4). */
export function withSummary(system: string, summary: string): string {
  if (!summary.trim()) return system;
  return `${system}\n\n## Conversation so far (older turns were summarized)\n${summary.trim()}`;
}
