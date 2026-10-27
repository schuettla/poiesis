import { useState } from "react";
import { useAppStore } from "../../lib/store";
import { workspaceAvailable } from "../../lib/experiments";
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
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const setWorkspaceMode = useAppStore((s) => s.setWorkspaceMode);
  const workspaceOffered = useAppStore((s) => workspaceAvailable(s));
  const setEnginesOff = useAppStore((s) => s.setEnginesOff);
  const anyEngine = useAppStore((s) => s.engines.some((e) => e.enabled && e.delegate_ready));
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
            {/* The mode is the chat's own, so it is changed from the chat's own
                menu as well as by `/workspace`. An experiment (`EXP-3`): not
                offered until it has been turned on in Settings. */}
            {workspaceOffered && (
              <>
                <button
                  className="row-menu-item"
                  role="menuitem"
                  onClick={() => {
                    setOpen(false);
                    setWorkspaceMode(!workspaceMode);
                  }}
                >
                  {workspaceMode ? "Turn off Workspace mode" : "Turn on Workspace mode"}
                </button>
                <hr className="row-menu-sep" />
              </>
            )}
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
            {/* `RTE-5`: only offered once an agent app is on, so the menu of a
                person who never set one up does not grow a line they cannot use. */}
            {anyEngine && (
              <button
                className="row-menu-item"
                role="menuitemcheckbox"
                aria-checked={!conversation.enginesOff}
                onClick={() => {
                  setOpen(false);
                  void setEnginesOff(conversation.id, !conversation.enginesOff);
                }}
              >
                {conversation.enginesOff ? "Use external agents in this chat" : "Keep external agents out of this chat"}
              </button>
            )}
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
