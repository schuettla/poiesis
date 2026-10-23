/**
 * @vitest-environment jsdom
 *
 * `CLI-UI-4`: a persona that can be handed work may run on an agent app instead of a
 * model of Poiesis's. Choosing one takes the model and tools away and says why.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const updatePersona = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  updatePersona: (...a: unknown[]) => updatePersona(...a),
  listPersonas: () => Promise.resolve([]),
}));

import PersonaEditor from "./PersonaEditor";
import { useAppStore } from "../../lib/store";
import type { Persona } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const persona = (over: Partial<Persona> = {}): Persona => ({
  id: "p1",
  name: "Refactorer",
  system_prompt: "Keep diffs small.",
  model_id: null,
  params_json: null,
  is_default: false,
  created_at: 0,
  updated_at: 0,
  tools_json: null,
  skills_json: null,
  description: "Renames and moves code",
  spawnable: true,
  engine_id: null,
  ...over,
});

const engine = (id: string, name: string, over: Record<string, unknown> = {}) =>
  ({ id, name, found: true, enabled: true, delegate_ready: true, ...over }) as never;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  updatePersona.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function open(p: Persona, engines: unknown[]) {
  useAppStore.setState({ personas: [p], engines, models: [], toolsets: [], skills: [], view: "settings", engineFocus: null } as never);
  act(() => root.render(<PersonaEditor />));
  act(() => {
    Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Edit")!.click();
  });
}

const select = () => container.querySelector<HTMLSelectElement>("#persona-runs-on");
const labels = () => Array.from(container.querySelectorAll("label.field-label")).map((l) => l.textContent);

function choose(id: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
  act(() => {
    setter.call(select(), id);
    select()!.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

describe("Runs on", () => {
  it("offers Poiesis and each agent app that can take a job", () => {
    open(persona(), [engine("claude-code", "Claude Code"), engine("codex", "Codex", { delegate_ready: false }), engine("acp:opencode", "OpenCode", { enabled: false })]);
    const options = Array.from(select()!.options).map((o) => o.textContent);
    expect(options).toEqual(["Poiesis (I choose a model)", "Claude Code"]);
  });

  it("is not there for a persona that cannot be handed work, or when no agent app is set up", () => {
    open(persona({ spawnable: false }), [engine("claude-code", "Claude Code")]);
    expect(select()).toBeNull();
    act(() => root.unmount());
    root = createRoot(container);
    open(persona(), []);
    expect(select()).toBeNull();
  });

  it("choosing an agent app takes the model and tools away and says what it brings", () => {
    open(persona(), [engine("claude-code", "Claude Code")]);
    expect(labels()).toContain("Model (optional)");
    choose("claude-code");
    expect(labels()).not.toContain("Model (optional)");
    expect(container.textContent).toContain("It brings its own model and tools.");
    expect(container.textContent).toContain("I give it this persona's instructions with the task, and the folder.");
  });

  it("saves the agent app with the persona, and Poiesis again as nothing", async () => {
    open(persona(), [engine("claude-code", "Claude Code")]);
    choose("claude-code");
    await act(async () => {
      Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Save changes")!.click();
    });
    expect(updatePersona).toHaveBeenLastCalledWith(expect.objectContaining({ id: "p1", engine_id: "claude-code" }));

    open(persona({ engine_id: "claude-code" }), [engine("claude-code", "Claude Code")]);
    expect(select()!.value).toBe("claude-code");
    choose("");
    await act(async () => {
      Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Save changes")!.click();
    });
    expect(updatePersona).toHaveBeenLastCalledWith(expect.objectContaining({ engine_id: null }));
  });

  it("an agent app that has since been turned off stays on the persona, struck with a way to turn it on", () => {
    open(persona({ engine_id: "claude-code" }), [engine("claude-code", "Claude Code", { enabled: false })]);
    expect(select()!.value).toBe("claude-code");
    expect(Array.from(select()!.options).map((o) => o.textContent)).toContain("Claude Code (off, turn it on in External agents)");
    expect(container.textContent).toContain("Claude Code is off, so I won't hand this agent any work.");
    act(() => {
      Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Turn it on in External agents")!.click();
    });
    expect(useAppStore.getState().view).toBe("engines");
    expect(useAppStore.getState().engineFocus).toBe("claude-code");
  });
});
