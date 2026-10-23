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

  it("says who paid for a persona's job on a free model", () => {
    show(run({ engine: { ...run().engine!, paid_by: "free", because: "persona", because_text: null } }));
    const chips = Array.from(container.querySelectorAll(".fleet-chip")).map((c) => c.textContent);
    expect(chips).toEqual(["free model", "its persona"]);
    expect(container.textContent).not.toContain("This shouldn't have gone");
  });
});
