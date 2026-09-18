import { useState } from "react";
import { useAppStore } from "../../lib/store";
import type { Decision, PermissionRequest } from "../../lib/api";
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

/** `BRW-3`/`SYS-1`: a one-off capability consent — visiting a domain, taking a
 * screenshot, launching an app. Plain three-way, no "for this chat" middle
 * ground — "Always" is a per-capability standing answer, not a session one. */
function capabilityChoices(kind: string, target: string): { decision: Decision; label: string; primary?: boolean }[] {
  const always = kind === "screen" ? "Always allow this" : `Always allow ${target}`;
  return [
    { decision: "once", label: "Once", primary: true },
    { decision: "forever", label: always },
    { decision: "deny", label: "No" },
  ];
}

function minutes(secs: number | undefined): string {
  if (!secs) return "";
  return secs % 60 === 0 ? `${secs / 60} min` : `${secs} s`;
}

/**
 * `COD-UI-4`: running something in a project. Two shapes, on purpose.
 *
 * A declared task reads as something the user recognises: a plain question,
 * the command it runs on one line, where and for how long. A free-form command
 * reads as what it is: the program and every argument on a line of its own, so
 * a long or strange command cannot hide in a wall of text. Collapsing the two
 * into one generic prompt is how people learn to click through.
 */
function ExecutionPrompt({
  request,
  agent,
  onResolve,
}: {
  request: PermissionRequest;
  agent?: string;
  onResolve: (decision: Decision) => void;
}) {
  const [remember, setRemember] = useState(false);
  const isTask = request.capability === "task";
  const argv = request.argv ?? [];
  return (
    <>
      <p className="permission-eyebrow">
        {agent ? `The ${agent} agent I started is asking` : isTask ? "Run a project task" : "Run a command"}
      </p>
      <p className="permission-summary">{request.summary}</p>
      {isTask ? (
        <code className="permission-command">{argv.join(" ")}</code>
      ) : (
        <ol className="permission-argv" aria-label="The command, one argument per line">
          {argv.map((token, i) => (
            <li key={i}>{token}</li>
          ))}
        </ol>
      )}
      <p className="permission-where">
        in {request.project}
        {request.timeout_secs ? ` · stopped after ${minutes(request.timeout_secs)}` : ""}
      </p>
      <p className="permission-path">{request.path}</p>
      <label className="permission-remember">
        <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
        {isTask ? (
          <span>Always allow this task in this project</span>
        ) : (
          <span>
            Always allow <code>{request.remember}</code> in this project
          </span>
        )}
      </label>
      <div className="permission-actions">
        <button className="permission-btn primary" onClick={() => onResolve(remember ? "forever" : "once")}>
          Run
        </button>
        <button className="permission-btn deny" onClick={() => onResolve("deny")}>
          Don't run
        </button>
      </div>
    </>
  );
}

/** Calm side-panel consent prompt (PRD §5.4.4). Shows the oldest pending
 *  request; the agent loop is paused awaiting the answer. */
export default function PermissionPanel() {
  const pending = useAppStore((s) => s.pendingPermissions);
  const resolve = useAppStore((s) => s.resolvePermission);
  const permissionAgents = useAppStore((s) => s.permissionAgents);
  const request = pending[0];
  if (!request) return null;
  // `SUB-UI-4`: a prompt from a delegated child has to say whose it is. "Poiesis
  // Agent is asking" is a lie when the thing asking is one of three agents the
  // lead started, and the answer is a different one depending on which.
  const agent = permissionAgents[request.id];

  if (request.capability === "task" || request.capability === "command") {
    return (
      <div className="side-panel" role="dialog" aria-label="Permission request">
        <div className="side-panel-inner">
          {/* Keyed by request, so "always allow" never carries over from the
              prompt before it. */}
          <ExecutionPrompt
            key={request.id}
            request={request}
            agent={agent}
            onResolve={(d) => resolve(request.id, d)}
          />
          {pending.length > 1 && (
            <p className="permission-queue">{pending.length - 1} more request(s) waiting</p>
          )}
        </div>
      </div>
    );
  }

  const inFolder = request.in_folder;
  const capability = request.capability;
  const choices = capability
    ? capabilityChoices(capability, request.path)
    : inFolder
      ? OPERATION_CHOICES
      : SCOPE_CHOICES;

  return (
    <div className="side-panel" role="dialog" aria-label="Permission request">
      <div className="side-panel-inner">
        <p className="permission-eyebrow">
          {!capability && inFolder
            ? "Review this change"
            : agent
              ? `The ${agent} agent I started is asking`
              : "Poiesis Agent is asking"}
        </p>
        {/* A capability's summary is already first-person and carries the
            detail that matters (which document, which domain) — there's
            nothing for the panel to restate. */}
        <p className="permission-summary">{request.summary}</p>
        {!capability && (
          <p className="permission-path">
            {request.path}
            {!inFolder && ` · ${request.mode === "read-write" ? "read & write" : "read only"}`}
          </p>
        )}
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
