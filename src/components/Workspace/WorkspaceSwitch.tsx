import { useAppStore } from "../../lib/store";
import { LayoutIcon, MessageIcon } from "../Icons/Icons";
import "./WorkspaceSwitch.css";

/**
 * Workspace mode, said in the top bar and always one click away. The mode is on
 * for the chat; this is only which of its two views is up: the message stream or
 * the workspace canvas. It is there whenever the mode is on, in either view, the
 * way the mic indicator is there whenever voice is live. Turning the mode off is
 * the chip under the composer, or `Turn off` in the workspace.
 *
 * The dot says the canvas changed while it was out of sight.
 */
export default function WorkspaceSwitch() {
  const view = useAppStore((s) => s.view);
  const on = useAppStore((s) => s.workspaceMode);
  const shown = useAppStore((s) => s.workspaceShown);
  const unseen = useAppStore((s) => !!(s.activeConversationId && s.surfaceUnseen[s.activeConversationId]));
  const setWorkspaceShown = useAppStore((s) => s.setWorkspaceShown);
  const showConversation = useAppStore((s) => s.showConversation);
  if (view !== "chat" || !on) return null;

  // An open file or artifact covers the conversation. Choosing a view is
  // choosing to look at the conversation again.
  const pick = (canvas: boolean) => {
    showConversation();
    setWorkspaceShown(canvas);
  };

  return (
    <div className="segmented topbar-ws" role="group" aria-label="This chat is in Workspace mode. View">
      <button
        className={`segmented-item${shown ? "" : " on"}`}
        aria-pressed={!shown}
        title="The message stream. Workspace mode stays on."
        onClick={() => pick(false)}
      >
        <MessageIcon size={13} />
        Chat
      </button>
      <button
        className={`segmented-item${shown ? " on" : ""}`}
        aria-pressed={shown}
        title="The workspace: the interface I built for this chat"
        onClick={() => pick(true)}
      >
        <LayoutIcon size={13} />
        Workspace
        {unseen && !shown && <span className="topbar-ws-dot" role="img" aria-label="changed since you last looked" />}
      </button>
    </div>
  );
}
