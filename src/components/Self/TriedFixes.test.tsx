/**
 * @vitest-environment jsdom
 *
 * `IMP-UI-1`: the card of a fix that was tried on replayed chats says which fixes were
 * tried and how each did, in one collapsed line first.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChangeProposal } from "../../lib/api";
import TriedFixes from "./TriedFixes";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const row = (over: Partial<ChangeProposal>): ChangeProposal => ({
  id: "r",
  target: "habit",
  slug: "check-header",
  proposed_text: "{}",
  rationale: "why",
  description: null,
  status: "pending",
  created_at: 1,
  problem_cue: "check the header row",
  form: "check",
  group_id: "g1",
  rehearsal_json: JSON.stringify({ rehearsed: true, right: 4, tries: 4, base_right: 1, conversations: ["c1", "gone"] }),
  ...over,
});

const winner = row({ id: "w" });
const loser = row({
  id: "l",
  target: "lesson",
  form: "reminder",
  status: "dropped",
  rehearsal_json: JSON.stringify({ rehearsed: true, right: 1, tries: 4, base_right: 1, conversations: ["c1"] }),
});

let container: HTMLDivElement;
let root: Root;
const button = (label: string | RegExp) =>
  Array.from(container.querySelectorAll("button")).find((b) =>
    typeof label === "string" ? b.textContent === label : label.test(b.textContent ?? "")
  ) as HTMLButtonElement;

beforeEach(() => {
  vi.stubGlobal("__TAURI_INTERNALS__", {});
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ conversations: [{ id: "c1", title: "Bank export", updatedAt: 1 }] } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("I tried fixes", () => {
  it("is one line until it is opened, then lists each fix and the setup as it is", async () => {
    const api = await import("../../lib/api");
    vi.spyOn(api, "inTauri").mockReturnValue(true);
    vi.spyOn(api, "improvementGroup").mockResolvedValue([loser, winner]);
    await act(async () => root.render(<TriedFixes proposal={winner} />));
    expect(container.textContent).toBe("check the header row came up again. I tried 2 fixes on the chats where it happened.");
    await act(async () => button(/came up again/).click());
    const text = container.textContent ?? "";
    expect(text).toContain("a check on the tool: right in 4 of 4 replays");
    expect(text).toContain("a written reminder: right in 1 of 4 replays");
    expect(text).toContain("the setup as it is: right in 1 of 4 replays");
    await act(async () => button("Show replays").click());
    expect(button("Bank export")).toBeTruthy();
    expect(container.textContent).toContain("a chat that is gone");
  });

  it("says plainly when there was no chat to try it on", async () => {
    const untried = row({ rehearsal_json: JSON.stringify({ rehearsed: false }) });
    await act(async () => root.render(<TriedFixes proposal={untried} />));
    expect(container.textContent).toBe("I had no chat to try this on, so it is untried.");
  });

  it("shows nothing for an ordinary proposal", async () => {
    await act(async () => root.render(<TriedFixes proposal={row({ group_id: null })} />));
    expect(container.textContent).toBe("");
  });
});
