import { useAppStore } from "../../lib/store";
import { CloseIcon } from "../Icons/Icons";
import "./WorkspaceOffer.css";

/**
 * The agent changed the interface while the message stream was up. A quiet line
 * above the composer says so and opens it. It is an offer, never a switch of view
 * on its own: you may be reading something. Only a chat in Workspace mode has an
 * interface to change, so a chat outside it never shows this.
 */
export default function WorkspaceOffer() {
  const convId = useAppStore((s) => s.activeConversationId);
  const unseen = useAppStore((s) => !!(s.activeConversationId && s.surfaceUnseen[s.activeConversationId]));
  const modeOn = useAppStore((s) => s.workspaceMode);
  const setWorkspaceShown = useAppStore((s) => s.setWorkspaceShown);
  const clear = useAppStore((s) => s.clearSurfaceUnseen);
  if (!convId || !unseen || !modeOn) return null;

  return (
    <div className="ws-offer" role="status">
      <span className="ws-offer-text">The workspace changed.</span>
      <button className="btn-secondary ws-offer-open" onClick={() => setWorkspaceShown(true)}>
        Open workspace
      </button>
      <button className="ws-offer-x" aria-label="Dismiss" title="Dismiss" onClick={() => clear(convId)}>
        <CloseIcon size={12} />
      </button>
    </div>
  );
}
