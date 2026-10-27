/**
 * @vitest-environment jsdom
 *
 * `USE-UI-8`: a quick decision's line under the question, for every kind the panel
 * draws, and the question it adds before one of Poiesis's own tools runs.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PermissionPanel from "./PermissionPanel";
import { useAppStore } from "../../lib/store";
import type { PermissionRequest } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const resolvePermission = vi.fn();

beforeEach(() => {
  resolvePermission.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ pendingPermissions: [] } as never);
});

const base: PermissionRequest = {
  id: "perm_1",
  summary: "A question",
  path: "somewhere",
  mode: "read",
  in_folder: false,
};

async function show(r: PermissionRequest) {
  useAppStore.setState({ pendingPermissions: [r], resolvePermission } as never);
  await act(async () => root.render(<PermissionPanel />));
}

const reason = () => container.querySelector(".permission-reason")?.textContent ?? null;
const buttons = () => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).map((b) => b.textContent);
const press = (label: string) =>
  act(() => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === label)!.click());

const LINE = "Flagged: this may not be what you asked for";

describe("the reason line", () => {
  it("sits under the question of every kind a tool or an app raises", async () => {
    const kinds: PermissionRequest[] = [
      { ...base, in_folder: true, diff: "- a\n+ b" },
      { ...base, capability: "domain", path: "example.com" },
      { ...base, capability: "open-app", path: "Terminal" },
      { ...base, capability: "command", argv: ["git", "push"], project: "poiesis", timeout_secs: 60, remember: "git push" },
      { ...base, capability: "task", argv: ["npm", "test"], project: "poiesis", timeout_secs: 60, remember: "test" },
      { ...base, capability: "engine", engine: "Claude Code", tool: "Bash", argv: ["ls"] },
      { ...base, capability: "app", call_kind: "change", diff: "{}" },
    ];
    for (const k of kinds) {
      await show({ ...k, reason: LINE });
      expect(reason(), String(k.capability)).toBe(LINE);
    }
  });

  // `SEE-4`: the line is words, and the number a quick decision gave is its hover.
  it("keeps the number off the line and on its hover", async () => {
    await show({
      ...base,
      capability: "app",
      call_kind: "read",
      diff: "{}",
      reason: LINE,
      reason_hover: "Quick decision, this may not be what you asked for: 0.74",
    });
    expect(reason()).toBe(LINE);
    expect(reason()).not.toMatch(/\d\.\d/);
    expect(container.querySelector(".permission-reason")?.getAttribute("title")).toBe(
      "Quick decision, this may not be what you asked for: 0.74"
    );
  });

  it("is not drawn when the decision found nothing to say", async () => {
    await show({ ...base, capability: "app", call_kind: "read", diff: "{}" });
    expect(container.querySelector(".permission-reason")).toBeNull();
    await show({ ...base, capability: "command", argv: ["ls"], project: "p", timeout_secs: 5, remember: "ls" });
    expect(container.querySelector(".permission-reason")).toBeNull();
  });

  it("leaves the buttons of an agent app's approval exactly as they were", async () => {
    await show({ ...base, capability: "engine", engine: "Codex", tool: "Bash", argv: ["rm", "x"], reason: "Flagged: x (0.9)" });
    expect(buttons()).toEqual(["Allow once", "Allow for this run", "Deny"]);
  });

  /** A tool taken for a send is gated as one, and the panel draws it as one. */
  it("draws a tool raised to a send in the outbound layout, with its line", async () => {
    await show({
      ...base,
      summary: "I'd like to send something through Slack: update conversation",
      capability: "app",
      call_kind: "send",
      cap: 25,
      diff: JSON.stringify({ channel: "#general" }),
      reason: "Asked because this looks like it sends something (0.81)",
    });
    expect(container.querySelector(".side-panel.outbound")).not.toBeNull();
    expect(reason()).toBe("Asked because this looks like it sends something (0.81)");
    expect(buttons()).toEqual(["Send once", "Up to 25 a day for somewhere", "No"]);
  });
});

describe("the question a quick decision adds", () => {
  const risk = (over: Partial<PermissionRequest> = {}): PermissionRequest => ({
    ...base,
    id: "perm_r1",
    summary: "I'd like to run command → git push --force origin main.",
    path: "run_command",
    capability: "risk",
    tool: "run_command",
    diff: JSON.stringify({ command: "git push --force origin main" }),
    reason: "Asked because this looks irreversible (0.84)",
    ...over,
  });

  it("says what is about to happen, why it was asked and shows the arguments", async () => {
    await show(risk());
    expect(container.querySelector(".permission-summary")?.textContent).toContain("git push --force origin main");
    expect(reason()).toBe("Asked because this looks irreversible (0.84)");
    expect(container.querySelector(".permission-args")?.textContent).toContain("git push --force origin main");
  });

  it("offers a yes and a no and nothing that would remember the answer", async () => {
    await show(risk());
    expect(buttons()).toEqual(["Go ahead", "Don't do it"]);
    expect(container.textContent).not.toContain("Always");
  });

  it("answers once for go ahead and deny for no", async () => {
    await show(risk());
    press("Go ahead");
    press("Don't do it");
    expect(resolvePermission.mock.calls).toEqual([
      ["perm_r1", "once"],
      ["perm_r1", "deny"],
    ]);
  });

  it("is not the handover or the page card", async () => {
    await show(risk());
    expect(container.textContent).not.toContain("Hand it over");
    expect(container.textContent).not.toContain("The browser stopped");
  });
});
