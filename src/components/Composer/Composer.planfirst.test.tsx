/**
 * @vitest-environment jsdom
 *
 * `PLF-5`, `DEF-5`, `CMP-8`: Plan first as a chip on the next message, flipped by
 * Shift+Tab, labelled when it is my default, and a box that says what typing in it
 * will do while a question is open or a plan is being revised.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Composer from "./Composer";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

let container: HTMLDivElement;
let root: Root;
const sent: string[] = [];

beforeEach(() => {
  sent.length = 0;
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
  });
  act(() => {
    root.render(<Composer onSend={(t) => sent.push(t)} />);
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const input = () => container.querySelector("input[type=text]") as HTMLInputElement;
const chips = () => Array.from(container.querySelectorAll(".modifier-chip-label")).map((c) => c.textContent);
const makeDefaultButtons = () => container.querySelectorAll(".modifier-chip-default").length;

function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setter.call(input(), text);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function press(key: string, init: KeyboardEventInit = {}) {
  await act(async () => {
    input().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init }));
  });
}

describe("the Plan first chip", () => {
  it("is not there until it is asked for", () => {
    expect(chips()).toEqual([]);
  });

  it("Shift+Tab turns it on and off, and does nothing else", async () => {
    await press("Tab", { shiftKey: true });
    expect(chips()).toEqual(["Plan first"]);
    expect(useAppStore.getState().turnModifiers.planFirst).toBe(true);
    await press("Tab", { shiftKey: true });
    expect(chips()).toEqual([]);
    expect(input().value).toBe("");
  });

  it("/plan sets it and sends nothing", async () => {
    type("/plan");
    await press("Enter");
    expect(chips()).toEqual(["Plan first"]);
    expect(sent).toEqual([]);
    expect(input().value).toBe("");
  });

  it("is labelled when it is my default, and has nothing to make default", () => {
    act(() => {
      useAppStore.setState({
        planFirstDefault: true,
        turnModifiers: { planFirst: true, planFirstIsDefault: true },
      });
    });
    expect(chips()).toEqual(["Plan first (default)"]);
    expect(makeDefaultButtons()).toBe(0);
  });

  it("one set by hand offers to become the default", () => {
    act(() => useAppStore.setState({ turnModifiers: { planFirst: true } }));
    expect(chips()).toEqual(["Plan first"]);
    expect(makeDefaultButtons()).toBe(1);
  });

  it("removing the one my default put there is only for this message", () => {
    act(() => {
      useAppStore.setState({
        planFirstDefault: true,
        turnModifiers: { planFirst: true, planFirstIsDefault: true },
      });
    });
    act(() => container.querySelector<HTMLButtonElement>(".modifier-chip-x")!.click());
    expect(chips()).toEqual([]);
    expect(useAppStore.getState().planFirstDefault).toBe(true);
  });
});

describe("what the box says it will do", () => {
  it("asks what should change while a plan is being revised", () => {
    act(() => useAppStore.setState({ planRevising: true }));
    expect(input().placeholder).toBe("What should change in the plan?");
  });

  it("says typing answers my question while one is open", () => {
    act(() =>
      useAppStore.setState({
        pendingQuestion: {
          runId: "r1",
          id: "q1",
          convId: "c1",
          messageId: "m1",
          question: "Which?",
          options: [{ label: "A" }, { label: "B" }],
          multi: false,
        },
      })
    );
    expect(input().placeholder).toContain("Answer my question");
  });
});
