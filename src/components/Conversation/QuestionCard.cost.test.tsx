/**
 * @vitest-environment jsdom
 *
 * `TEAM-12`: the cost card is the question card with its own heading and a box beside its
 * buttons. Start and Not now are the buttons; the box goes back with the choice. Typing in the
 * composer does not answer it, and a run the limit stopped says so.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import QuestionCard from "./QuestionCard";
import AgentRun from "./AgentRun";
import { useAppStore } from "../../lib/store";
import type { Message, PendingQuestion } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

let container: HTMLDivElement;
let root: Root;
let answer: ReturnType<typeof vi.fn>;

const card = (over: Partial<PendingQuestion> = {}): PendingQuestion => ({
  runId: "r1",
  id: "team-cost",
  convId: "c1",
  messageId: "m1",
  question:
    "4 personas: Mira (Claude Sonnet), Atlas (GPT-5).\n\nA task like this has cost about $0.40 to $1.20.\n\nIt stops at $2.00 or after 90 minutes.",
  options: [{ label: "Start" }, { label: "Not now" }],
  multi: false,
  title: "Shop team is about to start",
  check: "Don't ask for this team",
  ...over,
});

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  answer = vi.fn().mockResolvedValue(true);
  useAppStore.setState({ answerQuestion: answer as never });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = (node: React.ReactElement) => act(() => root.render(node));
const click = (el: Element | null | undefined) => act(() => (el as HTMLElement).click());
const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes(label));

describe("the cost card (TEAM-12)", () => {
  it("has the team's heading, the three lines, and Start and Not now", () => {
    render(<QuestionCard q={card()} />);
    expect(container.querySelector(".question-card-head")?.textContent).toBe("Shop team is about to start");
    const text = container.querySelector(".question-card-text")?.textContent ?? "";
    expect(text).toContain("4 personas: Mira (Claude Sonnet), Atlas (GPT-5).");
    expect(text).toContain("A task like this has cost about $0.40 to $1.20.");
    expect(text).toContain("It stops at $2.00 or after 90 minutes.");
    expect(Array.from(container.querySelectorAll(".question-card-label")).map((e) => e.textContent)).toEqual([
      "Start",
      "Not now",
    ]);
    expect(container.textContent).toContain("Don't ask for this team");
    expect(container.textContent).not.toContain("Something else");
    expect(container.textContent).not.toContain("—");
  });

  it("starts with Start, or says Not now, at one click", () => {
    render(<QuestionCard q={card()} />);
    click(button("Start"));
    expect(answer).toHaveBeenLastCalledWith({ choices: ["Start"] });
    click(button("Not now"));
    expect(answer).toHaveBeenLastCalledWith({ choices: ["Not now"] });
  });

  it("sends the box with Start when it is ticked", () => {
    render(<QuestionCard q={card()} />);
    click(container.querySelector<HTMLInputElement>(".question-card-check input"));
    click(button("Start"));
    expect(answer).toHaveBeenCalledWith({ choices: ["Start", "Don't ask for this team"] });
  });

  it("is still the plain card when it has no heading or box", () => {
    render(<QuestionCard q={card({ title: undefined, check: undefined, question: "Which database?" })} />);
    expect(container.querySelector(".question-card-head")?.textContent).toBe("I need you to decide");
    expect(container.querySelector(".question-card-check")).toBeNull();
    expect(container.textContent).toContain("Something else");
  });
});

describe("a task the limit stopped (TEAM-12)", () => {
  it("says the team reached its limit", () => {
    const m = { id: "m1", role: "assistant", text: "Two items are done.", streaming: false, stopReason: "budget" } as Message;
    render(<AgentRun message={m} />);
    expect(container.querySelector(".run-stopped")?.textContent).toContain("The team reached its limit");
  });
});
