/**
 * @vitest-environment jsdom
 *
 * `CLI-T-11`, `CLI-UI-6`: choosing who `/delegate` hands a task to, in the same `/`
 * menu everything else uses. These drive the real input.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Composer from "./Composer";
import { useAppStore } from "../../lib/store";
import type { DelegateTarget } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

const target = (over: Partial<DelegateTarget>): DelegateTarget => ({
  slug: "claude-code",
  name: "Claude Code",
  kind: "engine",
  engine_id: "claude-code",
  hint: "Agent app",
  disabled_reason: null,
  ...over,
});

const TARGETS: DelegateTarget[] = [
  target({ slug: "general", name: "General", kind: "general", engine_id: null, hint: "Same tools as me" }),
  target({}),
  target({ slug: "codex", name: "Codex", engine_id: "codex", disabled_reason: "Off. Turn it on in External agents" }),
];

let container: HTMLDivElement;
let root: Root;
const sent: { text: string; extras: unknown }[] = [];

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
    skills: [],
    turnModifiers: {},
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    activeRun: null,
    busy: false,
    changeSets: {},
    workspaceMode: false,
    toolsEnabled: true,
    delegateTargets: TARGETS,
    engines: [{ id: "claude-code", enabled: true, delegate_ready: true } as never],
    view: "chat",
    engineFocus: null,
  });
  act(() => {
    root.render(<Composer onSend={(text, _a, extras) => sent.push({ text, extras })} />);
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const input = () => container.querySelector("input[type=text]") as HTMLInputElement;
const choices = () => Array.from(container.querySelectorAll(".command-menu-choice")).map((n) => n.textContent);
const rows = () => Array.from(container.querySelectorAll<HTMLElement>(".command-menu-row"));

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

describe("/delegate names an agent in the menu", () => {
  it("lists who can take it while the first word is being typed, a disabled one with its reason", () => {
    type("/delegate ");
    expect(choices()).toEqual(["General", "Claude Code", "Codex"]);
    const codex = rows().find((r) => r.textContent?.includes("Codex"))!;
    expect(codex.getAttribute("aria-disabled")).toBe("true");
    expect(codex.textContent).toContain("Off. Turn it on in External agents");
  });

  it("narrows to what is typed, and shows no choices once the task begins", () => {
    type("/delegate cl");
    expect(choices()).toEqual(["Claude Code"]);
    type("/delegate claude-code fix the tests");
    expect(choices()).toEqual([]);
  });

  it("Tab takes the agent into the box and leaves the task to be typed", async () => {
    type("/delegate cl");
    await press("Tab");
    expect(input().value).toBe("/delegate claude-code ");
    expect(sent).toHaveLength(0);
  });

  it("Enter on an agent takes it in, and does not send", async () => {
    type("/delegate ");
    await press("ArrowDown");
    await press("Enter");
    expect(input().value).toBe("/delegate claude-code ");
    expect(sent).toHaveLength(0);
  });

  it("then the task goes out with the agent named for that message", async () => {
    type("/delegate claude-code fix the tests");
    await press("Enter");
    expect(sent).toHaveLength(1);
    expect(sent[0].text).toBe("fix the tests");
    expect(sent[0].extras).toMatchObject({ skill: "delegate", skillArgs: "fix the tests", delegateTo: "claude-code" });
  });

  it("a task with no agent goes out as it always did", async () => {
    type("/delegate fix the tests");
    await press("Enter");
    expect(sent[0].text).toBe("fix the tests");
    expect(sent[0].extras).toMatchObject({ skill: "delegate" });
    expect((sent[0].extras as { delegateTo?: string }).delegateTo).toBeUndefined();
  });

  it("choosing one that is off opens its settings instead of going into the box", async () => {
    type("/delegate ");
    await press("ArrowDown");
    await press("ArrowDown");
    await press("Enter");
    expect(useAppStore.getState().view).toBe("engines");
    expect(useAppStore.getState().engineFocus).toBe("codex");
    expect(input().value).toBe("");
    expect(sent).toHaveLength(0);
  });
});

describe("the chip for a chat that keeps agent apps out", () => {
  const chip = () => Array.from(container.querySelectorAll(".mode-chip-label")).map((n) => n.textContent);

  it("is not there in the normal state", () => {
    expect(chip()).not.toContain("External agents off");
  });

  it("is there when the chat turned them off, and the x turns them back on", async () => {
    await act(async () => {
      useAppStore.setState({ conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], enginesOff: true }] });
    });
    expect(chip()).toContain("External agents off");
    const x = container.querySelector<HTMLButtonElement>('button[aria-label="Let me hand work to external agents again"]')!;
    await act(async () => x.click());
    expect(useAppStore.getState().conversations[0].enginesOff).toBe(false);
  });

  it("stays away when no agent app could have been used anyway", async () => {
    await act(async () => {
      useAppStore.setState({
        engines: [],
        conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], enginesOff: true }],
      });
    });
    expect(chip()).not.toContain("External agents off");
  });
});
