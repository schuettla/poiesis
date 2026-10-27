/**
 * @vitest-environment jsdom
 *
 * `DEC-UI-1`: the Decision model tab in Models holds the models on this computer and in
 * the cloud in one place, and "Use this one" on either is the whole choice.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CloudDecisionCatalog, CloudDecisionStatus, DecideModels } from "../../lib/api";

const decideModels = vi.fn<() => Promise<DecideModels>>();
const decideCloudStatus = vi.fn<() => Promise<CloudDecisionStatus>>();
const decideCloudCatalog = vi.fn<() => Promise<CloudDecisionCatalog>>();
const setDecideBackend = vi.fn<(b: "local" | "cloud") => Promise<void>>();
const setDefaultDecideModel = vi.fn<(id: string) => Promise<void>>();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  decideModels: () => decideModels(),
  decideCloudStatus: () => decideCloudStatus(),
  decideCloudCatalog: () => decideCloudCatalog(),
  setDecideBackend: (b: "local" | "cloud") => setDecideBackend(b),
  setDefaultDecideModel: (id: string) => setDefaultDecideModel(id),
}));

import { DecisionModelGroup } from "./DecisionModel";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const local = (over = {}) => ({
  name: "Clef-flash",
  note: "",
  size_label: "~6.5 GB",
  size_mb: 6500,
  id: null,
  installed: false,
  is_default: false,
  fit: "cpu" as const,
  ...over,
});
const cloudStatus = (over: Partial<CloudDecisionStatus> = {}): CloudDecisionStatus => ({
  backend: "local",
  model: "cloudflare/clef-flash",
  key_set: true,
  state: "ready",
  until: null,
  median_ms: null,
  ...over,
});
const flush = () => act(async () => {});

/** Rendered from nothing each time, so the panel reads what the mocks say now. */
async function mount() {
  await act(async () => root.render(null));
  await act(async () => root.render(<DecisionModelGroup />));
  await flush();
}

beforeEach(() => {
  decideModels.mockReset().mockResolvedValue({ models: [local(), local({ name: "Clef", fit: "wont-fit" })], chat_model: null, placement: "auto" });
  decideCloudStatus.mockReset().mockResolvedValue(cloudStatus());
  decideCloudCatalog.mockReset().mockResolvedValue({
    key_set: true,
    models: [],
    fetched_at: null,
    error: null,
    selected: "cloudflare/clef-flash",
    backend: "local",
  });
  setDecideBackend.mockReset().mockResolvedValue(undefined);
  setDefaultDecideModel.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the Decision model tab", () => {
  it("lists the models on this computer, then the cloud, with nothing to choose besides the models", async () => {
    await mount();
    expect(Array.from(container.querySelectorAll(".section-title")).map((h) => h.textContent?.trim())).toEqual([
      "Decision model",
      "On this PC",
      "Via OpenRouter",
    ]);
    expect(Array.from(container.querySelectorAll(".model-row-name")).map((n) => n.textContent)).toEqual(["Clef-flash", "Clef"]);
    expect(container.querySelector(".backend-list")).toBeNull();
  });

  it("follows the page's All / On this PC / Cloud filter", async () => {
    await act(async () => root.render(null));
    await act(async () => root.render(<DecisionModelGroup filter="local" />));
    await flush();
    expect(Array.from(container.querySelectorAll(".section-title")).map((h) => h.textContent?.trim())).toEqual(["Decision model", "On this PC"]);
    await act(async () => root.render(null));
    await act(async () => root.render(<DecisionModelGroup filter="cloud" />));
    await flush();
    expect(Array.from(container.querySelectorAll(".section-title")).map((h) => h.textContent?.trim())).toEqual(["Decision model", "Via OpenRouter"]);
  });

  it("says it is not set up while nothing runs, and where it runs once something does", async () => {
    await mount();
    expect(container.querySelector(".section-note")?.textContent).toBe("Not set up");

    decideModels.mockResolvedValue({ models: [local({ installed: true, is_default: true, id: "m1" })], chat_model: null, placement: "auto" });
    await mount();
    expect(container.querySelector(".section-note")?.textContent).toBe("Running on this computer");

    decideCloudStatus.mockResolvedValue(cloudStatus({ backend: "cloud" }));
    await mount();
    expect(container.querySelector(".section-note")?.textContent).toBe("Running in the cloud");
  });

  it("using a local model also moves quick decisions back to this computer", async () => {
    decideCloudStatus.mockResolvedValue(cloudStatus({ backend: "cloud" }));
    decideModels.mockResolvedValue({ models: [local({ installed: true, is_default: true, id: "m1" })], chat_model: null, placement: "auto" });
    await mount();
    const row = container.querySelector(".model-row")!;
    expect(row.classList.contains("in-use")).toBe(false);
    await act(async () =>
      Array.from(row.querySelectorAll("button")).find((b) => b.textContent === "Use this one")!.click()
    );
    await flush();
    expect(setDefaultDecideModel).toHaveBeenCalledWith("m1");
    expect(setDecideBackend).toHaveBeenCalledWith("local");
  });
});
