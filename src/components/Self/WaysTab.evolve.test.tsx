/**
 * @vitest-environment jsdom
 *
 * `WRK-19`, `WRK-20`, `WRK-21`: the organization evolves, and the person decides. A card
 * asks, in the words the backend gave and with the answers it offered; what evolution did
 * on its own can be undone; a preset has a history it can be taken back through, and a
 * day to go back to.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PresetHistoryEntry, WaysView } from "../../lib/api";

const api = vi.hoisted(() => ({
  waysView: vi.fn(),
  updateWay: vi.fn(),
  updatePreset: vi.fn(),
  wayProposalAnswer: vi.fn(),
  waysUndo: vi.fn(),
  waysGoBack: vi.fn(),
  presetBack: vi.fn(),
  presetMerge: vi.fn(),
  presetHistory: vi.fn(),
}));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  ...api,
}));

import WaysTab from "./WaysTab";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const preset = (over: Record<string, unknown> = {}) => ({
  slug: "lean",
  name: "Lean",
  line: "One thing at a time",
  glyph: "lean",
  built_in: true,
  locked: false,
  version: 3,
  rules: [],
  ...over,
});

const view = (over: Partial<WaysView> = {}): WaysView => ({
  narrative: "I've changed how I work 2 times since November. Lately I started trying a second look for mail.",
  ways: [],
  presets: [preset({ slug: "ad-hoc", name: "Ad hoc", glyph: "ad-hoc", version: 1 }), preset()],
  preset: "lean",
  rules: [],
  global_default: "ad-hoc",
  defaults: [],
  deleted: [],
  proposals: [],
  moments: [],
  recent: [],
  triggers: [{ value: "start", label: "I start" }],
  moves: [{ value: "split", label: "I split it" }],
  scopes: [{ value: "everywhere", label: "Everywhere" }],
  kinds: [],
  ...over,
});

const entry = (over: Partial<PresetHistoryEntry>): PresetHistoryEntry => ({
  seq: 1,
  at: Date.parse("2026-11-05T12:00:00"),
  by: "you",
  what: "version",
  why: "",
  version: 1,
  added: [],
  removed: [],
  rules: [],
  current: false,
  ...over,
});

let container: HTMLDivElement;
let root: Root;

async function show(v: WaysView = view()) {
  api.waysView.mockResolvedValue(v);
  await act(async () => root.render(<WaysTab />));
  await act(async () => {});
}

const buttons = () => [...container.querySelectorAll<HTMLButtonElement>("button")];
const all = () => [...document.querySelectorAll<HTMLButtonElement>("button")];
const byText = (text: string) => buttons().find((b) => b.textContent?.trim() === text)!;
const click = async (el: Element) => act(async () => (el as HTMLElement).click());

beforeEach(() => {
  Object.values(api).forEach((f) => f.mockReset().mockResolvedValue(undefined));
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ presets: [], openPresetDialog: vi.fn(), loadPresets: vi.fn().mockResolvedValue(undefined), setActiveConversation: vi.fn() } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.body.innerHTML = "";
});

describe("what I would like to change", () => {
  const card = {
    id: "p1",
    action: "add_way",
    lead: "I'd like to work differently:",
    quote: "For code: When I'm about to call an item done, I get a second look.",
    why: "you put my changes back in 3 different code chats.",
    answers: [
      ["try", "Try it"],
      ["always", "Always"],
      ["no", "No"],
    ] as [string, string][],
  };

  it("asks in the words it was given, and says why", async () => {
    await show(view({ proposals: [card] }));
    expect(container.querySelector(".ways-proposal-lead")?.textContent).toBe("I'd like to work differently:");
    expect(container.querySelector(".ways-proposal-quote")?.textContent).toContain("For code:");
    expect(container.textContent).toContain("Why: you put my changes back in 3 different code chats.");
    expect(buttons().map((b) => b.textContent).filter((t) => ["Try it", "Always", "No"].includes(t ?? ""))).toEqual(["Try it", "Always", "No"]);
  });

  it("sends the answer that was clicked, and reads the list again", async () => {
    await show(view({ proposals: [card] }));
    await click(byText("Try it"));
    expect(api.wayProposalAnswer).toHaveBeenCalledWith("p1", "try");
    expect(api.waysView).toHaveBeenCalledTimes(2);
    await click(byText("No"));
    expect(api.wayProposalAnswer).toHaveBeenLastCalledWith("p1", "no");
  });

  it("has no section when nothing is waiting", async () => {
    await show();
    expect(container.textContent).not.toContain("Waiting for you");
  });

  it("shows what was changed on its own with Undo, and what I only asked about without", async () => {
    await show(
      view({
        recent: [
          { id: "r1", said: "started trying a second look for mail", undoable: true },
          { id: "r2", said: "kept a second look", undoable: false },
        ],
      })
    );
    expect(container.textContent).toContain("I started trying a second look for mail.");
    expect(container.textContent).not.toContain("kept a second look.");
    await click(byText("Undo"));
    expect(api.waysUndo).toHaveBeenCalledWith("r1");
  });
});

describe("a way being tried out", () => {
  const trying = {
    slug: "t",
    text: "When I'm about to call an item done, I get a second look.",
    when: "before-done",
    move: "second-look",
    scope: "kind:mail",
    scope_label: "Kind of work: Mail",
    status: "trying" as const,
    origin: "evolution",
    since: "2026-11-07",
    evidence: [],
    trial: { done: 2, tasks: 5, days_left: 12 },
  };

  it("says how far the trial is, in words", async () => {
    await show(view({ ways: [trying] }));
    expect(container.textContent).toContain("Trying it for the next 5 tasks of its kind, 2 so far, at most 12 days more.");
  });

  it("can be kept from its menu, and a learned one can be tried for a while", async () => {
    await show(view({ ways: [trying, { ...trying, slug: "l", status: "learned", trial: null, text: "When I start, I split it." }] }));
    await click(container.querySelector<HTMLElement>(`[aria-label="More for: ${trying.text}"]`)!);
    await click(byText("Keep it"));
    expect(api.updateWay).toHaveBeenCalledWith("t", { status: "learned" });
    await click(container.querySelector<HTMLElement>(`[aria-label="More for: When I start, I split it."]`)!);
    await click(byText("Try it for a while"));
    expect(api.updateWay).toHaveBeenLastCalledWith("l", { status: "trying" });
  });
});

describe("a preset's versions", () => {
  it("names where a preset came from next to its version", async () => {
    await show(view({ presets: [preset({ slug: "mine", name: "Mine", built_in: false, version: 1, lineage: "from Lean, Nov 7" })], preset: "mine" }));
    expect(container.textContent).toContain("Mine, 1st version · from Lean, Nov 7");
  });

  it("goes back to the original in one click and then offers to lock it like that", async () => {
    await show();
    await click(byText("Back to original"));
    expect(api.presetBack).toHaveBeenCalledWith("lean", null);
    expect(container.textContent).toContain("Lock it like this?");
    await click(byText("Lock"));
    expect(api.updatePreset).toHaveBeenCalledWith("lean", { locked: true });
  });

  it("does not offer Back to original for a preset that has only the one version", async () => {
    await show(view({ presets: [preset({ version: 1 })] }));
    expect(buttons().some((b) => b.textContent === "Back to original")).toBe(false);
  });

  it("brings an offspring back into the preset it branched from", async () => {
    const kid = preset({ slug: "kid", name: "Lean for code", built_in: false, parent: "lean", version: 1 });
    await show(view({ presets: [preset(), kid], preset: "kid" }));
    await click(container.querySelector<HTMLElement>(`[aria-label="More for Lean for code"]`)!);
    await click(byText("Bring back into Lean"));
    expect(api.presetMerge).toHaveBeenCalledWith("kid");
  });

  it("opens the history, picks an older version, compares and goes back to it", async () => {
    const rule = "When I've changed many files, I split it into smaller pieces.";
    api.presetHistory.mockResolvedValue([
      entry({ seq: 3, version: 3, current: true, removed: [rule], rules: [] }),
      entry({ seq: 2, version: 2, by: "evolution", why: "you skipped it three times", rules: [rule] }),
      entry({ seq: 1, version: 1, what: "original", rules: [rule] }),
    ]);
    await show();
    await click(byText("History"));
    await act(async () => {});
    expect(document.body.textContent).toContain("Lean · history");
    expect(document.body.textContent).toContain("3rd version");
    expect(document.body.textContent).toContain("Original");
    expect(document.body.textContent).toContain("you skipped it three times");
    expect(document.body.textContent).toContain(`− ${rule}`);
    // The newest version that is not the one in use is picked to begin with.
    expect(document.querySelector('[aria-selected="true"]')?.textContent).toContain("2nd version");
    await click(all().find((b) => b.textContent === "Compare with now")!);
    expect(document.body.textContent).toContain(`Going back switches on: ${rule}`);
    await click(all().find((b) => b.textContent === "Go back to this version")!);
    expect(api.presetBack).toHaveBeenCalledWith("lean", 2);
  });

  it("will not go back to the version in use, and says so", async () => {
    api.presetHistory.mockResolvedValue([entry({ seq: 1, version: 1, what: "original", current: true })]);
    await show();
    await click(byText("History"));
    await act(async () => {});
    await click(all().find((b) => b.textContent === "Go back to this version")!);
    expect(api.presetBack).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Pick a version first.");
  });

  it("shows a lock as an entry of its own that cannot be gone back to", async () => {
    api.presetHistory.mockResolvedValue([
      entry({ seq: 2, what: "locked", version: 1 }),
      entry({ seq: 1, what: "original", version: 1, current: true }),
    ]);
    await show();
    await click(byText("History"));
    await act(async () => {});
    expect(document.body.textContent).toContain("Locked");
    await click([...document.querySelectorAll('[role="option"]')][0]);
    expect(document.querySelector('[aria-selected="true"]')).toBeNull();
  });
});

describe("going back to a day", () => {
  const moments = [
    { id: "300", at: Date.parse("2026-11-07T12:00:00"), date: "2026-11-07", label: "Added: a second look." },
    { id: "200", at: Date.parse("2026-11-03T12:00:00"), date: "2026-11-03", label: "Changed where chats start" },
    { id: "100", at: Date.parse("2026-10-30T12:00:00"), date: "2026-10-30", label: "The way I came" },
  ];

  it("lists every day there is a copy of, and asks before going back", async () => {
    await show(view({ moments }));
    expect(container.textContent).toContain("Go back to how I worked on");
    await click(byText("Go back"));
    expect(document.body.textContent).toContain("What I have now is kept first");
    expect(api.waysGoBack).not.toHaveBeenCalled();
    await click(all().find((b) => b.textContent === "Go back" && b.className.includes("confirm-go"))!);
    expect(api.waysGoBack).toHaveBeenCalledWith("300");
  });

  it("is not offered while there is only the way I came", async () => {
    await show(view({ moments: [moments[2]] }));
    expect(container.textContent).not.toContain("Go back to how I worked on");
  });
});
