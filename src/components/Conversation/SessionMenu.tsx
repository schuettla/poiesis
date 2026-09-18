import { useState } from "react";
import { useAppStore } from "../../lib/store";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import ProjectMenuItems from "./ProjectMenuItems";

/**
 * The session's own actions, top-right of the chat. Mirrors the rail row's
 * context menu so a chat can be scheduled or removed from wherever you happen
 * to be looking at it — and asks the same question before removing it.
 */
export default function SessionMenu() {
  const conversation = useAppStore((s) =>
    s.conversations.find((c) => c.id === s.activeConversationId)
  );
  const deleteConversation = useAppStore((s) => s.deleteConversation);
  const scheduleConversation = useAppStore((s) => s.scheduleConversation);
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
            {/* Turn this chat into something I do on a schedule. The moment
                you want a task is usually just after you had the agent do the
                thing once, by hand — which is this chat. */}
            <button
              className="row-menu-item"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                scheduleConversation(conversation.id);
              }}
            >
              Schedule this…
            </button>
            <ProjectMenuItems conversationId={conversation.id} onDone={() => setOpen(false)} />
            <hr className="row-menu-sep" />
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
