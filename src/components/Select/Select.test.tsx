/**
 * @vitest-environment jsdom
 */
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Select from "./Select";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OPTIONS = [
  { value: "a", label: "Alpha" },
  { value: "b", label: "Beta", hint: "2" },
  { value: "c", label: "Gamma" },
];

let container: HTMLDivElement;
let root: Root;
let last = "";

function Host() {
  const [v, setV] = useState("a");
  return (
    <Select
      label="Pick one"
      value={v}
      options={OPTIONS}
      onChange={(x) => {
        last = x;
        setV(x);
      }}
    />
  );
}

beforeEach(() => {
  last = "";
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const trigger = () => container.querySelector<HTMLButtonElement>("button")!;
const key = (k: string) => act(async () => void trigger().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));

describe("Select", () => {
  it("shows the chosen label and opens a list of options", async () => {
    await act(async () => root.render(<Host />));
    expect(trigger().textContent).toBe("Alpha");
    expect(container.querySelector('[role="listbox"]')).toBeNull();
    await act(async () => trigger().click());
    expect(container.querySelectorAll('[role="option"]')).toHaveLength(3);
    expect(container.querySelector('[aria-selected="true"]')?.textContent).toBe("Alpha");
  });

  it("picks with the mouse and closes", async () => {
    await act(async () => root.render(<Host />));
    await act(async () => trigger().click());
    await act(async () => container.querySelectorAll<HTMLElement>('[role="option"]')[2].click());
    expect(last).toBe("c");
    expect(trigger().textContent).toBe("Gamma");
    expect(container.querySelector('[role="listbox"]')).toBeNull();
  });

  it("moves with the arrow keys, picks with Enter and closes on Escape", async () => {
    await act(async () => root.render(<Host />));
    await key("ArrowDown"); // opens
    await key("ArrowDown"); // Beta
    await key("Enter");
    expect(last).toBe("b");
    await key("ArrowDown");
    expect(container.querySelector('[role="listbox"]')).not.toBeNull();
    await key("Escape");
    expect(container.querySelector('[role="listbox"]')).toBeNull();
    expect(last).toBe("b");
  });
});
