/**
 * @vitest-environment jsdom
 *
 * `TEAM-22`: "Keep these helpers as a team" is in the session menu only when the backend has a team
 * to make from what the session did, and opens it as a New team.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Team } from "../../lib/api";

const api = vi.hoisted(() => ({ teamFromSession: vi.fn() }));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  ...api,
}));

import SessionMenu from "./SessionMenu";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const made = { slug: "", name: "Shop rebuild team", members: [] } as unknown as Team;
const openTeamDraft = vi.fn();

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  api.teamFromSession.mockReset();
  openTeamDraft.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    conversations: [{ id: "c1", title: "Shop rebuild", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    engines: [],
    openTeamDraft,
  } as never);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const openMenu = async () => {
  await act(async () => root.render(<SessionMenu />));
  await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="Session actions"]')!.click());
};
const keep = () => Array.from(container.querySelectorAll<HTMLButtonElement>("[role=menuitem]")).find((b) => b.textContent === "Keep these helpers as a team");

describe("Keep these helpers as a team", () => {
  it("is not in the menu when the session has no team to make", async () => {
    api.teamFromSession.mockResolvedValue(null);
    await openMenu();
    expect(api.teamFromSession).toHaveBeenCalledWith("c1");
    expect(keep()).toBeUndefined();
  });

  it("is in the menu when it has, and opens the New team", async () => {
    api.teamFromSession.mockResolvedValue(made);
    await openMenu();
    expect(keep()).toBeDefined();
    await act(async () => keep()!.click());
    expect(openTeamDraft).toHaveBeenCalledWith(made);
  });

  it("asks nothing until the menu is opened", async () => {
    await act(async () => root.render(<SessionMenu />));
    expect(api.teamFromSession).not.toHaveBeenCalled();
  });
});
