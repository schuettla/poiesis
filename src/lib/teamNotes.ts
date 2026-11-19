import type { EngineInfo, Persona, Team } from "./api";
import { tierOf, TIER_LABEL } from "./tier";
import type { Model } from "./types";
import { joinWords } from "./thought";

/**
 * `TEAM-9`: what the Teams tab says next to a place when its persona does not fit it well
 * (`TEAMS_PLAN` I.4). Notes only: nothing is forbidden. A helper, and a persona that takes the
 * chat's model, have no note, because nothing is known about the model they will have.
 */

export interface PlaceNote {
  /** `coordinator` or the member's role. */
  who: string;
  text: string;
}

function noteFor(
  who: string,
  persona: Persona | undefined,
  coordinator: boolean,
  models: Model[],
  engines: EngineInfo[]
): PlaceNote | null {
  if (!persona) return null;
  const app = persona.spawnable && persona.engine_id ? engines.find((e) => e.id === persona.engine_id) : undefined;
  if (app) {
    // It works through the agent app. It can post to the team only with Poiesis's own tools on for it.
    return {
      who,
      text: app.memory_ready && app.memory_tools
        ? `${persona.name} runs on ${app.name}, which has my tools on, so it can post to the team.`
        : `${persona.name} runs on ${app.name}. It can post to the team only when my tools are switched on for ${app.name}; otherwise it hands back its result.`,
    };
  }
  const model = persona.model_id ? models.find((m) => m.id === persona.model_id) : undefined;
  if (!model) return null;
  const tier = tierOf(model);
  if (coordinator && tier !== "full") {
    return { who, text: `${persona.name} works on a ${TIER_LABEL[tier]} model. A coordinator needs a Full model to plan.` };
  }
  if (!coordinator && tier === "chat") {
    return { who, text: `${persona.name} works on a Chat only model, so ${persona.name} has no tools and fits best as a reviewer or an adviser.` };
  }
  return null;
}

/** The note for one persona in one place, for the Add a member dialog. */
export function placeNote(
  who: string,
  personaRef: string,
  personas: Persona[],
  models: Model[],
  engines: EngineInfo[]
): PlaceNote | null {
  const persona = personas.find((p) => `persona:${p.id}` === personaRef);
  return noteFor(who, persona, who === "coordinator", models, engines);
}

/** Every note a team has: the coordinator's, then the members'. */
export function teamNotes(team: Team, personas: Persona[], models: Model[], engines: EngineInfo[]): PlaceNote[] {
  const list = [placeNote("coordinator", team.coordinator, personas, models, engines)];
  for (const m of team.members) list.push(placeNote(m.role, m.persona, personas, models, engines));
  // A member's note is about the member, not the coordinator: only the coordinator is judged as one.
  return list.filter((n): n is PlaceNote => !!n);
}

/** Where a persona sits in the teams: a place, or the coordinator. */
export interface TeamPlace {
  slug: string;
  team: string;
  /** The member's role, or `coordinator`. */
  role: string;
}

export function placesOf(personaId: string, teams: Team[]): TeamPlace[] {
  const ref = `persona:${personaId}`;
  const out: TeamPlace[] = [];
  for (const t of teams) {
    if (t.coordinator === ref) out.push({ slug: t.slug, team: t.name, role: "coordinator" });
    for (const m of t.members) if (m.persona === ref) out.push({ slug: t.slug, team: t.name, role: m.role });
  }
  return out;
}

/** What deleting a persona does, in the words of IV.1: the chats, and each place in a team. */
export function deleteWords(name: string, ledChats: number, places: TeamPlace[]): string {
  const teams = [...new Set(places.map((p) => p.team))];
  const leads = ledChats > 0 ? `${name} leads ${ledChats} ${ledChats === 1 ? "chat" : "chats"}` : `${name} leads no chats`;
  const isIn = teams.length ? ` and is in ${joinWords(teams)}` : "";
  const chats = ledChats > 0 ? `${ledChats === 1 ? "The chat goes" : "The chats go"} back to Poiesis` : "";
  const placed = teams.map((team) => {
    const mine = places.filter((p) => p.team === team);
    const asMember = mine.some((p) => p.role !== "coordinator");
    const asLead = mine.some((p) => p.role === "coordinator");
    return [
      asMember ? `in ${team}, ${name}'s place becomes a helper with the same job` : "",
      asLead ? `Poiesis coordinates ${team}` : "",
    ]
      .filter(Boolean)
      .join(", and ");
  });
  const effects = [chats, ...placed].filter(Boolean);
  return `${leads}${isIn}.${effects.length ? ` ${effects.join("; ")}.` : ""}`;
}
