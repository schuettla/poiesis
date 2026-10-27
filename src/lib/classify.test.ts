import { describe, expect, it } from "vitest";
import {
  arrange,
  csvCell,
  csvName,
  lowCount,
  nextSort,
  readClassify,
  readCorrections,
  shown,
  toCsv,
  type ClassifyData,
} from "./classify";

const data = (): ClassifyData =>
  readClassify({
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
      { item: "pic.png", path: "C:/pics/pic.png", image: true, answer: "feature", value: "feature", confidence: 0.9, low: false, decision: "d3" },
      { item: "notes.bin", path: "C:/notes.bin", note: "it is not a text file" },
    ],
  });

describe("readClassify (USE-UI-5)", () => {
  it("reads the block's data, row by row, with the position a correction is kept under", () => {
    const d = data();
    expect(d.rows.map((r) => r.index)).toEqual([0, 1, 2, 3, 4]);
    expect(d.rows[3]).toMatchObject({ image: true, path: "C:/pics/pic.png" });
    expect(d.rows[4]).toMatchObject({ note: "it is not a text file", low: false, answer: undefined });
    expect(d.bar).toBe(0.7);
  });

  it("is lenient with a block that is missing or malformed", () => {
    const d = readClassify(null);
    expect(d).toMatchObject({ question: "", kind: "choice", rows: [], choices: [], notReached: 0, stopped: null });
    expect(readClassify({ rows: [7, "x", { item: 3, confidence: "high" }] }).rows).toHaveLength(3);
    expect(readClassify({ kind: "scale" }).kind).toBe("scale");
    expect(readClassify({ kind: "nonsense" }).kind).toBe("choice");
  });

  it("reads corrections from the block's state and ignores what is not an answer", () => {
    expect(readCorrections({ corrections: { "1": "bug", "2": 5 } })).toEqual({ "1": "bug" });
    expect(readCorrections(undefined)).toEqual({});
  });
});

describe("a corrected row (USE-UI-5)", () => {
  it("shows the person's answer, with no confidence, and is no longer low", () => {
    const d = data();
    expect(shown(d.rows[1], d, { "1": "bug" })).toEqual({ answer: "bug", low: false, corrected: true });
  });

  it("treats choosing the models own answer as a confirmation: still its answer and number, no longer low", () => {
    const d = data();
    expect(shown(d.rows[1], d, { "1": "feature" })).toEqual({ answer: "feature", confidence: 0.55, low: false, corrected: false });
  });

  it("leaves an untouched row as the model had it", () => {
    const d = data();
    expect(shown(d.rows[2], d, {})).toEqual({ answer: "bug", confidence: 0.62, low: true, corrected: false });
  });

  it("counts the low rows again after a correction", () => {
    const d = data();
    expect(lowCount(d, {})).toBe(2);
    expect(lowCount(d, { "1": "bug" })).toBe(1);
    expect(lowCount(d, { "1": "feature", "2": "feature" })).toBe(0);
  });
});

describe("arrange (USE-UI-5)", () => {
  it("keeps the order the list came in until a column is sorted", () => {
    expect(arrange(data(), {}, "all", null).map((r) => r.index)).toEqual([0, 1, 2, 3, 4]);
  });

  it("filters to the low-confidence rows, in order", () => {
    expect(arrange(data(), {}, "low", null).map((r) => r.item)).toEqual(["Add dark mode", "Crash on save"]);
    // A corrected row leaves the filter.
    expect(arrange(data(), { "1": "bug" }, "low", null).map((r) => r.item)).toEqual(["Crash on save"]);
  });

  it("sorts by confidence, lowest first, with rows that have none last either way", () => {
    const asc = arrange(data(), {}, "all", { key: "confidence", dir: "asc" }).map((r) => r.index);
    expect(asc).toEqual([1, 2, 3, 0, 4]);
    const desc = arrange(data(), { "2": "feature" }, "all", { key: "confidence", dir: "desc" }).map((r) => r.index);
    // The corrected row and the unsorted one have no number, so they come last.
    expect(desc).toEqual([0, 3, 1, 2, 4]);
  });

  it("sorts by item and by answer, ties in the original order", () => {
    expect(arrange(data(), {}, "all", { key: "item", dir: "asc" }).map((r) => r.item)).toEqual([
      "Add dark mode",
      "Crash on save",
      "Login fails",
      "notes.bin",
      "pic.png",
    ]);
    const byAnswer = arrange(data(), {}, "all", { key: "answer", dir: "asc" }).map((r) => r.index);
    expect(byAnswer).toEqual([0, 2, 1, 3, 4]);
  });

  it("goes ascending, then descending, then back to the original order", () => {
    const first = nextSort(null, "confidence");
    expect(first).toEqual({ key: "confidence", dir: "asc" });
    const second = nextSort(first, "confidence");
    expect(second).toEqual({ key: "confidence", dir: "desc" });
    expect(nextSort(second, "confidence")).toBeNull();
    expect(nextSort(second, "item")).toEqual({ key: "item", dir: "asc" });
  });
});

describe("toCsv (USE-UI-5)", () => {
  it("writes every row with the persons corrections and the status of each", () => {
    const csv = toCsv(data(), { "1": "bug" });
    expect(csv.split("\r\n")).toEqual([
      "item,path,answer,confidence,status",
      "Login fails,,bug,0.95,",
      "Add dark mode,,bug,,corrected",
      "Crash on save,,bug,0.62,low confidence",
      "pic.png,C:/pics/pic.png,feature,0.90,",
      "notes.bin,C:/notes.bin,,,not sorted: it is not a text file",
      "",
    ]);
  });

  it("leaves out the path column when no item has one", () => {
    const d = readClassify({ rows: [{ item: "a", answer: "yes", value: "true", confidence: 0.8, low: false }] });
    expect(toCsv(d, {}).split("\r\n")[0]).toBe("item,answer,confidence,status");
  });

  it("quotes what needs it and defuses a cell a spreadsheet would run as a formula", () => {
    expect(csvCell("plain")).toBe("plain");
    expect(csvCell('say "hi", then go')).toBe('"say ""hi"", then go"');
    expect(csvCell("line\nbreak")).toBe('"line\nbreak"');
    expect(csvCell("=HYPERLINK(\"http://evil\")")).toBe('"\'=HYPERLINK(""http://evil"")"');
    expect(csvCell("-1 degree")).toBe("'-1 degree");
    expect(csvCell("@mention")).toBe("'@mention");
  });

  it("names the file after the question", () => {
    expect(csvName("What kind of ticket is this?")).toBe("sorted-what-kind-of-ticket-is-this.csv");
    expect(csvName("???")).toBe("sorted-list.csv");
    expect(csvName("a".repeat(100)).length).toBeLessThanOrEqual("sorted-.csv".length + 40);
  });
});
