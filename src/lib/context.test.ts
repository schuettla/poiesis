/**
 * Plain-assert checks for the turn budgeter (CTX-2). No test framework is
 * installed; run this file with esbuild + node:
 *
 *   npx esbuild src/lib/context.test.ts --bundle --format=esm --platform=node | node --input-type=module
 */

import type { ChatTurnMessage } from "./api";
import { budgetTurns, estimateTokens, withSummary, KEEP_RECENT } from "./context";

/** Minimal assert — no test framework is installed and none is being added. */
const assert = {
  ok(cond: unknown, msg?: string) {
    if (!cond) throw new Error(`assertion failed: ${msg ?? "expected truthy"}`);
  },
  equal(actual: unknown, expected: unknown, msg?: string) {
    if (actual !== expected) {
      throw new Error(`assertion failed: ${msg ?? ""} — got ${String(actual)}, want ${String(expected)}`);
    }
  },
};

const user = (text: string): ChatTurnMessage => ({ role: "user", content: text });
const bot = (text: string): ChatTurnMessage => ({ role: "assistant", content: text });

// --- estimator ---
assert.equal(estimateTokens(""), 0);
assert.equal(estimateTokens("abcd"), 1);
assert.equal(estimateTokens("abcde"), 2, "rounds up — better to over-estimate");

// --- a short conversation fits untouched ---
{
  const prior = [user("hello"), bot("hi there")];
  const bt = budgetTurns("You are Poiesis Agent.", prior, user("what's next?"), 4096);
  assert.equal(bt.needsCompaction, false);
  assert.equal(bt.overflow.length, 0);
  assert.equal(bt.turns.length, prior.length + 2, "system + prior + current");
  assert.equal(bt.turns[0].role, "system");
  assert.equal(bt.turns[bt.turns.length - 1].content, "what's next?");
}

// --- a long conversation overflows, oldest-first, and asks for compaction ---
{
  const prior: ChatTurnMessage[] = [];
  for (let i = 0; i < 100; i++) prior.push(user(`turn ${i} `.padEnd(400, "x")));
  const bt = budgetTurns("system", prior, user("now what?"), 4096);

  assert.equal(bt.needsCompaction, true);
  assert.ok(bt.overflow.length > 0);
  assert.equal(bt.overflow[0], prior[0], "the oldest turns are the ones dropped");
  assert.ok(
    bt.usedTokens <= 4096 * 0.75,
    `stays under the 75% ceiling, got ${bt.usedTokens}`
  );
  // Kept turns must be a contiguous newest-suffix of prior.
  const kept = bt.turns.slice(1, -1);
  assert.equal(kept[kept.length - 1], prior[prior.length - 1]);
  assert.equal(bt.overflow.length + kept.length, prior.length, "nothing is lost or duplicated");
}

// --- the recent thread and the current turn survive even a hopeless budget ---
{
  const prior: ChatTurnMessage[] = [];
  for (let i = 0; i < 20; i++) prior.push(user("y".repeat(5000)));
  const bt = budgetTurns("system", prior, user("the actual question"), 128);

  const kept = bt.turns.slice(1, -1);
  assert.equal(kept.length, KEEP_RECENT, "never drops the live exchange");
  assert.equal(bt.turns[bt.turns.length - 1].content, "the actual question");
  assert.equal(bt.needsCompaction, true);
}

// --- workspace mode keeps a shorter tail ---
{
  const prior: ChatTurnMessage[] = [];
  for (let i = 0; i < 20; i++) prior.push(user("y".repeat(5000)));
  const bt = budgetTurns("system", prior, user("q"), 128, 3);
  assert.equal(bt.turns.slice(1, -1).length, 3);
}

// --- summary is appended under a labelled heading, and empties are ignored ---
{
  assert.equal(withSummary("base", "   "), "base");
  const merged = withSummary("base", "FACTS: a");
  assert.ok(merged.startsWith("base"));
  assert.ok(merged.includes("## Conversation so far (older turns were summarized)"));
  assert.ok(merged.endsWith("FACTS: a"));
}

console.log("context.test.ts: all assertions passed");
