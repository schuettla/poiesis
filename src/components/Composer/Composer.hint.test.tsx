/**
 * @vitest-environment jsdom
 *
 * `SEE-3`, `SEE-T-7`: the composer's hint, in the box. One line before the send button, its
 * action becomes a chip for this message alone, a message to a working run can be sent as an
 * addition, nothing is read with the experiment off, and the rows around the box stay as they
 * were (the height itself is CSS, which jsdom does not run: one fixed line, checked by eye).
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ComposerHint } from "../../lib/api";

const asked: { text: string; runId?: string }[] = [];
const labels: { decision: string; kind: string; taken: boolean }[] = [];
let answer: ComposerHint | null = null;

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  composerHint: (_conv: string, text: string, runId?: string) => {
    asked.push({ text, runId });
    return Promise.resolve(answer);
  },
  composerHintLabel: (decision: string, kind: string, taken: boolean) => {
    labels.push({ decision, kind, taken });
    return Promise.resolve();
  },
}));

import Composer from "./Composer";
import { turnRunOptions, useAppStore } from "../../lib/store";
import { experiment } from "../../lib/experiments.fixture";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};


const web: ComposerHint = {
  kind: "web",
  text: "Needs the web, which is off",
  title: "This looks like it needs the web, and Web search is off (90% sure).",
  action: "Turn on for this message",
  decision: "d-web",
  p: 0.9,
};
const handover: ComposerHint = {
  kind: "handover",
  text: "Claude Code may take this (your rule U2)",
  title: "Claude Code may take this under your rule U2.",
  action: "Do it yourself",
  decision: "d-hand",
  p: 0.91,
};
const stop: ComposerHint = {
  kind: "stop",
  text: "This reads as a stop",
  title: "This reads as a stop for the run that is working (93% sure).",
  action: "Add it instead",
  decision: "d-stop",
  p: 0.93,
};

let container: HTMLDivElement;
let root: Root;
const sent: string[] = [];
const steer = vi.fn(async () => true);

const input = () => container.querySelector("input[type=text]") as HTMLInputElement;
const line = () => container.querySelector(".composer-hint");
const chips = () => Array.from(container.querySelectorAll(".modifier-chip-label")).map((c) => c.textContent);
const click = (el: Element | null) => act(() => (el as HTMLElement).click());

function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setter.call(input(), text);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const wait = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const press = (key: string) =>
  act(async () => {
    input().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });

async function readAs(hint: ComposerHint | null, text = "look up the latest release notes") {
  answer = hint;
  type(text);
  await wait(900);
}

beforeEach(() => {
  vi.useFakeTimers();
  asked.length = 0;
  labels.length = 0;
  sent.length = 0;
  answer = null;
  steer.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    skills: [],
    turnModifiers: {},
    planFirstDefault: false,
    planRevising: false,
    pendingQuestion: null,
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    activeRun: null,
    busy: false,
    changeSets: {},
    workspaceMode: false,
    toolsEnabled: true,
    experiments: [experiment({ id: "fast-slow", on: true })],
    steerActiveRun: steer,
  });
  act(() => {
    root.render(<Composer onSend={(t) => sent.push(t)} />);
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

describe("the line", () => {
  it("is one line in the box's row, before the send button, with the number only on the hover", async () => {
    await readAs(web);
    const el = line()!;
    expect(el.textContent).toContain("Needs the web, which is off");
    expect(el.textContent).not.toMatch(/\d/);
    expect(el.getAttribute("title")).toContain("90% sure");
    const box = container.querySelector(".composer")!;
    expect(el.parentElement).toBe(box);
    expect(el.nextElementSibling).toBe(box.querySelector(".send"));
    expect(container.querySelectorAll(".composer-hint")).toHaveLength(1);
  });

  it("adds nothing outside the box: the rows around it are the same with or without it", async () => {
    const before = Array.from(container.querySelector(".composer-col")!.children).map((c) => c.className);
    await readAs(web);
    const after = Array.from(container.querySelector(".composer-col")!.children).map((c) => c.className);
    expect(after).toEqual(before);
  });

  it("is not asked for, and not shown, with the experiment off", async () => {
    act(() => useAppStore.setState({ experiments: [experiment({ id: "fast-slow", on: false })] }));
    await readAs(web);
    expect(asked).toEqual([]);
    expect(line()).toBeNull();
  });

  it("is not asked for while a picture is being described or a question is open", async () => {
    act(() => useAppStore.setState({ planRevising: true }));
    await readAs(web);
    expect(asked).toEqual([]);
  });

  it("goes when the message is sent", async () => {
    await readAs(web);
    expect(line()).not.toBeNull();
    await press("Enter");
    expect(sent).toEqual(["look up the latest release notes"]);
    expect(line()).toBeNull();
  });
});

describe("the action", () => {
  it("web: puts Web search on for this message as a chip and says it was right", async () => {
    await readAs(web);
    click(container.querySelector(".composer-hint-action"));
    expect(useAppStore.getState().turnModifiers.web).toBe(true);
    expect(chips()).toEqual(["Web search on"]);
    expect(container.querySelectorAll(".modifier-chip-default")).toHaveLength(0);
    expect(line()).toBeNull();
    expect(labels).toEqual([{ decision: "d-web", kind: "web", taken: true }]);
    expect(turnRunOptions(useAppStore.getState().turnModifiers)).toEqual({ web: true });
  });

  it("handover: says do it yourself for this message and sends that to the run", async () => {
    await readAs(handover, "fix the failing tests in the parser");
    click(container.querySelector(".composer-hint-action"));
    expect(chips()).toEqual(["Do it yourself"]);
    expect(turnRunOptions(useAppStore.getState().turnModifiers)).toEqual({ noHandover: true });
    expect(labels).toEqual([{ decision: "d-hand", kind: "handover", taken: true }]);
  });

  it("the chip can be taken off again, and it is not a default", async () => {
    await readAs(web);
    click(container.querySelector(".composer-hint-action"));
    click(container.querySelector(".modifier-chip-x"));
    expect(useAppStore.getState().turnModifiers.web).toBeUndefined();
    expect(chips()).toEqual([]);
  });

  it("is not offered again once it has been taken", async () => {
    await readAs(web);
    click(container.querySelector(".composer-hint-action"));
    type("look up the latest release notes for Tauri");
    await wait(900);
    expect(line()).toBeNull();
  });

  it("putting the line away labels it and shows nothing", async () => {
    await readAs(web);
    click(container.querySelector(".composer-hint-x"));
    expect(line()).toBeNull();
    expect(chips()).toEqual([]);
    expect(labels).toEqual([{ decision: "d-web", kind: "web", taken: false }]);
  });

  it("a hint whose choice is already on the message is not shown", async () => {
    act(() => useAppStore.setState({ turnModifiers: { web: true } }));
    await readAs(web);
    expect(line()).toBeNull();
  });
});

describe("a message to a working run", () => {
  beforeEach(() => {
    act(() =>
      useAppStore.setState({
        busy: true,
        activeRun: { runId: "r1", convId: "c1" } as never,
      })
    );
    act(() => {
      root.render(<Composer onSend={(t) => sent.push(t)} busy />);
    });
  });

  it("is read with the run's id", async () => {
    await readAs(stop, "actually forget the whole thing");
    expect(asked).toEqual([{ text: "actually forget the whole thing", runId: "r1" }]);
    expect(line()!.textContent).toContain("This reads as a stop");
  });

  it("Add it instead sends it to the run as an addition, once", async () => {
    await readAs(stop, "actually forget the whole thing");
    click(container.querySelector(".composer-hint-action"));
    expect(chips()).toEqual(["As an addition"]);
    await press("Enter");
    expect(steer).toHaveBeenCalledWith("actually forget the whole thing", { asAddition: true });
    expect(chips()).toEqual([]);
    expect(labels).toEqual([{ decision: "d-stop", kind: "stop", taken: true }]);
  });

  it("without the action, Enter sends it as it always did", async () => {
    await readAs(stop, "actually forget the whole thing");
    await press("Enter");
    expect(steer).toHaveBeenCalledWith("actually forget the whole thing");
  });
});

describe("the choices as run options", () => {
  it("an ordinary message sends neither", () => {
    expect(turnRunOptions({})).toBeUndefined();
    expect(turnRunOptions({ asAddition: true })).toBeUndefined();
  });

  it("both go together with the other chips", () => {
    expect(turnRunOptions({ effort: "high", web: true, noHandover: true })).toEqual({
      effort: "high",
      web: true,
      noHandover: true,
    });
  });
});
