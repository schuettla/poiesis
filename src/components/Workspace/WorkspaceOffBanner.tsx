import { useState } from "react";
import { useAppStore } from "../../lib/store";
import { workspaceAvailable } from "../../lib/experiments";
import "./WorkspaceOffer.css";

/**
 * `EXP-3`: a chat that was a Workspace session, opened while the experiment is
 * off. It shows as a normal chat (the blocks it made still render inline,
 * because they are messages) and says so, with the way back. Nothing was deleted.
 */
export default function WorkspaceOffBanner() {
  const wasWorkspace = useAppStore((s) => !!s.conversations.find((c) => c.id === s.activeConversationId)?.workspace);
  // Not read yet counts as available, so this never flashes during startup.
  const off = useAppStore((s) => !workspaceAvailable(s));
  const setExperiment = useAppStore((s) => s.setExperiment);
  const [failed, setFailed] = useState(false);
  if (!wasWorkspace || !off) return null;

  return (
    <div className="ws-offer" role="status">
      <span className="ws-offer-text">
        {failed ? "I couldn't turn it on. Try Settings → Experiments." : "This was a Workspace session."}
      </span>
      <button
        className="btn-secondary ws-offer-open"
        onClick={() => setExperiment("workspace", true).catch(() => setFailed(true))}
      >
        Turn Workspace on
      </button>
    </div>
  );
}
