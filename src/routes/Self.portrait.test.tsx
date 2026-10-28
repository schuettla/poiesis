/**
 * @vitest-environment jsdom
 *
 * `MET-UI-1`: the Self header says how I do, in words.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../components/Self/SelfPanel", () => ({ default: () => null, useSelfRefresh: () => {} }));
vi.mock("../components/Self/GrowthRings", () => ({ default: () => null }));

import Self from "./Self";
import { useAppStore } from "../lib/store";
import type { Vitality } from "../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const vitality = (portrait: string): Vitality => ({
  facts: 3,
  lessons: 1,
  skills: 0,
  skill_uses: 0,
  quarantined: [],
  engine_restarts_session: 0,
  pending_proposals: 0,
  unsure: 0,
  last_reflection: null,
  tool_health: [],
  kinds: [],
  confidence: [],
  portrait,
});

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("Self portrait", () => {
  it("shows the sentence between the narrative and the counts", () => {
    const text = "I'm reliable with mail. I often get reports wrong on the first try, so I check that work before I go on.";
    useAppStore.setState({ vitality: vitality(text) });
    act(() => root.render(<Self />));
    const portrait = container.querySelector(".self-portrait");
    expect(portrait?.textContent).toBe(text);
    expect(portrait?.previousElementSibling?.className).toContain("lede");
    expect(portrait?.nextElementSibling?.className).toContain("self-strip");
  });

  it("shows nothing until a kind of work has been judged", () => {
    useAppStore.setState({ vitality: vitality("") });
    act(() => root.render(<Self />));
    expect(container.querySelector(".self-portrait")).toBeNull();
  });
});
