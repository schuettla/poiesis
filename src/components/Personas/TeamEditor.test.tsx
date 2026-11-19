/**
 * @vitest-environment jsdom
 *
 * `TEAM-9`: the Teams tab. The teams are cards; the picked one shows who is in it, the chart in the
 * layout of its shape, and its lines in words. A line is added from the Add a line row or by
 * clicking two places, and found in the list by clicking it; a shape asks before it replaces lines
 * that were drawn by hand; New team opens a blank team on the page, with presets to start from;
 * the dialog and the chart keep their size.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Persona, Team, TeamLine, TeamMember, TeamShape } from "../../lib/api";

const l = (kind: TeamLine["kind"], from: string, to: string): TeamLine => ({ kind, from, to });
const FIXED = [l("reports", "coordinator", "you"), l("talks", "you", "coordinator")];

/** The documented lines of a shape for the shapes these tests use. The real ones are the Rust
 * `shape_lines`, which has its own test of the same table. */
function shapeLines(shape: TeamShape, members: TeamMember[]): TeamLine[] {
  const roles = members.map((m) => m.role);
  const out: TeamLine[] = [];
  if (shape === "hub") roles.forEach((r) => out.push(l("hands-over", "coordinator", r)));
  if (shape === "pipeline" && roles.length) {
    out.push(l("hands-over", "coordinator", roles[0]));
    roles.slice(1).forEach((r, i) => out.push(l("hands-over", roles[i], r)));
    out.push(l("reports", roles[roles.length - 1], "coordinator"));
  }
  if (shape === "review") {
    const workers = roles.length >= 2 ? roles.slice(0, -1) : roles;
    workers.forEach((r) => out.push(l("hands-over", "coordinator", r)));
    if (roles.length >= 2) workers.forEach((r) => out.push(l("reviews", roles[roles.length - 1], r)));
  }
  return [...out, ...FIXED];
}

const api = vi.hoisted(() => ({
  teamsList: vi.fn(),
  teamBlank: vi.fn(),
  teamShapeLines: vi.fn(),
  teamSave: vi.fn(),
  teamHistory: vi.fn(),
  teamBack: vi.fn(),
  teamDelete: vi.fn(),
}));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  ...api,
}));

import TeamEditor from "./TeamEditor";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const persona = (id: string, name: string, extra: Partial<Persona> = {}): Persona =>
  ({ id, name, system_prompt: "x", model_id: null, params_json: null, is_default: false, created_at: 0, updated_at: 0, tools_json: null, skills_json: null, description: null, spawnable: false, line: null, ...extra }) as Persona;

const member = (role: string, who: string, job = ""): TeamMember => ({ role, persona: who, job, owns: [] });

function shop(extra: Partial<Team> = {}): Team {
  const members = [member("api", "persona:p2", "Builds the API"), member("ui", "helper"), member("verifier", "persona:p3")];
  return {
    slug: "shop",
    name: "Shop team",
    line: "Builds and checks web shop features",
    coordinator: "persona:p1",
    style: "",
    member_style: "",
    shape: "review",
    report: "each-item",
    limits: { at_once: 3, budget: 2, minutes: 90, rounds: 3, ask_cost: true },
    folder: "copies",
    locked: true,
    version: 4,
    members,
    lines: shapeLines("review", members),
    ...extra,
  };
}

const blank = (): Team => ({
  slug: "",
  name: "",
  line: "",
  coordinator: "poiesis",
  style: "",
  member_style: "",
  shape: "hub",
  report: "each-item",
  limits: { at_once: 4, budget: 7.5, minutes: 33, rounds: 2, ask_cost: false },
  folder: "copies",
  locked: false,
  version: 0,
  members: [],
  lines: FIXED,
});

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  api.teamsList.mockReset().mockResolvedValue([{ team: shop(), problems: [] }]);
  api.teamBlank.mockReset().mockImplementation(async () => blank());
  api.teamShapeLines.mockReset().mockImplementation(async (shape: TeamShape, members: TeamMember[]) => shapeLines(shape, members));
  api.teamSave.mockReset().mockImplementation(async (t: Team) => ({ ...t, slug: t.slug || "made", locked: t.slug ? t.locked : true, version: t.slug ? t.version + 1 : 1 }));
  api.teamHistory.mockReset().mockResolvedValue([]);
  api.teamBack.mockReset();
  api.teamDelete.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    personas: [persona("p1", "Mira"), persona("p2", "Atlas"), persona("p3", "Kai", { model_id: "chatty" })],
    models: [{ id: "chatty", name: "Chatty", provenance: "local", tier: "chat" }],
    engines: [],
    presets: [],
    conversations: [],
    personasFocus: null,
    expert: false,
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = async () => {
  await act(async () => root.render(<TeamEditor />));
};
const button = (match: (b: HTMLButtonElement) => boolean, within: ParentNode = document) =>
  Array.from(within.querySelectorAll<HTMLButtonElement>("button")).find(match);
const press = (match: (b: HTMLButtonElement) => boolean, within: ParentNode = document) =>
  act(async () => {
    button(match, within)!.click();
  });
const pressText = (text: string) => press((b) => b.textContent?.trim() === text);
/** A place on the chart, by the role under it. */
const place = (role: string) => Array.from(container.querySelectorAll<SVGGElement>(".org-node")).find((g) => g.querySelector(".org-name")?.textContent === role)!;
const click = (el: Element) =>
  act(async () => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
const words = () => Array.from(container.querySelectorAll(".team-words li")).map((li) => li.querySelector("span")!.textContent);
const folds = () => Array.from(container.querySelectorAll<HTMLButtonElement>(".persona-fold-head"));
const type = (el: HTMLInputElement, value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });

/** Opens the Select with this label and picks the option with this label. */
const choose = async (label: string, option: string) => {
  await act(async () => document.querySelector<HTMLButtonElement>(`button[role="combobox"][aria-label="${label}"]`)!.click());
  await act(async () => {
    Array.from(document.querySelectorAll<HTMLElement>('[role="option"]'))
      .find((o) => o.querySelector(".select-option-label")?.textContent === option)!
      .click();
  });
};
/** The kinds of line the Add a line row offers now. */
const kindsOffered = async () => {
  const trigger = document.querySelector<HTMLButtonElement>('button[role="combobox"][aria-label="What the line is"]');
  if (!trigger) return [];
  await act(async () => trigger.click());
  const out = Array.from(document.querySelectorAll('[role="option"] .select-option-label')).map((o) => o.textContent);
  await act(async () => trigger.click());
  return out;
};
const shownLines = () => Array.from(container.querySelectorAll(".team-words li.on")).map((li) => li.textContent ?? "");
/** api may ask ui, drawn by clicking the two places on the chart. */
const drawAsk = async () => {
  await click(place("api"));
  await click(place("ui"));
  await choose("What the line is", "may ask");
  await pressText("Add line");
};

describe("the cards and the chart", () => {
  it("shows the teams as cards, and a way to make one, and the first team under them", async () => {
    await render();
    expect(Array.from(container.querySelectorAll(".ways-style-name")).map((i) => i.textContent)).toEqual(["Shop team", "New team"]);
    expect(container.querySelector(".ways-style.on .ways-style-name")?.textContent).toBe("Shop team");
    expect(container.querySelector(".persona-head-name")?.textContent).toBe("Shop team");
    expect(container.querySelector(".persona-head-led")?.textContent).toContain("Has not led a chat yet · 4th version · History · Back to original");
    expect(container.querySelector(".team-lock")?.textContent).toContain("Locked: I never change its members or lines on my own.");
  });

  it("lists who is in it, played by whom and with what job", async () => {
    await render();
    const rows = Array.from(container.querySelectorAll(".team-roster-row")).map((r) => r.querySelector(".team-roster-row-text")?.textContent);
    expect(rows[0]).toBe("Atlas as apiBuilds the API");
    expect(rows[1]).toBe("A helper as ui");
    expect(rows[2]).toContain("Kai as verifier");
  });

  it("draws You, the coordinator and the members of the team, the reviewer under the work it reviews", async () => {
    await render();
    const names = Array.from(container.querySelectorAll(".org-node")).map((g) => g.querySelector(".org-name")?.textContent);
    expect(names).toEqual(expect.arrayContaining(["You", "coordinator", "api", "ui", "verifier"]));
    const y = (role: string) => Number(place(role).querySelector(".org-shape")!.getAttribute("cy"));
    expect(y("verifier")).toBeGreaterThan(y("api"));
    expect(y("api")).toBe(y("ui"));
    expect(y("api")).toBeGreaterThan(y("coordinator"));
    // A persona is its letter in its hue; a helper is a plain place.
    expect(place("api").querySelector(".org-letter")?.textContent).toBe("A");
    expect(place("api").getAttribute("data-hue")).toBeTruthy();
    expect(place("ui").classList.contains("helper")).toBe(true);
  });

  it("keeps the drawing the same size however many members there are", async () => {
    api.teamsList.mockResolvedValue([{ team: shop({ members: [], lines: FIXED, shape: "hub" }), problems: [] }]);
    await render();
    const box = () => container.querySelector("svg.org-svg")!.getAttribute("viewBox");
    const empty = box();
    await press((b) => b.textContent?.includes("Add a member") ?? false);
    await press((b) => b.textContent === "Add member");
    expect(box()).toBe(empty);
    expect(empty).toBe("0 0 480 240");
  });

  it("says its lines in words, and the two fixed lines cannot be taken away", async () => {
    await render();
    expect(words()).toEqual([
      "Mira hands over work to api.",
      "Mira hands over work to ui.",
      "verifier reviews api's work.",
      "verifier reviews ui's work.",
      "Mira reports to you.",
      "You can talk with Mira.",
    ]);
    const rows = Array.from(container.querySelectorAll(".team-words li"));
    expect(rows[4].textContent).toContain("Always there");
    expect(rows[5].textContent).toContain("Always there");
    expect(rows[0].querySelector("button")?.textContent).toBe("Remove");
  });

  it("folds three sections, and more than one may be open", async () => {
    await render();
    expect(folds().map((f) => f.textContent)).toEqual(["How it runs", "Limits", "The folder"]);
    expect(folds().filter((f) => f.getAttribute("aria-expanded") === "true")).toEqual([]);
    await pressText("Limits");
    await pressText("The folder");
    expect(folds().filter((f) => f.getAttribute("aria-expanded") === "true").map((f) => f.textContent)).toEqual(["Limits", "The folder"]);
  });

  it("keeps how many work at once and the rounds for expert mode", async () => {
    await render();
    await pressText("Limits");
    expect(container.querySelector("#team-budget")).not.toBeNull();
    expect(container.querySelector("#team-at-once")).toBeNull();
    act(() => useAppStore.setState({ expert: true } as never));
    expect(container.querySelector("#team-at-once")).not.toBeNull();
    expect(container.querySelector("#team-rounds")).not.toBeNull();
  });
});

describe("drawing lines", () => {
  it("click two places, say what the line is, and it is drawn and read in words", async () => {
    await render();
    await click(place("api"));
    expect(container.querySelector(".team-roster-row.on")?.textContent).toContain("as api");
    await click(place("ui"));
    expect(await kindsOffered()).toEqual(["hands over work to", "reviews", "may ask", "reports to"]);
    await choose("What the line is", "may ask");
    await pressText("Add line");
    expect(words()).toContain("api may ask ui.");
    expect(shownLines()).toEqual(["api may ask ui.Remove"]);
    // It is a change to save, not yet a version.
    expect(api.teamSave).not.toHaveBeenCalled();
    expect(button((b) => b.textContent === "Save changes")).toBeDefined();
    await act(async () => container.querySelector<HTMLButtonElement>(".team-words li.on button")!.click());
    expect(button((b) => b.textContent === "Save changes")).toBeUndefined();
  });

  it("draws a line from the row alone, the first kind that fits chosen", async () => {
    await render();
    await choose("From", "api");
    await choose("To", "ui");
    await pressText("Add line");
    expect(words()).toContain("api hands over work to ui.");
  });

  it("offers You only what makes sense, and nothing already drawn", async () => {
    await render();
    await click(place("You"));
    await click(place("api"));
    expect(await kindsOffered()).toEqual(["can talk with"]);
    await pressText("Add line");
    expect(words()).toContain("You can talk with api.");
    // The same two places again: that line is there, so no kind is left.
    await click(place("You"));
    await click(place("api"));
    expect(container.querySelector(".team-addline")?.textContent).toContain("has every line it can with");
    expect(button((b) => b.textContent === "Add line")!.disabled).toBe(true);
  });

  it("clicking a line finds it in the list, and a fixed line says it is always there", async () => {
    await render();
    await click(container.querySelector('.org-line .org-hit[aria-label^="api and verifier"]')!);
    expect(shownLines()).toEqual(["verifier reviews api's work.Remove"]);
    await click(container.querySelector('.org-line .org-hit[aria-label^="coordinator and you"]')!);
    expect(shownLines().length).toBeGreaterThan(0);
    expect(shownLines().every((t) => t.includes("Always there"))).toBe(true);
  });

  it("removes a line from the team", async () => {
    await render();
    await click(container.querySelector('.org-line .org-hit[aria-label^="api and verifier"]')!);
    await act(async () => container.querySelector<HTMLButtonElement>(".team-words li.on button")!.click());
    await pressText("Save changes");
    const saved = api.teamSave.mock.calls[0][0] as Team;
    expect(saved.lines.some((x) => x.kind === "reviews" && x.to === "api")).toBe(false);
    expect(saved.lines.some((x) => x.kind === "reviews" && x.to === "ui")).toBe(true);
  });
});

describe("the shape", () => {
  const chips = () => Array.from(container.querySelectorAll(".team-startfrom .team-chip")).map((b) => `${b.textContent}:${b.getAttribute("aria-pressed")}`);

  it("shows the shape the team has", async () => {
    await render();
    expect(chips()).toEqual(["Hub:false", "Pipeline:false", "Review:true", "Open:false"]);
  });

  it("puts the lines of the shape in, for the members as they are, when none were drawn by hand", async () => {
    await render();
    await pressText("Pipeline");
    expect(api.teamShapeLines).toHaveBeenLastCalledWith("pipeline", shop().members);
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
    expect(words()).toEqual([
      "Mira hands over work to api.",
      "api hands over work to ui.",
      "ui hands over work to verifier.",
      "verifier reports to Mira.",
      "Mira reports to you.",
      "You can talk with Mira.",
    ]);
    expect(chips()[1]).toBe("Pipeline:true");
  });

  it("asks before it replaces lines that were drawn by hand, and Keep mine keeps them", async () => {
    await render();
    await drawAsk();
    await pressText("Hub");
    const dialog = document.querySelector('[role="alertdialog"]')!;
    expect(dialog.textContent).toContain("Replace the lines you drew with Hub's?");
    await press((b) => b.textContent === "Keep mine", dialog);
    expect(chips()[2]).toBe("Review:true");
    expect(words()).toContain("api may ask ui.");
  });

  it("Replace puts the shape's lines in place of the drawn ones", async () => {
    await render();
    await drawAsk();
    await pressText("Hub");
    await press((b) => b.textContent === "Replace", document.querySelector('[role="alertdialog"]')!);
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
    expect(words()).not.toContain("api may ask ui.");
    expect(words()).toContain("Mira hands over work to verifier.");
    expect(chips()[0]).toBe("Hub:true");
  });
});

describe("Add a member", () => {
  const dialog = () => document.querySelector<HTMLElement>(".confirm-card")!;
  const slots = () => ({
    note: dialog().querySelector(".team-dialog-note"),
    problem: dialog().querySelector(".team-dialog-problem"),
    fields: dialog().querySelectorAll("input").length,
  });

  it("has a place for the note and a place for what is wrong whether or not there is anything to say", async () => {
    await render();
    await press((b) => b.textContent?.includes("Add a member") ?? false);
    const plain = slots();
    expect(plain.note).not.toBeNull();
    expect(plain.problem).not.toBeNull();
    expect(plain.note!.textContent).toBe("");
    // Pick the persona on a Chat only model: the note comes, in the same place.
    const select = dialog().querySelector<HTMLButtonElement>('button[role="combobox"]')!;
    await act(async () => select.click());
    await act(async () => {
      Array.from(document.querySelectorAll<HTMLElement>('[role="option"]')).find((o) => o.textContent?.startsWith("Kai"))!.click();
    });
    expect(dialog().querySelector(".team-dialog-note")!.textContent).toContain("Kai works on a Chat only model");
    expect(slots().fields).toBe(plain.fields);
    // And a wrong role name is said in its place, without a new row.
    type(dialog().querySelector<HTMLInputElement>("#member-role")!, "you");
    await press((b) => b.textContent === "Add member", dialog());
    expect(dialog().querySelector(".team-dialog-problem")!.textContent).toContain("you is taken");
    expect(dialog().querySelector(".team-dialog-note")!.textContent).toContain("Kai works on a Chat only model");
    expect(dialog().querySelectorAll("input").length).toBe(plain.fields);
  });

  it("asks who plays it first, then the job, and makes the role from the name", async () => {
    await render();
    await press((b) => b.textContent?.includes("Add a member") ?? false);
    const order = Array.from(dialog().querySelectorAll(".field-label")).map((l) => l.firstChild?.textContent?.trim());
    expect(order).toEqual(["Who plays it", "Job", "Role name", "Owns"]);
    expect(dialog().querySelector<HTMLInputElement>("#member-role")!.value).toBe("mira");
    await act(async () => dialog().querySelector<HTMLButtonElement>('button[role="combobox"]')!.click());
    expect(Array.from(document.querySelectorAll('[role="option"] .select-option-label')).map((o) => o.textContent)).toEqual([
      "Mira",
      "Atlas",
      "Kai",
      "A helper",
      "New persona…",
    ]);
  });

  it("adds the member with the lines its shape gives it, the reviewer staying last in Review", async () => {
    await render();
    await press((b) => b.textContent?.includes("Add a member") ?? false);
    type(dialog().querySelector<HTMLInputElement>("#member-role")!, "docs");
    await press((b) => b.textContent === "Add member", dialog());
    expect(document.querySelector(".confirm-card")).toBeNull();
    expect(place("docs")).toBeDefined();
    expect(words()).toContain("Mira hands over work to docs.");
    expect(words()).toContain("verifier reviews docs's work.");
    await pressText("Save changes");
    expect((api.teamSave.mock.calls[0][0] as Team).members.map((m) => m.role)).toEqual(["api", "ui", "docs", "verifier"]);
  });

  it("keeps the lines that were drawn by hand when a member comes", async () => {
    await render();
    await drawAsk();
    await press((b) => b.textContent?.includes("Add a member") ?? false);
    type(dialog().querySelector<HTMLInputElement>("#member-role")!, "docs");
    await press((b) => b.textContent === "Add member", dialog());
    expect(words()).toContain("api may ask ui.");
    expect(words()).toContain("Mira hands over work to docs.");
  });

  it("takes a member out with its lines, and a pipeline closes the gap", async () => {
    api.teamsList.mockResolvedValue([{ team: shop({ shape: "pipeline", lines: shapeLines("pipeline", shop().members) }), problems: [] }]);
    await render();
    await press((b) => b.getAttribute("aria-label") === "Remove ui");
    expect(container.querySelector('[class*="org-node"] .org-name')).not.toBeNull();
    expect(words()).toContain("api hands over work to verifier.");
    expect(words().some((w) => w?.includes("ui"))).toBe(false);
  });
});

describe("tier notes", () => {
  it("say so on the member's row when its model is Chat only", async () => {
    api.teamsList.mockResolvedValue([{ team: shop(), problems: [] }]);
    await render();
    expect(Array.from(container.querySelectorAll(".team-roster-row")).find((r) => r.textContent?.includes("as verifier"))?.textContent).toContain("Kai works on a Chat only model");
  });
});

describe("a new team", () => {
  const newTeam = () => press((b) => b.querySelector(".ways-style-name")?.textContent === "New team");
  const presets = () => Array.from(container.querySelectorAll(".persona-presets .team-chip")).map((b) => b.textContent);

  it("opens on the page as a blank team, the way a new persona does, and is made once it has a name", async () => {
    await render();
    await newTeam();
    expect(api.teamBlank).toHaveBeenCalled();
    expect(document.querySelector(".confirm-card")).toBeNull();
    expect(container.querySelector(".ways-style.new.on")).not.toBeNull();
    expect(container.querySelector(".persona-head-name")?.textContent).toBe("New team");
    expect(Array.from(container.querySelectorAll(".team-startfrom .team-chip")).map((b) => b.getAttribute("aria-pressed"))).toEqual(["true", "false", "false", "false"]);
    expect(button((b) => b.textContent === "Create team")!.disabled).toBe(true);
    type(container.querySelector<HTMLInputElement>("#team-name")!, "Desk");
    // The limits are the blank team's, not a copy kept here.
    await pressText("Limits");
    expect(container.querySelector<HTMLInputElement>("#team-budget")!.value).toBe("7.5");
    api.teamSave.mockImplementation(async (t: Team) => {
      const made = { ...t, slug: "desk", locked: true, version: 1 };
      api.teamsList.mockResolvedValue([{ team: shop(), problems: [] }, { team: made, problems: [] }]);
      return made;
    });
    await pressText("Create team");
    expect(api.teamSave).toHaveBeenCalledWith(expect.objectContaining({ slug: "", name: "Desk", shape: "hub" }));
    // And it is shown as made: a card of its own, first version.
    expect(container.querySelector(".ways-style.on .ways-style-name")?.textContent).toBe("Desk");
    expect(container.querySelector(".persona-head-led")?.textContent).toContain("1st version");
  });

  it("offers the presets, and one is made at once with the lines of its shape", async () => {
    api.teamSave.mockImplementation(async (t: Team) => {
      const made = { ...t, slug: "build-and-review", locked: true, version: 1 };
      api.teamsList.mockResolvedValue([{ team: shop(), problems: [] }, { team: made, problems: [] }]);
      return made;
    });
    await render();
    await newTeam();
    expect(presets()).toEqual(["Build and review", "Research and write", "Bug hunt", "Second opinion"]);
    await pressText("Build and review");
    const saved = api.teamSave.mock.calls[0][0] as Team;
    expect(saved).toMatchObject({ slug: "", name: "Build and review", line: "Builds a change, tests it, and has it reviewed", shape: "review" });
    expect(saved.members.map((m) => `${m.role}:${m.persona}`)).toEqual(["builder:helper", "tester:helper", "reviewer:helper"]);
    expect(saved.lines).toEqual(shapeLines("review", saved.members));
    expect(saved.limits.budget).toBe(7.5);
    expect(container.querySelector(".persona-head-name")?.textContent).toBe("Build and review");
    // A team that is made offers no presets.
    expect(presets()).toEqual([]);
  });

  it("does not offer a preset that is a team already", async () => {
    api.teamsList.mockResolvedValue([{ team: shop({ name: "Bug hunt" }), problems: [] }]);
    await render();
    await newTeam();
    expect(presets()).not.toContain("Bug hunt");
  });
});

describe("changing a team", () => {
  it("saves what was changed as one version, and Revert gives it up", async () => {
    await render();
    await pressText("How it runs");
    await act(async () => {
      container.querySelector<HTMLInputElement>('input[name="team-report"]:not(:checked)')!.click();
    });
    expect(button((b) => b.textContent === "Save changes")).toBeDefined();
    await pressText("Revert");
    expect(button((b) => b.textContent === "Save changes")).toBeUndefined();
    await act(async () => {
      container.querySelectorAll<HTMLInputElement>('input[name="team-report"]')[1].click();
    });
    await pressText("Save changes");
    expect(api.teamSave).toHaveBeenCalledTimes(1);
    expect((api.teamSave.mock.calls[0][0] as Team).report).toBe("end");
  });

  it("deleting says what happens to the chats it led", async () => {
    useAppStore.setState({ conversations: [{ id: "c1", title: "Shop", updatedAt: 0, messages: [], leader: "team:shop" }] } as never);
    await render();
    await pressText("Delete");
    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain("Shop team led 1 chat. It goes back to Poiesis.");
    await pressText("Delete Shop team");
    expect(api.teamDelete).toHaveBeenCalledWith("shop");
  });

  it("says what could not be read in its file", async () => {
    api.teamsList.mockResolvedValue([{ team: shop(), problems: ["I skipped the line “api flies ui”: I don't know that kind of line."] }]);
    await render();
    expect(container.querySelector(".team-problems")?.textContent).toContain("api flies ui");
  });
});

describe("keeping a session's helpers (TEAM-22)", () => {
  const kept = (): Team => ({
    ...blank(),
    name: "Shop rebuild team",
    coordinator: "persona:p1",
    members: [member("atlas", "persona:p2"), member("helper", "helper", "Find every place the old price is used")],
    lines: [l("hands-over", "coordinator", "atlas"), l("hands-over", "coordinator", "helper"), ...FIXED],
  });

  it("opens as a New team with the members and lines the session made, not saved yet", async () => {
    await render();
    await act(async () => {
      useAppStore.getState().openTeamDraft(kept());
    });
    expect(container.querySelector<HTMLInputElement>("#team-name")?.value).toBe("Shop rebuild team");
    const names = Array.from(container.querySelectorAll(".org-node")).map((g) => g.querySelector(".org-name")?.textContent);
    expect(names).toEqual(expect.arrayContaining(["coordinator", "atlas", "helper"]));
    expect(api.teamSave).not.toHaveBeenCalled();
    expect(useAppStore.getState().teamDraft, "taken once").toBeNull();
    await pressText("Create team");
    expect((api.teamSave.mock.calls[0][0] as Team).members.map((m) => m.role)).toEqual(["atlas", "helper"]);
  });
});
