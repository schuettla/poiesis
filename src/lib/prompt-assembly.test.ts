/**
 * `CTX-4`: the equivalence gate, frontend half.
 *
 * Prompt assembly exists twice while the port is in flight — here in `store.ts`
 * plus `context.ts`, and in `src-tauri/src/agent/context.rs`. The switchover is
 * only safe if the two produce the same bytes, so both render the same fixture
 * and compare against the same golden file. Both matching one file is the same
 * claim as the two matching each other, and it needs no running app and no IPC.
 *
 * The rendering is deliberately dumb: plain concatenation with a marker line
 * between turns, no JSON serializer anywhere. Comparing serialized arrays would
 * have tested `JSON.stringify` against `serde_json` — two things that are
 * allowed to differ — instead of testing the two assemblies.
 *
 * **When this fails, one of the two implementations changed.** Fix the one that
 * was not meant to, or update the golden and both sides together in one commit.
 * The Rust side rewrites the golden when run with `UPDATE_PROMPT_GOLDEN=1`.
 */
import { describe, expect, it } from "vitest";
import { budgetTurns, withSummary } from "./context";
import { composeSystemPrompt, type PlanMode } from "./store";
import type { ChatTurnMessage } from "./api";
import fixtureJson from "../../fixtures/prompt-assembly.json";
import golden from "../../fixtures/prompt-assembly.golden.txt?raw";

interface Fixture {
  base: string;
  about_you: string;
  soul: string;
  project_name: string;
  project_instructions: string;
  memory_index: string;
  fact_count: number;
  tools_enabled: boolean;
  memory_enabled: boolean;
  plan_mode: PlanMode;
  skills: { name: string; description: string; when_to_use: string | null }[];
  blocks: FixtureBlock[];
  surface: { data_json: string; state_json: string | null };
  session_state_json: string;
  tool_health: { tool_name: string; ok: number; total: number }[];
  summary: string;
  budget: number;
  keep_recent: number;
  prior: ChatTurnMessage[];
  current: ChatTurnMessage;
}

// Imported through the bundler rather than read with `node:fs`, so the frontend
// still needs no Node type definitions to type-check.
const fixture = fixtureJson as unknown as Fixture;

interface FixtureBlock {
  id: string;
  title: string;
  kind: string;
  data_json: string;
  state_json: string | null;
}

/** Stored JSON becomes an object here, because that is the shape `store.ts`
 * takes. Parsing and re-stringifying preserves key order in JavaScript, which is
 * exactly the property the Rust side protects by never re-serializing at all. */
const parse = (text: string | null | undefined) => (text ? JSON.parse(text) : undefined);

function budgeted() {
  const blocks = (fixture.blocks as FixtureBlock[]).map((b) => ({
    id: b.id,
    title: b.title,
    kind: b.kind,
    data: parse(b.data_json),
    state: parse(b.state_json),
  }));

  let system = composeSystemPrompt(fixture.base, {
    // Only `messages[].blocks` is read off the conversation, and only for the
    // block registry.
    conv: { messages: [{ blocks }] } as never,
    sessionState: parse(fixture.session_state_json),
    toolsEnabled: fixture.tools_enabled,
    surface: {
      data: parse(fixture.surface.data_json),
      state: parse(fixture.surface.state_json),
    } as never,
    memory: {
      index: fixture.memory_index,
      soul: fixture.soul,
      about_you: fixture.about_you,
      fact_count: fixture.fact_count,
    },
    memoryEnabled: fixture.memory_enabled,
    projectName: fixture.project_name,
    projectInstructions: fixture.project_instructions,
    planMode: fixture.plan_mode,
    toolHealth: fixture.tool_health,
    skills: (fixture.skills as { name: string; description: string; when_to_use: string | null }[])
      .map((s) => ({ ...s, enabled: true })) as never,
  });
  system = withSummary(system, fixture.summary);

  return budgetTurns(
    system,
    fixture.prior as ChatTurnMessage[],
    fixture.current as ChatTurnMessage,
    fixture.budget,
    fixture.keep_recent
  );
}

describe("prompt assembly (CTX-4)", () => {
  it("produces the same bytes the Rust assembly produces", () => {
    const rendered = budgeted()
      .turns.map((t) => `\n<<<turn role=${t.role}>>>\n${t.content as string}\n`)
      .join("");
    expect(rendered).toBe(golden);
  });

  /** The budget must actually bite on this fixture. A golden recorded from a
   * window nothing overflowed would pass forever while proving nothing about
   * the half of assembly that decides what to drop. */
  it("exercises the budget rather than fitting everything", () => {
    const bt = budgeted();
    expect(bt.needsCompaction).toBe(true);
    expect(bt.overflow.length).toBeGreaterThan(0);
    expect(bt.overflow.length).toBeLessThan(fixture.prior.length);
  });
});
