/**
 * @vitest-environment jsdom
 *
 * `RTE-5`: keep agent apps out of one chat, from that chat's own ⋯ menu. The line
 * only exists once an agent app is switched on and ready.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const setConversationEnginesOff = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  setConversationEnginesOff: (...a: unknown[]) => setConversationEnginesOff(...a),
}));

import SessionMenu from "./SessionMenu";
import { useAppStore } from "../../lib/store";
import type { Conversation } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  setConversationEnginesOff.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ engines: [] } as never);
});

function setup(over: Partial<Conversation>, engines: unknown[]) {
  const c: Conversation = { id: "c1", title: "A chat", updatedAt: 0, messages: [], ...over };
  useAppStore.setState({ conversations: [c], activeConversationId: c.id, projects: [], engines } as never);
  act(() => root.render(<SessionMenu />));
  act(() => container.querySelector<HTMLButtonElement>(".session-more")!.click());
}

const labels = () => Array.from(container.querySelectorAll(".row-menu-item")).map((b) => b.textContent);
const ready = [{ id: "claude-code", enabled: true, delegate_ready: true }];

describe("the session menu and external agents", () => {
  it("has no such line when no agent app is on", () => {
    setup({}, [{ id: "claude-code", enabled: false, delegate_ready: true }]);
    expect(labels().join("|")).not.toContain("external agents");
  });

  it("keeps agent apps out of this chat, and remembers it", async () => {
    setup({}, ready);
    await act(async () => {
      Array.from(container.querySelectorAll<HTMLButtonElement>(".row-menu-item"))
        .find((b) => b.textContent === "Keep external agents out of this chat")!
        .click();
    });
    expect(setConversationEnginesOff).toHaveBeenCalledWith("c1", true);
    expect(useAppStore.getState().conversations[0].enginesOff).toBe(true);
  });

  it("lets them back in", async () => {
    setup({ enginesOff: true }, ready);
    expect(labels()).toContain("Use external agents in this chat");
    await act(async () => {
      Array.from(container.querySelectorAll<HTMLButtonElement>(".row-menu-item"))
        .find((b) => b.textContent === "Use external agents in this chat")!
        .click();
    });
    expect(setConversationEnginesOff).toHaveBeenCalledWith("c1", false);
    expect(useAppStore.getState().conversations[0].enginesOff).toBe(false);
  });
});
