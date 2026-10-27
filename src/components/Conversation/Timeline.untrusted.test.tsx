/**
 * @vitest-environment jsdom
 *
 * The `◇ from outside` chip says what the pattern scan found, and (`USE-8`) what
 * the quick decision model found after it: text that reads like orders to the
 * assistant. The flag arrives late, so it only changes the chip.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Timeline from "./Timeline";
import type { AgentStep } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

type Source = NonNullable<AgentStep["untrusted"]>[number];

function stepWith(...untrusted: Source[]): AgentStep {
  return { id: "1", verb: "read", target: "a page", status: "done", untrusted } as AgentStep;
}

const page = (over: Partial<Source> = {}): Source => ({
  label: "page at example.com",
  risk: 0,
  flags: [],
  text: "Some ordinary text.",
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

const chip = () => container.querySelector<HTMLButtonElement>(".untrusted-chip")!;

describe("the from-outside chip", () => {
  it("is the quiet marker when nothing was found", () => {
    act(() => root.render(<Timeline steps={[stepWith(page())]} live />));
    expect(chip().textContent).toBe("◇ from outside");
    expect(chip().className).toContain("risk-0");
  });

  it("says it ignored the instructions when the pattern scan found some", () => {
    act(() => root.render(<Timeline steps={[stepWith(page({ risk: 2, flags: ["override-instructions"] }))]} live />));
    expect(chip().textContent).toBe("◇ from outside: I ignored its instructions");
  });

  it("says the text reads like orders when only the quick check found that", () => {
    act(() => root.render(<Timeline steps={[stepWith(page({ readsAsInstructions: 0.83 }))]} live />));
    expect(chip().textContent).toBe("◇ from outside: it reads like orders to me");
    // Drawn like a scan of risk 2, because it is the same kind of warning.
    expect(chip().className).toContain("risk-2");
  });

  it("keeps the scan's words when both found something", () => {
    const both = page({ risk: 3, flags: ["exfiltrate"], readsAsInstructions: 0.9 });
    act(() => root.render(<Timeline steps={[stepWith(both)]} live />));
    expect(chip().textContent).toBe("◇ from outside: I ignored its instructions");
    expect(chip().getAttribute("aria-label")).toContain("exfiltrate");
  });

  it("says it in words in the disclosure with the number on the hover, and no empty box for a report with no text", () => {
    const report = page({ label: "report from claude-code", text: "", readsAsInstructions: 0.74 });
    act(() => root.render(<Timeline steps={[stepWith(report)]} live />));
    act(() => chip().click());
    const label = container.querySelector(".untrusted-source-label")!;
    expect(label.textContent).toBe("report from claude-code · reads like orders to the assistant");
    expect(label.getAttribute("title")).toBe("quick check, sure 0.74");
    expect(container.querySelector(".untrusted-source-text")).toBeNull();
  });

  it("shows the text of a source the check did not flag, with no number", () => {
    act(() => root.render(<Timeline steps={[stepWith(page())]} live />));
    act(() => chip().click());
    expect(container.querySelector(".untrusted-source-label")!.textContent).toBe("page at example.com");
    expect(container.querySelector(".untrusted-source-text")!.textContent).toBe("Some ordinary text.");
  });
});
