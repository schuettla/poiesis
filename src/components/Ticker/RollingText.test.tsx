/**
 * @vitest-environment jsdom
 *
 * Numbers in a line flap only where they change: a line read back from history stands still.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RollingText } from "./RollingNumber";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const render = (text: string) => act(() => root.render(<RollingText text={text} />));
const moving = () => [...container.querySelectorAll(".tick-text.in")].map((e) => e.textContent);
const numbers = () => [...container.querySelectorAll(".roll")].map((e) => e.getAttribute("aria-label"));

describe("RollingText", () => {
  it("stands still until something changes", () => {
    render("plan 1 of 5");
    expect(container.textContent).toBe("plan 1 of 5");
    expect(numbers()).toEqual(["1", "5"]);
    expect(moving()).toEqual([]);
  });

  it("flaps only the digit that changed", () => {
    render("plan 1 of 5");
    render("plan 2 of 5");
    expect(moving()).toEqual(["2"]);
    act(() => vi.runAllTimers());
    expect(container.textContent).toBe("plan 2 of 5");
  });

  it("flaps a digit that a longer number gains", () => {
    render("$0.09 so far");
    render("$10.09 so far");
    expect(moving()).toEqual(["1"]);
    expect(numbers()).toEqual(["10.09"]);
  });
});
