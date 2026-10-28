/**
 * @vitest-environment jsdom
 *
 * `BLF-UI-1`: the Memory and Lessons tab sections for beliefs with evidence.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Fact } from "../../lib/api";

const calls: string[][] = [];
const side = (name: string, evidence: string[]): Fact => ({
  name,
  description: `About ${name}`,
  kind: "preference",
  created: "2026-10-01",
  source_conversation: null,
  body: `Body of ${name}`,
  scope: null,
  status: "contested",
  evidence,
});

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  listUnsure: () => Promise.resolve([[side("prefers-tea", ["c1", "c2"]), side("prefers-coffee", ["c3"])], [side("alone", [])]]),
  resolveUnsure: (keep: string, both: boolean) => {
    calls.push(["resolve", keep, String(both)]);
    return Promise.resolve();
  },
  listLetGo: () =>
    Promise.resolve([
      { file: "1-facts-old-tea", collection: "facts", name: "old-tea", description: "Drank tea", at: 1 },
      { file: "2-facts-old-coffee", collection: "facts", name: "old-coffee", description: "Drank coffee", at: 2 },
      { file: "3-lessons-check-it", collection: "lessons", name: "check-it", description: "Check it", at: 3 },
    ]),
  listIntentions: () =>
    Promise.resolve([
      { name: "remind-call-anna", task: "call Anna", when: 'When you are in the project "Book"', state: "waiting", created: "2026-10-27" },
      { name: "remind-answer", task: "answer Bo", when: "When Bo writes, and a message has come in", state: "ready", created: "2026-10-27" },
    ]),
  forgetMemoryFact: (name: string) => {
    calls.push(["forget", name]);
    return Promise.resolve("token");
  },
  restoreMemoryFact: (file: string) => {
    calls.push(["restore", file]);
    return Promise.resolve();
  },
  listRecentChanges: () =>
    Promise.resolve([
      { at: Date.now(), line: 'I remembered "prefers tea".', collection: "facts" },
      { at: Date.now(), line: 'I learned "check header".', collection: "lessons" },
    ]),
}));

import { useAppStore } from "../../lib/store";
import { Evidence, LetGo, RecentChanges, ThingsIWillDo, Unsure } from "./BeliefSections";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const flush = () => act(async () => void (await Promise.resolve()));
const click = (el: Element | undefined) => act(() => void (el as HTMLElement).click());
const button = (label: string) => [...container.querySelectorAll("button")].find((b) => b.textContent?.includes(label));

beforeEach(() => {
  calls.length = 0;
  useAppStore.setState({
    conversations: [
      { id: "c1", title: "Tea talk", updatedAt: Date.UTC(2026, 9, 3), messages: [] },
      { id: "c2", title: "More tea", updatedAt: Date.UTC(2026, 9, 5), messages: [] },
      { id: "c3", title: "Coffee chat", updatedAt: Date.UTC(2026, 9, 7), messages: [] },
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

describe("Evidence", () => {
  it("lists where a belief was said, newest first, and says so when a chat is gone", () => {
    act(() => root.render(<Evidence ids={["gone", "c1", "c2"]} />));
    const items = [...container.querySelectorAll("li")].map((li) => li.textContent);
    expect(items[0]).toContain("More tea");
    expect(items[1]).toContain("Tea talk");
    expect(items[2]).toBe("from a chat that is gone");
  });

  it("shows three and counts the rest, and nothing for no evidence", () => {
    act(() => root.render(<Evidence ids={["a", "b", "c", "d", "e"]} />));
    expect(container.textContent).toContain("and 2 more");
    act(() => root.render(<Evidence ids={[]} />));
    expect(container.textContent).toBe("");
  });
});

describe("Unsure", () => {
  it("shows both sides with their sources and one button per side", async () => {
    act(() => root.render(<Unsure version={0} onChanged={() => {}} />));
    await flush();
    expect(container.textContent).toContain("About prefers-tea");
    expect(container.textContent).toContain("About prefers-coffee");
    expect(container.textContent).toContain("from Tea talk");
    expect(container.textContent).toContain("from Coffee chat");
    expect(container.querySelectorAll("button").length).toBeGreaterThan(3);
  });

  it("keeps one side, or both for different situations", async () => {
    let changed = 0;
    act(() => root.render(<Unsure version={0} onChanged={() => void changed++} />));
    await flush();
    click(container.querySelectorAll(".memory-unsure-side button.btn-secondary")[1]);
    await flush();
    click(button("Both, in different situations"));
    await flush();
    expect(calls).toEqual([
      ["resolve", "prefers-coffee", "false"],
      ["resolve", "prefers-tea", "true"],
    ]);
    expect(changed).toBe(2);
  });

  it("offers a lone doubt only 'Still true'", async () => {
    act(() => root.render(<Unsure version={0} onChanged={() => {}} />));
    await flush();
    expect(button("Still true")).toBeTruthy();
  });
});

describe("ThingsIWillDo", () => {
  it("lists what I will remind you of with when in words, and drops one", async () => {
    let changed = 0;
    act(() => root.render(<ThingsIWillDo version={0} onChanged={() => void changed++} />));
    await flush();
    expect(container.textContent).toContain("Things I'll do");
    expect(container.textContent).toContain("Remind you: call Anna");
    expect(container.textContent).toContain('When you are in the project "Book"');
    expect(container.textContent).toContain("When Bo writes, and a message has come in");
    click(container.querySelectorAll("button")[1]);
    await flush();
    expect(calls).toEqual([["forget", "remind-answer"]]);
    expect(changed).toBe(1);
  });
});

describe("LetGo", () => {
  it("lists only its own collection, searches it, and brings one back", async () => {
    let changed = 0;
    act(() => root.render(<LetGo collection="facts" version={0} onChanged={() => void changed++} />));
    await flush();
    expect(container.textContent).toContain("Let go (2)");
    expect(container.textContent).not.toContain("check-it");
    const search = container.querySelector("input") as HTMLInputElement;
    act(() => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(search, "coffee");
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(container.textContent).toContain("old-coffee");
    expect(container.textContent).not.toContain("old-tea");
    click(button("Bring it back"));
    await flush();
    expect(calls).toEqual([["restore", "2-facts-old-coffee"]]);
    expect(changed).toBe(1);
  });
});

describe("RecentChanges", () => {
  it("shows the lines of its own tab", async () => {
    act(() => root.render(<RecentChanges collection="lessons" version={0} />));
    await flush();
    expect(container.textContent).toContain('I learned "check header".');
    expect(container.textContent).not.toContain("prefers tea");
  });
});
