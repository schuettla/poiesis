/**
 * @vitest-environment jsdom
 *
 * Settings → External agents (`CLI-T-10`): found, not found, untested and
 * not-signed-in rows; the switch is off-limits when it cannot work; edits send
 * the right patch; the key field is write-only; a deep link opens the right row.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EngineInfo } from "../lib/api";

const updateEngineSettings = vi.fn();
const setEngineKey = vi.fn();
const detectEngines = vi.fn();
const listEngineModels = vi.fn();
const markEnginesSeen = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  updateEngineSettings: (...a: unknown[]) => updateEngineSettings(...a),
  setEngineKey: (...a: unknown[]) => setEngineKey(...a),
  detectEngines: (...a: unknown[]) => detectEngines(...a),
  listEngineModels: (...a: unknown[]) => listEngineModels(...a),
  markEnginesSeen: (...a: unknown[]) => markEnginesSeen(...a),
  getSetting: () => Promise.resolve(null),
  setSetting: () => Promise.resolve(),
}));

import Engines from "./Engines";
import { useAppStore } from "../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollIntoView = () => {};

const engine = (over: Partial<EngineInfo> = {}): EngineInfo => ({
  id: "claude-code",
  kind: "claude-code",
  name: "Claude Code",
  builtin: true,
  found: true,
  shim: false,
  program: null,
  found_path: "C:\\tools\\claude.exe",
  version: "2.1.301",
  tested: "yes",
  signed_in: true,
  paid_by: "plan",
  last_error: null,
  last_probe_at: 1,
  enabled: false,
  payment: "plan",
  trust_ceiling: "confirm",
  headless: false,
  memory_tools: false,
  app_tools: false,
  model: null,
  extra_args: [],
  env: {},
  args: [],
  has_key: false,
  supports_api_key: true,
  can_pick_model: true,
  install_url: "https://example.test/install",
  is_new: false,
  use_mode: "suggest",
  guards: { code_projects_only: true, min_trust: "confirm", max_per_conversation: 5, not_in_projects: [] },
  rules: [],
  delegate_ready: true,
  memory_ready: true,
  diagnostics: { runs: 0, failed: 0, unknown_events: 0, last_error: null, last_stderr: "" },
  ...over,
});

let container: HTMLDivElement;
let root: Root;

function show(engines: EngineInfo[]) {
  detectEngines.mockResolvedValue(engines);
  act(() => {
    useAppStore.setState({ engines, enginesNewlyFound: engines.some((e) => e.is_new), engineFocus: null } as never);
    root.render(<Engines />);
  });
}

const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 0))));
const text = () => container.textContent ?? "";
const switchFor = (name: string) =>
  container.querySelector<HTMLInputElement>(`input[aria-label="Let me use ${name}"]`)!;

beforeEach(() => {
  for (const m of [updateEngineSettings, setEngineKey, detectEngines, listEngineModels, markEnginesSeen]) m.mockReset();
  listEngineModels.mockResolvedValue([]);
  markEnginesSeen.mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("External agents", () => {
  it("shows a found agent with its version, sign-in and who pays", async () => {
    show([engine()]);
    await settle();
    expect(text()).toContain("Claude Code");
    expect(text()).toContain("2.1.301 · signed in · on your plan");
    expect(text()).not.toContain("Not found");
  });

  it("says plainly when nothing was found and keeps the not-found agent offered", async () => {
    show([engine({ found: false, found_path: null, version: null, signed_in: null, paid_by: null })]);
    await settle();
    expect(text()).toContain("I didn't find any agent apps on this computer");
    expect(text()).toContain("I didn't find it on this computer.");
    expect(text()).toContain("Set path…");
    expect(text()).toContain("How to install");
  });

  it("marks a version nobody has tested in amber words, and still lets it be used", async () => {
    show([engine({ tested: "newer", version: "2.9.0" })]);
    await settle();
    expect(text()).toContain("newer than I've been tested with");
    expect(switchFor("Claude Code").disabled).toBe(false);
  });

  it("keeps the switch off-limits while it is not signed in, with the reason, and offers Sign in", async () => {
    show([engine({ signed_in: false, paid_by: null })]);
    await settle();
    const sw = switchFor("Claude Code");
    expect(sw.disabled).toBe(true);
    expect(sw.closest("label")?.getAttribute("title")).toBe("Sign in first.");
    expect(text()).toContain("not signed in");
    expect(text()).toContain("I never see your password or tokens");
    expect(Array.from(container.querySelectorAll("button")).some((b) => b.textContent === "Sign in")).toBe(true);
  });

  it("a found agent is off until it is turned on, and the row says so", async () => {
    show([engine({ is_new: true })]);
    await settle();
    const sw = switchFor("Claude Code");
    expect(sw.getAttribute("role")).toBe("switch");
    expect(sw.checked).toBe(false);
    expect(sw.closest("label")?.textContent).toBe("Off");
    expect(updateEngineSettings).not.toHaveBeenCalled();
  });

  it("turning the switch on sends exactly that", async () => {
    updateEngineSettings.mockResolvedValue([engine({ enabled: true })]);
    show([engine()]);
    await settle();
    await act(async () => switchFor("Claude Code").click());
    expect(updateEngineSettings).toHaveBeenCalledWith("claude-code", { enabled: true });
  });

  it("shows the backend's refusal next to the control instead of swallowing it", async () => {
    updateEngineSettings.mockRejectedValue(new Error("I can't find it on this computer yet."));
    show([engine()]);
    await settle();
    await act(async () => switchFor("Claude Code").click());
    expect(container.querySelector("[role=alert]")?.textContent).toContain("I can't find it on this computer yet.");
  });

  it("a deep link opens that agent's settings", async () => {
    const list = [engine(), engine({ id: "codex", kind: "codex", name: "Codex", supports_api_key: false })];
    detectEngines.mockResolvedValue(list);
    act(() => {
      useAppStore.setState({ engines: list, engineFocus: "codex" } as never);
      root.render(<Engines />);
    });
    await settle();
    expect(container.querySelector("#engine-body-codex")).not.toBeNull();
    expect(container.querySelector("#engine-body-claude-code")).toBeNull();
    expect(useAppStore.getState().engineFocus).toBeNull();
  });

  it("opening the section clears the dot and says what was found", async () => {
    show([engine({ is_new: true })]);
    await settle();
    expect(text()).toContain("I found Claude Code. Turn it on when you want me to hand work to it.");
    expect(markEnginesSeen).toHaveBeenCalled();
  });

  /** `MCPS-2`: the switch for my own tools is off by default, says what it lets the
   * agent do, and only appears for an agent that can be given them. */
  it("Let it use my memory: shown only where it can work, off by default, and a click sends exactly that", async () => {
    const open = async (e: EngineInfo) => {
      detectEngines.mockResolvedValue([e]);
      act(() => {
        useAppStore.setState({ engines: [e], engineFocus: e.id } as never);
        root.render(<Engines />);
      });
      await settle();
    };
    const memorySwitch = () =>
      Array.from(container.querySelectorAll<HTMLInputElement>("input[type=checkbox]")).find((c) =>
        c.closest("label")?.textContent?.includes("Let it use my memory"),
      );

    await open(engine({ memory_ready: false }));
    expect(memorySwitch()).toBeUndefined();

    act(() => root.unmount());
    root = createRoot(container);
    updateEngineSettings.mockResolvedValue([engine({ memory_tools: true })]);
    await open(engine());
    const sw = memorySwitch()!;
    expect(sw.checked).toBe(false);
    expect(sw.closest("label")?.textContent).toContain("What it finds goes to its model");
    expect(sw.closest("label")?.textContent).toContain("you read each suggestion first");
    await act(async () => sw.click());
    expect(updateEngineSettings).toHaveBeenCalledWith("claude-code", { memory_tools: true });
  });

  it("has a switch of its own for the connected apps, only for an agent that can be given tools", async () => {
    const open = async (e: EngineInfo) => {
      detectEngines.mockResolvedValue([e]);
      act(() => {
        useAppStore.setState({ engines: [e], engineFocus: e.id } as never);
        root.render(<Engines />);
      });
      await settle();
    };
    const appsSwitch = () =>
      Array.from(container.querySelectorAll<HTMLInputElement>("input[type=checkbox]")).find((c) =>
        c.closest("label")?.textContent?.includes("Let it use my connected apps"),
      );

    await open(engine({ memory_ready: false }));
    expect(appsSwitch()).toBeUndefined();

    act(() => root.unmount());
    root = createRoot(container);
    updateEngineSettings.mockResolvedValue([engine({ app_tools: true })]);
    await open(engine());
    const sw = appsSwitch()!;
    expect(sw.checked).toBe(false);
    expect(sw.closest("label")?.textContent).toContain("I ask you before anything is changed or sent");
    expect(sw.closest("label")?.textContent).toContain("never gets my Composio key");
    await act(async () => sw.click());
    expect(updateEngineSettings).toHaveBeenCalledWith("claude-code", { app_tools: true });
  });

  it("the key field is write-only: no key comes back, and a saved key shows no value", async () => {
    setEngineKey.mockResolvedValue([engine({ has_key: true })]);
    updateEngineSettings.mockResolvedValue([engine({ has_key: true, payment: "api-key" })]);
    detectEngines.mockResolvedValue([engine()]);
    act(() => {
      useAppStore.setState({ engines: [engine()], engineFocus: "claude-code" } as never);
      root.render(<Engines />);
    });
    await settle();
    const radio = Array.from(container.querySelectorAll<HTMLInputElement>("input[type=radio]")).find((r) =>
      r.closest("label")?.textContent?.includes("An API key"),
    )!;
    await act(async () => radio.click());
    const field = container.querySelector<HTMLInputElement>('input[type=password][aria-label="Claude Code API key"]')!;
    expect(field.value).toBe("");
    // Typing a key and saving sends it once; the payment follows.
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    await act(async () => {
      setter.call(field, "sk-test-123");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const saveKey = Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Save key")!;
    await act(async () => saveKey.click());
    expect(setEngineKey).toHaveBeenCalledWith("claude-code", "sk-test-123");
    expect(updateEngineSettings).toHaveBeenCalledWith("claude-code", { payment: "api-key" });
    // Nothing on the page holds the key afterwards.
    expect(container.innerHTML).not.toContain("sk-test-123");
  });
});
