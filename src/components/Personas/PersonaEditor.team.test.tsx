/**
 * @vitest-environment jsdom
 *
 * `TEAM-4`: Personas & teams. Poiesis is the first entry and is fixed; the sections of the
 * picked one fold, one open at a time; the glyph is made from the name until one is chosen;
 * deleting says what happens to the chats.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  waysView: vi.fn(),
  updatePersona: vi.fn(),
  deletePersona: vi.fn(),
  listPersonas: vi.fn(),
  setSystemPrompt: vi.fn(),
  teamsList: vi.fn(),
}));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  listToolsets: () => Promise.resolve([]),
  ...api,
}));

import PersonaEditor from "./PersonaEditor";
import { useAppStore } from "../../lib/store";
import type { Persona } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const mira: Persona = {
  id: "p1",
  name: "Mira",
  system_prompt: "Write warmly.",
  model_id: null,
  params_json: null,
  is_default: false,
  created_at: 0,
  updated_at: 0,
  tools_json: null,
  skills_json: null,
  description: null,
  spawnable: false,
  engine_id: null,
  line: "Writes and edits, warm but direct",
};

let container: HTMLDivElement;
let root: Root;
const setSystemPrompt = vi.fn();

beforeEach(() => {
  api.waysView.mockReset().mockResolvedValue({ defaults: [] });
  api.updatePersona.mockReset().mockResolvedValue(undefined);
  api.deletePersona.mockReset().mockResolvedValue(undefined);
  api.listPersonas.mockReset().mockResolvedValue([]);
  api.teamsList.mockReset().mockResolvedValue([]);
  setSystemPrompt.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    personas: [mira],
    engines: [],
    models: [],
    skills: [],
    presets: [],
    view: "personas",
    personasFocus: null,
    systemPrompt: "Be useful.",
    setSystemPrompt,
    conversations: [
      { id: "c1", title: "a", updatedAt: 0, messages: [], leader: "persona:p1", personaId: "p1" },
      { id: "c2", title: "b", updatedAt: 0, messages: [], leader: null, personaId: "p1" },
      { id: "c3", title: "c", updatedAt: 0, messages: [], leader: "poiesis", personaId: null },
    ],
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = async () => {
  await act(async () => root.render(<PersonaEditor />));
};
const button = (match: (b: HTMLButtonElement) => boolean) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find(match);
const press = (match: (b: HTMLButtonElement) => boolean) =>
  act(async () => {
    button(match)!.click();
  });
const pick = (name: string) => press((b) => b.querySelector(".ways-style-name")?.textContent === name);
const folds = () => Array.from(container.querySelectorAll<HTMLButtonElement>(".persona-fold-head"));
const openFolds = () => folds().filter((f) => f.getAttribute("aria-expanded") === "true").map((f) => f.textContent);
const type = (el: HTMLInputElement | HTMLTextAreaElement, value: string) => {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
};

describe("the cards", () => {
  it("has Poiesis first, then the personas, then a way to make one", async () => {
    await render();
    const items = Array.from(container.querySelectorAll(".ways-style-name")).map((i) => i.textContent);
    expect(items).toEqual(["Poiesis", "Mira", "New persona"]);
    // Poiesis leads new chats when nobody else was chosen.
    expect(container.querySelectorAll(".ways-style")[0].querySelector(".ways-badge")?.textContent).toBe("Default");
  });

  it("draws each persona's glyph from its name until one is chosen", async () => {
    await render();
    const glyph = () => container.querySelectorAll(".ways-style")[1].querySelector(".persona-glyph")!;
    expect(glyph().textContent).toBe("M");
    act(() => useAppStore.setState({ personas: [{ ...mira, look: JSON.stringify({ letter: "Q", hue: 5 }) }] } as never));
    expect(glyph().textContent).toBe("Q");
    expect(glyph().getAttribute("data-hue")).toBe("5");
  });
});

describe("sections", () => {
  it("show who Mira is at once, and fold the rest, several open if wanted", async () => {
    await render();
    await pick("Mira");
    expect(container.querySelector<HTMLInputElement>("#persona-name")?.value).toBe("Mira");
    expect(container.querySelector("#persona-prompt")).not.toBeNull();
    expect(openFolds()).toEqual([]);
    await press((b) => b.textContent === "Model and style");
    await press((b) => b.textContent === "Handing Mira work");
    expect(openFolds()).toEqual(["Model and style", "Handing Mira work"]);
    await press((b) => b.textContent === "Model and style");
    expect(openFolds()).toEqual(["Handing Mira work"]);
  });

  it("are these, in this order", async () => {
    await render();
    await pick("Mira");
    expect(folds().map((f) => f.textContent)).toEqual(["Model and style", "Handing Mira work", "What Mira knows"]);
  });

  it("say Mira in the wording of handing work", async () => {
    await render();
    await pick("Mira");
    await press((b) => b.textContent === "Handing Mira work");
    expect(container.textContent).toContain("When should I hand Mira work?");
    expect(container.textContent).toContain("I can hand Mira work");
  });
});

describe("Poiesis", () => {
  it("cannot be deleted or renamed, and says what it uses, how it works and what it knows", async () => {
    await render();
    expect(Array.from(container.querySelectorAll(".poiesis-lines dt")).map((d) => d.textContent)).toEqual([
      "What Poiesis uses",
      "How Poiesis works",
      "What Poiesis knows",
    ]);
    expect(button((b) => b.textContent === "Delete")).toBeUndefined();
    expect(container.querySelector("#persona-name")).toBeNull();
  });

  it("is not a name anyone else can take", async () => {
    await render();
    await pick("Mira");
    type(container.querySelector<HTMLInputElement>("#persona-name")!, "poiesis");
    expect(container.textContent).toContain("Poiesis is taken");
    expect(button((b) => b.textContent === "Save changes")!.disabled).toBe(true);
  });

  it("is edited as Who Poiesis is, which is the global prompt", async () => {
    await render();
    const area = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Who Poiesis is"]')!;
    expect(area.value).toBe("Be useful.");
    type(area, "Be brief.");
    await press((b) => b.textContent === "Save");
    expect(setSystemPrompt).toHaveBeenCalledWith("Be brief.");
  });
});

describe("deleting a persona", () => {
  it("says what happens to the chats it leads, and then sends them back to Poiesis", async () => {
    await render();
    await pick("Mira");
    await press((b) => b.textContent === "Delete");
    expect(container.querySelector('[role="alertdialog"]')?.textContent).toContain("Mira leads 2 chats. The chats go back to Poiesis.");
    await press((b) => b.textContent === "Delete Mira");
    expect(api.deletePersona).toHaveBeenCalledWith("p1");
    const chats = useAppStore.getState().conversations;
    expect(chats.map((c) => c.leader)).toEqual([null, null, "poiesis"]);
    expect(chats.map((c) => c.personaId)).toEqual([null, null, null]);
    // Back on Poiesis.
    expect(container.querySelector('textarea[aria-label="Who Poiesis is"]')).not.toBeNull();
  });

  it("says what happens in each team it is in, and the page lists those teams", async () => {
    api.teamsList.mockResolvedValue([
      {
        problems: [],
        team: {
          slug: "shop",
          name: "Shop team",
          coordinator: "poiesis",
          members: [{ role: "verifier", persona: "persona:p1", job: "Checks every change", owns: [] }],
        },
      },
    ]);
    await render();
    await pick("Mira");
    expect(container.querySelector(".persona-teams")?.textContent).toContain("In Shop team as verifier");
    await press((b) => b.textContent === "Delete");
    expect(container.querySelector('[role="alertdialog"]')?.textContent).toContain(
      "Mira leads 2 chats and is in Shop team. The chats go back to Poiesis; in Shop team, Mira's place becomes a helper with the same job."
    );
  });

  it("can be thought better of", async () => {
    await render();
    await pick("Mira");
    await press((b) => b.textContent === "Delete");
    await press((b) => b.textContent === "Cancel");
    expect(container.querySelector('[role="alertdialog"]')).toBeNull();
    expect(api.deletePersona).not.toHaveBeenCalled();
  });
});

describe("the face", () => {
  it("is saved with the persona once a letter or a colour is chosen", async () => {
    await render();
    await pick("Mira");
    await press((b) => b.getAttribute("aria-label") === "Colour 6");
    await press((b) => b.textContent === "Save changes");
    expect(api.updatePersona).toHaveBeenCalledWith(expect.objectContaining({ id: "p1", look: JSON.stringify({ letter: "M", hue: 6 }) }));
  });

  it("goes back to the name's face when asked", async () => {
    await render();
    await pick("Mira");
    await press((b) => b.getAttribute("aria-label") === "Colour 6");
    await press((b) => b.textContent === "From the name");
    expect(button((b) => b.textContent === "From the name")).toBeUndefined();
    expect(button((b) => b.textContent === "Save changes")).toBeUndefined();
  });
});

describe("Mira's chats (TEAM-6)", () => {
  it("says how many chats Mira has led and when, and links to them in the sidebar", async () => {
    useAppStore.setState({
      conversations: [
        { id: "c1", title: "a", updatedAt: new Date(2026, 10, 14).getTime(), messages: [], leader: "persona:p1", personaId: "p1" },
        { id: "c2", title: "b", updatedAt: new Date(2026, 10, 2).getTime(), messages: [], leader: null, personaId: "p1" },
        { id: "c3", title: "c", updatedAt: 0, messages: [], leader: "poiesis", personaId: null },
      ],
      leaderFilter: null,
    } as never);
    await render();
    await pick("Mira");
    expect(container.querySelector(".persona-head-led")?.textContent).toMatch(/^Led 2 chats, last on .*14.* · Mira's chats$/);
    await press((b) => b.textContent === "Mira's chats");
    expect(useAppStore.getState().leaderFilter).toBe("persona:p1");
  });

  it("has no link before Mira has led a chat", async () => {
    useAppStore.setState({ conversations: [] } as never);
    await render();
    await pick("Mira");
    expect(container.querySelector(".persona-head-led")?.textContent).toBe("Has not led a chat yet.");
    expect(button((b) => b.textContent === "Mira's chats")).toBeUndefined();
  });
});

describe("where chats start (TEAM-5b)", () => {
  const setDefaultPersona = vi.fn();
  const openSelf = vi.fn();
  const starts = (leader: string | null) => ({ defaults: [], starts: [{ scope: "global", label: "Everywhere", style: null, leader }] });
  beforeEach(() => {
    setDefaultPersona.mockReset().mockResolvedValue(undefined);
    openSelf.mockReset();
    useAppStore.setState({ setDefaultPersona, openSelf } as never);
  });

  it("offers to start new chats with her until the Everywhere row says so", async () => {
    api.waysView.mockResolvedValue(starts("poiesis"));
    await render();
    await pick("Mira");
    expect(button((b) => b.textContent === "Start new chats with Mira")).toBeDefined();
    expect(container.textContent).not.toContain("New chats start here");

    api.waysView.mockResolvedValue(starts("persona:p1"));
    await press((b) => b.textContent === "Start new chats with Mira");
    expect(setDefaultPersona).toHaveBeenCalledWith("p1");
    expect(container.textContent).toContain("New chats start here");
    expect(button((b) => b.textContent === "Start new chats with Mira")).toBeUndefined();
  });

  it("says it already does, and links to where that is changed", async () => {
    api.waysView.mockResolvedValue(starts("persona:p1"));
    await render();
    await pick("Mira");
    expect(container.textContent).toContain("New chats start here");
    await press((b) => b.textContent === "Change");
    expect(openSelf).toHaveBeenCalledWith("ways", "starts");
  });

  it("links the style Mira's chats start in to How I work, and to Where chats start", async () => {
    api.waysView.mockResolvedValue({ defaults: [{ scope: "persona:p1", preset: "lean" }], starts: [] });
    await render();
    await pick("Mira");
    await press((b) => b.textContent === "Model and style");
    await press((b) => b.textContent === "Styles and rules are in How I work");
    expect(openSelf).toHaveBeenCalledWith("ways", "lean");
    await press((b) => b.textContent === "Where chats start");
    expect(openSelf).toHaveBeenCalledWith("ways", "starts");
  });
});
