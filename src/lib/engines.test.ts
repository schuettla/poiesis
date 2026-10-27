import { describe, expect, it } from "vitest";
import type { EngineInfo, EngineRoutePreview, EngineTryResult } from "./api";
import {
  MODE_LABELS,
  answeredBy,
  becauseChip,
  cannotEnable,
  engineStatus,
  envToMap,
  linesToArgs,
  matchCounter,
  matchedTitle,
  modeHelp,
  paidByLabel,
  payChip,
  previewLine,
  ruleLine,
  ruleLooksWrong,
  tryLine,
} from "./engines";

const base = {
  found: true,
  version: "2.1.301",
  signed_in: true,
  paid_by: "plan",
  payment: "plan",
  tested: "yes",
  last_error: null,
  model: null,
} as unknown as EngineInfo;

describe("engine wording", () => {
  it("builds the status line from what the probe found", () => {
    expect(engineStatus(base)).toEqual({ text: "2.1.301 · signed in · on your plan", tone: "plain" });
    expect(engineStatus({ ...base, tested: "newer" })).toEqual({
      text: "2.1.301 · signed in · on your plan · newer than I've been tested with",
      tone: "warn",
    });
    expect(engineStatus({ ...base, signed_in: false, paid_by: null })).toEqual({
      text: "2.1.301 · not signed in",
      tone: "bad",
    });
    expect(engineStatus({ ...base, signed_in: null, paid_by: "own", model: "opencode/beam-free" }).text).toBe(
      "2.1.301 · free model",
    );
    expect(engineStatus({ ...base, payment: "api-key" }).text).toContain("with your API key");
  });

  it("tells a never-found agent from one that went away", () => {
    expect(engineStatus({ ...base, found: false, last_error: null }).text).toBe("I didn't find it on this computer.");
    expect(engineStatus({ ...base, found: false, last_error: "gone" }).text).toBe("I can't find it any more.");
  });

  it("gives the reason the switch cannot be turned on", () => {
    expect(cannotEnable(base)).toBeNull();
    expect(cannotEnable({ ...base, found: false })).toBe("I can't find it on this computer.");
    expect(cannotEnable({ ...base, signed_in: false })).toBe("Sign in first.");
    // With an API key it does not need the app's own sign-in.
    expect(cannotEnable({ ...base, signed_in: false, payment: "api-key" })).toBeNull();
  });

  it("writes the Try it result the way the plan words it", () => {
    const ok = { ok: true, reply: "ready", seconds: 4.23, paid_by: "plan", model: null, error: null } as EngineTryResult;
    expect(tryLine(ok)).toBe("Ready in 4.2 s · on your plan");
    expect(tryLine({ ...ok, ok: false, error: "It isn't signed in" })).toBe("It isn't signed in");
    expect(paidByLabel("own", "opencode/x-free")).toBe("free model");
    expect(paidByLabel(null)).toBeNull();
  });

  it("words the chips on a Fleet row and the three modes", () => {
    expect(payChip("plan")).toBe("on your plan");
    expect(payChip("api-key")).toBe("API key");
    expect(payChip(null)).toBeNull();
    expect(becauseChip("U2")).toBe("because U2");
    expect(becauseChip("asked")).toBe("you asked");
    expect(becauseChip(null)).toBeNull();
    expect(MODE_LABELS.on_request).toBe("Only when you ask");
    expect(modeHelp("on_request", "Claude Code", "claude-code")).toBe(
      "I won't pick it myself. Use /delegate claude-code … when you want it.",
    );
    expect(modeHelp("auto", "Claude Code", "claude-code")).toContain("tell you which rule");
    expect(modeHelp("on_request", "OpenCode", "acp:opencode")).toContain("/delegate opencode …");
  });

  it("says in one line what Try a request found", () => {
    const base = { agent: "Claude Code", label: "U1", rule_text: "x", consequence: "ask", blocked: null } as EngineRoutePreview;
    expect(previewLine({ ...base, agent: null, label: null, consequence: null })).toBe("I'd do this myself. No rule fits.");
    expect(previewLine(base)).toBe("I'd hand this to Claude Code, because of U1. I'd ask you first.");
    expect(previewLine({ ...base, consequence: "start" })).toContain("It would start at once.");
    expect(previewLine({ ...base, consequence: "only_when_asked" })).toContain("only when you ask");
    expect(previewLine({ ...base, blocked: "it is a read-only folder." })).toBe(
      "I'd want Claude Code (U1), but it is a read-only folder.",
    );
  });

  it("marks a rule that was declined more often than it was used", () => {
    expect(ruleLooksWrong({ hits: 1, overrides: 2 })).toBe(true);
    expect(ruleLooksWrong({ hits: 2, overrides: 2 })).toBe(false);
    expect(ruleLooksWrong({ hits: 0, overrides: 0 })).toBe(false);
  });

  it("turns the form's text into what the backend wants", () => {
    expect(linesToArgs("  --a \n\n--b\r\n")).toEqual(["--a", "--b"]);
    expect(envToMap([{ name: " HTTP_PROXY ", value: "x" }, { name: "", value: "ignored" }])).toEqual({ HTTP_PROXY: "x" });
  });

  it("says who paid for an agent that has no plan of ours, and why a job was handed over", () => {
    expect(payChip("own")).toBe("own provider");
    expect(payChip("free")).toBe("free model");
    expect(paidByLabel("own", "opencode/beam-free")).toBe("free model");
    expect(becauseChip("persona")).toBe("its persona");
    expect(becauseChip("asked")).toBe("you asked");
  });
});

describe("the handover pre-match (USE-UI-10)", () => {
  it("says a rule was matched early only when a quick decision matched it", () => {
    expect(ruleLine({ label: "U2", text: "Run the tests" })).toBe("Because of your rule U2: Run the tests");
    expect(ruleLine({ label: "U2", text: "Run the tests", matched: null })).toBe("Because of your rule U2: Run the tests");
    expect(ruleLine({ label: "U2", text: "Run the tests", matched: 0.931 })).toBe("Matches your rule U2: Run the tests");
  });

  // `SEE-T-8`: a match is an unfitted number to the card, so only 0.9 and up is said plainly.
  it("says a middling match with probably, and never shows the number", () => {
    expect(ruleLine({ label: "U2", text: "Run the tests", matched: 0.86 })).toBe("Probably matches your rule U2: Run the tests");
    expect(ruleLine({ label: "U2", text: "Run the tests", matched: 0.97 })).not.toMatch(/\d\.\d/);
  });

  it("words the dot's hover and which path answered", () => {
    expect(matchedTitle(0.93)).toBe("matched before I started (0.93)");
    expect(answeredBy({ quick_ms: 41 })).toBe("quick decision \u00b7 41 ms");
    expect(answeredBy({ quick_ms: null })).toBe("asked my chat model");
    expect(answeredBy({})).toBe("asked my chat model");
  });

  it("counts how a rule fared, and says nothing when no quick decision matched it", () => {
    expect(matchCounter(undefined)).toBeNull();
    expect(matchCounter({ matched: 0, declined: 0 })).toBeNull();
    expect(matchCounter({ matched: 12, declined: 1 })).toBe("matched early 12\u00d7 \u00b7 you said do it yourself 1\u00d7");
    expect(matchCounter({ matched: 3, declined: 0 })).toBe("matched early 3\u00d7");
    expect(matchCounter({ matched: 0, declined: 2 })).toBe("you said do it yourself 2\u00d7");
  });
});
