import type { Persona, Team } from "./api";
import { leaderOf } from "./leaders";
import { useAppStore, type AppState } from "./store";

/**
 * `TEAM-17`: who the person may speak to directly in a session a team leads (`TEAMS_PLAN` I.8). A
 * member is one the team draws a `you talks` line to, and that is a persona that still exists: a
 * plain helper has no voice of its own. The coordinator is always there and is the default, so it is
 * not in this list.
 */
export interface Addressable {
  role: string;
  /** What it is called to the person: the persona's name. */
  name: string;
  persona: Pick<Persona, "id" | "name" | "look">;
}

export function addressableMembers(team: Pick<Team, "members" | "lines">, personas: Pick<Persona, "id" | "name" | "look">[]): Addressable[] {
  return team.members.flatMap((m) => {
    if (!team.lines.some((l) => l.kind === "talks" && l.from === "you" && l.to === m.role)) return [];
    const persona = personas.find((p) => `persona:${p.id}` === m.persona);
    return persona ? [{ role: m.role, name: persona.name, persona }] : [];
  });
}

/**
 * `TEAM-23`: the personas that play a member the person has no line to. `/ask` names them in its answer,
 * and remembers the try so that Poiesis can offer to draw the line when it keeps happening. A plain
 * helper is left out: it has no voice, so a line to it would give nothing.
 */
export function membersWithoutLine(team: Pick<Team, "members" | "lines">, personas: Pick<Persona, "id" | "name" | "look">[]): Addressable[] {
  const reachable = new Set(addressableMembers(team, personas).map((m) => m.role));
  return team.members.flatMap((m) => {
    const persona = personas.find((p) => `persona:${p.id}` === m.persona);
    return persona && !reachable.has(m.role) ? [{ role: m.role, name: persona.name, persona }] : [];
  });
}

/** The team that leads the chat on screen, from the teams the store holds. */
export function ledTeamOf(s: Pick<AppState, "conversations" | "teams" | "activeConversationId">): Team | undefined {
  const conv = s.conversations.find((c) => c.id === s.activeConversationId);
  const leader = conv ? leaderOf(conv) : "";
  return leader.startsWith("team:") ? s.teams.find((t) => t.slug === leader.slice(5)) : undefined;
}

/** `/ask kai fix the intro`: the member named first (by persona name or role), then the message. */
export function matchMember(members: Addressable[], typed: string): { member: Addressable; rest: string } | null {
  const lower = typed.trim().toLowerCase();
  const keys = members
    .flatMap((m) => [m.name.toLowerCase(), m.role.toLowerCase()].map((key) => ({ key, m })))
    .sort((a, b) => b.key.length - a.key.length);
  for (const { key, m } of keys) {
    if (lower === key) return { member: m, rest: "" };
    if (lower.startsWith(`${key} `)) return { member: m, rest: typed.trim().slice(key.length).trim() };
  }
  return null;
}

/** The members the person may speak to in this chat, and who the next message goes to. */
export function useAddressing(): { members: Addressable[]; selected: Addressable | null; team: Team | undefined } {
  const team = useAppStore(ledTeamOf);
  const personas = useAppStore((s) => s.personas);
  const addressed = useAppStore((s) => s.turnModifiers.addressed);
  const members = team ? addressableMembers(team, personas) : [];
  return { team, members, selected: members.find((m) => m.role === addressed) ?? null };
}
