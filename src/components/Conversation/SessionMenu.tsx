import { useState } from "react";
import { useAppStore } from "../../lib/store";
import ConfirmDialog from "../Confirm/ConfirmDialog";

/**
 * The session's own actions, top-right of the chat. Mirrors the rail row's
 * context menu so a chat can be removed from wherever you happen to be looking
 * at it — and asks the same question before it does.
 */
export default function SessionMenu() {
  const conversation = useAppStore((s) =>
    s.conversations.find((c) => c.id === s.activeConversationId)
  );
  const deleteConversation = useAppStore((s) => s.deleteConversation);
  const [open, setOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);

  if (!conversation) return null;

  return (
    <div className="session-menu-wrap">
      <button
        className="session-more"
        aria-label="Session actions"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Session actions"
        onClick={() => setOpen((v) => !v)}
      >
        ⋯
      </button>
      {open && (
        <>
          <div className="row-menu-backdrop" onClick={() => setOpen(false)} />
          <div className="row-menu" role="menu">
            <button
              className="row-menu-item danger"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                setConfirming(true);
              }}
            >
              Delete chat
            </button>
          </div>
        </>
      )}
      {confirming && (
        <ConfirmDialog
          title="Delete this chat?"
          body={`“${conversation.title}” and everything said in it will be removed. This can't be undone.`}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            deleteConversation(conversation.id);
          }}
        />
      )}
    </div>
  );
}
