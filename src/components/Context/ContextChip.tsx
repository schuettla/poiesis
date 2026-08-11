import { useAppStore } from "../../lib/store";
import "./Context.css";

/** The live active stack, in plain words, so the chip means something before
 * anyone clicks it (WHY-4): "Soul · The Editor · Notes". Cheap enough to
 * compute on every render — no manifest fetch happens until the panel opens. */
function activeStackLabel(state: {
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
  return parts.length ? parts.join(" · ") : "What I'm working from";
}

/** WHY-4's first entry point: a quiet chip under the composer naming the
 * active stack, opening the shared `ContextPanel` on the live view. */
export default function ContextChip() {
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const label = useAppStore(activeStackLabel);
  const openContextPanel = useAppStore((s) => s.openContextPanel);

  if (!activeConversationId) return null;

  return (
    <div className="context-chip-wrap">
      <button
        className="context-chip"
        title="See what I'm working from, layer by layer"
        aria-label={`What I'm working from: ${label}`}
        onClick={() => openContextPanel({ conversationId: activeConversationId })}
      >
        {label}
      </button>
    </div>
  );
}
