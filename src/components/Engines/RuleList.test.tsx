/**
 * @vitest-environment jsdom
 *
 * "When I use it" (`RTE-T-5`): the mode, the rules in words, editing in place,
 * the guards that Rust checks, and Try a request.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EngineInfo, EngineRule } from "../../lib/api";

const updateEngineSettings = vi.fn();
const addEngineRule = vi.fn();
const updateEngineRule = vi.fn();
const deleteEngineRule = vi.fn();
const reorderEngineRules = vi.fn();
const resetEngineRules = vi.fn();
const previewEngineRoute = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  updateEngineSettings: (...a: unknown[]) => updateEngineSettings(...a),
  addEngineRule: (...a: unknown[]) => addEngineRule(...a),
  updateEngineRule: (...a: unknown[]) => updateEngineRule(...a),
  deleteEngineRule: (...a: unknown[]) => deleteEngineRule(...a),
  reorderEngineRules: (...a: unknown[]) => reorderEngineRules(...a),
  resetEngineRules: (...a: unknown[]) => resetEngineRules(...a),
  previewEngineRoute: (...a: unknown[]) => previewEngineRoute(...a),
}));

import RuleList, { RecentJobs, TryRequest } from "./RuleList";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const rule = (over: Partial<EngineRule> = {}): EngineRule => ({
  id: "r1",
  engine_id: "claude-code",
  kind: "use",
  text: "A change across many files",
  enabled: true,
  auto: false,
  position: 0,
  source: "default",
  hits: 0,
  overrides: 0,
  label: "U1",
  ...over,
});

const engine = (over: Partial<EngineInfo> = {}): EngineInfo =>
  ({
    id: "claude-code",
    kind: "claude-code",
    name: "Claude Code",
    found: true,
    enabled: true,
    use_mode: "suggest",
    guards: { code_projects_only: true, min_trust: "confirm", max_per_conversation: 5, not_in_projects: [] },
    rules: [
      rule(),
      rule({ id: "r2", text: "Anything that runs the tests", position: 1, label: "U2" }),
      rule({ id: "r3", kind: "avoid", text: "Quick questions", position: 2, label: "A1" }),
    ],
    delegate_ready: true,
    memory_ready: true,
    diagnostics: { runs: 0, failed: 0, unknown_events: 0, last_error: null, last_stderr: "" },
    ...over,
  }) as EngineInfo;

let container: HTMLDivElement;
let root: Root;

const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 0))));
const text = () => container.textContent ?? "";
const button = (label: string | RegExp) =>
  Array.from(container.querySelectorAll("button")).find((b) =>
    typeof label === "string" ? b.textContent === label : label.test(b.textContent ?? ""),
  ) as HTMLButtonElement;
const type = (el: HTMLInputElement, value: string) => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
};
const key = (el: Element, k: string) => el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));

function show(e: EngineInfo) {
  useAppStore.setState({ engines: [e] } as never);
  act(() => root.render(<RuleList engine={e} />));
}

beforeEach(() => {
  for (const m of [updateEngineSettings, addEngineRule, updateEngineRule, deleteEngineRule, reorderEngineRules, resetEngineRules, previewEngineRoute]) {
    m.mockReset();
    m.mockResolvedValue([engine()]);
  }
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("When I use it", () => {
  it("shows the mode, the two lists and the labels the lead cites", () => {
    show(engine());
    expect(text()).toContain("Only when you ask");
    expect(text()).toContain("Suggest it, you confirm");
    expect(text()).toContain("I'll suggest it when a rule fits, and you confirm each time.");
    expect(text()).toContain("Use it for");
    expect(text()).toContain("Don't use it for");
    expect(text()).toContain("U1");
    expect(text()).toContain("A1");
    expect(container.querySelector('[role=radio][aria-checked="true"]')?.textContent).toBe("Suggest it, you confirm");
  });

  it("changing the mode sends only the mode", async () => {
    show(engine());
    await act(async () => button("On its own").click());
    expect(updateEngineSettings).toHaveBeenCalledWith("claude-code", { use_mode: "auto" });
  });

  it("a suggested change shows at the top for its own agent only", () => {
    const p = (id: string, engine_id: string) => ({
      id,
      target: "engine-rule",
      slug: "r1",
      proposed_text: JSON.stringify({ engine_id, action: "off", rule_id: "r1" }),
      rationale: `Suggestion ${id}`,
      description: null,
      status: "pending",
      created_at: 1,
    });
    useAppStore.setState({ changeProposals: [p("p1", "claude-code"), p("p2", "codex")] } as never);
    show(engine());
    expect(text()).toContain("Suggestion p1");
    expect(text()).not.toContain("Suggestion p2");
    expect(button("Turn it off")).toBeTruthy();
    useAppStore.setState({ changeProposals: [] } as never);
  });

  it("an agent I cannot hand work to yet shows a sentence instead of controls", () => {
    show(engine({ delegate_ready: false, name: "Codex" }));
    expect(text()).toContain("I can't hand work to Codex yet. It is set up and testable here.");
    expect(container.querySelector("[role=radio]")).toBeNull();
  });

  it("Enter saves an edit, with a counter that stops at 140", async () => {
    show(engine());
    await act(async () => button("A change across many files").click());
    const input = container.querySelector<HTMLInputElement>('input[aria-label="Rule U1"]')!;
    expect(input.maxLength).toBe(140);
    expect(text()).toContain("26/140");
    await act(async () => type(input, "Work that touches many files"));
    await act(async () => key(input, "Enter"));
    expect(updateEngineRule).toHaveBeenCalledWith("r1", { text: "Work that touches many files" });
  });

  it("Escape throws the edit away", async () => {
    show(engine());
    await act(async () => button("A change across many files").click());
    const input = container.querySelector<HTMLInputElement>('input[aria-label="Rule U1"]')!;
    await act(async () => type(input, "something else"));
    await act(async () => key(input, "Escape"));
    expect(updateEngineRule).not.toHaveBeenCalled();
    expect(text()).toContain("A change across many files");
  });

  it("switches a rule off, deletes it and moves it to the other list", async () => {
    show(engine());
    await act(async () => container.querySelector<HTMLInputElement>('input[aria-label="Rule U1 is on"]')!.click());
    expect(updateEngineRule).toHaveBeenCalledWith("r1", { enabled: false });
    await act(async () => button(/^Move to don't use it for$/)!.click());
    expect(updateEngineRule).toHaveBeenCalledWith("r1", { kind: "avoid" });
    await act(async () => button("Delete").click());
    expect(deleteEngineRule).toHaveBeenCalledWith("r1");
  });

  it("moving a rule down swaps it with the next one of its list", async () => {
    show(engine());
    await act(async () => button("↓").click());
    expect(reorderEngineRules).toHaveBeenCalledWith("claude-code", ["r2", "r1", "r3"]);
  });

  it("the first rule cannot go up and the last cannot go down", () => {
    show(engine());
    const up = container.querySelector<HTMLButtonElement>('button[aria-label="Move U1 up"]')!;
    const down = container.querySelector<HTMLButtonElement>('button[aria-label="Move U2 down"]')!;
    expect(up.disabled).toBe(true);
    expect(down.disabled).toBe(true);
  });

  it("adds a rule to the list it was added in", async () => {
    show(engine());
    await act(async () => button("+ Add a rule").click());
    const input = container.querySelector<HTMLInputElement>('input[aria-label="New rule: Use it for"]')!;
    await act(async () => type(input, "Refactors"));
    await act(async () => key(input, "Enter"));
    expect(addEngineRule).toHaveBeenCalledWith("claude-code", "use", "Refactors");
  });

  it("shows how a rule fared, in amber when it was declined more than used", () => {
    show(
      engine({
        rules: [rule({ hits: 1, overrides: 3, auto: true }), rule({ id: "r2", label: "U2", position: 1, hits: 4, overrides: 1 })],
      }),
    );
    const uses = Array.from(container.querySelectorAll(".rule-uses"));
    expect(uses[0].textContent).toContain("used 1 · declined 3");
    expect(uses[0].classList.contains("warn")).toBe(true);
    expect(uses[1].classList.contains("warn")).toBe(false);
    expect(text()).toContain("starts without asking");
  });

  it("pressing the auto mark asks again next time", async () => {
    show(engine({ rules: [rule({ auto: true })] }));
    await act(async () => button(/starts without asking/).click());
    expect(updateEngineRule).toHaveBeenCalledWith("r1", { auto: false });
  });

  it("tags a rule I suggested", () => {
    show(engine({ rules: [rule({ source: "proposed" })] }));
    expect(text()).toContain("suggested by me");
  });

  it("the folder guard is locked and says why; the others are saved as a whole", async () => {
    show(engine());
    expect(container.querySelector(".engines-opt.locked")?.getAttribute("title")).toBe("Agents always work in a folder");
    const code = Array.from(container.querySelectorAll<HTMLInputElement>(".rule-guards input")).find((i) =>
      i.closest("label")?.textContent?.includes("Code projects only"),
    )!;
    await act(async () => code.click());
    expect(updateEngineSettings).toHaveBeenCalledWith("claude-code", {
      guards: { code_projects_only: false, min_trust: "confirm", max_per_conversation: 5, not_in_projects: [] },
    });
  });

  it("Reset to defaults asks first", async () => {
    show(engine());
    await act(async () => button("Reset to defaults").click());
    expect(resetEngineRules).not.toHaveBeenCalled();
    expect(container.querySelector("[role=alertdialog]")).not.toBeNull();
    await act(async () => button("Reset").click());
    expect(resetEngineRules).toHaveBeenCalledWith("claude-code");
  });

  it("shows the backend's refusal next to the rules", async () => {
    addEngineRule.mockRejectedValue(new Error("That rule is too long."));
    show(engine());
    await act(async () => button("+ Add a rule").click());
    const input = container.querySelector<HTMLInputElement>('input[aria-label="New rule: Use it for"]')!;
    await act(async () => type(input, "x"));
    await act(async () => key(input, "Enter"));
    expect(container.querySelector("[role=alert]")?.textContent).toContain("That rule is too long.");
  });
});

describe("Recent jobs", () => {
  it("counts the jobs and keeps the agent's own words behind a fold", () => {
    act(() =>
      root.render(
        <RecentJobs
          engine={engine({
            diagnostics: { runs: 3, failed: 1, unknown_events: 2, last_error: "It stopped.", last_stderr: "boom" },
          })}
        />,
      ),
    );
    expect(text()).toContain("3 jobs since I started, 1 failed, 2 messages I didn't understand.");
    expect(text()).toContain("Last problem: It stopped.");
    expect(container.querySelector("details pre")?.textContent).toBe("boom");
  });

  it("warns that an update may have changed the protocol only when something was not understood", () => {
    const render = (unknown_events: number) =>
      act(() =>
        root.render(
          <RecentJobs engine={engine({ diagnostics: { runs: 4, failed: 0, unknown_events, last_error: null, last_stderr: "" } })} />,
        ),
      );
    render(0);
    expect(text()).not.toContain("talks to me differently");
    render(3);
    expect(text()).toContain("If this number keeps rising after an update, Claude Code now talks to me differently.");
  });

  it("says so when nothing ran", () => {
    act(() => root.render(<RecentJobs engine={engine()} />));
    expect(text()).toContain("No jobs since I started.");
    expect(container.querySelector("details")).toBeNull();
  });
});

describe("Try a request", () => {
  it("asks once and shows the answer in one line", async () => {
    previewEngineRoute.mockResolvedValue({
      agent: "Claude Code",
      label: "U1",
      rule_text: "A change across many files",
      consequence: "ask",
      blocked: null,
    });
    act(() => root.render(<TryRequest />));
    const input = container.querySelector<HTMLInputElement>('input[aria-label="A request to try"]')!;
    await act(async () => type(input, "Rename it everywhere"));
    await act(async () => button("Try").click());
    await settle();
    expect(previewEngineRoute).toHaveBeenCalledWith("Rename it everywhere");
    expect(container.querySelector("[role=status]")?.textContent).toContain(
      "I'd hand this to Claude Code, because of U1. I'd ask you first.",
    );
  });

  it("shows why it could not answer", async () => {
    previewEngineRoute.mockRejectedValue(new Error("No model is set up."));
    act(() => root.render(<TryRequest />));
    const input = container.querySelector<HTMLInputElement>('input[aria-label="A request to try"]')!;
    await act(async () => type(input, "x"));
    await act(async () => button("Try").click());
    await settle();
    expect(container.querySelector("[role=alert]")?.textContent).toContain("No model is set up.");
  });
});
