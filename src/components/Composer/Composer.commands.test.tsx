/**
 * @vitest-environment jsdom
 *
 * `CMP-T2`, `CMP-T3`: the `/` button and menu, and what the `+` is left holding.
 *
 * These drive the real input and the real buttons: the first version of the
 * skill list shipped as dead UI because only its handler was tested.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Composer from "./Composer";
import { useAppStore } from "../../lib/store";
import type { SkillView } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

function skill(name: string): SkillView {
  return {
    name,
    description: `does ${name}`,
    when_to_use: null,
    source: "personal",
    dir: `/skills/${name}`,
    enabled: true,
    unsupported: [],
    used: 0,
    rough: 0,
    risk: 0,
    risk_flags: [],
  };
}

let container: HTMLDivElement;
let root: Root;
const sent: string[] = [];

beforeEach(() => {
  sent.length = 0;
  try {
    localStorage.clear();
  } catch {
    /* none */
  }
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    skills: [skill("weekly-report")],
    turnModifiers: {},
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
const menu = () => container.querySelector(".command-menu");
const groups = () =>
  Array.from(container.querySelectorAll(".command-menu-category .command-menu-title")).map(
    (n) => n.textContent
  );
const rowNames = () =>
  Array.from(container.querySelectorAll("[data-command]")).map((n) => n.getAttribute("data-command"));
const row = (name: string) => container.querySelector<HTMLElement>(`[data-command="${name}"]`)!;
const group = (section: string) =>
  container.querySelector<HTMLElement>(`.command-menu-category[data-section="${section}"]`)!;
/** The `/` button on an empty box: the dialog of groups. */
const openDialog = () => act(() => slashButton().click());
/** Open a group from the first page and wait for the click to land. */
const openGroup = (section: string) => act(() => group(section).click());
const slashButton = () => container.querySelector<HTMLButtonElement>("button.slash-btn")!;
const plusButton = () => container.querySelector<HTMLButtonElement>("button.plus-btn")!;
const live = () => container.querySelector(".command-menu-live")?.textContent ?? "";

function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setter.call(input(), text);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}

/** Enter runs a command through an async handler, so give it the microtasks it
 * needs before looking at what happened. */
async function press(key: string, init: KeyboardEventInit = {}) {
  await act(async () => {
    input().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init }));
  });
}

describe("the / button", () => {
  it("is beside the +, and labelled as commands", async () => {
    expect(slashButton().getAttribute("aria-label")).toBe("Commands");
    expect(slashButton().getAttribute("aria-haspopup")).toBe("listbox");
    expect(plusButton().closest(".composer-menu-wrap")!.nextElementSibling).toBe(slashButton());
  });

  it("with an empty box types the slash, and the menu opens from the text", async () => {
    expect(menu()).toBeNull();
    act(() => slashButton().click());
    expect(input().value).toBe("/");
    expect(menu()).not.toBeNull();
  });

  it("with text in the box opens a popover that puts the command in front", async () => {
    type("add dark mode");
    act(() => slashButton().click());
    expect(menu()).not.toBeNull();
    expect(input().value).toBe("add dark mode");
    openGroup("turn");
    act(() => row("steps").click());
    expect(input().value).toBe("/steps add dark mode");
  });
});

describe("the menu", () => {
  it("opens on the groups in the plan's order, with my skills last", async () => {
    openDialog();
    expect(groups()).toEqual([
      "This message",
      "This chat",
      "What I know",
      "How I work",
      "Helpers and later",
      "Make something",
      "Your project",
      "Look inside",
      "My skills",
    ]);
    // The first page is groups, not a wall of commands.
    expect(rowNames()).toEqual([]);
  });

  it("typing the slash yourself opens the plain list of every command, not the groups", async () => {
    type("/");
    expect(groups()).toEqual([]);
    expect(rowNames()).toEqual(expect.arrayContaining(["effort", "tools", "persona", "weekly-report"]));
    expect(
      Array.from(container.querySelectorAll(".command-menu-header")).map((h) => h.textContent)
    ).toContain("How I work");
    // And it completes as you go on.
    type("/too");
    expect(container.querySelector(".composer-ghost")!.lastChild!.textContent).toBe("ls");
  });

  it("a typed search shows each command's own name and marks what matched", async () => {
    type("/thinkin");
    const cmd = row("effort").querySelector(".command-menu-cmd")!;
    expect(cmd.textContent).toBe("/effort");
    // The title carries the match here; the command has none to mark.
    expect(row("effort").querySelector(".command-menu-title mark")!.textContent).toBe("Thinkin");

    type("/eff");
    expect(row("effort").querySelector(".command-menu-cmd mark")!.textContent).toBe("eff");

    // Anywhere in the name, not only the start.
    type("/space");
    expect(row("workspace").querySelector(".command-menu-cmd")!.textContent).toBe("/workspace");
    expect(row("workspace").querySelector(".command-menu-cmd mark")!.textContent).toBe("space");
  });

  it("the dialog of groups keeps the readable names only", async () => {
    openDialog();
    openGroup("modes");
    expect(container.querySelector(".command-menu-cmd")).toBeNull();
  });

  it("opens a group in place, and the way back returns to the groups", async () => {
    openDialog();
    openGroup("modes");
    expect(rowNames()).toEqual(["workspace", "tools", "persona", "model"]);
    expect(groups()).toEqual([]);
    act(() => container.querySelector<HTMLButtonElement>(".command-menu-back")!.click());
    expect(rowNames()).toEqual([]);
    expect(groups()).toHaveLength(9);
  });

  it("Escape steps out of a group before it closes the menu", async () => {
    openDialog();
    openGroup("modes");
    await press("Escape");
    expect(menu()).not.toBeNull();
    expect(groups()).toHaveLength(9);
    await press("Escape");
    expect(menu()).toBeNull();
  });

  it("hides /stop with no run, and shows it with one", async () => {
    type("/stop");
    expect(rowNames()).not.toContain("stop");
    act(() => {
      useAppStore.setState({
        activeRun: {
          runId: "r1",
          convId: "c1",
          step: 1,
          maxSteps: 12,
          startedAt: Date.now(),
          contextTokens: 0,
          contextWindow: null,
          thinking: "",
        },
        busy: true,
      });
    });
    type("/");
    type("/stop");
    expect(rowNames()).toContain("stop");
  });

  it("finds a command by what it is called in the menu, not only by its name", async () => {
    type("/thinking");
    expect(rowNames()[0]).toBe("effort");
  });

  it("dims a command that cannot run, says why, and does not run it on Enter", async () => {
    type("/init");
    const dim = container.querySelector<HTMLElement>(".command-menu-row")!;
    expect(dim.getAttribute("aria-disabled")).toBe("true");
    expect(dim.textContent).toContain("needs a working folder");
    await press("Enter");
    expect(sent).toHaveLength(0);
    expect(input().value).toBe("/init");
    expect(live()).toBe("needs a working folder");
  });

  it("Tab completes the name and enters the argument phase", async () => {
    type("/effo");
    await press("Tab");
    expect(input().value).toBe("/effort ");
    expect(container.querySelectorAll(".command-menu-choice").length).toBeGreaterThan(0);
  });

  it("Enter on a command that needs an argument completes instead of running", async () => {
    type("/rename");
    await press("Enter");
    expect(input().value).toBe("/rename ");
    expect(sent).toHaveLength(0);
  });

  it("Escape closes the menu and keeps the text", async () => {
    type("/effo");
    await press("Escape");
    expect(menu()).toBeNull();
    expect(input().value).toBe("/effo");
  });

  it("is a listbox whose rows are options, with the active one on the input", async () => {
    type("/");
    expect(menu()!.getAttribute("role")).toBe("listbox");
    const options = container.querySelectorAll('[role="option"]');
    expect(options.length).toBeGreaterThanOrEqual(9);
    const active = input().getAttribute("aria-activedescendant");
    expect(active).toBeTruthy();
    expect(container.querySelector(`#${active}`)).not.toBeNull();
  });

  it("leaves an unknown /word to be sent as a message", async () => {
    type("/usr/bin/env");
    await press("Enter");
    expect(sent).toEqual(["/usr/bin/env"]);
  });
});

describe("settings on the rows", () => {
  const sw = (name: string) => row(name).querySelector<HTMLElement>('[role="switch"]')!;

  it("a switch shows the setting and flips it without closing the menu", async () => {
    openDialog();
    openGroup("modes");
    expect(sw("tools").getAttribute("aria-checked")).toBe("true");
    await act(async () => sw("tools").click());
    expect(useAppStore.getState().toolsEnabled).toBe(false);
    expect(menu()).not.toBeNull();
    expect(sw("tools").getAttribute("aria-checked")).toBe("false");
    expect(input().value).toBe("/");
  });

  it("Enter on a switch flips it while browsing, but runs it when it was typed", async () => {
    openDialog();
    openGroup("modes");
    await press("Enter");
    expect(useAppStore.getState().workspaceMode).toBe(true);
    expect(menu()).not.toBeNull();
    expect(sw("workspace").getAttribute("aria-checked")).toBe("true");

    type("/workspace");
    await press("Enter");
    expect(useAppStore.getState().workspaceMode).toBe(false);
    expect(input().value).toBe("");
    expect(menu()).toBeNull();
  });

  it("a refused change is said, and the switch stays where it was", async () => {
    act(() => useAppStore.setState({ workspaceMode: true }));
    openDialog();
    openGroup("modes");
    await act(async () => sw("tools").click());
    expect(useAppStore.getState().toolsEnabled).toBe(true);
    expect(container.querySelector(".composer-model-notice")?.textContent).toContain("Workspace mode needs my tools");
  });

  it("a row of choices sets the chip on a tap, and a second tap takes it off", async () => {
    openDialog();
    openGroup("turn");
    const high = () =>
      Array.from(row("effort").querySelectorAll<HTMLElement>('[role="radio"]')).find(
        (b) => b.textContent === "High"
      )!;
    await act(async () => high().click());
    expect(useAppStore.getState().turnModifiers.effort).toBe("high");
    expect(high().getAttribute("aria-checked")).toBe("true");
    expect(menu()).not.toBeNull();
    await act(async () => high().click());
    expect(useAppStore.getState().turnModifiers.effort).toBeUndefined();
  });

  it("plan first is a switch on the same chip Shift+Tab uses", async () => {
    openDialog();
    openGroup("turn");
    await act(async () => sw("plan").click());
    expect(useAppStore.getState().turnModifiers.planFirst).toBe(true);
    await act(async () => sw("plan").click());
    expect(useAppStore.getState().turnModifiers.planFirst).toBeUndefined();
  });

  it("the arrows step along a row's choices once nothing is typed after the slash", async () => {
    openDialog();
    openGroup("turn");
    // Rows in "This message": effort is the first, and it starts from the
    // default level (low).
    await press("ArrowRight");
    expect(useAppStore.getState().turnModifiers.effort).toBe("medium");
    await press("ArrowRight");
    expect(useAppStore.getState().turnModifiers.effort).toBe("high");
    await press("ArrowLeft");
    expect(useAppStore.getState().turnModifiers.effort).toBe("medium");
  });

  it("the thinking row highlights the level in force, as the model dialog does", async () => {
    const checked = () =>
      Array.from(row("effort").querySelectorAll<HTMLElement>('[role="radio"]'))
        .filter((b) => b.getAttribute("aria-checked") === "true")
        .map((b) => b.textContent);
    openDialog();
    openGroup("turn");
    // No chip: the stored default, which is where the model dialog starts too.
    expect(checked()).toEqual(["Low"]);
    act(() => useAppStore.getState().setTurnModifier({ effort: "high" }));
    expect(checked()).toEqual(["High"]);
    act(() => useAppStore.getState().clearTurnModifier("effort"));
    expect(checked()).toEqual(["Low"]);
  });

  it("the mark for what I can do on my own sits right after the name", async () => {
    openDialog();
    openGroup("turn");
    const glyph = row("plan").querySelector(".command-menu-glyph")!;
    expect(glyph.previousElementSibling?.classList.contains("command-menu-title")).toBe(true);
    expect(glyph.closest(".command-menu-name-line")).not.toBeNull();
  });

  it("a long choice opens its list on the row, marks the current one, and applies a pick", async () => {
    const chat = (id: string, name: string) =>
      ({ id, name, modality: "chat", provenance: "cloud" }) as unknown as import("../../lib/types").Model;
    act(() =>
      useAppStore.setState({
        models: [chat("m1", "Alpha"), chat("m2", "Beta")],
        selectedModelId: "m1",
      })
    );
    openDialog();
    openGroup("modes");
    expect(row("model").textContent).toContain("Alpha");
    act(() => row("model").click());
    const picks = Array.from(container.querySelectorAll<HTMLElement>(".command-menu-choice")).map(
      (c) => c.textContent
    );
    expect(picks).toEqual(["Alpha", "Beta"]);
    expect(container.querySelectorAll(".command-menu-check")).toHaveLength(1);
    const beta = Array.from(container.querySelectorAll<HTMLElement>('[role="option"]')).find((o) =>
      o.textContent?.startsWith("Beta")
    )!;
    await act(async () => beta.click());
    expect(useAppStore.getState().selectedModelId).toBe("m2");
    // Back on the group, now naming the new pick.
    expect(row("model").textContent).toContain("Beta");
  });
});

describe("completing while typing", () => {
  const ghost = () => container.querySelector(".composer-ghost")?.textContent ?? "";

  it("draws the rest of the best match after what is typed", async () => {
    type("/effo");
    expect(ghost()).toBe("/effort");
    expect(container.querySelector(".composer-ghost")!.lastChild!.textContent).toBe("rt");
  });

  it("→ at the end takes it, the same as Tab, and moves on to the argument", async () => {
    type("/effo");
    input().setSelectionRange(5, 5);
    await press("ArrowRight");
    expect(input().value).toBe("/effort ");
    type("/effort hi");
    expect(container.querySelector(".composer-ghost")!.lastChild!.textContent).toBe("gh");
    input().setSelectionRange(10, 10);
    await press("ArrowRight");
    expect(input().value).toBe("/effort high");
  });

  it("shows nothing when the typed text is not the start of a name", async () => {
    type("/thinking");
    expect(container.querySelector(".composer-ghost")).toBeNull();
  });

  it("Tab on a switch completes its name instead of flipping it", async () => {
    type("/tool");
    await press("Tab");
    expect(input().value).toBe("/tools ");
    expect(useAppStore.getState().toolsEnabled).toBe(true);
  });
});

describe("a modifier command", () => {
  it("/effort high sets a chip and sends nothing", async () => {
    type("/effort high");
    await press("Enter");
    expect(sent).toHaveLength(0);
    expect(useAppStore.getState().turnModifiers.effort).toBe("high");
    expect(input().value).toBe("");
    expect(container.querySelector(".modifier-chip")?.textContent).toContain("High");
  });

  it("/effort high add tests sets the chip and sends the words at once", async () => {
    type("/effort high add tests");
    await press("Enter");
    expect(useAppStore.getState().turnModifiers.effort).toBe("high");
    expect(sent).toEqual(["add tests"]);
  });

  it("the chip's x takes it off again", async () => {
    act(() => useAppStore.getState().setTurnModifier({ maxSteps: 30 }));
    const x = container.querySelector<HTMLButtonElement>(".modifier-chip-x")!;
    act(() => x.click());
    expect(useAppStore.getState().turnModifiers).toEqual({});
  });

  it("make default keeps the chip and leaves a note", async () => {
    act(() => useAppStore.getState().setTurnModifier({ maxSteps: 30 }));
    const btn = container.querySelector<HTMLButtonElement>(".modifier-chip-default")!;
    await act(async () => btn.click());
    expect(useAppStore.getState().turnModifiers.maxSteps).toBe(30);
    const notes = useAppStore.getState().commandNotes.c1 ?? [];
    expect(notes[notes.length - 1]?.note).toBe("/steps 30 is now my default");
  });
});

describe("the + menu (CMP-T3)", () => {
  it("holds the four things you can add, and nothing that changes a mode", async () => {
    act(() => plusButton().click());
    const items = Array.from(container.querySelectorAll('[role="menuitem"] .mi-body')).map(
      (b) => b.firstChild?.textContent
    );
    expect(items).toEqual([
      "Files and images",
      "A folder",
      "From my library",
      "An earlier conversation",
    ]);
    expect(container.textContent).not.toContain("Workspace mode");
    expect(container.textContent).not.toContain("Create image");
  });

  it("no longer lights up for Workspace mode", async () => {
    act(() => {
      useAppStore.setState({ workspaceMode: true });
    });
    expect(plusButton().className).not.toContain("lit");
    expect(plusButton().className).not.toContain(" on");
  });
});

describe("mode chips (CMP-7)", () => {
  it("name Workspace mode in the footer, and the x turns it off", async () => {
    act(() => {
      useAppStore.setState({ workspaceMode: true });
    });
    const chip = container.querySelector(".mode-chip")!;
    expect(chip.textContent).toContain("Workspace");
    act(() => chip.querySelector<HTMLButtonElement>(".mode-chip-x")!.click());
    expect(useAppStore.getState().workspaceMode).toBe(false);
  });

  it("show Tools off", async () => {
    act(() => {
      useAppStore.setState({ toolsEnabled: false });
    });
    expect(container.querySelector(".mode-chips")?.textContent).toContain("Tools off");
  });
});
