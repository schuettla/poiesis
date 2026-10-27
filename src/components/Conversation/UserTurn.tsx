import type { SteerKind } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";
import ImageByPath from "./ImageByPath";
import { SpokenGlyph } from "./SpokenMark";

/** Split off a trailing ```poiesis-action …``` fence (Generative UI, Phase B) so
 * the transcript shows the human sentence plus a compact chip, never raw JSON. */
function splitAction(text: string): { human: string; isAction: boolean } {
  const idx = text.indexOf("```poiesis-action");
  if (idx === -1) return { human: text, isAction: false };
  return { human: text.slice(0, idx).trim(), isAction: true };
}

/** `USE-UI-2`: the words for what a quick decision read a mid-run message as. */
const STEER_TAG: Record<SteerKind, string> = {
  stop: "stop",
  redirect: "redirect",
  add: "addition",
  question: "question",
};

export default function UserTurn({ message }: { message: Message }) {
  const { human, isAction } = splitAction(message.text);
  const viewArtifactByPath = useAppStore((s) => s.viewArtifactByPath);
  const answerSteerStop = useAppStore((s) => s.answerSteerStop);
  const steer = message.steer;
  return (
    <div className="turn-user">
      {/* `HRN-UI-1`: this went to a run that was already working, so it is
          not waiting for a next turn. Saying which of the two it is matters:
          "sending" still could be missed, "sent mid-run" was read. Rendered
          only when there is a mark — an empty row still carried its margin
          and made the top padding visibly deeper than the bottom.
          `USE-UI-2`: a held message has not been sent at all, so it says so. */}
      {(message.midRun || steer) && (
        <div className="role">
          {message.midRun && (
            <span className={`mid-run-mark ${message.midRun}`}>
              {message.midRun === "pending" ? "sending…" : message.midRun === "held" ? "not sent yet" : "sent mid-run"}
            </span>
          )}
          {steer && (
            <span
              className="steer-tag"
              title={`Read as ${steer.kind === "add" ? "an addition" : `a ${steer.kind}`} (${steer.p.toFixed(2)})`}
            >
              {STEER_TAG[steer.kind]}
            </span>
          )}
        </div>
      )}
      {/* `SKC-2`: a skill command went out as `/name`, with that skill's
          instructions loaded before I answered. Say so, and let the user see
          exactly what they asked for rather than a message they never wrote. */}
      {message.command && (
        <details className="command-chip">
          <summary>
            <span className="command-chip-name">/{message.command.name}</span>
            {message.command.args && <span className="command-chip-args"> {message.command.args}</span>}
          </summary>
          <p className="command-chip-note">
            What I was asked: run my {message.command.name} skill
            {message.command.args ? ` on “${message.command.args}”` : ""}. Its instructions were
            loaded before I answered.
          </p>
        </details>
      )}
      {/* The chip already shows the words after the name, so the body would
          only say them twice. */}
      {human && !message.command && <div className="body">{human}</div>}
      {/* `USE-UI-2`: a sure stop is not sent to the run. It asks first, and both
          answers are labels for the decision that asked. */}
      {message.midRun === "held" && steer && (
        <div className="steer-stop" role="group" aria-label="Stop the run?">
          <span className="steer-stop-q">Stop the run?</span>
          <button className="btn-text" onClick={() => void answerSteerStop(message.id, true)}>
            Stop
          </button>
          <button className="btn-text" onClick={() => void answerSteerStop(message.id, false)}>
            Keep going
          </button>
        </div>
      )}
      {/* The same reading the Rail puts at the end of a chat row: when. */}
      {message.createdAt > 0 && (
        <span className="turn-stamp" aria-hidden="true">
          {message.spoken && <SpokenGlyph />}
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
