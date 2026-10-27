/**
 * @vitest-environment jsdom
 *
 * `PRES-5`, `EPI-UI-1`: the ambient "Recently learned" line is one line at a time. It
 * yields while the composer offers "Last time".
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useAppStore } from "../../lib/store";
import Introduction from "./Introduction";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const today = () => new Date().toISOString().slice(0, 10);

beforeEach(() => {
  useAppStore.setState({
    selfIntroduced: true,
    lastTimeOffer: null,
    lessons: [
      {
        name: "check-header",
        description: "Read the header row first",
        kind: "lesson",
        created: today(),
        source_conversation: null,
        body: "",
      },
    ],
  } as never);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the ambient line", () => {
  it("says what was learned this week", () => {
    act(() => root.render(<Introduction />));
    expect(container.textContent).toContain("Recently learned: Read the header row first");
  });

  it("is absent while the composer offers Last time, and back when it does not", () => {
    act(() => root.render(<Introduction />));
    act(() =>
      useAppStore.getState().setLastTimeOffer({
        conversation_id: "old-1",
        at: Date.now(),
        goal: "g",
        line: "I made the summary.",
        digest: "I made the summary.",
        skill: null,
      })
    );
    expect(container.textContent).toBe("");
    act(() => useAppStore.getState().setLastTimeOffer(null));
    expect(container.textContent).toContain("Recently learned");
  });
});
