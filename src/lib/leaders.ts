/**
 * `TEAM-6`: who leads a chat, as the sidebar and the filter read it. `leader` is what the
 * chat stores (`poiesis` or `persona:<id>`); an older chat has only `personaId`, and one with
 * neither is led by Poiesis.
 */
export function leaderOf(c: { leader?: string | null; personaId?: string | null }): string {
  return c.leader ?? (c.personaId ? `persona:${c.personaId}` : "poiesis");
}
