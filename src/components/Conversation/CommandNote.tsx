import type { CommandNoteView } from "../../lib/types";
import type { CheckupLine } from "../../lib/api";
import { shortTime } from "../../lib/time";
import CheckupCard from "./CheckupCard";

/**
 * `REG-4`: one muted line for a command that changed something, placed among the
 * turns by time the way the compaction divider is. It says who did it — "you",
 * or, for the things I do on my own, nothing but what I did — and when.
 *
 * Navigation (`/self`, `/usage`) leaves no line: nothing changed.
 */
export default function CommandNote({ note }: { note: CommandNoteView }) {
  const time = shortTime(note.at);
  // `CHK-UI-1`: a checkup is a card, kept in its own trace (the lines ride in
  // `args`), so it is the same card after a reload.
  if (note.name === "checkup") {
    try {
      const lines = JSON.parse(note.args) as CheckupLine[];
      if (Array.isArray(lines) && lines.length) {
        return <CheckupCard lines={lines} summary={note.note} at={time} />;
      }
    } catch {
      /* an unreadable card is just its sentence */
    }
  }
  // A note that is itself a sentence about a default reads whole; anything else
  // is the command as typed, with what happened in the tooltip.
  const sentence = note.note?.startsWith("/") ? note.note : null;
  const text = sentence ?? `/${note.name}${note.args ? ` ${note.args}` : ""}`;
  // `AGC-4`: your answer to something I asked ("you said yes to /skillify") is a
  // whole sentence already, and already says who.
  const answer = (note.outcome === "accepted" || note.outcome === "declined") && !!note.note;

  return (
    <div
      className={`command-note ${note.by === "agent" ? "by-agent" : ""} ${note.outcome === "failed" ? "failed" : ""}`}
      title={sentence ? undefined : note.note}
    >
      {answer ? (
        <>
          {note.note!.charAt(0).toUpperCase() + note.note!.slice(1)}
          <span className="command-note-meta"> · {time}</span>
        </>
      ) : note.by === "agent" ? (
        <>
          <span aria-hidden="true">◆ </span>
          {note.note ?? text}
          <span className="command-note-meta"> · {time}</span>
        </>
      ) : (
        <>
          {text}
          <span className="command-note-meta"> · you · {time}</span>
        </>
      )}
    </div>
  );
}
