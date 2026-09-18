/**
 * @vitest-environment jsdom
 *
 * How hard to think, in the composer beside the model picker.
 *
 * The bug behind this control: the app sent no reasoning parameter at all, so
 * every provider applied its own default — which is always its maximum. That is
 * slow, expensive, and the state most likely to end in a model that thinks for
 * ten minutes and never answers.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const saved: Array<[string, string]> = [];
let stored: string | null = null;

vi.mock("../../lib/api", () => ({
  getSetting: (k: string) => Promise.resolve(k === "models.reasoning_effort" ? stored : null),
  setSetting: (k: string, v: string) => {
    saved.push([k, v]);
    return Promise.resolve();
  },
}));

import EffortPicker from "./EffortPicker";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  saved.length = 0;
  stored = null;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

async function render() {
  await act(async () => {
    root.render(<EffortPicker />);
  });
}

const trigger = () => container.querySelector<HTMLButtonElement>(".effort-trigger")!;
const options = () => Array.from(container.querySelectorAll<HTMLElement>(".effort-option"));

async function open() {
  await act(async () => trigger().click());
}

describe("the effort picker", () => {
  it("starts at brief, not at the provider's maximum", async () => {
    await render();
    expect(trigger().textContent).toContain("Think briefly");
  });

  it("shows what is stored when something is stored", async () => {
    stored = "high";
    await render();
    expect(trigger().textContent).toContain("Think hard");
  });

  it("offers off through hard, plus taking the provider's own setting", async () => {
    await render();
    await open();
    expect(options().map((o) => o.querySelector(".name")?.textContent)).toEqual([
      "No thinking",
      "Think briefly",
      "Think",
      "Think hard",
      "Model's default",
    ]);
  });

  it("saves the choice and closes", async () => {
    await render();
    await open();
    await act(async () => options()[3].click());
    expect(saved).toEqual([["models.reasoning_effort", "high"]]);
    expect(options()).toHaveLength(0);
    expect(trigger().textContent).toContain("Think hard");
  });

  it("is a button and a listbox, not a native select", async () => {
    // A native `<select>` renders the operating system's own combobox, which
    // in this footer would sit next to a bespoke pill looking like it came
    // from a different application. That was the first attempt.
    await render();
    expect(container.querySelector("select")).toBeNull();
    expect(trigger().getAttribute("aria-haspopup")).toBe("listbox");
    await open();
    expect(container.querySelector('[role="listbox"]')).not.toBeNull();
  });

  it("closes on Escape, the same as the model picker beside it", async () => {
    await render();
    await open();
    expect(options()).toHaveLength(5);
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(options()).toHaveLength(0);
  });

  it("draws the provider's default as unknown rather than as a level", async () => {
    stored = "provider";
    await render();
    // Three outlined bars, no filled ones: it is not a point on this scale.
    expect(trigger().querySelectorAll(".effort-meter i.on")).toHaveLength(0);
    expect(trigger().querySelectorAll(".effort-meter i.unknown")).toHaveLength(3);
  });

  it("fills the meter to the level", async () => {
    stored = "medium";
    await render();
    expect(trigger().querySelectorAll(".effort-meter i.on")).toHaveLength(2);
  });
});
