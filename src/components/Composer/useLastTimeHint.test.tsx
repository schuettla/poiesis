/**
 * @vitest-environment jsdom
 *
 * `EPI-UI-1`: "Last time" is asked after a pause, only for the first message of a new
 * conversation, at most three times a conversation, and not again once taken up or put away.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LastTime } from "../../lib/api";

const asked: { conv: string; text: string }[] = [];
let answers: (LastTime | null)[] = [];

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  lastTime: (conv: string, text: string) => {
    asked.push({ conv, text });
    return Promise.resolve(answers.length ? answers.shift()! : null);
  },
}));

import { LAST_TIME_DEBOUNCE_MS, LAST_TIME_MAX_ASKS, useLastTimeHint } from "./useLastTimeHint";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const offer: LastTime = {
  conversation_id: "old-1",
  at: Date.UTC(2026, 9, 12),
  goal: "expense summary",
  line: "I made the expense summary per category.",
  digest: "I made the expense summary per category.",
  skill: null,
};

let container: HTMLDivElement;
let root: Root;
let fx: ReturnType<typeof useLastTimeHint>;
let props: { text: string; convId: string | null; fresh: boolean; on: boolean };

function Probe() {
  fx = useLastTimeHint(props);
  return <span data-testid="offer">{fx.offer?.line ?? ""}</span>;
}
const render = () => act(() => root.render(<Probe />));
const set = (patch: Partial<typeof props>) => {
  props = { ...props, ...patch };
  render();
};
const wait = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const shown = () => container.querySelector("[data-testid=offer]")!.textContent;
const REPORT = "make the expense summary per category from the bank csv";

beforeEach(() => {
  vi.useFakeTimers();
  asked.length = 0;
  answers = [];
  props = { text: "", convId: "c1", fresh: true, on: true };
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  render();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

describe("when it is asked", () => {
  it("is asked after the pause, about the typed text", async () => {
    set({ text: REPORT });
    await wait(LAST_TIME_DEBOUNCE_MS - 1);
    expect(asked).toEqual([]);
    await wait(1);
    expect(asked).toEqual([{ conv: "c1", text: REPORT }]);
  });

  it("is not asked once the conversation has a message, for a short text, a command, or when off", async () => {
    for (const patch of [
      { fresh: false, text: REPORT },
      { text: "make it" },
      { text: "/delegate claude-code fix the tests" },
      { on: false, text: REPORT },
    ]) {
      set({ fresh: true, on: true, ...patch });
      await wait(LAST_TIME_DEBOUNCE_MS * 3);
    }
    expect(asked).toEqual([]);
  });

  it(`is asked at most ${LAST_TIME_MAX_ASKS} times in one conversation`, async () => {
    for (let i = 0; i < 6; i++) {
      set({ text: `${REPORT} ${i}` });
      await wait(LAST_TIME_DEBOUNCE_MS);
    }
    expect(asked).toHaveLength(LAST_TIME_MAX_ASKS);
  });
});

describe("what is offered", () => {
  it("shows the answer, and keeps it while the text changes", async () => {
    answers = [offer];
    set({ text: REPORT });
    await wait(LAST_TIME_DEBOUNCE_MS);
    expect(shown()).toBe(offer.line);
    set({ text: `${REPORT} and sort it` });
    await wait(LAST_TIME_DEBOUNCE_MS * 2);
    expect(shown()).toBe(offer.line);
    expect(asked).toHaveLength(1);
  });

  it("goes when the box is cleared", async () => {
    answers = [offer];
    set({ text: REPORT });
    await wait(LAST_TIME_DEBOUNCE_MS);
    set({ text: "" });
    expect(shown()).toBe("");
  });

  it("does not follow the person into another conversation", async () => {
    answers = [offer];
    set({ text: REPORT });
    await wait(LAST_TIME_DEBOUNCE_MS);
    set({ convId: "c2", text: "" });
    expect(shown()).toBe("");
  });

  it("is taken up once: the offer comes back for the caller and is not asked again", async () => {
    answers = [offer, offer];
    set({ text: REPORT });
    await wait(LAST_TIME_DEBOUNCE_MS);
    let taken: LastTime | null = null;
    act(() => {
      taken = fx.take();
    });
    expect(taken).toEqual(offer);
    expect(shown()).toBe("");
    set({ text: `${REPORT} again` });
    await wait(LAST_TIME_DEBOUNCE_MS * 2);
    expect(asked).toHaveLength(1);
  });

  it("is put away with Start fresh and not asked again in that conversation, but is in the next", async () => {
    answers = [offer, offer];
    set({ text: REPORT });
    await wait(LAST_TIME_DEBOUNCE_MS);
    act(() => fx.dismiss());
    set({ text: `${REPORT} again` });
    await wait(LAST_TIME_DEBOUNCE_MS * 2);
    expect(asked).toHaveLength(1);
    set({ convId: "c2", text: REPORT });
    await wait(LAST_TIME_DEBOUNCE_MS);
    expect(asked).toHaveLength(2);
    expect(shown()).toBe(offer.line);
  });

  it("ignores an answer that came after the text was cleared", async () => {
    let release: (o: LastTime | null) => void = () => {};
    const api = await import("../../lib/api");
    const spy = vi.spyOn(api, "lastTime").mockImplementation(() => new Promise((r) => (release = r)));
    set({ text: REPORT });
    await wait(LAST_TIME_DEBOUNCE_MS);
    set({ text: "" });
    await act(async () => release(offer));
    expect(shown()).toBe("");
    spy.mockRestore();
  });
});
