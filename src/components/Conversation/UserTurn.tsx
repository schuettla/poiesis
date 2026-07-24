import type { Message } from "../../lib/types";

/** Split off a trailing ```nexus-action …``` fence (Generative UI, Phase B) so
 * the transcript shows the human sentence plus a compact chip, never raw JSON. */
function splitAction(text: string): { human: string; isAction: boolean } {
  const idx = text.indexOf("```nexus-action");
  if (idx === -1) return { human: text, isAction: false };
  return { human: text.slice(0, idx).trim(), isAction: true };
}

export default function UserTurn({ message }: { message: Message }) {
  const { human, isAction } = splitAction(message.text);
  return (
    <div className="turn-user">
      <div className="role">You</div>
      {human && <div className="body">{human}</div>}
      {isAction && (
        <span className="block-action-chip" aria-label="block action">
          ⌁ block action
        </span>
      )}
      {message.attachments && message.attachments.length > 0 && (
        <div className="user-attachments">
          {message.attachments.map((a) => (
            <span className="user-attachment" key={a.id}>
              {a.kind === "image" ? "▣" : "▤"} {a.name}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
