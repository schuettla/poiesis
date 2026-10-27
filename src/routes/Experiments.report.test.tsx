/**
 * @vitest-environment jsdom
 *
 * `EXP-4`: each card says what the experiment did in the last 30 days, and
 * "Reset its data" forgets it after asking.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExperimentReport } from "../lib/api";

const experimentReport = vi.fn<(id: string) => Promise<ExperimentReport>>();
const resetExperiment = vi.fn<(id: string) => Promise<ExperimentReport>>();
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  experimentsSeen: () => Promise.resolve(),
  experimentReport: (id: string) => experimentReport(id),
  resetExperiment: (id: string) => resetExperiment(id),
  decidePolicy: () => Promise.resolve([]),
  decideCalibration: () => Promise.resolve([]),
}));

import Experiments, { reportLine, resetBody } from "./Experiments";
import { useAppStore } from "../lib/store";
import { experiment } from "../lib/experiments.fixture";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const fastSlow = (over = {}) =>
  experiment({
    id: "fast-slow",
    title: "Fast & slow thinking",
    on: true,
    records: "Each decision, its confidence, and what you did about it.",
    requires: [{ kind: "decision-model", label: "A decision model", met: true, fix_view: "models" }],
    ...over,
  });
const report = (over: Partial<ExperimentReport> = {}): ExperimentReport => ({
  days: 30,
  lines: ["Held back tools on 214 of 640 turns", "brought back on 9 of them (4%)"],
  records: 1820,
  ...over,
});
const button = (label: string) =>
  Array.from(document.querySelectorAll("button")).find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;
const row = () => Array.from(container.querySelectorAll(".exp-row")).find((r) => r.textContent?.startsWith("Last 30 days"));
const flush = () => act(async () => {});

async function mount(list = [fastSlow()]) {
  act(() => useAppStore.setState({ experiments: list }));
  await act(async () => root.render(<Experiments />));
  await flush();
}

beforeEach(() => {
  experimentReport.mockReset().mockResolvedValue(report());
  resetExperiment.mockReset().mockResolvedValue(report({ lines: [], records: 0 }));
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ experiments: null });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the last 30 days", () => {
  it("is one line of what the data says, joined with a dot", async () => {
    await mount();
    expect(experimentReport).toHaveBeenCalledWith("fast-slow");
    expect(row()?.textContent).toContain("Held back tools on 214 of 640 turns · brought back on 9 of them (4%)");
  });

  it("says so when nothing was recorded", async () => {
    experimentReport.mockResolvedValue(report({ lines: [], records: 0 }));
    await mount();
    expect(row()?.textContent).toContain("Nothing recorded in the last 30 days.");
    expect(button("Reset its data")).toBeUndefined();
  });

  it("is read again when the switch is turned off or on", async () => {
    await mount();
    expect(experimentReport).toHaveBeenCalledTimes(1);
    await mount([fastSlow({ on: false })]);
    expect(experimentReport).toHaveBeenCalledTimes(2);
  });

  it("is left out, quietly, when the backend cannot say", async () => {
    experimentReport.mockRejectedValue("no");
    await mount();
    expect(row()).toBeUndefined();
    expect(container.querySelector(".exp-error")).toBeNull();
  });

  it("reads as the plan's examples do", () => {
    expect(reportLine({ days: 30, lines: ["Triaged 640 mails", "you retagged 22 (3%)"] })).toBe(
      "Triaged 640 mails · you retagged 22 (3%)",
    );
  });
});

describe("Reset its data", () => {
  it("asks first, saying what goes and what stays, and does nothing on Cancel", async () => {
    await mount();
    await act(async () => button("Reset its data")!.click());
    const dialog = document.querySelector("[role=alertdialog]");
    expect(dialog?.textContent).toContain("1820 records will be forgotten.");
    expect(dialog?.textContent).toContain("Each decision, its confidence, and what you did about it.");
    expect(dialog?.textContent).toContain("the switch stays as it is");
    expect(dialog?.textContent).toContain("is kept");
    await act(async () => button("Cancel")!.click());
    expect(resetExperiment).not.toHaveBeenCalled();
    expect(document.querySelector("[role=alertdialog]")).toBeNull();
  });

  it("forgets on confirm and shows the report after, with no button left", async () => {
    await mount();
    await act(async () => button("Reset its data")!.click());
    // The dialog's own button has the same words as the link; it is the one inside it.
    const go = document.querySelector<HTMLButtonElement>(".confirm-go")!;
    await act(async () => go.click());
    await flush();
    expect(resetExperiment).toHaveBeenCalledWith("fast-slow");
    expect(row()?.textContent).toContain("Nothing recorded");
    expect(button("Reset its data")).toBeUndefined();
    expect(useAppStore.getState().experiments?.[0].on).toBe(true);
  });

  it("says why when it fails and keeps the report", async () => {
    resetExperiment.mockRejectedValue("The database is busy.");
    await mount();
    await act(async () => button("Reset its data")!.click());
    await act(async () => document.querySelector<HTMLButtonElement>(".confirm-go")!.click());
    await flush();
    expect(container.querySelector(".exp-error")?.textContent).toBe("The database is busy.");
    expect(row()?.textContent).toContain("Held back tools");
  });

  it("does not claim a calibration is forgotten, and does not say it is kept on a card that does not decide", () => {
    const body = resetBody({ records: "Nothing of its own.", requires: [] }, 1);
    expect(body).toBe(
      "1 record will be forgotten. Nothing of its own. Your chats and settings are not touched, and the switch stays as it is.",
    );
    expect(resetBody(fastSlow(), 5)).toContain("What I measured about how sure my quick decisions are is kept.");
  });
});
