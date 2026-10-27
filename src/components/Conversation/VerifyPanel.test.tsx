/**
 * @vitest-environment jsdom
 *
 * `USE-UI-7`: Verify on an answer. The action shows only while fast-slow is on.
 * A running check shows its progress with Cancel. A finished one underlines the
 * claims that are worth a look, summarises the rest and opens a card with the
 * evidence. Right and wrong go to the store; Revise asks for a new answer.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AgentRun from "./AgentRun";
import { useAppStore } from "../../lib/store";
import { experiment } from "../../lib/experiments.fixture";
import type { Message } from "../../lib/types";
import type { VerifyCheck, VerifyClaim } from "../../lib/api";
import type { VerifyEntry } from "../../lib/verify";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

const TEXT = "The tower is 330 metres tall. It opened in 1889 and Paris is in France.";

const message = (over: Partial<Message> = {}): Message =>
  ({ id: "msg-1", role: "assistant", text: TEXT, streaming: false, ...over }) as Message;

const claim = (over: Partial<VerifyClaim> = {}): VerifyClaim => ({
  id: "c1",
  text: "The tower is 330 metres tall.",
  quote: "330 metres tall",
  source: null,
  importance: 2,
  kind: "numeric",
  finding: "refuted",
  p: 0.9,
  how: "reviewed",
  settled: true,
  fast: "refuted",
  slow: "refuted",
  reason: "Evidence 1 gives 324 metres.",
  passages: [
    {
      text: "The tower is 324 metres tall including antennas.",
      source: "wikipedia.org",
      url: null,
      date: "2024-03-05",
      retrieved_at: new Date(2026, 9, 23, 12).getTime(),
      via: "web",
    },
  ],
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
  mode: "deep",
  summary: null,
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
const reviseFromVerification = vi.fn(async () => {});
const loadVerifications = vi.fn(async () => {});

let container: HTMLDivElement;
let root: Root;

const on = (isOn: boolean) => useAppStore.setState({ experiments: [experiment({ id: "fast-slow", on: isOn })] });

beforeEach(() => {
  for (const f of [verifyMessage, cancelVerify, markClaim, reviseFromVerification, loadVerifications]) f.mockClear();
  useAppStore.setState({
    busy: false,
    activeConversationId: "c1",
    experiments: [experiment({ id: "fast-slow", on: true })],
    verifications: {},
    verifyMessage,
    cancelVerify,
    markClaim,
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

function render(m: Message = message()) {
  act(() => {
    root.render(<AgentRun message={m} last />);
  });
}

const q = (sel: string) => container.querySelector<HTMLElement>(sel);
const qa = (sel: string) => Array.from(container.querySelectorAll<HTMLElement>(sel));
const actions = () => qa(".turn-actions button").map((b) => b.textContent);
const click = (el: HTMLElement | null) => act(() => el!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
const button = (label: string) => qa("button").find((b) => b.textContent === label) ?? null;

describe("the Check more deeply action", () => {
  it("is offered on a finished answer while fast-slow is on", () => {
    render();
    expect(actions()).toContain("Check more deeply");
    click(button("Check more deeply"));
    expect(verifyMessage).toHaveBeenCalledWith("msg-1");
  });

  it("is not there while the experiment is off, or before the experiments were read", () => {
    on(false);
    render();
    expect(actions()).not.toContain("Check more deeply");
    useAppStore.setState({ experiments: null });
    render();
    expect(actions()).not.toContain("Check more deeply");
  });

  // `SEE-T-9`: Verify is part of fast-slow. The old experiment's id means nothing.
  it("follows fast-slow and not the old verify id", () => {
    useAppStore.setState({ experiments: [experiment({ id: "verify", on: true })] });
    render();
    expect(actions()).not.toContain("Check more deeply");
    useAppStore.setState({ experiments: [experiment({ id: "fast-slow", on: true })] });
    render();
    expect(actions()).toContain("Check more deeply");
  });

  it("is not there for a line that holds no claim, a streaming turn or a busy chat", () => {
    render(message({ text: "Done." }));
    expect(actions()).not.toContain("Check more deeply");
    render(message({ streaming: true }));
    expect(actions()).not.toContain("Check more deeply");
    act(() => useAppStore.setState({ busy: true }));
    render();
    expect(actions()).not.toContain("Check more deeply");
  });

  it("keeps its name once there is a check, and is gone while one runs", () => {
    useAppStore.setState({ verifications: { "msg-1": entry({ check: check([claim()]) }) } });
    render();
    expect(actions()).toContain("Check more deeply");
    act(() => useAppStore.setState({ verifications: { "msg-1": entry({ running: true }) } }));
    expect(actions()).not.toContain("Check more deeply");
  });

  it("reads the chat's kept checks when the experiment is on", () => {
    render();
    expect(loadVerifications).toHaveBeenCalledWith("c1");
    loadVerifications.mockClear();
    on(false);
    render();
    expect(loadVerifications).not.toHaveBeenCalled();
  });
});

describe("a check that is running", () => {
  it("shows its progress with Cancel", () => {
    useAppStore.setState({
      verifications: { "msg-1": entry({ running: true, progress: { phase: "checking", done: 4, total: 13 } }) },
    });
    render();
    expect(q(".verify-row")!.textContent).toContain("Checking 13 claims · 4 done");
    click(button("Cancel"));
    expect(cancelVerify).toHaveBeenCalledTimes(1);
  });
});

describe("a check that is done", () => {
  const claims = [
    claim(),
    claim({ id: "c2", text: "It opened in 1889.", quote: "opened in 1889", finding: "supported", how: "fast", reason: null }),
    claim({ id: "c3", text: "Paris is in France.", quote: "Paris is in France", finding: "insufficient", how: "none", p: null, passages: [], settled: true }),
  ];
  beforeEach(() => {
    useAppStore.setState({ verifications: { "msg-1": entry({ check: check(claims, { skipped: 2, note: "One note." }) }) } });
  });

  it("underlines what is worth a look and leaves supported claims plain", () => {
    render();
    const marks = qa("mark.vf-mark");
    expect(marks.map((m) => m.textContent)).toEqual(["330 metres tall", "Paris is in France"]);
    expect(marks.map((m) => m.className)).toEqual(["vf-mark vf-refuted", "vf-mark vf-insufficient"]);
    expect(marks[0].getAttribute("title")).toBe("Refuted, reviewed. Click for the evidence.");
    // The text itself is unchanged.
    expect(q(".run-text")!.textContent).toBe(TEXT);
  });

  it("underlines supported claims when asked", () => {
    render();
    const toggle = q(".verify-toggle input") as HTMLInputElement;
    click(toggle);
    expect(qa("mark.vf-supported").map((m) => m.textContent)).toEqual(["opened in 1889"]);
  });

  it("summarises, says what was left out and shows the note", () => {
    render();
    const panel = q(".verify-panel")!.textContent!;
    expect(panel).toContain("1 supported · 1 refuted · 1 not confirmed");
    expect(panel).toContain("2 opinions left out");
    expect(panel).toContain("One note.");
  });

  it("lists every claim, and a claim with no words to mark is still in the list", () => {
    useAppStore.setState({
      verifications: { "msg-1": entry({ check: check([claim({ quote: "" })]) }) },
    });
    render();
    expect(qa("mark.vf-mark")).toHaveLength(0);
    expect(qa(".verify-claim").map((b) => b.textContent)).toEqual(["RefutedThe tower is 330 metres tall."]);
  });

  it("opens a card with the evidence when an underline is clicked, and closes it on a second click", () => {
    render();
    expect(q(".verify-card")).toBeNull();
    click(qa("mark.vf-mark")[0]);
    const card = q(".verify-card")!;
    expect(card.textContent).toContain("Refuted");
    expect(card.textContent).toContain("Reviewed against the evidence");
    expect(card.textContent).toContain("Evidence 1 gives 324 metres.");
    expect(card.textContent).toContain("The tower is 324 metres tall including antennas.");
    expect(card.textContent).toContain("wikipedia.org · dated 2024-03-05 · read 23 Oct · the web");
    expect(qa("mark.vf-mark")[0].className).toContain("picked");
    click(qa("mark.vf-mark")[0]);
    expect(q(".verify-card")).toBeNull();
  });

  it("opens the card from the list, and says so when there was no evidence", () => {
    render();
    click(qa(".verify-claim")[2]);
    expect(q(".verify-card")!.textContent).toContain("No evidence was found.");
  });

  it("sends right and wrong to the store", () => {
    render();
    click(qa("mark.vf-mark")[0]);
    click(button("This is wrong"));
    expect(markClaim).toHaveBeenCalledWith("msg-1", "c1", "wrong");
    click(button("This is right"));
    expect(markClaim).toHaveBeenLastCalledWith("msg-1", "c1", "right");
  });

  it("shows the person's word on the card", () => {
    useAppStore.setState({
      verifications: { "msg-1": entry({ check: check([claim({ mark: "wrong" })]) }) },
    });
    render();
    click(qa("mark.vf-mark")[0]);
    expect(button("This is wrong")!.getAttribute("aria-pressed")).toBe("true");
    expect(button("This is right")!.getAttribute("aria-pressed")).toBe("false");
  });

  it("asks for a new answer with Revise, and the old text stays", () => {
    render();
    click(button("Revise"));
    expect(reviseFromVerification).toHaveBeenCalledWith("msg-1");
    expect(q(".run-text")!.textContent).toBe(TEXT);
  });

  it("shows why Revise failed", async () => {
    reviseFromVerification.mockRejectedValueOnce(new Error("There is nothing to revise."));
    render();
    await act(async () => {
      click(button("Revise"));
    });
    expect(q(".verify-error")!.textContent).toBe("There is nothing to revise.");
  });

  it("offers no Revise when every claim was supported", () => {
    useAppStore.setState({
      verifications: { "msg-1": entry({ check: check([claim({ finding: "supported" })]) }) },
    });
    render();
    expect(button("Revise")).toBeNull();
  });
});

describe("a check that failed", () => {
  it("shows the error and still lets the answer be checked again", () => {
    useAppStore.setState({ verifications: { "msg-1": entry({ error: "No model is loaded." }) } });
    render();
    expect(q(".verify-error")!.textContent).toBe("No model is loaded.");
    expect(actions()).toContain("Check more deeply");
  });
});

describe("with fast-slow off", () => {
  it("draws the answer with no underline and no panel, whatever was kept", () => {
    on(false);
    useAppStore.setState({ verifications: { "msg-1": entry({ check: check([claim()]) }) } });
    render();
    expect(qa("mark")).toHaveLength(0);
    expect(q(".verify-panel")).toBeNull();
  });
});
