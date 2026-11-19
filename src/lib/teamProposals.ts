import type { ChangeProposal } from "./api";

/**
 * `TEAM-23`: what Poiesis would like to change about a team (`TEAMS_PLAN` Part VII). The backend stores
 * such a proposal under the target `team`, with the words to ask it in and the change itself; it is
 * answered on the team it is about, in Settings, Personas & teams, and badges that section, not Self.
 */
export const TEAM_TARGET = "team";

export interface TeamProposal {
  id: string;
  /** The team it is about. */
  team: string;
  /** "I'd like to change Shop team:" */
  lead: string;
  /** "Take docs out of the team." */
  quote: string;
  /** "docs got no work in its last 5 tasks." */
  why: string;
}

/** The proposals about teams, in the order they were made, those that cannot be read left out. */
export function teamProposals(list: Pick<ChangeProposal, "id" | "target" | "proposed_text" | "rationale" | "status">[]): TeamProposal[] {
  return list
    .filter((p) => p.target === TEAM_TARGET && p.status === "pending")
    .flatMap((p) => {
      try {
        const stored = JSON.parse(p.proposed_text) as { lead?: string; quote?: string; change?: { team?: string } };
        const team = stored.change?.team;
        if (!team || !stored.lead || !stored.quote) return [];
        return [{ id: p.id, team, lead: stored.lead, quote: stored.quote, why: p.rationale }];
      } catch {
        return [];
      }
    });
}
