import { useEffect, useState } from "react";
import { changesSince, inTauri } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import "./RewindDialog.css";

/** One line standing in for a turn. */
function preview(text: string): string {
  const first = (text.split(/\r?\n/)[0] ?? "").trim();
  return first.length > 48 ? `${first.slice(0, 47).trimEnd()}…` : first;
}

/**
 * `RWD-UI-1`: go back to before one of your turns.
 *
 * Rewind removes that turn and everything after it from this chat, for good, so
 * the dialog says so. The files are the other question: the checkbox for them
 * appears only when I actually changed some since that turn, and starts on: you
 * are going back, so the files should usually go back with you.
 *
 * "Try again" and an edited message use the same dialog, and only when there is
 * something to ask: files to take back, or later messages that would go.
 */
export default function RewindDialog() {
  const messageId = useAppStore((s) => s.rewindRequest);
  const convId = useAppStore((s) => s.activeConversationId);
  const turn = useAppStore((s) => {
    const conv = s.conversations.find((c) => c.id === s.activeConversationId);
    return conv?.messages.find((m) => m.id === s.rewindRequest);
  });
  const retry = useAppStore((s) => s.rewindRetry);
  const text = useAppStore((s) => s.rewindText);
  const edited = retry && text !== null;
  // More than the one answer goes when you wrote again after this message.
  const later = useAppStore((s) => {
    const conv = s.conversations.find((c) => c.id === s.activeConversationId);
    const list = conv?.messages ?? [];
    const at = list.findIndex((m) => m.id === s.rewindRequest);
    return at >= 0 && list.slice(at + 1).some((m) => m.role === "user");
  });
  const close = useAppStore((s) => s.requestRewind);
  const rewind = useAppStore((s) => s.rewindTo);

  const [files, setFiles] = useState(0);
  const [undoFiles, setUndoFiles] = useState(true);
  const [putBack, setPutBack] = useState(true);

  // How many files would go back, asked before the user commits to anything.
  useEffect(() => {
    setFiles(0);
    setUndoFiles(true);
    setPutBack(true);
    if (!messageId || !convId || !inTauri()) return;
    let live = true;
    changesSince(convId, messageId)
      .then((n) => live && setFiles(n))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [messageId, convId]);

  if (!messageId || !turn) return null;

  return (
    <ConfirmDialog
      title={
        edited
          ? "Send your changed message?"
          : retry
            ? `Try “${preview(turn.text)}” again?`
            : `Go back to before “${preview(turn.text)}”?`
      }
      body={
        edited
          ? "Everything after this message is removed from this chat, then I answer your new words. That can't be undone."
          : !retry
            ? "Everything from this message on is removed from this chat. That can't be undone."
            : later
              ? "Everything after this message is removed from this chat, then I answer it again. That can't be undone."
              : "The answer you have is removed from this chat and I answer again."
      }
      confirmLabel={edited ? "Send" : retry ? "Try again" : "Rewind"}
      onCancel={() => close(null)}
      onConfirm={() => {
        close(null);
        const undo = files > 0 && undoFiles;
        void rewind(
          messageId,
          retry
            ? { undoFiles: undo, putBack: false, retry, ...(text === null ? {} : { text }) }
            : { undoFiles: undo, putBack }
        );
      }}
    >
      <div className="rewind-options">
        {files > 0 && (
          <label className="rewind-option">
            <input type="checkbox" checked={undoFiles} onChange={(e) => setUndoFiles(e.target.checked)} />
            <span>
              Also take back my changes to {files} file{files === 1 ? "" : "s"}
            </span>
          </label>
        )}
        {!retry && (
          <label className="rewind-option">
            <input type="checkbox" checked={putBack} onChange={(e) => setPutBack(e.target.checked)} />
            <span>Put your message back in the box</span>
          </label>
        )}
      </div>
    </ConfirmDialog>
  );
}
