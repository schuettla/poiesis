/**
 * @vitest-environment jsdom
 *
 * `CLI-UI-3` and `RTE-UI-2`: an agent app asking to run something, and the
 * handover card. The first has to name who is asking and show the command one
 * piece to a line; the second has to say which of the user's rules made me think
 * of it.
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
  useAppStore.setState({ pendingPermissions: [], engines: [] } as never);
});

function show(r: PermissionRequest) {
  useAppStore.setState({
    pendingPermissions: [r],
    resolvePermission,
    engines: [{ id: "claude-code", name: "Claude Code" }],
  } as never);
  act(() => root.render(<PermissionPanel />));
}

const buttons = () => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).map((b) => b.textContent);
const press = (label: string) =>
  act(() => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === label)!.click());

describe("an agent app asks to run something", () => {
  const ask = (over: Partial<PermissionRequest> = {}): PermissionRequest => ({
    id: "perm_e1",
    summary: "Claude Code wants to run: npm test",
    path: "C:\\work\\app",
    mode: "read-write",
    in_folder: false,
    capability: "engine",
    engine: "Claude Code",
    tool: "Bash",
    argv: ["npm", "test", "--", "--run"],
    ...over,
  });

  it("names who is asking and lists the command one piece to a line", () => {
    show(ask());
    expect(container.textContent).toContain("Claude Code is asking");
    expect(Array.from(container.querySelectorAll(".permission-argv li")).map((li) => li.textContent)).toEqual([
      "npm",
      "test",
      "--",
      "--run",
    ]);
    expect(container.textContent).toContain("It runs outside my sandbox, in C:\\work\\app");
  });

  it("offers once, for this run, or no, and sends exactly that", () => {
    show(ask());
    expect(buttons().slice(0, 3)).toEqual(["Allow once", "Allow for this run", "Deny"]);
    press("Allow for this run");
    expect(resolvePermission).toHaveBeenCalledWith("perm_e1", "chat");
    press("Deny");
    expect(resolvePermission).toHaveBeenCalledWith("perm_e1", "deny");
  });

  it("shows an edit as a change and links to what the agent may do", () => {
    show(ask({ argv: undefined, diff: "- old\n+ new" }));
    expect(container.querySelector(".permission-diff")?.textContent).toBe("- old\n+ new");
    expect(container.querySelector(".permission-argv")).toBeNull();
    press("Change what Claude Code may do");
    expect(useAppStore.getState().view).toBe("engines");
    expect(useAppStore.getState().engineFocus).toBe("claude-code");
  });
});

describe("the handover card", () => {
  const card = (over: Partial<PermissionRequest> = {}): PermissionRequest => ({
    id: "perm_h1",
    summary: "I'd like to hand this to Claude Code",
    path: "C:\\work\\app",
    mode: "read-write",
    in_folder: false,
    capability: "handover",
    engine: "Claude Code",
    diff: "Rename the option everywhere",
    rule: { label: "U1", text: "A change across many files" },
    ...over,
  });

  it("says which of my rules made me think of it", () => {
    show(card());
    expect(container.textContent).toContain("Because of your rule U1: A change across many files");
    expect(container.textContent).toContain("“Rename the option everywhere”");
    expect(buttons().slice(0, 3)).toEqual(["Hand it over", "Do it yourself", "Always for this"]);
  });

  it("maps the three answers to once, deny and forever", () => {
    show(card());
    press("Hand it over");
    press("Do it yourself");
    press("Always for this");
    expect(resolvePermission.mock.calls.map((c) => c[1])).toEqual(["once", "deny", "forever"]);
  });

  it("says a quick decision matched the rule before I started, and keeps the same buttons", () => {
    show(card({ rule: { label: "U2", text: "Running the tests", matched: 0.93 } }));
    expect(container.textContent).toContain("Matches your rule U2: Running the tests");
    expect(container.textContent).not.toContain("Because of your rule");
    expect(container.querySelector(".permission-where")?.getAttribute("title")).toBe("matched before I started (0.93)");
    expect(buttons().slice(0, 3)).toEqual(["Hand it over", "Do it yourself", "Always for this"]);
  });

  it("adds a second line when I cited a rule the quick decision had scored low", () => {
    show(card({ rule: { label: "U2", text: "Running the tests", doubt: "I'm not sure this matches U2." } }));
    expect(container.textContent).toContain("Because of your rule U2: Running the tests");
    expect(container.textContent).toContain("I'm not sure this matches U2.");
    const lines = Array.from(container.querySelectorAll(".permission-where")).map((p) => p.textContent);
    expect(lines).toHaveLength(2);
  });

  it("has no doubt line on an ordinary card", () => {
    show(card());
    expect(container.querySelectorAll(".permission-where")).toHaveLength(1);
  });

  it("has no Always when you named the agent yourself", () => {
    show(card({ rule: undefined }));
    expect(container.textContent).toContain("You asked for it.");
    expect(buttons()).not.toContain("Always for this");
  });
});
