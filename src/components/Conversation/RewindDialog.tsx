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
 * Rewind never edits the conversation you are in. It opens a branch that holds
 * everything before the turn and leaves the original in your list, so there is
 * nothing here to be afraid of except the files, and the files are the one
 * question the dialog asks. The checkbox for them appears only when I actually
 * changed some since that turn, and starts on: you are going back, so the files
 * should usually go back with you.
 */
export default function RewindDialog() {
  const messageId = useAppStore((s) => s.rewindRequest);
  const convId = useAppStore((s) => s.activeConversationId);
  const turn = useAppStore((s) => {
    const conv = s.conversations.find((c) => c.id === s.activeConversationId);
    return conv?.messages.find((m) => m.id === s.rewindRequest);
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
      title={`Go back to before “${preview(turn.text)}”?`}
      body="I'll open a branch from there. This chat stays exactly as it is, in your list."
      confirmLabel="Rewind"
      onCancel={() => close(null)}
      onConfirm={() => {
        close(null);
        void rewind(messageId, { undoFiles: files > 0 && undoFiles, putBack });
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
        <label className="rewind-option">
          <input type="checkbox" checked={putBack} onChange={(e) => setPutBack(e.target.checked)} />
          <span>Put your message back in the box</span>
        </label>
      </div>
    </ConfirmDialog>
  );
}
