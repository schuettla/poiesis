/**
 * @vitest-environment jsdom
 *
 * `WRK-21`: the rings have a second kind of entry. Every change to how I work is a small
 * dot on the ring of its week, so the organization's growth can be seen next to what was learned.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import GrowthRings from "./GrowthRings";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const day = (ago: number) => new Date(Date.now() - ago * 86_400_000).toISOString().slice(0, 10);
const born = Date.now() - 20 * 86_400_000;

describe("the rings", () => {
  it("draw only rings when nothing changed about how I work", () => {
    act(() => root.render(<GrowthRings entries={[{ created: day(3) }]} born={born} />));
    expect(container.querySelectorAll("circle").length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".growth-change").length).toBe(0);
  });

  it("add a dot for each change, three at most for one week", () => {
    const changes = [1, 1, 1, 1, 1].map((d) => ({ created: day(d) }));
    act(() => root.render(<GrowthRings entries={[]} born={born} changes={changes} />));
    expect(container.querySelectorAll(".growth-change").length).toBe(3);
  });

  it("put a dot on the week it happened and not on another", () => {
    const changes = [{ created: day(15) }, { created: day(1) }];
    act(() => root.render(<GrowthRings entries={[]} born={born} changes={changes} />));
    expect(container.querySelectorAll(".growth-change").length).toBe(2);
  });
});
