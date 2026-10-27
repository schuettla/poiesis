/**
 * @vitest-environment jsdom
 *
 * `HAB-3`: a lesson I keep relearning is offered as a check before its tool. The
 * standing instruction is the second button on the same card, and nothing changes
 * until a click.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChangeProposal } from "../../lib/api";
import ProposalCard from "./ProposalCard";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const resolve = vi.fn();
const standing = vi.fn();

const habit: ChangeProposal = {
  id: "h1",
  target: "habit",
  slug: "check-header",
  proposed_text: '{"on":"read_file","check":"header_row"}',
  rationale: "I've learned this three times.",
  description: "Check the header row.",
  status: "pending",
  created_at: 1,
};

let container: HTMLDivElement;
let root: Root;

const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent === label) as HTMLButtonElement;

beforeEach(() => {
  resolve.mockReset().mockResolvedValue(undefined);
  standing.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ changeProposals: [habit], resolveChangeProposal: resolve, makeStandingInstead: standing } as never);
  act(() => root.render(<ProposalCard id="h1" />));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("a lesson offered as a check before a tool", () => {
  it("says what it would do in words, and changes nothing yet", () => {
    expect(container.textContent).toContain("I keep relearning check-header.");
    expect(container.textContent).toContain("I check this before read file, and afterwards I look at the header row.");
    expect(resolve).not.toHaveBeenCalled();
    expect(standing).not.toHaveBeenCalled();
  });

  it("Check it first accepts the habit", async () => {
    await act(async () => button("Check it first").click());
    expect(resolve).toHaveBeenCalledWith("h1", true);
  });

  it("the standing instruction is the second button on the same card", async () => {
    await act(async () => button("Make it a standing instruction instead").click());
    expect(standing).toHaveBeenCalledWith("h1");
    expect(resolve).not.toHaveBeenCalled();
  });

  it("Not now refuses it", async () => {
    await act(async () => button("Not now").click());
    expect(resolve).toHaveBeenCalledWith("h1", false);
  });

  it("shows a refusal in words", async () => {
    standing.mockRejectedValue("Standing instructions are switched off.");
    await act(async () => button("Make it a standing instruction instead").click());
    expect(container.textContent).toContain("Standing instructions are switched off.");
  });
});
