/**
 * @vitest-environment jsdom
 *
 * `MCPS-2` and `RTE-6`: what an agent app suggests keeping, and a suggested change
 * to a rule, are shown in words, change nothing until the click, and say so when
 * the click fails. Also the `QuestionCard` header when an agent app is asking.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChangeProposal } from "../../lib/api";

import ProposalCard, { parseRuleChange } from "./ProposalCard";
import QuestionCard from "./QuestionCard";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const resolve = vi.fn();
const loadEngines = vi.fn();
const answerQuestion = vi.fn();

const proposal = (over: Partial<ChangeProposal>): ChangeProposal =>
  ({
    id: "p1",
    target: "engine-memory",
    slug: "claude-code-pnpm-ab12",
    proposed_text: "This project uses pnpm, never npm.",
    rationale: "Claude Code suggested this.",
    description: null,
    status: "pending",
    created_at: 1,
    ...over,
  }) as ChangeProposal;

const change = (c: object) => JSON.stringify({ engine_id: "claude-code", ...c });

let container: HTMLDivElement;
let root: Root;

function show(p: ChangeProposal) {
  useAppStore.setState({ changeProposals: [p], resolveChangeProposal: resolve, loadEngines } as never);
  act(() => root.render(<ProposalCard id={p.id} />));
}

const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent === label) as HTMLButtonElement;
const text = () => container.textContent ?? "";

beforeEach(() => {
  resolve.mockReset().mockResolvedValue(undefined);
  loadEngines.mockReset().mockResolvedValue(undefined);
  answerQuestion.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("a memory an agent app suggested", () => {
  it("shows the whole text and changes nothing until Keep it", async () => {
    show(proposal({}));
    expect(text()).toContain("Claude Code suggested this. Keep this?");
    expect(container.querySelector(".proposal-preview")?.textContent).toBe("This project uses pnpm, never npm.");
    expect(resolve).not.toHaveBeenCalled();
    await act(async () => button("Keep it").click());
    expect(resolve).toHaveBeenCalledWith("p1", true);
  });

  it("Not now refuses it", async () => {
    show(proposal({}));
    await act(async () => button("Not now").click());
    expect(resolve).toHaveBeenCalledWith("p1", false);
  });

  it("shows the refusal in words when keeping it fails", async () => {
    resolve.mockRejectedValue(new Error("That looks like an instruction aimed at me."));
    show(proposal({}));
    await act(async () => button("Keep it").click());
    expect(text()).toContain("That looks like an instruction aimed at me.");
  });
});

describe("a suggested change to a rule", () => {
  it("names the action on the button and refreshes the agents after yes", async () => {
    for (const [action, label] of [
      ["add", "Add it"],
      ["edit", "Change it"],
      ["off", "Turn it off"],
    ] as const) {
      resolve.mockClear();
      loadEngines.mockClear();
      show(proposal({ target: "engine-rule", proposed_text: change({ action, rule_id: "r1" }), rationale: "You said no three times." }));
      expect(text()).toContain("You said no three times.");
      await act(async () => button(label).click());
      expect(resolve).toHaveBeenCalledWith("p1", true);
      expect(loadEngines).toHaveBeenCalledTimes(1);
    }
  });

  it("Not now leaves the rules alone", async () => {
    show(proposal({ target: "engine-rule", proposed_text: change({ action: "off", rule_id: "r1" }) }));
    await act(async () => button("Not now").click());
    expect(resolve).toHaveBeenCalledWith("p1", false);
    expect(loadEngines).not.toHaveBeenCalled();
  });

  it("shows the refusal when the rule is gone", async () => {
    resolve.mockRejectedValue(new Error("That rule is not there any more."));
    show(proposal({ target: "engine-rule", proposed_text: change({ action: "edit", rule_id: "r9" }) }));
    await act(async () => button("Change it").click());
    expect(text()).toContain("That rule is not there any more.");
    expect(loadEngines).not.toHaveBeenCalled();
  });

  it("a payload it cannot read shows nothing instead of a button that does something unknown", () => {
    show(proposal({ target: "engine-rule", proposed_text: "not json" }));
    expect(container.querySelector(".proposal-card")).toBeNull();
    expect(parseRuleChange(change({ action: "delete" }))).toBeNull();
    expect(parseRuleChange(change({ action: "add", kind: "use", text: "x" }))?.action).toBe("add");
  });
});

describe("a question from an agent app", () => {
  const q = (over: object = {}) => ({
    runId: "run-1",
    id: "q1",
    convId: "c1",
    messageId: "m1",
    question: "Which package manager?",
    options: [{ label: "pnpm" }, { label: "npm" }],
    multi: false,
    ...over,
  });

  it("says who is asking, and the lead's own question keeps the old words", () => {
    useAppStore.setState({ answerQuestion } as never);
    act(() => root.render(<QuestionCard q={q({ agent: "Claude Code" })} />));
    expect(container.querySelector("[role=group]")?.getAttribute("aria-label")).toBe("Claude Code asks you");
    expect(text()).toContain("Claude Code asks you");
    act(() => root.render(<QuestionCard q={q()} />));
    expect(text()).toContain("I need you to decide");
  });

  it("a question with no options opens the answer box", () => {
    useAppStore.setState({ answerQuestion } as never);
    act(() => root.render(<QuestionCard q={q({ agent: "Claude Code", options: [] })} />));
    expect(container.querySelector('input[aria-label="Your own answer"]')).not.toBeNull();
  });
});
