/**
 * @vitest-environment jsdom
 *
 * `CUE-UI-1`: a recalled entry says why it came, and can be called outdated (with what is
 * right now) or not relevant here.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SearchHit } from "../../lib/api";

const calls: string[][] = [];
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  outdateMemoryFact: (name: string, now: string, conv: string | null) => {
    calls.push(["outdated", name, now, String(conv)]);
    return Promise.resolve("lives-in-berlin");
  },
  notRelevantMemory: (name: string, conv: string | null) => {
    calls.push(["not-relevant", name, String(conv)]);
    return Promise.resolve();
  },
}));

import { useAppStore } from "../../lib/store";
import RecallMemoryRow from "./RecallMemoryRow";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const fact: SearchHit = {
  source: "memory",
  conversation_id: "c-src",
  title: "lives-in-hamburg",
  created_at: 0,
  snippet: "Lives in Hamburg",
  kind: "preference",
  why: "you named Hamburg",
};
const lesson: SearchHit = { ...fact, title: "check-header", kind: "lesson", why: "about check header", conversation_id: null };

let container: HTMLDivElement;
let root: Root;
const text = () => container.textContent ?? "";
const button = (label: string) =>
  [...container.querySelectorAll("button")].find((b) => b.textContent === label) as HTMLButtonElement | undefined;
const click = (b: HTMLButtonElement | undefined) => act(async () => void b!.click());

beforeEach(() => {
  calls.length = 0;
  useAppStore.setState({ activeConversationId: "c-now", refreshMemoryContext: async () => {} } as never);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("a recalled entry", () => {
  it("says why it came and where it was learned", () => {
    act(() => root.render(<RecallMemoryRow hit={fact} label="◆ remembered" date="saved" />));
    expect(text()).toContain("you named Hamburg");
    expect(button("from that chat")).toBeTruthy();
  });

  it("asks what is right now and sends the answer with the chat it happened in", async () => {
    act(() => root.render(<RecallMemoryRow hit={fact} label="◆ remembered" date="saved" />));
    await click(button("Outdated"));
    expect(text()).toContain("What is right now?");
    const input = container.querySelector("input")!;
    expect(button("Replace")!.disabled).toBe(true);
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(input, "Lives in Berlin");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click(button("Replace"));
    expect(calls).toEqual([["outdated", "lives-in-hamburg", "Lives in Berlin", "c-now"]]);
    expect(text()).toContain("Replaced. Now: Lives in Berlin");
    expect(button("Outdated")).toBeUndefined();
  });

  it("can be called not relevant here, once", async () => {
    act(() => root.render(<RecallMemoryRow hit={fact} label="◆ remembered" date="saved" />));
    await click(button("Not relevant here"));
    expect(calls).toEqual([["not-relevant", "lives-in-hamburg", "c-now"]]);
    expect(text()).toContain("I will use it less here");
    expect(button("Not relevant here")).toBeUndefined();
  });

  it("offers a lesson no Outdated: it is not a belief", () => {
    act(() => root.render(<RecallMemoryRow hit={lesson} label="◆ learned" date="saved" />));
    expect(button("Outdated")).toBeUndefined();
    expect(button("Not relevant here")).toBeTruthy();
    expect(button("from that chat")).toBeUndefined();
  });
});
