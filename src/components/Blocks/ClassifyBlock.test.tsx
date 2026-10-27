/**
 * @vitest-environment jsdom
 *
 * `USE-UI-5`: the table a `classify` call hands the user. Every item with its
 * answer and confidence, the low-confidence filter, the sort, the export, and a
 * correction that becomes a label.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const classifyCorrect = vi.fn((_decision: string, _answer: string) => Promise.resolve("corrected" as const));
const saveArtifactFile = vi.fn((_dest: string, _kind: string, _content: string) => Promise.resolve());
const save = vi.fn((_opts: unknown) => Promise.resolve("C:/out/sorted.csv" as string | null));

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  classifyCorrect: (d: string, a: string) => classifyCorrect(d, a),
  saveArtifactFile: (d: string, k: string, c: string) => saveArtifactFile(d, k, c),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: (o: unknown) => save(o) }));

import { useAppStore } from "../../lib/store";
import type { BlockView } from "../../lib/types";
import BlockRenderer from "./BlockRenderer";
import ClassifyBlock from "./ClassifyBlock";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const data = {
  question: "What kind of ticket is this?",
  kind: "choice",
  choices: [
    { value: "bug", label: "bug" },
    { value: "feature", label: "feature" },
  ],
  bar: 0.7,
  calibrated: false,
  total: 5,
  not_reached: 0,
  stopped: null,
  rows: [
    { item: "Login fails", answer: "bug", value: "bug", confidence: 0.95, low: false, decision: "d0" },
    { item: "Add dark mode", answer: "feature", value: "feature", confidence: 0.55, low: true, decision: "d1" },
    { item: "Crash on save", answer: "bug", value: "bug", confidence: 0.62, low: true, decision: "d2" },
    {
      item: "pic.png",
      path: "C:/pics/pic.png",
      image: true,
      thumb: "data:image/jpeg;base64,AAAA",
      answer: "feature",
      value: "feature",
      confidence: 0.9,
      low: false,
      decision: "d3",
    },
    { item: "notes.bin", path: "C:/notes.bin", note: "it is not a text file" },
  ],
};

const block = (over: Partial<BlockView> = {}): BlockView => ({
  id: "b1",
  kind: "classify",
  title: "Sorted: What kind of ticket is this?",
  data,
  ...over,
});

let container: HTMLDivElement;
let root: Root;
const setBlockState = vi.fn();

async function show(b: BlockView = block()) {
  useAppStore.setState({ setBlockState });
  await act(async () => {
    root.render(<ClassifyBlock block={b} />);
  });
}

const text = () => container.textContent ?? "";
const items = () => Array.from(container.querySelectorAll("tbody tr")).map((r) => r.querySelector(".cls-item")?.textContent);
/** A button by its visible text or its accessible name. */
const button = (label: string | RegExp) =>
  Array.from(container.querySelectorAll("button")).find((b) =>
    [b.textContent ?? "", b.getAttribute("aria-label") ?? ""].some((name) =>
      typeof label === "string" ? name === label : label.test(name)
    )
  )!;

beforeEach(() => {
  classifyCorrect.mockClear();
  saveArtifactFile.mockClear();
  save.mockClear();
  setBlockState.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the classify table", () => {
  it("draws every item with its answer and confidence, and marks the low ones", async () => {
    await show();
    expect(items()).toEqual(["Login fails", "Add dark mode", "Crash on save", "pic.png", "notes.bin"]);
    const rows = Array.from(container.querySelectorAll("tbody tr"));
    expect(rows[0].textContent).toContain("0.95");
    expect(rows[1].className).toContain("cls-low");
    expect(rows[1].textContent).toContain("0.55");
    expect(rows[1].textContent).toContain("low");
    expect(rows[0].className).not.toContain("cls-low");
    expect(text()).toContain("5 rows \u00b7 2 low confidence \u00b7 1 not sorted");
  });

  it("says why an item was not sorted instead of showing an answer", async () => {
    await show();
    expect(text()).toContain("Not sorted: it is not a text file");
  });

  it("is reached from the block renderer by its kind", async () => {
    useAppStore.setState({ setBlockState });
    await act(async () => {
      root.render(<BlockRenderer block={block()} />);
    });
    expect(container.querySelector("table")).not.toBeNull();
    expect(container.querySelector(".block-kind")?.textContent).toBe("classify");
  });

  it("filters to the low-confidence rows, with their count on the chip", async () => {
    await show();
    expect(button(/^Low confidence/).textContent).toBe("Low confidence (2)");
    await act(async () => button(/^Low confidence/).click());
    expect(items()).toEqual(["Add dark mode", "Crash on save"]);
    await act(async () => button(/^All/).click());
    expect(items()).toHaveLength(5);
  });

  it("says so when no row is low confidence", async () => {
    await show(block({ data: { ...data, rows: [data.rows[0]] } }));
    await act(async () => button(/^Low confidence/).click());
    expect(text()).toContain("No row is low confidence.");
  });

  it("sorts by a column: ascending, descending, then the original order", async () => {
    await show();
    const header = () => container.querySelectorAll("th")[2];
    await act(async () => button("Confidence").click());
    expect(items()).toEqual(["Add dark mode", "Crash on save", "pic.png", "Login fails", "notes.bin"]);
    expect(header().getAttribute("aria-sort")).toBe("ascending");
    await act(async () => button(/^Confidence/).click());
    expect(items()).toEqual(["Login fails", "pic.png", "Crash on save", "Add dark mode", "notes.bin"]);
    expect(header().getAttribute("aria-sort")).toBe("descending");
    await act(async () => button(/^Confidence/).click());
    expect(items()[0]).toBe("Login fails");
    expect(header().getAttribute("aria-sort")).toBe("none");
  });

  it("says in the hint how far to trust the numbers until they are checked", async () => {
    await show();
    const hint = container.querySelectorAll("th")[2].getAttribute("title")!;
    expect(hint).toContain("Under 0.70 a row is marked low");
    expect(hint).toContain("have not been checked against examples");
    await show(block({ data: { ...data, calibrated: true } }));
    expect(container.querySelectorAll("th")[2].getAttribute("title")).not.toContain("have not been checked");
  });

  it("shows the thumbnail an image item carries, and nothing for one that has none", async () => {
    await show();
    const img = container.querySelector<HTMLImageElement>("img.cls-thumb")!;
    expect(img.getAttribute("src")).toBe("data:image/jpeg;base64,AAAA");
    expect(container.querySelectorAll("img")).toHaveLength(1);
    const noThumb = { ...data.rows[3], thumb: undefined };
    await show(block({ data: { ...data, rows: [noThumb] } }));
    expect(container.querySelectorAll("img")).toHaveLength(0);
  });

  it("draws a thumbnail only if it is a picture, whatever a block says", async () => {
    const hostile = { ...data.rows[3], thumb: "https://example.com/track.gif" };
    await show(block({ data: { ...data, rows: [hostile] } }));
    expect(container.querySelectorAll("img")).toHaveLength(0);
  });

  it("says when the list was cut short, and why", async () => {
    await show(block({ data: { ...data, not_reached: 12, stopped: "the time for one list ran out" } }));
    expect(text()).toContain("12 not reached (the time for one list ran out)");
  });
});

describe("correcting a row", () => {
  it("offers the questions answers, and picking another one is kept on the block and as a label", async () => {
    await show();
    await act(async () => button(/Change the answer for Login fails/).click());
    const choices = container.querySelector(".cls-choices")!;
    expect(Array.from(choices.querySelectorAll("button")).map((b) => b.textContent)).toEqual(["bug", "feature"]);
    expect(choices.querySelector("[aria-pressed=true]")?.textContent).toBe("bug");
    await act(async () => Array.from(choices.querySelectorAll("button")).find((b) => b.textContent === "feature")!.click());
    expect(classifyCorrect).toHaveBeenCalledWith("d0", "feature");
    expect(setBlockState).toHaveBeenCalledWith("b1", { corrections: { 0: "feature" } });
  });

  it("shows a corrected row as the persons own, no longer low, and drops it from the filter", async () => {
    await show(block({ state: { corrections: { 1: "bug" } } }));
    const row = container.querySelectorAll("tbody tr")[1];
    expect(row.textContent).toContain("bug");
    expect(row.textContent).toContain("you");
    expect(row.className).not.toContain("cls-low");
    expect(text()).toContain("1 low confidence");
    await act(async () => button(/^Low confidence/).click());
    expect(items()).toEqual(["Crash on save"]);
  });

  it("puts the row back and says so when the label could not be kept", async () => {
    classifyCorrect.mockRejectedValueOnce(new Error("That row's decision is no longer kept."));
    await show();
    await act(async () => button(/Change the answer for Login fails/).click());
    await act(async () => button("feature").click());
    expect(setBlockState).toHaveBeenLastCalledWith("b1", { corrections: {} });
    expect(container.querySelector("[role=alert]")?.textContent).toBe("That row's decision is no longer kept.");
  });

  it("lets a long list of answers be picked from a list instead of a row of buttons", async () => {
    const many = Array.from({ length: 8 }, (_, i) => ({ value: `c${i}`, label: `Class ${i}` }));
    await show(block({ data: { ...data, choices: many, rows: [{ ...data.rows[0], value: "c1", answer: "Class 1" }] } }));
    await act(async () => button(/Change the answer for Login fails/).click());
    expect(container.querySelector(".cls-choices")).toBeNull();
    expect(container.querySelector("[role=combobox]")).not.toBeNull();
  });

  it("closes the choices with Escape and changes nothing", async () => {
    await show();
    await act(async () => button(/Change the answer for Login fails/).click());
    await act(async () => {
      container.querySelector(".cls-choices")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(container.querySelector(".cls-choices")).toBeNull();
    expect(classifyCorrect).not.toHaveBeenCalled();
    expect(setBlockState).not.toHaveBeenCalled();
  });
});

describe("exporting", () => {
  it("writes the list as CSV where the person chooses, with their corrections in it", async () => {
    await show(block({ state: { corrections: { 1: "bug" } } }));
    await act(async () => button("Export CSV").click());
    expect(save).toHaveBeenCalledWith({ defaultPath: "sorted-what-kind-of-ticket-is-this.csv" });
    expect(saveArtifactFile).toHaveBeenCalledTimes(1);
    const [dest, kind, csv] = saveArtifactFile.mock.calls[0];
    expect([dest, kind]).toEqual(["C:/out/sorted.csv", "csv"]);
    expect(csv).toContain("item,path,answer,confidence,status");
    expect(csv).toContain("Add dark mode,,bug,,corrected");
  });

  it("writes nothing when the person cancels the dialog", async () => {
    save.mockResolvedValueOnce(null);
    await show();
    await act(async () => button("Export CSV").click());
    expect(saveArtifactFile).not.toHaveBeenCalled();
  });
});
