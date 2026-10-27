/**
 * @vitest-environment jsdom
 *
 * `BLF-2`, IV.9: the toast says when a belief replaced an older one or is in doubt, and
 * its Undo puts the older one back.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls: string[][] = [];
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  undoBeliefChange: (neu: string, old: string) => {
    calls.push(["undo", neu, old]);
    return Promise.resolve();
  },
  forgetMemoryFact: (name: string) => {
    calls.push(["forget", name]);
    return Promise.resolve("file");
  },
  getMemoryContext: () => Promise.resolve({ index: "", soul: "", about_you: "", fact_count: 0 }),
}));

import { useAppStore } from "../../lib/store";
import MemoryToast, { beliefSentence } from "./MemoryToast";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  calls.length = 0;
  useAppStore.setState({ memoryOnboarded: true } as never);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ memoryToast: null } as never);
});

describe("the belief leads", () => {
  it("say where the old belief went", () => {
    expect(beliefSentence("replace", "Prefers coffee")).toBe(
      "I replaced what I knew with “Prefers coffee”. The old one is under Earlier versions."
    );
    expect(beliefSentence("contest", "Prefers coffee")).toBe(
      "I'm not sure which is right about “Prefers coffee”. It's under Unsure."
    );
    expect(beliefSentence("save", "x")).toBeNull();
  });

  it("show on the toast, and Undo puts the older belief back", async () => {
    act(() =>
      useAppStore.setState({
        memoryToast: {
          op: "replace",
          name: "prefers-coffee",
          description: "Prefers coffee",
          collection: "facts",
          undoToken: "prefers-tea",
        },
      } as never)
    );
    act(() => root.render(<MemoryToast />));
    expect(container.textContent).toContain("I replaced what I knew");
    const undo = [...container.querySelectorAll("button")].find((b) => b.textContent === "Undo")!;
    await act(async () => undo.click());
    expect(calls).toEqual([["undo", "prefers-coffee", "prefers-tea"]]);
  });

  it("a contest says it is unsure; a repeat says it was known and offers no Undo", () => {
    act(() =>
      useAppStore.setState({
        memoryToast: { op: "contest", name: "a", description: "Prefers coffee", collection: "facts", undoToken: "b" },
      } as never)
    );
    act(() => root.render(<MemoryToast />));
    expect(container.textContent).toContain("under Unsure");
    act(() =>
      useAppStore.setState({
        memoryToast: { op: "merge", name: "a", description: "Prefers tea", collection: "facts", undoToken: "" },
      } as never)
    );
    expect(container.textContent).toContain("I knew that already");
    expect([...container.querySelectorAll("button")].some((b) => b.textContent === "Undo")).toBe(false);
  });
});
