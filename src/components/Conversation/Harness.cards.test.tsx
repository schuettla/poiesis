/**
 * @vitest-environment jsdom
 *
 * `AGC-T2`, `AGC-4`, `BTW-UI-1`, `RWD-UI-1`, `CHK-UI-1`: the small cards the agent's
 * side and the recovery commands put on screen. Each one asks a person something
 * and carries the answer to one place in the store.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import QuestionCard from "./QuestionCard";
import HarnessProposal from "./HarnessProposal";
import CheckupCard from "./CheckupCard";
import RewindDialog from "./RewindDialog";
import CommandNote from "./CommandNote";
import BtwCard from "../Composer/BtwCard";
import SuggestionChip from "../Composer/SuggestionChip";
import { useAppStore } from "../../lib/store";
import type { CheckupLine } from "../../lib/api";
import type { PendingQuestion } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    busy: false,
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = (node: React.ReactElement) => act(() => root.render(node));
const click = (el: Element | null | undefined) => act(() => (el as HTMLElement).click());
const texts = (selector: string) => Array.from(container.querySelectorAll(selector)).map((e) => e.textContent);

describe("a question (AGC-3)", () => {
  const question = (over: Partial<PendingQuestion> = {}): PendingQuestion => ({
    runId: "r1",
    id: "q1",
    convId: "c1",
    messageId: "m1",
    question: "Which database?",
    options: [{ label: "SQLite", detail: "one file" }, { label: "Postgres" }],
    multi: false,
    ...over,
  });
  let answer: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    answer = vi.fn().mockResolvedValue(true);
    useAppStore.setState({ answerQuestion: answer as never });
  });

  it("says what it is, shows the question, and numbers the options", () => {
    render(<QuestionCard q={question()} />);
    expect(container.querySelector(".question-card-head")?.textContent).toBe("I need you to decide");
    expect(container.querySelector(".question-card-text")?.textContent).toBe("Which database?");
    expect(texts(".question-card-key")).toEqual(["1", "2"]);
    expect(container.textContent).toContain("one file");
  });

  it("answers with the option you click, at once", () => {
    render(<QuestionCard q={question()} />);
    click(container.querySelectorAll(".question-card-option")[1]);
    expect(answer).toHaveBeenCalledWith({ choices: ["Postgres"] });
  });

  it("answers with the number key when nothing editable has focus", () => {
    render(<QuestionCard q={question()} />);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "1", bubbles: true }));
    });
    expect(answer).toHaveBeenCalledWith({ choices: ["SQLite"] });
  });

  it("leaves the digits to the composer while you are typing in it", () => {
    const input = document.createElement("input");
    document.body.appendChild(input);
    render(<QuestionCard q={question()} />);
    act(() => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "1", bubbles: true }));
    });
    expect(answer).not.toHaveBeenCalled();
    input.remove();
  });

  it("lets several be chosen, and sends them together", () => {
    render(<QuestionCard q={question({ multi: true })} />);
    const [a, b] = Array.from(container.querySelectorAll(".question-card-option"));
    click(a);
    click(b);
    expect(answer).not.toHaveBeenCalled();
    const send = Array.from(container.querySelectorAll("button")).find((x) => x.textContent === "Send");
    click(send);
    expect(answer).toHaveBeenCalledWith({ choices: ["SQLite", "Postgres"], text: undefined });
  });

  it("always has a way to say it in your own words", () => {
    render(<QuestionCard q={question()} />);
    click(Array.from(container.querySelectorAll("button")).find((x) => x.textContent?.startsWith("Something else")));
    const input = container.querySelector<HTMLInputElement>(".question-card-input")!;
    act(() => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(input, "whichever is faster");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    click(Array.from(container.querySelectorAll("button")).find((x) => x.textContent === "Send"));
    expect(answer).toHaveBeenCalledWith({ choices: [], text: "whichever is faster" });
  });
});

describe("something I asked to change (AGC-4)", () => {
  const resolve = vi.fn().mockResolvedValue(undefined);
  beforeEach(() => {
    resolve.mockClear();
    useAppStore.setState({ resolveHarnessProposal: resolve });
  });
  const proposal = (name: "switch_mode" | "schedule", payload: object) => ({
    id: "p1",
    runId: "r1",
    convId: "c1",
    messageId: "m1",
    name,
    reason: "it is a big change",
    payload,
  });

  it("asks to switch a mode in the first person, with Switch and Not now", () => {
    render(<HarnessProposal proposal={proposal("switch_mode", { mode: "plan_first" })} />);
    expect(container.textContent).toContain("I'd like to switch to Plan first: it is a big change");
    expect(texts(".harness-proposal-actions button")).toEqual(["Switch", "Not now"]);
  });

  it("asks to schedule, saying when and what, with Set it up and Not now", () => {
    render(<HarnessProposal proposal={proposal("schedule", { when: "every weekday at 9", task: "summarise my mail" })} />);
    expect(container.textContent).toContain("I'd like to run this every weekday at 9: summarise my mail");
    expect(texts(".harness-proposal-actions button")).toEqual(["Set it up", "Not now"]);
  });

  it("carries your answer to the store", () => {
    render(<HarnessProposal proposal={proposal("switch_mode", { mode: "workspace" })} />);
    const [yes, no] = Array.from(container.querySelectorAll(".harness-proposal-actions button"));
    click(yes);
    expect(resolve).toHaveBeenLastCalledWith("p1", true);
    click(no);
    expect(resolve).toHaveBeenLastCalledWith("p1", false);
  });
});

describe("a suggestion (AGC-2)", () => {
  const suggest = (command: string, convId = "c1") =>
    useAppStore.setState({ activeSuggestion: { convId, runId: "r1", command, reason: "That worked well" } });

  it("offers one command, with Do it and Not now", () => {
    suggest("compact");
    useAppStore.setState({
      conversations: [
        {
          id: "c1",
          title: "t",
          updatedAt: 0,
          messages: [
            { id: "m1", role: "user", text: "hi", createdAt: 0 },
            { id: "m2", role: "assistant", text: "hello", createdAt: 0 },
          ],
        },
      ],
    });
    render(<SuggestionChip />);
    expect(container.textContent).toContain("That worked well");
    expect(container.textContent).toContain("/compact");
    expect(texts(".suggestion-chip-actions button")).toEqual(["Do it", "Not now"]);
  });

  it("says nothing for a command that could not run right now", () => {
    suggest("compact"); // no conversation to compact
    render(<SuggestionChip />);
    expect(container.querySelector(".suggestion-chip")).toBeNull();
  });

  it("stays out of another chat", () => {
    suggest("compact", "other");
    render(<SuggestionChip />);
    expect(container.querySelector(".suggestion-chip")).toBeNull();
  });
});

describe("a side answer (BTW-UI-1)", () => {
  const side = (over: object) =>
    act(() =>
      useAppStore.setState({
        sideAnswer: { convId: "c1", question: "which file was it?", answer: "", status: "waiting", ...over },
      } as never)
    );

  it("says it is waiting on my engine until the first word", () => {
    side({});
    render(<BtwCard />);
    expect(container.textContent).toContain("waiting for my engine…");
    expect(texts(".btw-card-actions button")).toEqual(["Dismiss"]);
  });

  it("offers Keep in chat only once the answer is whole", () => {
    side({ status: "streaming", answer: "parser" });
    render(<BtwCard />);
    expect(texts(".btw-card-actions button")).toEqual(["Dismiss"]);
    side({ status: "done", answer: "parser.rs" });
    expect(texts(".btw-card-actions button")).toEqual(["Keep in chat", "Dismiss"]);
  });

  it("goes with Escape", () => {
    const dismiss = vi.fn();
    useAppStore.setState({ dismissSideAnswer: dismiss });
    side({ status: "done", answer: "x" });
    render(<BtwCard />);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(dismiss).toHaveBeenCalled();
  });

  it("is not shown in another chat", () => {
    side({ convId: "other" });
    render(<BtwCard />);
    expect(container.querySelector(".btw-card")).toBeNull();
  });
});

describe("going back (RWD-UI-1)", () => {
  const rewindTo = vi.fn().mockResolvedValue(undefined);
  beforeEach(() => {
    rewindTo.mockClear();
    useAppStore.setState({
      rewindTo,
      rewindRequest: "m1",
      conversations: [
        {
          id: "c1",
          title: "t",
          updatedAt: 0,
          messages: [{ id: "m1", role: "user", text: "refactor the parser\nplease", createdAt: 0 }],
        },
      ],
    });
  });

  it("names the turn and asks whether to put your words back", () => {
    render(<RewindDialog />);
    expect(document.body.textContent).toContain("Go back to before “refactor the parser”?");
    expect(document.body.textContent).toContain("Put your message back in the box");
    // With nothing changed since, there is no question about files.
    expect(document.body.textContent).not.toContain("take back my changes");
  });

  it("rewinds with what you chose", () => {
    render(<RewindDialog />);
    click(Array.from(document.querySelectorAll("button")).find((b) => b.textContent === "Rewind"));
    expect(rewindTo).toHaveBeenCalledWith("m1", { undoFiles: false, putBack: true });
    expect(useAppStore.getState().rewindRequest).toBeNull();
  });

  it("goes away without doing anything on Cancel", () => {
    render(<RewindDialog />);
    click(Array.from(document.querySelectorAll("button")).find((b) => b.textContent === "Cancel"));
    expect(rewindTo).not.toHaveBeenCalled();
    expect(useAppStore.getState().rewindRequest).toBeNull();
  });
});

describe("a checkup (CHK-UI-1)", () => {
  const lines: CheckupLine[] = [
    { area: "engine", state: "fine", text: "My engine is running gemma.", action: null },
    {
      area: "tools",
      state: "needs_you",
      text: "browse has been failing for me lately (2 of 10).",
      action: { label: "Open Health", target: "health" },
    },
    { area: "recall", state: "off", text: "I can't search inside folders.", action: { label: "Open Runtime", target: "recall" } },
  ];

  it("says each line in words, never a colour, and links only where there is something to do", () => {
    render(<CheckupCard lines={lines} summary="I checked myself and one thing needs you." at="14:02" />);
    expect(texts(".checkup-card-state")).toEqual(["fine", "needs you", "off"]);
    expect(texts(".checkup-card-link")).toEqual(["Open Health", "Open Runtime"]);
    expect(container.textContent).toContain("I checked myself and one thing needs you.");
  });

  it("sends a link where it says", () => {
    const openSelf = vi.fn();
    useAppStore.setState({ openSelf });
    render(<CheckupCard lines={lines} at="14:02" />);
    click(container.querySelectorAll(".checkup-card-link")[0]);
    expect(openSelf).toHaveBeenCalledWith("health");
  });

  it("comes back as the same card from its own trace after a reload", () => {
    render(
      <CommandNote
        note={{
          id: "n1",
          name: "checkup",
          args: JSON.stringify(lines),
          by: "user",
          outcome: "done",
          note: "I checked myself and one thing needs you.",
          at: 0,
        }}
      />
    );
    expect(container.querySelector(".checkup-card")).not.toBeNull();
    expect(texts(".checkup-card-state")).toHaveLength(3);
  });

  it("falls back to its sentence when the trace cannot be read", () => {
    render(
      <CommandNote
        note={{ id: "n2", name: "checkup", args: "not json", by: "user", outcome: "done", note: "I checked myself.", at: 0 }}
      />
    );
    expect(container.querySelector(".checkup-card")).toBeNull();
  });
});

describe("your answer to something I asked, in the transcript (AGC-4)", () => {
  it("reads as a whole sentence, once, without repeating who", () => {
    render(
      <CommandNote
        note={{
          id: "n3",
          name: "suggest",
          args: "skillify",
          by: "user",
          outcome: "declined",
          note: "you said not now to /skillify",
          at: 0,
        }}
      />
    );
    expect(container.textContent).toMatch(/^You said not now to \/skillify/);
    expect(container.textContent).not.toContain("· you ·");
  });
});
