/**
 * @vitest-environment jsdom
 *
 * `LRN-T5`: Settings → Tools says why a switched-on tool is not being offered
 * (no mail account, no image model), and says nothing for one that is ready.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ToolsetInfo } from "../lib/api";

let toolsets: ToolsetInfo[] = [];
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  listToolsets: () => Promise.resolve(toolsets),
  getToolStats: () => Promise.resolve([]),
  listIndexRoots: () => Promise.resolve([]),
  getSetting: () => Promise.resolve(null),
  setSetting: () => Promise.resolve(),
  setToolsetEnabled: () => Promise.resolve(),
}));

import Tools from "./Tools";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const set = (over: Partial<ToolsetInfo>): ToolsetInfo => ({
  id: "mail",
  label: "Mail",
  description: "Read, search, and send email.",
  enabled: true,
  sensitive: false,
  ...over,
});

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

async function show(list: ToolsetInfo[]) {
  toolsets = list;
  await act(async () => {
    root.render(<Tools />);
  });
}

describe("a tool that is on but cannot be offered yet", () => {
  it("says what is missing, in first person, under its description", async () => {
    await show([set({ needs: "I can't use this yet. Add a mail account in Settings → Mail." })]);
    expect(container.querySelector(".toolset-needs")?.textContent).toBe(
      "I can't use this yet. Add a mail account in Settings → Mail."
    );
  });

  it("says nothing for a tool that is ready", async () => {
    await show([set({}), set({ id: "image_gen", label: "Image generation" })]);
    expect(container.querySelector(".toolset-needs")).toBeNull();
  });
});
