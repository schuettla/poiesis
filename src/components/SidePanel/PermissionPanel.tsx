import { useAppStore } from "../../lib/store";
import type { Decision } from "../../lib/api";
import "./PermissionPanel.css";

/** Widening scope: "may I reach into this folder at all?" */
const SCOPE_CHOICES: { decision: Decision; label: string; primary?: boolean }[] = [
  { decision: "once", label: "Allow once", primary: true },
  { decision: "chat", label: "Allow for this chat" },
  { decision: "forever", label: "Always allow this folder" },
  { decision: "deny", label: "Deny" },
];

/** Confirming one operation inside the folder that's already attached. Scope is
 * settled, so the four-way choice would be answering a question nobody asked —
 * what's left is this change, and whether to stop being asked. */
const OPERATION_CHOICES: { decision: Decision; label: string; primary?: boolean }[] = [
  { decision: "once", label: "Allow", primary: true },
  { decision: "deny", label: "Deny" },
  { decision: "forever", label: "Don't ask again in this folder" },
];

/** Calm side-panel consent prompt (PRD §5.4.4). Shows the oldest pending
 *  request; the agent loop is paused awaiting the answer. */
export default function PermissionPanel() {
  const pending = useAppStore((s) => s.pendingPermissions);
  const resolve = useAppStore((s) => s.resolvePermission);
  const request = pending[0];
  if (!request) return null;

  const inFolder = request.in_folder;
  const choices = inFolder ? OPERATION_CHOICES : SCOPE_CHOICES;

  return (
    <div className="side-panel" role="dialog" aria-label="Permission request">
      <div className="side-panel-inner">
        <p className="permission-eyebrow">
          {inFolder ? "Review this change" : "Poiesis Agent is asking"}
        </p>
        <p className="permission-summary">{request.summary}</p>
        <p className="permission-path">
          {request.path}
          {!inFolder && ` · ${request.mode === "read-write" ? "read & write" : "read only"}`}
        </p>
        {/* Approving an edit should be reviewing a change, not trusting a
            sentence about one. */}
        {request.diff && <pre className="permission-diff">{request.diff}</pre>}
        <div className="permission-actions">
          {choices.map((c) => (
            <button
              key={c.decision}
              className={`permission-btn ${c.primary ? "primary" : ""} ${
                c.decision === "deny" ? "deny" : ""
              }`}
              onClick={() => resolve(request.id, c.decision)}
            >
              {c.label}
            </button>
          ))}
        </div>
        {pending.length > 1 && (
          <p className="permission-queue">{pending.length - 1} more request(s) waiting</p>
        )}
      </div>
    </div>
  );
}
