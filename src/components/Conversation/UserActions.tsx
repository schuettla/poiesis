import { useLayoutEffect, useRef, useState } from "react";
import { isPersistedId, useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";
import { busyIn } from "../../lib/runs";
import { EditIcon, RefreshIcon } from "../Icons/Icons";
import CopyButton from "./CopyButton";

/**
 * `HRN-UI-5`: what you can do with one of your messages.
 *
 * **Copy** takes what you typed. **Try again** sends it once more and **Edit**
 * sends your new words in its place; both remove the message and everything
 * after it from this chat first.
 *
 * `copy` is what Copy takes; `onEdit` is absent for a message that was not typed
 * (a click in the interface).
 */
export function UserActions({ message, copy, onEdit }: { message: Message; copy: string; onEdit?: () => void }) {
  const retryTurn = useAppStore((s) => s.retryTurn);
  const busy = useAppStore((s) => busyIn(s, s.activeConversationId));
  const [pending, setPending] = useState(false);
  // A message sent into a working run is part of that run, not a turn to start over from.
  const canAct = !busy && isPersistedId(message.id) && !message.midRun;
  return (
    <div className="turn-actions">
      {copy && <CopyButton text={copy} tip="Copy your message" />}
      {canAct && (
        <button
          className="turn-btn"
          aria-label="Try again"
          data-tip="Try again: send this again and replace what came after"
          disabled={pending}
          onClick={() => {
            setPending(true);
            void retryTurn(message.id).finally(() => setPending(false));
          }}
        >
          <RefreshIcon size={14} strokeWidth={1.5} />
        </button>
      )}
      {canAct && onEdit && (
        <button
          className="turn-btn"
          aria-label="Edit"
          data-tip="Edit: change your message and start again from here"
          onClick={onEdit}
        >
          <EditIcon size={14} strokeWidth={1.5} />
        </button>
      )}
    </div>
  );
}

/** Your message, open for changes where it stands: the card is the field, and the
 * words keep their place and their type. Enter sends, Shift+Enter is a
 * new line, Escape leaves it as it was. It stays open until the old turn is gone,
 * so a refusal or a Cancel in the files question loses nothing. */
export function EditTurn({ message, onClose }: { message: Message; onClose: () => void }) {
  const retryTurn = useAppStore((s) => s.retryTurn);
  const busy = useAppStore((s) => busyIn(s, s.activeConversationId));
  // For a `/skill` message the words you typed are the ones after the name.
  const [draft, setDraft] = useState(message.command ? message.command.args : message.text);
  const [pending, setPending] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);

  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    // Measured without a scrollbar: one would narrow the box and add a line.
    el.style.overflowY = "hidden";
    el.style.height = "auto";
    // `scrollHeight` leaves the border out.
    el.style.height = `${el.scrollHeight + el.offsetHeight - el.clientHeight}px`;
    el.style.overflowY = "";
  }, [draft]);

  // Empty is only a message where it was one: a bare `/name`, or files alone.
  const mayBeEmpty = message.command ? !message.command.args : !!message.attachments?.length;
  const canSend = !busy && !pending && (draft.trim().length > 0 || mayBeEmpty);
  const send = () => {
    if (!canSend) return;
    setPending(true);
    void retryTurn(message.id, draft.trim()).finally(() => setPending(false));
  };

  return (
    <div className="turn-edit">
      {message.command && <span className="command-chip-name">/{message.command.name}</span>}
      <textarea
        ref={box}
        aria-label="Edit your message"
        rows={1}
        autoFocus
        value={draft}
        onFocus={(e) => e.target.setSelectionRange(e.target.value.length, e.target.value.length)}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onClose();
          } else if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            send();
          }
        }}
      />
      <div className="turn-edit-foot">
        <span className="turn-edit-note">Everything after this message is removed from this chat.</span>
        <button className="btn-secondary" onClick={onClose}>
          Cancel
        </button>
        <button className="btn-primary" disabled={!canSend} onClick={send}>
          Send
        </button>
      </div>
    </div>
  );
}
