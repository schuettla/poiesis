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
const headers = () =>
  Array.from(container.querySelectorAll(".command-menu-header")).map((h) => h.textContent);
const rowNames = () =>
  Array.from(container.querySelectorAll(".command-menu-name")).map((n) => n.textContent);
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
    const row = Array.from(container.querySelectorAll<HTMLElement>(".command-menu-row")).find(
      (r) => r.querySelector(".command-menu-name")?.textContent === "steps"
    )!;
    act(() => row.click());
    expect(input().value).toBe("/steps add dark mode");
  });
});

describe("the menu", () => {
  it("lists the sections in the plan's order, with my skills last", async () => {
    type("/");
    expect(headers()).toEqual([
      "This turn",
      "Conversation",
      "Me",
      "Modes and persona",
      "Agents and time",
      "Create",
      "Work",
      "Inspect",
      "My skills",
    ]);
  });

  it("hides /stop with no run, and shows it with one", async () => {
    type("/");
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
    type("/s");
    expect(rowNames()).toContain("stop");
  });

  it("dims a command that cannot run, says why, and does not run it on Enter", async () => {
    type("/init");
    const row = container.querySelector<HTMLElement>(".command-menu-row")!;
    expect(row.getAttribute("aria-disabled")).toBe("true");
    expect(row.textContent).toContain("needs a working folder");
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
    expect(options.length).toBeGreaterThan(10);
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

describe("a modifier command", () => {
  it("/effort high sets a chip and sends nothing", async () => {
    type("/effort high");
    await press("Enter");
    expect(sent).toHaveLength(0);
    expect(useAppStore.getState().turnModifiers.effort).toBe("high");
    expect(input().value).toBe("");
    expect(container.querySelector(".modifier-chip")?.textContent).toContain("Think: high");
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
