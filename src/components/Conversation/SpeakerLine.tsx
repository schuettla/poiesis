import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";
import PersonaGlyph, { PoiesisGlyph } from "../Personas/PersonaGlyph";

/**
 * `TEAM-3`: the replies whose speaker is not the one before. A chat led by one
 * leader has none, and reads as a chat always has; a name appears only where it
 * changes: after a handover, or when one message was led by someone else.
 *
 * Only replies that say who gave them count. One from before authors were kept
 * says nothing, and neither starts nor ends a run of the same voice.
 */
export function speakerChanges(messages: Message[]): Set<string> {
  const changed = new Set<string>();
  let previous: string | undefined;
  for (const m of messages) {
    if (m.role !== "assistant" || !m.author) continue;
    if (previous && previous !== m.author) changed.add(m.id);
    previous = m.author;
  }
  return changed;
}

/** One quiet line before a reply: the speaker's face and name. */
export default function SpeakerLine({ author }: { author: string }) {
  const persona = useAppStore((s) => (author.startsWith("persona:") ? s.personas.find((p) => p.id === author.slice(8)) : undefined));
  // A persona that was deleted is gone from the list; its old replies do not name it.
  const name = author === "poiesis" ? "Poiesis" : persona?.name;
  if (!name) return null;
  return (
    <div className="speaker-line">
      {persona ? <PersonaGlyph name={name} look={persona.look} size={16} /> : <PoiesisGlyph size={16} />}
      <span>{name}</span>
    </div>
  );
}
