/**
 * @vitest-environment jsdom
 *
 * `SEE-UI-1`: the check every answer gets while it streams. A live check shows its count and
 * the backed sentences sweep; a finished one shows the trust line, whose parts filter the claim
 * list; an answer that read nothing says so and offers the web; a sentence the assistant says it
 * did and no step did is red.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

let webOn = true;
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  listToolsets: async () => [{ id: "web_search", label: "Web search", description: "", enabled: webOn, sensitive: true }],
}));

import CheckedAnswer from "./VerifyPanel";
import { useAppStore } from "../../lib/store";
import { experiment } from "../../lib/experiments.fixture";
import type { Message } from "../../lib/types";
import type { VerifyCheck, VerifyClaim } from "../../lib/api";
import type { VerifyEntry } from "../../lib/verify";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const TEXT = "Paris is the capital of France. The tower is 450 metres tall. I sent the email to Ana.";

const message = (over: Partial<Message> = {}): Message =>
  ({ id: "msg-1", role: "assistant", text: TEXT, streaming: false, ...over }) as Message;

const claim = (over: Partial<VerifyClaim> = {}): VerifyClaim => ({
  id: "s1",
  text: "Paris is the capital of France.",
  quote: "Paris is the capital of France.",
  source: null,
  importance: 2,
  kind: "fact",
  finding: "supported",
  p: 0.95,
  how: "fast",
  settled: true,
  fast: "supported",
  slow: null,
  reason: null,
  passages: [{ text: "Paris is the capital of France.", source: "trip.md", url: null, date: null, retrieved_at: 1, via: "folder" }],
  decision: "d1",
  mark: null,
  ...over,
});

const check = (claims: VerifyClaim[], over: Partial<VerifyCheck> = {}): VerifyCheck => ({
  message_id: "msg-1",
  conversation_id: "c1",
  created_at: 1,
  claims,
  note: null,
  skipped: 0,
  mode: "auto",
  summary: { sentences: 3, asked: 3, answered: 3, over: 0, pool: 4, sources: 2, factual: 0 },
  ...over,
});

const entry = (over: Partial<VerifyEntry> = {}): VerifyEntry => ({
  running: false,
  progress: null,
  check: null,
  error: null,
  ...over,
});

const verifyMessage = vi.fn(async () => {});
const cancelVerify = vi.fn();
const markClaim = vi.fn(async () => {});
const reviewClaim = vi.fn(async () => {});
const reviseFromVerification = vi.fn(async () => {});
const loadVerifications = vi.fn(async () => {});

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  webOn = true;
  for (const f of [verifyMessage, cancelVerify, markClaim, reviewClaim, reviseFromVerification, loadVerifications]) f.mockClear();
  useAppStore.setState({
    busy: false,
    activeConversationId: "c1",
    experiments: [experiment({ id: "fast-slow", on: true })],
    verifications: {},
    verifyMessage,
    cancelVerify,
    markClaim,
    reviewClaim,
    reviseFromVerification,
    loadVerifications,
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = (m: Message = message()) => act(() => root.render(<CheckedAnswer message={m} />));
const q = (sel: string) => container.querySelector<HTMLElement>(sel);
const qa = (sel: string) => Array.from(container.querySelectorAll<HTMLElement>(sel));
const click = (el: HTMLElement | null) => act(() => el!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
const button = (label: string) => qa("button").find((b) => b.textContent === label) ?? null;
const set = (e: VerifyEntry) => act(() => useAppStore.setState({ verifications: { "msg-1": e } }));

describe("a check that is running", () => {
  it("counts the sentences and offers no Cancel", () => {
    render(message({ streaming: true }));
    set(entry({ running: true, progress: { phase: "auto", done: 1, total: 3 }, check: check([claim()]) }));
    const row = q(".trust-line")!;
    expect(row.textContent).toBe("Checking against what I read · 1 of 3");
    expect(button("Cancel")).toBeNull();
    expect(row.getAttribute("role")).toBe("status");
  });

  it("still cancels a deep check", () => {
    set(entry({ running: true, progress: { phase: "checking", done: 1, total: 3 } }));
    render();
    click(button("Cancel"));
    expect(cancelVerify).toHaveBeenCalledTimes(1);
  });

  it("draws a backed sentence as the check passes over it, and only one that arrives live", () => {
    render(message({ streaming: true }));
    set(entry({ running: true, progress: { phase: "auto", done: 0, total: 3 } }));
    set(entry({ running: true, progress: { phase: "auto", done: 1, total: 3 }, check: check([claim()]) }));
    const marks = qa("mark.vf-mark");
    expect(marks.map((m) => m.textContent)).toEqual(["Paris is the capital of France."]);
    expect(marks[0].className).toContain("vf-sweep");
    // The words themselves are untouched.
    expect(q(".run-text")!.textContent).toBe(TEXT);
  });

  it("does not sweep a check that was read from storage", () => {
    set(entry({ check: check([claim()]) }));
    render();
    expect(qa("mark.vf-mark")).toHaveLength(0);
  });

  it("settles back to plain text once the sweep is over", () => {
    vi.useFakeTimers();
    try {
      render(message({ streaming: true }));
      set(entry({ running: true, progress: { phase: "auto", done: 0, total: 3 } }));
      set(entry({ running: true, progress: { phase: "auto", done: 1, total: 3 }, check: check([claim()]) }));
      expect(qa("mark.vf-sweep")).toHaveLength(1);
      act(() => {
        vi.advanceTimersByTime(1700);
      });
      expect(qa("mark.vf-mark")).toHaveLength(0);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("the trust line", () => {
  const claims = [
    claim(),
    claim({
      id: "s2",
      text: "The tower is 450 metres tall.",
      quote: "The tower is 450 metres tall.",
      finding: "refuted",
      fast: "refuted",
      how: "reviewed",
      reason: "Evidence 1 says 330.",
      passages: [{ text: "The tower is 330 metres tall.", source: "nytimes.com", url: null, date: null, retrieved_at: 1, via: "web" }],
    }),
    claim({
      id: "s3",
      text: "I sent the email to Ana.",
      quote: "I sent the email to Ana.",
      kind: "action",
      finding: "undone",
      how: "ledger",
      reason: "No step in this run did this.",
      passages: [],
    }),
  ];

  beforeEach(() => set(entry({ check: check(claims) })));

  it("counts what there is, in words, in one line", () => {
    render();
    expect(q(".trust-line")!.textContent).toContain("1 backed by what I read");
    expect(q(".trust-line")!.textContent).toContain("1 contradicts nytimes.com");
    expect(q(".trust-line")!.textContent).toContain("1 said done, no step did it");
    expect(q(".trust-line .verify-summary")).toBeNull();
  });

  it("marks what is worth a look, red for a contradiction and for something no step did", () => {
    render();
    const marks = qa("mark.vf-mark");
    expect(marks.map((m) => m.className)).toEqual(["vf-mark vf-refuted", "vf-mark vf-undone"]);
    expect(marks[1].getAttribute("title")).toBe("No step did this. Click for the details.");
  });

  it("keeps the claim list closed until a part of the line is picked, then lists only those", () => {
    render();
    const list = q("details.verify-claims") as HTMLDetailsElement;
    expect(list.open).toBe(false);
    click(button("1 contradicts nytimes.com"));
    expect((q("details.verify-claims") as HTMLDetailsElement).open).toBe(true);
    expect(qa(".verify-claim").map((b) => b.textContent)).toEqual(["RefutedThe tower is 450 metres tall."]);
    expect(q("details.verify-claims summary")!.textContent).toBe("1 of 3 claims");
    // A second click on the same part shows them all again.
    click(button("1 contradicts nytimes.com"));
    expect(qa(".verify-claim")).toHaveLength(3);
  });

  it("puts Revise on the line when anything is red, and asks for a new answer", () => {
    render();
    click(button("Revise"));
    expect(reviseFromVerification).toHaveBeenCalledWith("msg-1");
  });

  it("says when only the first sentences were checked", () => {
    set(entry({ check: check(claims, { summary: { sentences: 40, asked: 40, answered: 40, over: 6, pool: 4, sources: 2, factual: 0 } }) }));
    render();
    expect(q(".trust-line")!.textContent).toContain("first 40 sentences checked");
  });

  it("opens the card of a sentence no step did, with nothing to review it against", () => {
    render();
    click(qa("mark.vf-mark")[1]);
    const card = q(".verify-card")!;
    expect(card.textContent).toContain("No step did this");
    expect(card.textContent).toContain("Matched against what this run did");
    expect(card.textContent).toContain("No step in this run did this.");
    expect(card.textContent).toContain("No step of this run matches.");
    expect(button("Review this")).toBeNull();
  });
});

describe("Review this", () => {
  it("is on the card of a sentence the slow model has not read, and asks for the review", () => {
    set(entry({ check: check([claim({ finding: "refuted", fast: "refuted", settled: false })]) }));
    render();
    click(qa("mark.vf-mark")[0]);
    // The number is in the card and nowhere else.
    expect(q(".verify-card")!.textContent).toContain("Quick verdict, not sure (0.95)");
    expect(q(".trust-line")!.textContent).not.toMatch(/\d\.\d/);
    click(button("Review this"));
    expect(reviewClaim).toHaveBeenCalledWith("msg-1", "s1");
  });

  it("is not there once it was reviewed", () => {
    set(entry({ check: check([claim({ finding: "refuted", how: "reviewed", slow: "refuted" })]) }));
    render();
    click(qa("mark.vf-mark")[0]);
    expect(button("Review this")).toBeNull();
  });

  it("shows why a review failed", async () => {
    reviewClaim.mockRejectedValueOnce(new Error("The review gave no usable answer."));
    set(entry({ check: check([claim({ finding: "refuted", fast: "refuted", settled: false })]) }));
    render();
    click(qa("mark.vf-mark")[0]);
    await act(async () => {
      click(button("Review this"));
    });
    expect(q(".verify-card .verify-error")!.textContent).toBe("The review gave no usable answer.");
  });
});

describe("an answer that read nothing", () => {
  const knowledge = check([], { summary: { sentences: 4, asked: 0, answered: 0, over: 0, pool: 0, sources: 0, factual: 3 } });

  it("says it came from what I know, and offers the web while Web search is on", async () => {
    set(entry({ check: knowledge }));
    await act(async () => render());
    expect(q(".trust-line")!.textContent).toContain("Answered from what I know · nothing checked");
    click(button("Check against the web"));
    expect(verifyMessage).toHaveBeenCalledWith("msg-1");
    expect(qa("mark.vf-mark")).toHaveLength(0);
  });

  it("offers no web check while Web search is off", async () => {
    webOn = false;
    set(entry({ check: knowledge }));
    await act(async () => render());
    expect(q(".trust-line")!.textContent).toContain("Answered from what I know · nothing checked");
    expect(button("Check against the web")).toBeNull();
  });

  it("says nothing at all when the answer held nothing like a fact", async () => {
    set(entry({ check: check([], { summary: { sentences: 1, asked: 0, answered: 0, over: 0, pool: 0, sources: 0, factual: 0 } }) }));
    await act(async () => render());
    expect(q(".trust-line .verify-summary")).toBeNull();
  });
});

describe("with fast-slow off", () => {
  it("draws the answer with no mark and no line", () => {
    act(() => useAppStore.setState({ experiments: [experiment({ id: "fast-slow", on: false })] }));
    set(entry({ check: check([claim({ finding: "refuted" })]) }));
    render();
    expect(qa("mark")).toHaveLength(0);
    expect(q(".verify-panel")).toBeNull();
    expect(q(".trust-line")).toBeNull();
  });
});
