/**
 * @vitest-environment jsdom
 *
 * The first-steps screen: it says what is needed, and "Do it all for me" runs the
 * whole path (download, pick, start, open the chat) without another click.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const catalog = [
  {
    id: "too-big",
    name: "Huge",
    description: "",
    quant: "Q4",
    size_mb: 40000,
    vision: false,
    url: "https://example.com/huge.gguf",
    source: "x",
    license: null,
    fit: "wont-fit",
    speed: "",
  },
  {
    id: "small",
    name: "Small Chat",
    description: "",
    quant: "Q4",
    size_mb: 2048,
    vision: false,
    url: "https://example.com/small.gguf?download=true",
    source: "x",
    license: null,
    fit: "great",
    speed: "",
  },
];

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  detectHardware: () =>
    Promise.resolve({
      cpu: { brand: "Test CPU", physical_cores: 8, avx2: true, avx512: false },
      ram_mb: 16384,
      gpus: [],
    }),
  recommendedCatalog: () => Promise.resolve(catalog),
  listModels: () => Promise.resolve([{ id: "m1", name: "Small Chat", path: "C:\\models\\small.gguf" }]),
}));
// The optional voice step is not under test here.
vi.mock("../lib/voice/controller", () => ({ voiceNeeds: () => Promise.reject(new Error("off")) }));

import FirstSteps from "./FirstSteps";
import { useAppStore } from "../lib/store";

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

const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes(label));

async function mount(state: Record<string, unknown> = {}) {
  useAppStore.setState({
    libraryModels: [],
    providers: [],
    endpointModels: [],
    modelDownloads: {},
    loadingModel: null,
    view: "firststeps",
    ...state,
  } as never);
  await act(async () => root.render(<FirstSteps />));
}

describe("the first-steps screen", () => {
  it("explains what is needed and names the model it would download", async () => {
    await mount();
    const text = container.textContent ?? "";
    expect(text).toContain("What you need");
    expect(text).toContain("An internet connection");
    expect(text).toContain("About 2.0 GB of free disk space");
    // The one that does not fit is skipped.
    expect(text).toContain("Small Chat");
    expect(text).not.toContain("Huge");
    expect(button("Do it all for me")).toBeDefined();
  });

  it("does the whole path on one click: download, pick, start, open the chat", async () => {
    const download = vi.fn().mockResolvedValue(undefined);
    const select = vi.fn();
    const load = vi.fn().mockResolvedValue(undefined);
    await mount({ downloadCatalogModel: download, selectModel: select, loadModelById: load });

    await act(async () => button("Do it all for me")!.click());

    expect(download).toHaveBeenCalledWith(expect.objectContaining({ id: "small" }));
    // The file name is read off the URL without its query string.
    expect(select).toHaveBeenCalledWith("m1");
    expect(load).toHaveBeenCalledWith("m1");
    expect(useAppStore.getState().view).toBe("chat");
  });

  it("stays on the screen and offers a retry when the download fails", async () => {
    const download = vi.fn().mockRejectedValue(new Error("no connection"));
    await mount({ downloadCatalogModel: download });

    await act(async () => button("Do it all for me")!.click());

    expect(container.querySelector('[role="alert"]')?.textContent).toContain("no connection");
    expect(useAppStore.getState().view).toBe("firststeps");
    expect(button("Try again")).toBeDefined();
  });

  it("says the model did not start, and keeps the screen, when the start fails", async () => {
    const download = vi.fn().mockResolvedValue(undefined);
    const load = vi.fn().mockRejectedValue(new Error("out of memory"));
    await mount({ downloadCatalogModel: download, selectModel: vi.fn(), loadModelById: load });

    await act(async () => button("Do it all for me")!.click());

    const alert = container.querySelector('[role="alert"]')?.textContent ?? "";
    expect(alert).toContain("downloaded, but it did not start");
    expect(alert).toContain("out of memory");
    expect(useAppStore.getState().view).toBe("firststeps");
  });

  it("shows a ready state with Open chat when something can already chat", async () => {
    await mount({ providers: [{ key_set: true }] });
    expect(container.textContent).toContain("You are ready to chat");
    expect(button("Do it all for me")).toBeUndefined();
    await act(async () => button("Open chat")!.click());
    expect(useAppStore.getState().view).toBe("chat");
  });
});
