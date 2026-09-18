import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";
import ImageByPath from "./ImageByPath";

/** Split off a trailing ```poiesis-action …``` fence (Generative UI, Phase B) so
 * the transcript shows the human sentence plus a compact chip, never raw JSON. */
function splitAction(text: string): { human: string; isAction: boolean } {
  const idx = text.indexOf("```poiesis-action");
  if (idx === -1) return { human: text, isAction: false };
  return { human: text.slice(0, idx).trim(), isAction: true };
}

export default function UserTurn({ message }: { message: Message }) {
  const { human, isAction } = splitAction(message.text);
  const viewArtifactByPath = useAppStore((s) => s.viewArtifactByPath);
  return (
    <div className="turn-user">
      {/* `HRN-UI-1`: this went to a run that was already working, so it is
          not waiting for a next turn. Saying which of the two it is matters:
          "sending" still could be missed, "sent mid-run" was read. Rendered
          only when there is a mark — an empty row still carried its margin
          and made the top padding visibly deeper than the bottom. */}
      {message.midRun && (
        <div className="role">
          <span className={`mid-run-mark ${message.midRun}`}>
            {message.midRun === "pending" ? "sending…" : "sent mid-run"}
          </span>
        </div>
      )}
      {human && <div className="body">{human}</div>}
      {/* The same reading the Rail puts at the end of a chat row: when. */}
      {message.createdAt > 0 && (
        <span className="turn-stamp" aria-hidden="true">
          {new Date(message.createdAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </span>
      )}
      {isAction && (
        <span className="block-action-chip" aria-label="block action">
          ⌁ block action
        </span>
      )}
      {message.attachments && message.attachments.length > 0 && (
        <div className="user-attachments">
          {message.attachments.map((a) =>
            // `STR-3`: a dropped photo should look like the photo, not a
            // filename — a PDF has no picture to show, so it keeps the glyph.
            a.kind === "image" ? (
              <button
                key={a.id}
                className="user-attachment-thumb"
                onClick={() => viewArtifactByPath(a.path, a.dataUri)}
                title={a.name}
              >
                <ImageByPath path={a.path} dataUri={a.dataUri} alt={a.name} className="user-attachment-img" />
                <span className="user-attachment-name">{a.name}</span>
              </button>
            ) : (
              <span className="user-attachment" key={a.id}>
                ▤ {a.name}
              </span>
            )
          )}
        </div>
      )}
    </div>
  );
}
