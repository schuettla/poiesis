/**
 * @vitest-environment jsdom
 *
 * `TEAM-23`: what Poiesis would like to change about a team is answered on that team's page.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "../../lib/store";
import TeamProposals from "./TeamProposals";

const answer = vi.fn();
vi.mock("../../lib/api", async (orig) => ({
  ...(await orig<typeof import("../../lib/api")>()),
  wayProposalAnswer: (id: string, what: string) => answer(id, what),
}));

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const proposal = (id: string, team: string, quote: string) => ({
  id,
  target: "team",
  status: "pending",
  rationale: "docs got no work in its last 5 tasks.",
  proposed_text: JSON.stringify({ lead: "I'd like to change Shop team:", quote, change: { action: "team_take_out", team, role: "docs" } }),
});

let container: HTMLDivElement;
let root: Root;
const refresh = vi.fn(async () => {});
const changed = vi.fn(async () => {});

beforeEach(() => {
  answer.mockReset();
  answer.mockResolvedValue(undefined);
  refresh.mockClear();
  changed.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    refreshChangeProposals: refresh,
    changeProposals: [
      proposal("p1", "shop-team", "Take docs out of the team."),
      proposal("p2", "other-team", "Take ui out of the team."),
    ],
  } as never);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = (unsaved = false, slug = "shop-team") =>
  act(() => root.render(<TeamProposals slug={slug} unsaved={unsaved} onChanged={changed} />));
const button = (name: string) => [...container.querySelectorAll("button")].find((b) => b.textContent === name)!;
const settle = () => act(async () => {});

describe("what Poiesis would like to change about a team", () => {
  it("shows the sentence and the reason, for this team only", () => {
    render();
    expect(container.textContent).toContain("I'd like to change Shop team:");
    expect(container.textContent).toContain("Take docs out of the team.");
    expect(container.textContent).toContain("Why: docs got no work in its last 5 tasks.");
    expect(container.textContent).not.toContain("Take ui out");
  });

  it("shows nothing when nothing waits for this team", () => {
    render(false, "a-team-with-nothing");
    expect(container.querySelector(".team-proposals")).toBeNull();
  });

  it("Yes asks the backend to make the change, then shows the team as it is now", async () => {
    render();
    await act(async () => button("Yes").click());
    expect(answer).toHaveBeenCalledWith("p1", "yes");
    expect(refresh).toHaveBeenCalled();
    expect(changed).toHaveBeenCalled();
  });

  it("No is sent and remembered, without showing the team again as changed by Poiesis", async () => {
    render();
    await act(async () => button("No").click());
    expect(answer).toHaveBeenCalledWith("p1", "no");
  });

  it("waits for a Yes while something typed on the page is not saved, but No is still possible", () => {
    render(true);
    expect(button("Yes").disabled).toBe(true);
    expect(button("No").disabled).toBe(false);
    expect(container.textContent).toContain("Save or undo what you changed here first.");
  });

  it("says why when it could not be made, and leaves the page as it was", async () => {
    answer.mockRejectedValue(new Error("Shop team is locked, so I leave it alone."));
    render();
    await act(async () => button("Yes").click());
    await settle();
    expect(container.querySelector('[role="alert"]')?.textContent).toBe("Shop team is locked, so I leave it alone.");
    expect(changed).not.toHaveBeenCalled();
    expect(button("Yes").disabled).toBe(false);
  });
});
