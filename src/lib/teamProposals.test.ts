import { describe, expect, it } from "vitest";
import { teamProposals } from "./teamProposals";

const row = (over: Record<string, unknown> = {}) => ({
  id: "p1",
  target: "team",
  status: "pending",
  rationale: "docs got no work in its last 5 tasks.",
  proposed_text: JSON.stringify({
    lead: "I'd like to change Shop team:",
    quote: "Take docs out of the team.",
    change: { action: "team_take_out", team: "shop-team", role: "docs" },
  }),
  ...over,
});

describe("proposals about teams", () => {
  it("are read from what the backend stored: the words, the reason and the team", () => {
    expect(teamProposals([row()])).toEqual([
      {
        id: "p1",
        team: "shop-team",
        lead: "I'd like to change Shop team:",
        quote: "Take docs out of the team.",
        why: "docs got no work in its last 5 tasks.",
      },
    ]);
  });

  it("leave out what is not about a team, what is answered, and what cannot be read", () => {
    const list = [
      row({ id: "a", target: "way" }),
      row({ id: "b", status: "applied" }),
      row({ id: "c", proposed_text: "not json" }),
      row({ id: "d", proposed_text: JSON.stringify({ lead: "x", quote: "y", change: {} }) }),
      row({ id: "e" }),
    ];
    expect(teamProposals(list).map((p) => p.id)).toEqual(["e"]);
  });
});
