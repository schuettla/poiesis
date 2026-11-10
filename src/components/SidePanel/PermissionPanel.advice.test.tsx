/**
 * @vitest-environment jsdom
 *
 * `WRK-11`: when a way of working asked for advice before a risky act, the panel says
 * so in one line above what would change. The decision and its buttons are the same
 * with or without the line.
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
  summary: "Delete old/notes.md?",
  path: "old/notes.md",
  mode: "read-write",
  in_folder: true,
  diff: "old/notes.md",
};

async function show(r: PermissionRequest) {
  useAppStore.setState({ pendingPermissions: [r], resolvePermission } as never);
  await act(async () => root.render(<PermissionPanel />));
}

const advice = () => container.querySelector(".permission-advice")?.textContent ?? null;
const buttons = () => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).map((b) => b.textContent);

describe("advice in the permission panel (WRK-11)", () => {
  it("says what was asked first, above what would change", async () => {
    await show({ ...base, advice: "I asked first: your notes say /old is a backup folder; a second look found nothing against it." });
    expect(advice()).toBe("I asked first: your notes say /old is a backup folder; a second look found nothing against it.");
    const line = container.querySelector(".permission-advice")!;
    const summary = container.querySelector(".permission-summary")!;
    expect(summary.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("says nothing when none was asked for, and the buttons do not change", async () => {
    await show(base);
    const without = buttons();
    expect(advice()).toBeNull();
    await show({ ...base, advice: "I asked first: nothing speaks against it." });
    expect(buttons()).toEqual(without);
  });

  it("is on an app question too", async () => {
    await show({
      ...base,
      capability: "app",
      call_kind: "change",
      summary: "Change something in Notes?",
      advice: "I asked first: nothing speaks against it.",
    });
    expect(advice()).toBe("I asked first: nothing speaks against it.");
  });
});
