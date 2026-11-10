/**
 * @vitest-environment jsdom
 *
 * `WRK-17`: the way of working chats with a persona start in. Read from where the
 * defaults live, shown as a choice among the presets, and written only when it changed.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ waysView: vi.fn(), setStyleDefault: vi.fn(), updatePersona: vi.fn() }));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  listPersonas: () => Promise.resolve([]),
  ...api,
}));

import PersonaEditor from "./PersonaEditor";
import { useAppStore } from "../../lib/store";
import type { Persona } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const persona: Persona = {
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
  description: null,
  spawnable: false,
  engine_id: null,
};

let container: HTMLDivElement;
let root: Root;
const loadPresets = vi.fn();

beforeEach(() => {
  api.waysView.mockReset().mockResolvedValue({ defaults: [{ scope: "persona:p1", label: "x", preset: "lean", preset_name: "Lean" }] });
  api.setStyleDefault.mockReset().mockResolvedValue(undefined);
  api.updatePersona.mockReset().mockResolvedValue(undefined);
  loadPresets.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    personas: [persona],
    engines: [],
    models: [],
    toolsets: [],
    skills: [],
    view: "settings",
    engineFocus: null,
    loadPresets,
    updatePersona: api.updatePersona,
    presets: [
      { slug: "ad-hoc", name: "Ad hoc", line: "", glyph: "ad-hoc", built_in: true, locked: false, version: 1, rules: [] },
      { slug: "lean", name: "Lean", line: "", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] },
      { slug: "swarm", name: "Swarm", line: "", glyph: "swarm", built_in: true, locked: false, version: 1, rules: [] },
    ],
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const style = () => container.querySelector<HTMLSelectElement>("#persona-style")!;
const edit = async () => {
  act(() => root.render(<PersonaEditor />));
  await act(async () => {
    Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Edit")!.click();
  });
  await act(async () => {});
};
const save = () =>
  act(async () => {
    Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Save changes")!.click();
  });

describe("How this persona works", () => {
  it("offers the default and every preset, and shows the one stored", async () => {
    await edit();
    expect(Array.from(style().options).map((o) => o.textContent)).toEqual(["The default", "Ad hoc", "Lean", "Swarm"]);
    expect(style().value).toBe("lean");
  });

  it("writes nothing when it was not touched", async () => {
    await edit();
    await save();
    expect(api.updatePersona).toHaveBeenCalled();
    expect(api.setStyleDefault).not.toHaveBeenCalled();
  });

  const pick = (value: string) => {
    const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
    act(() => {
      setter.call(style(), value);
      style().dispatchEvent(new Event("change", { bubbles: true }));
    });
  };

  it("writes the change for this persona", async () => {
    await edit();
    pick("swarm");
    await save();
    expect(api.setStyleDefault).toHaveBeenCalledWith("persona:p1", "swarm");
    expect(loadPresets).toHaveBeenCalled();
  });

  it("takes the default away when The default is chosen again", async () => {
    await edit();
    pick("");
    await save();
    expect(api.setStyleDefault).toHaveBeenCalledWith("persona:p1", null);
  });
});
