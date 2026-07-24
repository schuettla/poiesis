import { useAppStore } from "../../lib/store";
import type { Decision } from "../../lib/api";
import "./PermissionPanel.css";

const CHOICES: { decision: Decision; label: string; primary?: boolean }[] = [
  { decision: "once", label: "Allow once", primary: true },
  { decision: "chat", label: "Allow for this chat" },
  { decision: "forever", label: "Always allow this folder" },
  { decision: "deny", label: "Deny" },
];

/** Calm side-panel consent prompt (PRD §5.4.4). Shows the oldest pending
 *  request; the agent loop is paused awaiting the answer. */
export default function PermissionPanel() {
  const pending = useAppStore((s) => s.pendingPermissions);
  const resolve = useAppStore((s) => s.resolvePermission);
  const request = pending[0];
  if (!request) return null;

  return (
    <div className="side-panel" role="dialog" aria-label="Permission request">
      <div className="side-panel-inner">
        <p className="permission-eyebrow">Poiesis is asking</p>
        <p className="permission-summary">{request.summary}</p>
        <p className="permission-path">
          {request.path} · {request.mode === "read-write" ? "read & write" : "read only"}
        </p>
        <div className="permission-actions">
          {CHOICES.map((c) => (
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
