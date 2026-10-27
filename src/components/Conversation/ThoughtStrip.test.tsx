/**
 * @vitest-environment jsdom
 *
 * `SEE-2`: the strip above the first word and the receipt at the foot. The strip
 * is one line of words, the click only for a group the person turned on, nothing
 * at all when nothing was understood. The receipt lists what acted, each row with
 * "That was wrong", and the numbers are on the hovers.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const decideStart = vi.fn<() => Promise<void>>();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  decideStart: () => decideStart(),
}));

import AgentRun from "./AgentRun";
import { ThoughtReceipt, ThoughtStrip } from "./ThoughtStrip";
import { useAppStore } from "../../lib/store";
import { emptyThought, type TurnThought } from "../../lib/thought";
import type { Message } from "../../lib/types";
import type { DecisionEntry, Thought } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

const WEB = { id: "web_search", words: "Not searching the web", action: "Search the web too" };

const turn = (over: Partial<Thought> = {}, rest: Partial<TurnThought> = {}): TurnThought => ({
  runId: "r1",
  from: 0,
  back: [],
  thought: { ...emptyThought(), ...over },
  ...rest,
});

const message = (thought: TurnThought | undefined, over: Partial<Message> = {}): Message =>
  ({ id: "msg-1", role: "assistant", text: "", streaming: true, thought, ...over }) as Message;

const decision = (over: Partial<DecisionEntry> = {}): DecisionEntry => ({
  id: "d1",
  conversation_id: "c1",
  run_id: "r1",
  purpose: "tool-select",
  label: "",
  background: false,
  experiment: "fast-slow",
  backend: "local:Clef-flash",
  model: "Clef-Flash",
  ms: 30,
  calibrated: false,
  answers: { web_search: { p: 0.97 } },
  input_tokens: 1,
  state_preview: "",
  action: "acted",
  at: 1,
  note: "Left out web search; you did not ask for it",
  outcomes: [],
  labels: [],
  ...over,
});

const restoreHeldTools = vi.fn();
const markReceiptWrong = vi.fn(async () => {});
const reopenPlanItem = vi.fn(async () => {});

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  for (const f of [restoreHeldTools, markReceiptWrong, reopenPlanItem, decideStart]) f.mockReset();
  decideStart.mockResolvedValue();
  useAppStore.setState({ activeConversationId: "c1", restoreHeldTools, markReceiptWrong, reopenPlanItem, busy: false });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = (ui: React.ReactElement) => act(() => root.render(ui));
const q = (sel: string) => container.querySelector<HTMLElement>(sel);
const qa = (sel: string) => Array.from(container.querySelectorAll<HTMLElement>(sel));
const button = (label: string) => qa("button").find((b) => b.textContent === label) ?? null;
const click = (el: HTMLElement | null) => act(() => el!.dispatchEvent(new MouseEvent("click", { bubbles: true })));

describe("the strip (SEE-T-5)", () => {
  it("reads as one line of words, in order, with the speed last", () => {
    render(<ThoughtStrip message={message(turn({ looking: ["in Google Calendar"], left_out: [WEB], ms: 38 }))} />);
    expect(q(".thought-strip")!.textContent).toBe("Looking in Google Calendar · not searching the web" + "Search the web too" + " · 38 ms");
    expect(q(".thought-strip svg")).not.toBeNull();
  });

  it("puts the left-out group back with one click, for that group only", () => {
    render(<ThoughtStrip message={message(turn({ direct: true, left_out: [WEB], ms: 9 }))} />);
    click(button("Search the web too"));
    expect(restoreHeldTools).toHaveBeenCalledWith("c1", ["web_search"]);
  });

  it("stops offering the click once the group is back", () => {
    render(<ThoughtStrip message={message(turn({ direct: true, left_out: [WEB], ms: 9 }, { back: ["web_search"] }))} />);
    expect(button("Search the web too")).toBeNull();
    expect(q(".thought-strip")!.textContent).toBe("Answering directly · 9 ms");
  });

  it("is not there when nothing was understood, and never spins", () => {
    render(<ThoughtStrip message={message(turn({ ms: 40 }))} />);
    expect(q(".thought-strip")).toBeNull();
    render(<ThoughtStrip message={message(undefined)} />);
    expect(q(".thought-strip")).toBeNull();
    expect(container.querySelector("[class*=spin], .orb")).toBeNull();
  });

  it("is not there once the run is over: the receipt takes its place", () => {
    render(<ThoughtStrip message={message(turn({ direct: true }), { streaming: false })} />);
    expect(q(".thought-strip")).toBeNull();
  });

  it("says the decision model is off, with Start while a model is installed, and says when it is on", async () => {
    render(<ThoughtStrip message={message(turn({ engine: "cold" }))} />);
    expect(q(".thought-strip")!.textContent).toContain("Quick thinking is off: the decision model isn't running");
    await act(async () => click(button("Start")));
    expect(decideStart).toHaveBeenCalledTimes(1);
    expect(q(".thought-strip")!.textContent).toContain("Quick thinking is on");
  });

  it("offers no Start when there is no model to start, and shows why a start failed", async () => {
    render(<ThoughtStrip message={message(turn({ engine: "unavailable" }))} />);
    expect(button("Start")).toBeNull();
    decideStart.mockRejectedValueOnce(new Error("Install a decision model first."));
    render(<ThoughtStrip message={message(turn({ engine: "cold" }))} />);
    await act(async () => click(button("Start")));
    expect(q(".thought-error")!.textContent).toContain("Install a decision model first.");
  });
});

describe("the receipt (SEE-T-6)", () => {
  const done = (decisions: DecisionEntry[], over: Partial<Thought> = {}, rest: Partial<TurnThought> = {}) =>
    message(turn({ held: [{ id: "web_search", tokens: 2900 }], ...over }, { decisions, ...rest }), { streaming: false });

  it("is one muted line with the count and the measured tokens, closed", () => {
    render(<ThoughtReceipt message={done([decision(), decision({ id: "d2", note: null, purpose: "steer-class" })])} />);
    expect(q(".thought-line")!.textContent).toBe(`2 quick decisions · prompt ${(2900).toLocaleString()} tokens shorter`);
    expect((q(".thought-receipt") as HTMLDetailsElement).open).toBe(false);
  });

  it("is not there while the run goes, or when no decision was made", () => {
    render(<ThoughtReceipt message={{ ...done([decision()]), streaming: true }} />);
    expect(q(".thought-receipt")).toBeNull();
    render(<ThoughtReceipt message={done([])} />);
    expect(q(".thought-receipt")).toBeNull();
    render(<ThoughtReceipt message={message(turn(), { streaming: false })} />);
    expect(q(".thought-receipt")).toBeNull();
  });

  it("lists a row only for a decision that acted, and counts the rest as changing nothing", () => {
    render(
      <ThoughtReceipt
        message={done([decision(), decision({ id: "d2", note: null }), decision({ id: "d3", note: null })])}
      />
    );
    expect(qa(".thought-row:not(.quiet) .thought-row-text").map((r) => r.textContent)).toEqual([
      "Left out web search; you did not ask for it",
    ]);
    expect(q(".thought-row.quiet")!.textContent).toBe("2 more that changed nothing");
  });

  it("keeps the numbers to the hover", () => {
    render(<ThoughtReceipt message={done([decision()])} />);
    const row = q(".thought-row:not(.quiet)")!;
    expect(row.getAttribute("title")).toBe("local:Clef-flash · Clef-Flash · 30 ms · sure 0.97");
    expect(q(".thought-receipt")!.textContent).not.toMatch(/\d\.\d/);
  });

  it("marks a row wrong from the receipt, naming the groups left out for the tool-menu row", () => {
    render(<ThoughtReceipt message={done([decision()])} />);
    click(button("That was wrong"));
    expect(markReceiptWrong).toHaveBeenCalledWith("c1", "r1", "d1", ["web_search"]);
  });

  it("shows a row already marked, with no second button", () => {
    render(<ThoughtReceipt message={done([decision({ labels: [{ label: "wrong", value: null, source: "receipt" }] })])} />);
    expect(button("That was wrong")).toBeNull();
    expect(q(".thought-marked")!.textContent).toBe("Marked wrong");
  });

  it("offers to reopen a ticked plan step after it was marked wrong, and not before", () => {
    const plan = { revisions: 0, items: [{ text: "a", status: "done" as const }] };
    const tick = decision({
      id: "d9",
      purpose: "plan-done",
      note: "Looks finished from the answer: plan step 1",
      answers: { done: { p: 0.9 } },
    });
    const m = (labels: DecisionEntry["labels"]) => ({
      ...done([{ ...tick, labels }], {}),
      id: "5f2c8e4a-0000-4000-8000-000000000001",
      plan,
    });
    render(<ThoughtReceipt message={m([])} />);
    expect(button("Reopen the step")).toBeNull();
    render(<ThoughtReceipt message={m([{ label: "wrong", value: null, source: "receipt" }])} />);
    click(button("Reopen the step"));
    expect(reopenPlanItem).toHaveBeenCalledWith("5f2c8e4a-0000-4000-8000-000000000001", 0);
  });
});

describe("the sentences part of the receipt (SEE-1)", () => {
  const summary = { sentences: 9, asked: 9, answered: 9, over: 0, pool: 6, sources: 4, factual: 0 };
  const checked = {
    message_id: "msg-1",
    conversation_id: "c1",
    created_at: 1,
    claims: [],
    note: null,
    skipped: 0,
    mode: "auto" as const,
    summary,
  };

  it("counts the sentences the answer checked, and sums them up in a row with nothing to mark wrong", () => {
    useAppStore.setState({ verifications: { "msg-1": { running: false, progress: null, check: checked, error: null } } });
    render(<ThoughtReceipt message={message(turn(), { streaming: false })} />);
    expect(q(".thought-line")!.textContent).toBe("9 sentences checked");
    const rows = qa(".thought-row:not(.quiet)");
    expect(rows.map((r) => r.querySelector(".thought-row-text")!.textContent)).toEqual([
      "Checked 9 sentences against 4 sources",
    ]);
    expect(rows[0].querySelector("button")).toBeNull();
  });

  it("waits for the check, which may finish after the run", () => {
    useAppStore.setState({ verifications: {} });
    render(<ThoughtReceipt message={message(turn(), { streaming: false })} />);
    expect(q(".thought-receipt")).toBeNull();
    act(() =>
      useAppStore.setState({ verifications: { "msg-1": { running: false, progress: null, check: checked, error: null } } })
    );
    expect(q(".thought-receipt")).not.toBeNull();
  });
});

describe("on a turn", () => {
  it("shows the strip while it runs and the receipt when it is over, never both", () => {
    const live = message(turn({ direct: true, ms: 9 }, { decisions: [decision()] }));
    render(<AgentRun message={live} />);
    expect(q(".thought-strip")).not.toBeNull();
    expect(q(".thought-receipt")).toBeNull();
    render(<AgentRun message={{ ...live, streaming: false }} />);
    expect(q(".thought-strip")).toBeNull();
    expect(q(".thought-receipt")).not.toBeNull();
  });
});
