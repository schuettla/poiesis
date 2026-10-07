/**
 * `VTN-2`, `VOC-T4`: the sentence that tells the model its reply will be heard
 * is written twice, here (`store.ts`) and in `agent/context.rs`. Both are held
 * to one file, so neither can drift. The Rust half is
 * `the_spoken_sentence_matches_the_shared_golden` in `context_golden.rs`.
 */
import { describe, expect, it } from "vitest";
import { composeSystemPrompt } from "../store";
import golden from "../../../fixtures/voice/spoken-prompt.golden.txt?raw";

const opts = { conv: undefined, sessionState: undefined, toolsEnabled: false };

describe("spoken prompt (VTN-2)", () => {
  it("matches the sentence Rust sends", () => {
    expect(composeSystemPrompt("BASE", { ...opts, spoken: true })).toBe(golden);
  });

  it("leaves a typed turn alone", () => {
    expect(composeSystemPrompt("BASE", opts)).toBe("BASE");
    expect(composeSystemPrompt("BASE", { ...opts, spoken: false })).toBe("BASE");
  });

  it("comes last, after the tool guidance", () => {
    const out = composeSystemPrompt("BASE", { ...opts, toolsEnabled: true, memoryEnabled: true, spoken: true });
    expect(out.endsWith(golden.slice("BASE\n\n".length))).toBe(true);
  });
});
