/** The live active stack, in plain words, so the composer's "this chat" menu
 * (`ChatSetup`) says something before anyone opens the panel (WHY-4):
 * "Soul · The Editor · Notes". Cheap enough to compute on every render: no
 * manifest fetch happens until the panel opens. */
export function activeStackLabel(state: {
  memoryContext: { soul: string; fact_count: number };
  conversations: { id: string; personaId?: string | null }[];
  activeConversationId: string | null;
  personas: { id: string; name: string }[];
}): string {
  const parts: string[] = [];
  if (state.memoryContext.soul.trim()) parts.push("Soul");
  const conv = state.conversations.find((c) => c.id === state.activeConversationId);
  const persona = conv?.personaId ? state.personas.find((p) => p.id === conv.personaId) : undefined;
  if (persona) parts.push(persona.name);
  if (state.memoryContext.fact_count > 0) parts.push("Notes");
  return parts.length ? parts.join(" · ") : "This conversation only";
}
