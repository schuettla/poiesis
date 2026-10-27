/**
 * @vitest-environment jsdom
 *
 * `DEC-UI-1` (the cloud group in Models) and `DEC-UI-2` (Where, in Runtime): the
 * words for a model, the Test, the choice of backend, and what the cloud line says
 * when it cannot be used.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CloudDecisionCatalog, CloudDecisionModel, CloudDecisionStatus, CloudProbe } from "../../lib/api";

const decideCloudCatalog = vi.fn<(refresh?: boolean) => Promise<CloudDecisionCatalog>>();
const decideCloudProbe = vi.fn<(slug: string) => Promise<CloudProbe>>();
const decideCloudSelect = vi.fn<(slug: string) => Promise<void>>();
const setDecideBackend = vi.fn<(b: "local" | "cloud") => Promise<void>>();
/** What the Runtime card reads, when a test says so; otherwise the real calls, which fail
 * outside the app and are ignored as they always were. */
let quick: CloudDecisionStatus | null = null;
vi.mock("../../lib/api", async (importOriginal) => {
  const real = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...real,
    inTauri: () => true,
    decideCloudCatalog: (r?: boolean) => decideCloudCatalog(r),
    decideCloudProbe: (s: string) => decideCloudProbe(s),
    decideCloudSelect: (s: string) => decideCloudSelect(s),
    setDecideBackend: (b: "local" | "cloud") => setDecideBackend(b),
    decideCloudStatus: () => (quick ? Promise.resolve(quick) : real.decideCloudStatus()),
    decideModels: () =>
      quick ? Promise.resolve({ models: [], chat_model: null, placement: "auto" as const }) : real.decideModels(),
    decideStatus: () => (quick ? Promise.resolve(null as never) : real.decideStatus()),
    decideCalibration: () => (quick ? Promise.resolve([]) : real.decideCalibration()),
  };
});

import { useAppStore } from "../../lib/store";
import {
  CloudModels,
  cloudLine,
  contextText,
  costPer1000,
  imagesExperimental,
  inputText,
  priceText,
  visionText,
} from "./CloudDecisions";
import { QuickDecisionsCard, cloudCell } from "./DecisionModel";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const model = (over: Partial<CloudDecisionModel> = {}): CloudDecisionModel => ({
  id: "cloudflare/clef-flash",
  slug: "cloudflare/clef-flash",
  name: "Clef Flash",
  context: 24_576,
  images: true,
  price_per_mtok: 0.019,
  free: false,
  zdr: true,
  same_as_local: "Clef-flash",
  featured: true,
  selected: false,
  probe: null,
  ...over,
});

const catalog = (models: CloudDecisionModel[], over: Partial<CloudDecisionCatalog> = {}): CloudDecisionCatalog => ({
  key_set: true,
  models,
  fetched_at: new Date(2026, 9, 26, 9).getTime(),
  error: null,
  selected: "cloudflare/clef-flash",
  backend: "local",
  ...over,
});

const status = (over: Partial<CloudDecisionStatus> = {}): CloudDecisionStatus => ({
  backend: "local",
  model: "cloudflare/clef-flash",
  key_set: true,
  state: "ready",
  until: null,
  median_ms: null,
  ...over,
});

describe("the words for a cloud model", () => {
  it("writes the context the way the catalog does", () => {
    expect(contextText(16_384)).toBe("16k");
    expect(contextText(65_536)).toBe("64k");
    expect(contextText(1_050_000)).toBe("1.1M");
    expect(contextText(0)).toBe("context not listed");
  });

  it("prices the input, and says when there is no price instead of calling it free", () => {
    expect(priceText(model())).toBe("$0.02 / M");
    expect(priceText(model({ price_per_mtok: 0.15 }))).toBe("$0.15 / M");
    expect(priceText(model({ free: true, price_per_mtok: 0 }))).toBe("free");
    expect(priceText(model({ price_per_mtok: null }))).toBe("price not listed");
  });

  it("works out about what a thousand decisions cost", () => {
    expect(costPer1000(model())).toBe("~$0.02 per 1,000 decisions");
    expect(costPer1000(model({ free: true, price_per_mtok: 0 }))).toBe("free");
    expect(costPer1000(model({ price_per_mtok: null }))).toBeNull();
    expect(costPer1000(undefined)).toBeNull();
  });

  it("says what goes in, and marks Jev's pictures experimental", () => {
    expect(inputText({ images: true })).toBe("text + images");
    expect(inputText({ images: false })).toBe("text");
    expect(imagesExperimental({ id: "typesafe/jev-1.14", images: true })).toBe(true);
    expect(imagesExperimental({ id: "typesafe/jev-1.14", images: false })).toBe(false);
    expect(imagesExperimental({ id: "cloudflare/clef", images: true })).toBe(false);
  });

  it("reports what the Test found about pictures, and does not claim it when it did not test", () => {
    const probe = (vision: boolean | null): CloudProbe => ({ at: 1, ok: true, ms: 120, vision, error: null });
    expect(visionText(model({ probe: probe(true) }))).toBe("images ✓");
    expect(visionText(model({ probe: probe(false) }))).toBe("images: not accepted via OpenRouter");
    expect(visionText(model())).toBe("images listed, not tested");
    expect(visionText(model({ images: false }))).toBeNull();
    // A failed Test says nothing about pictures.
    expect(visionText(model({ probe: { at: 1, ok: false, ms: 0, vision: null, error: "x" } }))).toBe("images listed, not tested");
  });
});

describe("cloudLine (DEC-UI-2)", () => {
  it("shows nothing while the cloud is not the chosen backend", () => {
    expect(cloudLine(null)).toBeNull();
    expect(cloudLine(status({ backend: "local", state: "credits" }))).toBeNull();
  });

  it("is green when ready, with the median time when there is one", () => {
    expect(cloudLine(status({ backend: "cloud" }))).toEqual({ tone: "ok", text: "Ready" });
    expect(cloudLine(status({ backend: "cloud", median_ms: 180 }))).toEqual({ tone: "ok", text: "Ready · 180 ms" });
  });

  it("says why it is using this computer", () => {
    const line = (state: CloudDecisionStatus["state"], until: number | null = null) =>
      cloudLine(status({ backend: "cloud", state, until }));
    expect(line("key")).toEqual({ tone: "bad", text: "OpenRouter key rejected. Using this computer." });
    expect(line("credits")?.text).toBe("Out of OpenRouter credits, using this computer.");
    expect(line("no-key")?.tone).toBe("bad");
    expect(line("no-endpoint")?.text).toContain("keeps nothing");
    const busy = line("overloaded", new Date(2026, 9, 26, 14, 32).getTime())!;
    expect(busy.tone).toBe("warn");
    expect(busy.text).toMatch(/^Cloud unavailable \(overloaded\), using this computer until \d{1,2}:32/);
    expect(line("overloaded")?.text).toBe("Cloud unavailable (overloaded), using this computer.");
  });
});

describe("cloudCell (Advanced table)", () => {
  it("leaves a decision someone waits for to the choice above", () => {
    expect(cloudCell({ purpose: "tool-select", background: false, backend_override: null }, true, vi.fn())).toBe(
      "follows the choice above"
    );
  });

  // `SEE-1`: the check of an answer is asked of the backend the person chose, like any decision they are there for.
  it("leaves the check of an answer to the choice above", () => {
    expect(cloudCell({ purpose: "claim-checkable", background: false, backend_override: null }, true, vi.fn())).toBe(
      "follows the choice above"
    );
  });
});

let container: HTMLDivElement;
let root: Root;
const setView = vi.fn();
const flush = () => act(async () => {});
const q = (sel: string) => container.querySelector<HTMLElement>(sel);
const qa = (sel: string) => Array.from(container.querySelectorAll<HTMLElement>(sel));
const button = (label: string) => qa("button").find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;
const click = (el: HTMLElement | undefined | null) => act(async () => void el!.dispatchEvent(new MouseEvent("click", { bubbles: true })));

beforeEach(() => {
  quick = null;
  for (const f of [decideCloudCatalog, decideCloudProbe, decideCloudSelect, setDecideBackend, setView]) f.mockReset();
  decideCloudSelect.mockResolvedValue();
  decideCloudProbe.mockResolvedValue({ at: 1, ok: true, ms: 140, vision: true, error: null });
  setDecideBackend.mockResolvedValue();
  useAppStore.setState({ setView });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const five = () => [
  model({ selected: true }),
  model({ id: "cloudflare/clef", slug: "cloudflare/clef", name: "Clef", same_as_local: "Clef" }),
  model({ id: "typesafe/jev-1.14", slug: "typesafe/jev-1.14-20261014", name: "Jev 1.14", same_as_local: null }),
  model({ id: "openai/gpt-6-luna-decisions", slug: "openai/gpt-6-luna-decisions-20261006", name: "GPT-6 Luna", featured: false, zdr: false, same_as_local: null }),
  model({ id: "nace-ai/drex-v2.0", slug: "nace-ai/drex-v2.0-20261009", name: "Drex 2.0", featured: false, images: false, same_as_local: null }),
];

describe("the cloud group in Models", () => {
  it("is one line with a link to Providers while there is no OpenRouter key", async () => {
    decideCloudCatalog.mockResolvedValue(catalog(five(), { key_set: false }));
    await act(async () => root.render(<CloudModels />));
    await flush();
    expect(q(".decide-cloud")!.textContent).toContain("Add an OpenRouter key in Providers");
    expect(qa(".model-row")).toHaveLength(0);
    await click(button("Open Providers →"));
    expect(setView).toHaveBeenCalledWith("providers");
  });

  it("shows the featured models and the chosen one, and the rest behind Show all", async () => {
    decideCloudCatalog.mockResolvedValue(catalog(five()));
    await act(async () => root.render(<CloudModels />));
    await flush();
    expect(qa(".model-row-name").map((n) => n.textContent)).toEqual(["Clef Flash", "Clef", "Jev 1.14"]);
    expect(q(".decide-cloud")!.textContent).toContain("2 more are hidden");
    await click(button("Show all 5"));
    expect(qa(".model-row-name")).toHaveLength(5);
    expect(button("Show fewer")).toBeDefined();
  });

  it("describes each row: input, context, price, same as local, and Jev's pictures as experimental", async () => {
    decideCloudCatalog.mockResolvedValue(catalog(five()));
    await act(async () => root.render(<CloudModels />));
    await flush();
    const rows = qa(".model-row");
    expect(rows[0].textContent).toContain("text + images");
    expect(rows[0].textContent).toContain("24k");
    expect(rows[0].textContent).toContain("$0.02 / M");
    expect(rows[0].textContent).toContain("same as local ✓");
    expect(rows[2].textContent).toContain("images: experimental");
    expect(rows[2].textContent).not.toContain("same as local");
  });

  it("greys a model with no zero-retention provider and does not let it be picked", async () => {
    decideCloudCatalog.mockResolvedValue(catalog(five()));
    await act(async () => root.render(<CloudModels />));
    await flush();
    await click(button("Show all 5"));
    const luna = qa(".model-row").find((r) => r.textContent!.includes("GPT-6 Luna"))!;
    expect(luna.className).toContain("unavailable");
    expect(luna.textContent).toContain("no zero-retention provider");
    expect((luna.querySelector("button") as HTMLButtonElement).disabled).toBe(true);
  });

  it("picking a model is the whole choice: it selects it, runs quick decisions in the cloud, and tests it once", async () => {
    const rows = five();
    decideCloudCatalog.mockResolvedValueOnce(catalog(rows));
    decideCloudCatalog.mockResolvedValue(
      catalog(
        rows.map((m) => (m.slug === "cloudflare/clef" ? { ...m, selected: true, probe: { at: 1, ok: true, ms: 140, vision: true, error: null } } : { ...m, selected: false })),
        { backend: "cloud" }
      )
    );
    await act(async () => root.render(<CloudModels />));
    await flush();
    const clef = qa(".model-row").find((r) => r.querySelector(".model-row-name")!.textContent === "Clef")!;
    await click(clef.querySelector("button"));
    await flush();
    expect(decideCloudSelect).toHaveBeenCalledWith("cloudflare/clef");
    expect(setDecideBackend).toHaveBeenCalledWith("cloud");
    expect(decideCloudProbe).toHaveBeenCalledWith("cloudflare/clef");
    expect(q(".decide-probe")!.textContent).toBe("Tested · answered in 140 ms");
    expect(q(".default-tag")!.textContent).toBe("In use");
  });

  it("a model that was picked before but is not running in the cloud offers Use this one, not In use", async () => {
    decideCloudCatalog.mockResolvedValue(catalog(five(), { backend: "local" }));
    await act(async () => root.render(<CloudModels />));
    await flush();
    expect(q(".default-tag")).toBeNull();
    const flash = qa(".model-row").find((r) => r.querySelector(".model-row-name")!.textContent === "Clef Flash")!;
    expect(flash.textContent).toContain("Use this one");
    await click(Array.from(flash.querySelectorAll("button")).find((b) => b.textContent === "Use this one")!);
    expect(setDecideBackend).toHaveBeenCalledWith("cloud");
  });

  it("tests the chosen model on request and shows why a Test failed", async () => {
    const rows = five();
    decideCloudCatalog.mockResolvedValueOnce(catalog(rows));
    decideCloudCatalog.mockResolvedValue(
      catalog(rows.map((m) => (m.selected ? { ...m, probe: { at: 1, ok: false, ms: 5000, vision: null, error: "OpenRouter rejected the key." } } : m)))
    );
    await act(async () => root.render(<CloudModels />));
    await flush();
    await click(button("Test"));
    await flush();
    expect(decideCloudProbe).toHaveBeenCalledWith("cloudflare/clef-flash");
    expect(q(".decide-probe.bad")!.textContent).toBe("Test failed: OpenRouter rejected the key.");
  });

  it("reads OpenRouter's list again on request, and shows why it could not", async () => {
    decideCloudCatalog.mockResolvedValueOnce(catalog(five()));
    decideCloudCatalog.mockResolvedValue(catalog(five(), { error: "Couldn't reach OpenRouter. Are you online?" }));
    await act(async () => root.render(<CloudModels />));
    await flush();
    await click(qa("button").find((b) => b.textContent!.startsWith("↻")));
    expect(decideCloudCatalog).toHaveBeenLastCalledWith(true);
    expect(q(".hw-note.error")!.textContent).toBe("Couldn't reach OpenRouter. Are you online?");
  });

  it("shows the error when picking a model is refused", async () => {
    decideCloudCatalog.mockResolvedValue(catalog(five()));
    decideCloudSelect.mockRejectedValue(new Error("No provider of that model keeps nothing."));
    await act(async () => root.render(<CloudModels />));
    await flush();
    await click(qa(".model-row")[1].querySelector("button"));
    await flush();
    expect(q(".hw-note.error")!.textContent).toContain("keeps nothing");
  });
});

describe("the Runtime card and typing (SEE-3)", () => {
  const typingLine = () => qa(".runtime-hint").find((p) => p.textContent?.includes("Hints while you type"));

  it("says in one line that hints while typing come from the cloud when quick decisions run there", async () => {
    quick = status({ backend: "cloud" });
    await act(async () => root.render(<QuickDecisionsCard />));
    await flush();
    expect(typingLine()?.textContent).toBe(
      "Hints while you type come from the cloud too, so what you type goes there before you send it.",
    );
  });

  it("says nothing of it while they run on this computer", async () => {
    quick = status({ backend: "local" });
    await act(async () => root.render(<QuickDecisionsCard />));
    await flush();
    expect(typingLine()).toBeUndefined();
  });
});
