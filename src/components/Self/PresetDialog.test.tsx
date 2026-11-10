/**
 * @vitest-environment jsdom
 *
 * `WRK-17`: the dialog that makes a preset of your own. The shared confirm shell; it
 * never changes height; it takes what it was opened with (a preset to start from, the
 * moves that fired in a run) and says plainly when the backend refuses.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ createPreset: vi.fn(), updatePreset: vi.fn() }));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  ...api,
}));

import PresetDialog from "./PresetDialog";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const loadPresets = vi.fn();
const onDone = vi.fn();

const set = (el: HTMLInputElement, value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
const inputs = () => [...container.querySelectorAll<HTMLInputElement>("input.field-input")];
const press = async (label: string) =>
  act(async () => [...container.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!.click());

beforeEach(() => {
  api.createPreset.mockReset().mockResolvedValue("careful");
  api.updatePreset.mockReset().mockResolvedValue(undefined);
  loadPresets.mockReset().mockResolvedValue(undefined);
  onDone.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    presets: [
      { slug: "ad-hoc", name: "Ad hoc", line: "", glyph: "ad-hoc", built_in: true, locked: false, version: 1, rules: [] },
      { slug: "lean", name: "Lean", line: "", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] },
    ],
    loadPresets,
    presetDialog: null,
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const open = (req: NonNullable<ReturnType<typeof useAppStore.getState>["presetDialog"]>) => {
  act(() => root.render(<PresetDialog />));
  act(() => useAppStore.getState().openPresetDialog(req));
};

describe("the preset dialog", () => {
  it("is not there until asked for", () => {
    act(() => root.render(<PresetDialog />));
    expect(container.querySelector(".confirm-card")).toBeNull();
  });

  it("makes a preset from one, locked unless told otherwise, and says it is done", async () => {
    open({ mode: "new", from: "lean", onDone });
    const [name, line] = inputs();
    set(name, "Careful with mail");
    set(line, "I ask before anything leaves the house");
    await press("Create");
    expect(api.createPreset).toHaveBeenCalledWith({
      name: "Careful with mail",
      line: "I ask before anything leaves the house",
      from: "lean",
      add: undefined,
      locked: true,
    });
    expect(loadPresets).toHaveBeenCalled();
    expect(onDone).toHaveBeenCalledWith("careful");
    expect(useAppStore.getState().presetDialog).toBeNull();
  });

  it("brings the moves that fired in a run, and says how many", async () => {
    open({ mode: "new", from: "lean", add: [{ when: "risky-action", move: "advice" }] });
    expect(container.textContent).toContain("One more move that fired in that run is kept.");
    set(inputs()[0], "Mine");
    await press("Create");
    expect(api.createPreset).toHaveBeenCalledWith(
      expect.objectContaining({ add: [{ when: "risky-action", move: "advice" }] })
    );
  });

  it("asks for a name before anything is sent", async () => {
    open({ mode: "new", from: null });
    await press("Create");
    expect(api.createPreset).not.toHaveBeenCalled();
    expect(container.querySelector(".preset-problem")?.textContent).toBe("Give it a name.");
    expect(container.querySelector(".confirm-card")).toBeTruthy();
  });

  it("says what the backend said, and stays open", async () => {
    api.createPreset.mockRejectedValue("There is already a way of working called Lean.");
    open({ mode: "new", from: null });
    set(inputs()[0], "Lean");
    await press("Create");
    expect(container.querySelector(".preset-problem")?.textContent).toContain("There is already a way of working");
    expect(useAppStore.getState().presetDialog).not.toBeNull();
  });

  it("renames with only a name and a line", async () => {
    open({ mode: "rename", slug: "mine", name: "Mine", line: "my way" });
    expect(container.textContent).not.toContain("Starts from");
    expect(inputs().map((i) => i.value)).toEqual(["Mine", "my way"]);
    set(inputs()[0], "Mine, better");
    await press("Save");
    expect(api.updatePreset).toHaveBeenCalledWith("mine", { name: "Mine, better", line: "my way" });
    expect(api.createPreset).not.toHaveBeenCalled();
  });

  it("starts from what it was asked, not from the last time", () => {
    open({ mode: "new", from: null, name: "First" });
    expect(inputs()[0].value).toBe("First");
    act(() => useAppStore.getState().closePresetDialog());
    open({ mode: "new", from: null });
    expect(inputs()[0].value).toBe("");
  });
});
