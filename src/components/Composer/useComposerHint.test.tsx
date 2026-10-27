/**
 * @vitest-environment jsdom
 *
 * `SEE-3`, `SEE-T-7`: the composer reads a message after a pause, shows at most one line,
 * does not flicker, ignores an answer from before the last keystroke, and does not bring
 * back a hint that was taken up or put away.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ComposerHint } from "../../lib/api";

const asked: { conv: string; text: string; runId?: string }[] = [];
const labels: { decision: string; kind: string; taken: boolean }[] = [];
let answers: (ComposerHint | null)[] = [];
let hold = false;
let release: ((h: ComposerHint | null) => void) | null = null;

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  composerHint: (conv: string, text: string, runId?: string) => {
    asked.push({ conv, text, runId });
    if (hold) return new Promise<ComposerHint | null>((r) => (release = r));
    return Promise.resolve(answers.length ? answers.shift()! : null);
  },
  composerHintLabel: (decision: string, kind: string, taken: boolean) => {
    labels.push({ decision, kind, taken });
    return Promise.resolve();
  },
}));

import { HINT_DEBOUNCE_MS, hintAskable, useComposerHint } from "./useComposerHint";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const web: ComposerHint = {
  kind: "web",
  text: "Needs the web, which is off",
  title: "90% sure",
  action: "Turn on for this message",
  decision: "d-web",
  p: 0.9,
};
const handover: ComposerHint = {
  kind: "handover",
  text: "Claude Code may take this (your rule U2)",
  title: "91% sure",
  action: "Do it yourself",
  decision: "d-hand",
  p: 0.91,
};

let container: HTMLDivElement;
let root: Root;
let api: ReturnType<typeof useComposerHint>;
let props: { text: string; convId: string | null; runId?: string; on: boolean };

function Probe() {
  api = useComposerHint(props);
  return <span data-testid="hint">{api.hint?.text ?? ""}</span>;
}

const render = () => act(() => root.render(<Probe />));
const set = (patch: Partial<typeof props>) => {
  props = { ...props, ...patch };
  render();
};
/** Time passes, and the answers that were waiting arrive. */
const wait = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const shown = () => container.querySelector("[data-testid=hint]")!.textContent;

beforeEach(() => {
  vi.useFakeTimers();
  asked.length = 0;
  labels.length = 0;
  answers = [];
  hold = false;
  release = null;
  props = { text: "", convId: "c1", on: true };
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

describe("what is read", () => {
  it("is twelve characters or more, and never a / command", () => {
    expect(hintAskable("fix the failing tests")).toBe(true);
    expect(hintAskable("  twelve chars!  ")).toBe(true);
    expect(hintAskable("fix it")).toBe(false);
    expect(hintAskable("/delegate claude-code fix the tests")).toBe(false);
    expect(hintAskable("ääääääääääää")).toBe(true);
    expect(hintAskable("äääääääääää")).toBe(false);
  });

  it("is asked 400 ms after the last keystroke and not before", async () => {
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS - 1);
    expect(asked).toEqual([]);
    await wait(1);
    expect(asked).toEqual([{ conv: "c1", text: "look up the latest release notes", runId: undefined }]);
  });

  it("is asked once for a burst of typing, about the last text", async () => {
    for (const text of ["look up the latest", "look up the latest release", "look up the latest release notes"]) {
      set({ text });
      await wait(200);
    }
    await wait(HINT_DEBOUNCE_MS);
    expect(asked.map((a) => a.text)).toEqual(["look up the latest release notes"]);
  });

  it("is not asked for a short text, a command, no chat, or when it is off", async () => {
    for (const patch of [
      { text: "fix it" },
      { text: "/delegate claude-code fix the tests" },
      { text: "look up the latest release notes", convId: null },
      { text: "look up the latest release notes", on: false },
    ]) {
      set({ convId: "c1", on: true, ...patch });
      await wait(HINT_DEBOUNCE_MS * 3);
    }
    expect(asked).toEqual([]);
  });

  it("goes with the run's id, so a message to a working run is read as one", async () => {
    set({ text: "actually forget the whole thing", runId: "r1" });
    await wait(HINT_DEBOUNCE_MS);
    expect(asked[0].runId).toBe("r1");
  });
});

describe("what is shown", () => {
  it("is nothing until the answer has held for a second pause", async () => {
    answers = [web];
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS);
    expect(shown()).toBe("");
    await wait(HINT_DEBOUNCE_MS - 1);
    expect(shown()).toBe("");
    await wait(1);
    expect(shown()).toBe(web.text);
  });

  it("is dropped, not shown late, when typing goes on in that second pause", async () => {
    answers = [web, null];
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS + 100);
    set({ text: "look up the latest release notes for" });
    await wait(HINT_DEBOUNCE_MS * 4);
    expect(shown()).toBe("");
  });

  it("stays put when an edit gives the same answer", async () => {
    answers = [web, web, web];
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS * 2);
    expect(shown()).toBe(web.text);
    const seen: string[] = [];
    for (const text of ["look up the latest release notes ", "look up the latest release notes for Tauri"]) {
      set({ text });
      seen.push(shown() ?? "");
      await wait(HINT_DEBOUNCE_MS);
      seen.push(shown() ?? "");
    }
    expect(seen).toEqual([web.text, web.text, web.text, web.text]);
    expect(asked).toHaveLength(3);
  });

  it("stays through the pause in which a different answer is settling, and changes after it", async () => {
    answers = [web, handover];
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS * 2);
    expect(shown()).toBe(web.text);
    set({ text: "have claude code fix the tests" });
    await wait(HINT_DEBOUNCE_MS);
    expect(shown()).toBe(web.text);
    await wait(HINT_DEBOUNCE_MS);
    expect(shown()).toBe(handover.text);
  });

  it("goes the moment the text is too short to read, or the chat is not for it", async () => {
    answers = [web];
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS * 2);
    expect(shown()).toBe(web.text);
    set({ text: "look up" });
    expect(shown()).toBe("");
  });

  it("ignores an answer that arrives after the text moved on", async () => {
    hold = true;
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS);
    const resolve = release!;
    hold = false;
    answers = [null];
    set({ text: "look up something else entirely" });
    await wait(HINT_DEBOUNCE_MS * 3);
    await act(async () => resolve(web));
    await wait(HINT_DEBOUNCE_MS * 3);
    expect(shown()).toBe("");
  });
});

describe("what a click does", () => {
  async function showWeb() {
    answers = [web, web, web];
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS * 2);
    expect(shown()).toBe(web.text);
  }

  it("taking the action hands the hint back, labels it, and the line goes", async () => {
    await showWeb();
    let taken: ComposerHint | null = null;
    act(() => {
      taken = api.take();
    });
    expect(taken).toMatchObject({ kind: "web", decision: "d-web" });
    expect(labels).toEqual([{ decision: "d-web", kind: "web", taken: true }]);
    expect(shown()).toBe("");
  });

  it("putting it away labels it too, and it does not come back for the same message", async () => {
    await showWeb();
    act(() => api.dismiss());
    expect(labels).toEqual([{ decision: "d-web", kind: "web", taken: false }]);
    // The same message, edited and asked again: the same answer is not shown again.
    set({ text: "look up the latest release notes please" });
    await wait(HINT_DEBOUNCE_MS * 3);
    expect(shown()).toBe("");
  });

  it("is free to come back for the next message", async () => {
    await showWeb();
    act(() => api.dismiss());
    set({ text: "" });
    answers = [web];
    set({ text: "look up the latest release notes" });
    await wait(HINT_DEBOUNCE_MS * 2);
    expect(shown()).toBe(web.text);
  });

  it("does nothing when there is no hint", () => {
    let taken: ComposerHint | null = web;
    act(() => {
      taken = api.take();
    });
    expect(taken).toBeNull();
    expect(labels).toEqual([]);
  });
});
