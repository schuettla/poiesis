/**
 * @vitest-environment jsdom
 *
 * `CLI-UI-2`, `RTE-UI-3`, `CLI-UI-5`: a Fleet row for a job an agent app ran says
 * who paid and which rule sent it there, can be taken back, can be continued in a
 * terminal, and is honest that it cannot be steered mid-step.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SubRun } from "../../lib/types";

const undoEngineHandover = vi.fn();
const continueEngineJob = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => false,
  undoEngineHandover: (...a: unknown[]) => undoEngineHandover(...a),
  continueEngineJob: (...a: unknown[]) => continueEngineJob(...a),
}));

import FleetCard from "./FleetCard";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const run = (over: Partial<SubRun> = {}): SubRun => ({
  runId: "run-1",
  conversationId: "conv-child",
  parentConversationId: "conv-lead",
  agent: "Claude Code",
  task: "Rename the option",
  status: "done",
  steps: [],
  text: "Done.",
  startedAt: 0,
  engine: { id: "claude-code", name: "Claude Code", paid_by: "plan", because: "U1", because_text: "A change across many files", can_steer: false },
  engineSession: "sess-1",
  ...over,
});

function show(r: SubRun) {
  useAppStore.setState({ subRuns: { [r.runId]: r }, personas: [] } as never);
  act(() => root.render(<FleetCard runIds={[r.runId]} />));
}

const button = (label: string | RegExp) =>
  Array.from(container.querySelectorAll("button")).find((b) =>
    typeof label === "string" ? b.textContent === label : label.test(b.textContent ?? ""),
  ) as HTMLButtonElement;

beforeEach(() => {
  undoEngineHandover.mockReset().mockResolvedValue(true);
  continueEngineJob.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ subRuns: {} });
});

describe("a Fleet row for an agent app", () => {
  it("shows who paid and which rule sent it there; the rule opens its settings", () => {
    show(run());
    const chips = Array.from(container.querySelectorAll(".fleet-chip")).map((c) => c.textContent);
    expect(chips).toEqual(["on your plan", "because U1"]);
    expect(container.querySelector(".fleet-chip-why")?.getAttribute("title")).toBe("A change across many files");
    act(() => button("because U1").click());
    expect(useAppStore.getState().view).toBe("engines");
    expect(useAppStore.getState().engineFocus).toBe("claude-code");
  });

  it("says 'Plan limit' next to who paid when the job ended on the plan limit, and in its end line", () => {
    show(run({ status: "error", stopReason: "plan_limit", text: "Claude Code hit its plan limit, back at 14:30." }));
    const chips = Array.from(container.querySelectorAll(".fleet-chip")).map((c) => c.textContent);
    expect(chips).toEqual(["on your plan", "Plan limit", "because U1"]);
    expect(container.querySelector(".fleet-chip-warn")).not.toBeNull();
    expect(container.textContent).toContain("hit its plan limit");
    show(run());
    expect(container.querySelector(".fleet-chip-warn")).toBeNull();
  });

  it("shows the agent's mark before its name, or its first letter when it has none", () => {
    show(run({ engine: { id: "acp:gemini", name: "Gemini CLI", logo: "gemini.svg", paid_by: "plan", because: "U1", because_text: null, can_steer: false } }));
    expect(container.querySelector(".fleet-who .app-logo img")?.getAttribute("src")).toMatch(/agent-logos\/gemini\.svg$/);
    show(run({ engine: { id: "acp:kimi", name: "Kimi Code", paid_by: "plan", because: "U1", because_text: null, can_steer: false } }));
    expect(container.querySelector(".fleet-who .app-logo img")).toBeNull();
    expect(container.querySelector(".fleet-who .app-logo")?.textContent).toBe("K");
  });

  it("says 'you asked' when the user named the agent, and offers no take-back", () => {
    show(run({ engine: { ...run().engine!, because: "asked", because_text: null } }));
    expect(container.textContent).toContain("you asked");
    expect(container.textContent).not.toContain("This shouldn't have gone");
  });

  it("is quiet for an ordinary child: no chips, no gear, no continue", () => {
    show(run({ engine: undefined, engineSession: undefined }));
    expect(container.querySelector(".fleet-chip")).toBeNull();
    expect(container.querySelector(".fleet-gear")).toBeNull();
    expect(container.querySelector(".engine-after")).toBeNull();
  });

  it("takes the handover back and says it counted against the rule", async () => {
    show(run());
    await act(async () => button("This shouldn't have gone to Claude Code").click());
    expect(undoEngineHandover).toHaveBeenCalledWith("run-1");
    expect(container.textContent).toContain("Noted. I'll count that against U1.");
  });

  it("continues in a terminal and copies the command either way", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    continueEngineJob.mockResolvedValue({ command: "claude --resume sess-1", opened: false, error: null });
    show(run());
    expect(container.textContent).toContain("I handed this to Claude Code.");
    await act(async () => button("Continue there").click());
    expect(writeText).toHaveBeenCalledWith("claude --resume sess-1");
    expect(container.textContent).toContain("Copied: claude --resume sess-1");
  });

  it("Continue here puts /resume and the session's word in the composer, for Claude Code and Codex", () => {
    const asked = vi.fn();
    useAppStore.setState({ requestComposer: asked } as never);
    show(run({ engineSession: "11111111-2222-4333-8444-555555555555" }));
    act(() => button("Continue here").click());
    expect(asked).toHaveBeenCalledWith("/resume cc-55555555 ");
    show(run({ runId: "run-2", engine: { ...run().engine!, id: "codex", name: "Codex" }, engineSession: "01a1d060-4ab7-72b2-b02b-1dbee1d5f52c" }));
    act(() => button("Continue here").click());
    expect(asked).toHaveBeenLastCalledWith("/resume cx-e1d5f52c ");
  });

  it("has no Continue here for another agent app, nor while it is still working", () => {
    show(run({ runId: "run-3", engine: { ...run().engine!, id: "acp:opencode", name: "OpenCode" }, engineSession: "ses_1" }));
    expect(button("Continue here")).toBeUndefined();
    expect(button("Continue there")).toBeDefined();
    show(run({ runId: "run-4", status: "running", engineSession: "11111111-2222-4333-8444-555555555555" }));
    expect(button("Continue here")).toBeUndefined();
  });

  it("has no Continue there before the agent has a session", () => {
    show(run({ engineSession: undefined }));
    expect(button("Continue there")).toBeUndefined();
  });

  it("is honest that a running job takes a message only between steps", () => {
    show(run({ status: "running", text: "" }));
    act(() => button("Steer").click());
    const input = container.querySelector<HTMLInputElement>(".fleet-steer input")!;
    expect(input.placeholder).toBe("I'll pass this on when it's done with this job");
  });

  it("keeps the normal placeholder for an agent that can take a message live", () => {
    show(run({ status: "running", text: "", engine: { ...run().engine!, can_steer: true } }));
    act(() => button("Steer").click());
    expect(container.querySelector<HTMLInputElement>(".fleet-steer input")!.placeholder).toBe("Tell this agent something");
  });

  it("the gear opens that agent's rules", () => {
    show(run());
    act(() => container.querySelector<HTMLButtonElement>(".fleet-gear")!.click());
    expect(useAppStore.getState().engineFocus).toBe("claude-code");
  });

  it("an agent app waiting for a free slot looks like any child that is waiting its turn", () => {
    show(run({ status: "queued", text: "", startedAt: 0 }));
    expect(container.textContent).toContain("waiting its turn");
  });

  it("says what the job changed and whether the check ran after it (PRF-2), in each of the five cases", () => {
    const line = (e: NonNullable<SubRun["evidence"]>) => {
      show(run({ evidence: e }));
      return container.querySelector(".fleet-evidence");
    };
    expect(line({ changed: 3, check: null, state: "none" })?.textContent).toBe("Changed 3 files.");
    expect(line({ changed: 3, check: "cargo test", state: "passed" })?.textContent).toBe(
      "Changed 3 files. Ran cargo test after the last change: passed.",
    );
    expect(container.querySelector(".fleet-evidence-warn")).toBeNull();
    const failed = line({ changed: 3, check: "cargo test", state: "failed" });
    expect(failed?.textContent).toBe("Changed 3 files. Ran cargo test after the last change: failed.");
    expect(failed?.classList.contains("fleet-evidence-warn")).toBe(true);
    expect(line({ changed: 1, check: "cargo test", state: "before-last" })?.textContent).toBe(
      "Changed 1 file. Ran cargo test, but not after its last change.",
    );
    const never = line({ changed: 3, check: "cargo test", state: "not-run" });
    expect(never?.textContent).toBe("Changed 3 files. It did not run the project's check (cargo test).");
    expect(never?.classList.contains("fleet-evidence-warn")).toBe(true);
  });

  it("says nothing when no file changed, and not while the job still works", () => {
    show(run());
    expect(container.querySelector(".fleet-evidence")).toBeNull();
    show(run({ status: "running", evidence: { changed: 1, check: null, state: "none" } }));
    expect(container.querySelector(".fleet-evidence")).toBeNull();
  });

  it("adds what the change left open: two items, then 'and N more', all of them in the tooltip (PRF-4)", () => {
    show(run({ evidence: { changed: 1, check: null, state: "none", left: ["a new TODO in src/a.rs"] } }));
    expect(container.querySelector(".fleet-evidence")?.textContent).toBe("Changed 1 file. Left: a new TODO in src/a.rs.");
    const left = ["a new TODO in a.rs", "a stub in b.rs", "a test switched off in c.ts", "a stub in d.rs"];
    show(run({ evidence: { changed: 4, check: null, state: "none", left } }));
    const el = container.querySelector(".fleet-evidence")!;
    expect(el.textContent).toBe("Changed 4 files. Left: a new TODO in a.rs, a stub in b.rs and 2 more.");
    expect(el.getAttribute("title")).toBe(left.join("\n"));
  });

  it("a helper waiting for a place another helper is changing says which (PRF-5)", () => {
    show(run({ status: "queued", text: "", queuedBecause: "Waiting: Codex is changing src/a.rs." }));
    expect(container.querySelector(".fleet-doing")?.textContent).toBe("Waiting: Codex is changing src/a.rs.");
    show(run({ status: "queued", text: "" }));
    expect(container.querySelector(".fleet-doing")?.textContent).toBe("waiting its turn");
  });

  it("says who paid for a persona's job on a free model", () => {
    show(run({ engine: { ...run().engine!, paid_by: "free", because: "persona", because_text: null } }));
    const chips = Array.from(container.querySelectorAll(".fleet-chip")).map((c) => c.textContent);
    expect(chips).toEqual(["free model", "its persona"]);
    expect(container.textContent).not.toContain("This shouldn't have gone");
  });
});
