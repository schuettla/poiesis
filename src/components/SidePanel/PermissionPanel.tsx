import { useState } from "react";
import { useAppStore } from "../../lib/store";
import type { AppTool, CallKind, Decision, PermissionRequest, ToolView } from "../../lib/api";
import { matchedTitle, ruleLine } from "../../lib/engines";
import { ToolChips } from "../Apps/ConnectorGate";
import ImageByPath from "../Conversation/ImageByPath";
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

/** `MCG-UI-1`: the three buttons a connected app's call gets. Which words depend
 * on what the call does, because "always" means something different for each: a
 * change can be allowed for good; a send is allowed up to a daily cap. */
export function appChoices(
  kind: CallKind | undefined,
  app: string,
  cap: number | undefined,
): { decision: Decision; label: string; primary?: boolean }[] {
  if (kind === "send") {
    return [
      { decision: "once", label: "Send once", primary: true },
      { decision: "forever", label: `Up to ${cap ?? 25} a day for ${app}` },
      { decision: "deny", label: "No" },
    ];
  }
  if (kind === "read") {
    return [
      { decision: "once", label: "Allow once", primary: true },
      { decision: "forever", label: `Always let me read ${app}` },
      { decision: "deny", label: "No" },
    ];
  }
  return [
    { decision: "once", label: "Allow once", primary: true },
    { decision: "forever", label: `Always for ${app} changes` },
    { decision: "deny", label: "No" },
  ];
}

/** Values longer than this are folded until asked for. */
const FOLD_AT = 120;

function Folded({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  if (text.length <= FOLD_AT) return <>{text}</>;
  return (
    <>
      {open ? text : `${text.slice(0, FOLD_AT)}…`}{" "}
      <button className="permission-fold" onClick={() => setOpen((v) => !v)}>
        {open ? "Show less" : "Show all"}
      </button>
    </>
  );
}

/** The arguments a call would run with, as a compact key/value list. Falls back
 * to the raw text when it isn't an object (a very long payload is cut mid-JSON). */
function ArgumentList({ json }: { json: string }) {
  let parsed: unknown = null;
  try {
    parsed = JSON.parse(json);
  } catch {
    parsed = null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return <pre className="permission-diff">{json}</pre>;
  }
  const rows = Object.entries(parsed as Record<string, unknown>);
  if (rows.length === 0) return null;
  return (
    <dl className="permission-args" aria-label="What I would send">
      {rows.map(([key, value]) => (
        <div key={key} className="permission-arg">
          <dt>{key}</dt>
          <dd>
            <Folded text={typeof value === "string" ? value : JSON.stringify(value)} />
          </dd>
        </div>
      ))}
    </dl>
  );
}

/** `USE-12`: what a quick decision noticed about the call, under the question. Only
 * a flag has one: a call that looks as the gate took it gets no line. Words only; the
 * number a quick decision gave is the hover (`SEE-4`). */
function Reason({ text, hover }: { text?: string; hover?: string }) {
  return text ? (
    <p className="permission-reason" title={hover}>
      {text}
    </p>
  ) : null;
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
      <Reason text={request.reason} hover={request.reason_hover} />
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

/** The agent app's id for a name on the panel, so a link can open its settings. */
function useEngineId(name: string | undefined): string | undefined {
  return useAppStore((s) => s.engines.find((e) => e.name === name)?.id);
}

/**
 * `CLI-UI-3`: an agent app asks to run a command or change a file. The header
 * names it, because it is not Poiesis asking and not one of Poiesis's own
 * agents. The command is shown one piece to a line and an edit as a change, so
 * what is being approved can be read rather than trusted. It says plainly that
 * the agent runs outside my sandbox: its own permission system is what stands
 * between it and the folder, and this panel is one half of that.
 */
function EnginePrompt({ request, onResolve }: { request: PermissionRequest; onResolve: (d: Decision) => void }) {
  const openEngines = useAppStore((s) => s.openEngines);
  const engineId = useEngineId(request.engine);
  const argv = request.argv ?? [];
  return (
    <>
      <p className="permission-eyebrow">{request.engine} is asking</p>
      <p className="permission-summary">{request.summary}</p>
      <Reason text={request.reason} hover={request.reason_hover} />
      {argv.length > 0 && (
        <ol className="permission-argv" aria-label="The command">
          {argv.map((token, i) => (
            <li key={i}>{token}</li>
          ))}
        </ol>
      )}
      {request.diff && <pre className="permission-diff">{request.diff}</pre>}
      <p className="permission-where">It runs outside my sandbox, in {request.path}</p>
      <div className="permission-actions">
        <button className="permission-btn primary" onClick={() => onResolve("once")}>
          Allow once
        </button>
        <button className="permission-btn" onClick={() => onResolve("chat")}>
          Allow for this run
        </button>
        <button className="permission-btn deny" onClick={() => onResolve("deny")}>
          Deny
        </button>
      </div>
      {engineId && (
        <button className="permission-fold" onClick={() => openEngines(engineId)}>
          Change what {request.engine} may do
        </button>
      )}
    </>
  );
}

/**
 * `RTE-UI-2`: the lead wants to hand a job to an agent app and the mode is
 * "suggest it, I confirm". The card says which of the user's own rules made the
 * lead think so, and "Always for this" relaxes that one rule, not the agent.
 */
function HandoverPrompt({ request, onResolve }: { request: PermissionRequest; onResolve: (d: Decision) => void }) {
  const openEngines = useAppStore((s) => s.openEngines);
  const engineId = useEngineId(request.engine);
  return (
    <>
      <p className="permission-eyebrow">{request.summary}</p>
      {request.diff && <p className="permission-summary">“{request.diff}”</p>}
      {request.rule ? (
        <p
          className="permission-where"
          title={request.rule.matched != null ? matchedTitle(request.rule.matched) : undefined}
        >
          {ruleLine(request.rule)}
        </p>
      ) : (
        <p className="permission-where">You asked for it.</p>
      )}
      {/* `USE-13`: the lead cited a rule that a quick decision had scored low. */}
      {request.rule?.doubt && <p className="permission-where">{request.rule.doubt}</p>}
      <div className="permission-actions">
        <button className="permission-btn primary" onClick={() => onResolve("once")}>
          Hand it over
        </button>
        <button className="permission-btn" onClick={() => onResolve("deny")}>
          Do it yourself
        </button>
        {request.rule && (
          <button className="permission-btn" onClick={() => onResolve("forever")}>
            Always for this
          </button>
        )}
      </div>
      {engineId && (
        <button className="permission-fold" onClick={() => openEngines(engineId)}>
          Change when I use {request.engine}
        </button>
      )}
    </>
  );
}

/**
 * `USE-UI-8`: a quick decision added this question before one of Poiesis's own
 * tools ran, because the call looked irreversible or not what was asked for. The
 * line says which, and how sure. Two buttons: the call is either made or it is not,
 * and the model is told why when it is not.
 */
function RiskPrompt({ request, onResolve }: { request: PermissionRequest; onResolve: (d: Decision) => void }) {
  return (
    <>
      <p className="permission-eyebrow">Before I do this</p>
      <p className="permission-summary">{request.summary}</p>
      <Reason text={request.reason} hover={request.reason_hover} />
      {request.diff && <ArgumentList key={request.id} json={request.diff} />}
      <div className="permission-actions">
        <button className="permission-btn primary" onClick={() => onResolve("once")}>
          Go ahead
        </button>
        <button className="permission-btn deny" onClick={() => onResolve("deny")}>
          Don't do it
        </button>
      </div>
    </>
  );
}

/**
 * `USE-UI-9`: the browser agent reached a login, a captcha or a page the quick
 * check could not place, and stopped. The card shows the page, so the person can
 * see what the agent saw. **Open browser** opens a window on the agent's own
 * profile on this page; the agent carries on when it is closed. The small link is
 * for a wrong stop: the page is fine, and it is written down as such.
 */
function PagePrompt({ request, onResolve }: { request: PermissionRequest; onResolve: (d: Decision) => void }) {
  const page = request.page;
  return (
    <>
      <p className="permission-eyebrow">The browser stopped on {page?.host ?? request.path}</p>
      <p className="permission-summary">{request.summary}</p>
      {page?.screenshot && (
        <div className="permission-shot">
          <ImageByPath path={page.screenshot} alt={`The page at ${page.host}`} className="permission-shot-img" />
        </div>
      )}
      {page && <p className="permission-where">{page.note}</p>}
      <p className="permission-where">
        A window opens on this page. Sign in or finish the check there, then close it and I carry on.
      </p>
      <div className="permission-actions permission-actions-spaced">
        <button className="permission-btn primary" onClick={() => onResolve("once")}>
          Open browser
        </button>
        <button className="permission-btn" onClick={() => onResolve("deny")}>
          Skip this step
        </button>
      </div>
      <button className="permission-fold" onClick={() => onResolve("chat")}>
        This page is fine, carry on
      </button>
    </>
  );
}

/**
 * `MCG-7`: what each app tool in this request is taken to do, marked R / C / S,
 * with the chip's own menu to correct a wrong guess. Composio shows no tool list
 * per app, so this is where a tool is first seen. A correction applies from the
 * next call: the question already asked stays as it is.
 */
export function ToolKinds({ tools }: { tools: AppTool[] }) {
  const [shown, setShown] = useState(tools);
  const [changed, setChanged] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const apps = [...new Set(shown.map((t) => t.app))];
  return (
    <div className="permission-kinds">
      {apps.map((app) => {
        const mine = shown.filter((t) => t.app === app);
        const views: ToolView[] = mine.map((t) => ({
          name: t.name,
          description: "",
          input_schema: null,
          kind: t.kind,
          auto_kind: t.auto_kind,
          overridden: t.overridden,
        }));
        return (
          <div className="connector-tools" key={app}>
            <ToolChips
              connector={{ id: mine[0].connector_id, tools: views }}
              app={app}
              lead={`${mine[0].app_name}:`}
              onChanged={(name, kind) => {
                setChanged(true);
                setError(null);
                setShown((all) =>
                  all.map((t) =>
                    t.app === app && t.name === name
                      ? { ...t, kind: kind ?? t.auto_kind, overridden: kind !== null && kind !== t.auto_kind }
                      : t
                  )
                );
              }}
              onError={(m) => setError(m.replace(/^Error:\s*/, ""))}
            />
          </div>
        );
      })}
      <p className="permission-kinds-note">
        {error ?? (changed ? "Noted. It applies from the next time." : "Wrong about what one does? Click it to correct me.")}
      </p>
    </div>
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

  if (
    request.capability === "engine" ||
    request.capability === "handover" ||
    request.capability === "page" ||
    request.capability === "risk"
  ) {
    const Prompt =
      request.capability === "engine"
        ? EnginePrompt
        : request.capability === "page"
          ? PagePrompt
          : request.capability === "risk"
            ? RiskPrompt
            : HandoverPrompt;
    return (
      <div className="side-panel" role="dialog" aria-label="Permission request">
        <div className="side-panel-inner">
          <Prompt key={request.id} request={request} onResolve={(d) => resolve(request.id, d)} />
          {pending.length > 1 && (
            <p className="permission-queue">{pending.length - 1} more request(s) waiting</p>
          )}
        </div>
      </div>
    );
  }

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

  if (request.capability === "app") {
    const send = request.call_kind === "send";
    return (
      <div className={`side-panel ${send ? "outbound" : ""}`} role="dialog" aria-label="Permission request">
        <div className="side-panel-inner">
          <p className="permission-eyebrow">
            {agent
              ? `The ${agent} agent I started is asking`
              : send
                ? "Poiesis Agent wants to send something"
                : "Poiesis Agent is asking"}
          </p>
          <p className="permission-summary">{request.summary}</p>
          <Reason text={request.reason} hover={request.reason_hover} />
          {request.diff && <ArgumentList key={request.id} json={request.diff} />}
          {request.tools && request.tools.length > 0 && <ToolKinds key={`kinds-${request.id}`} tools={request.tools} />}
          <div className="permission-actions">
            {appChoices(request.call_kind, request.path, request.cap).map((c) => (
              <button
                key={c.decision}
                className={`permission-btn ${c.primary ? "primary" : ""} ${c.decision === "deny" ? "deny" : ""}`}
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
        <Reason text={request.reason} hover={request.reason_hover} />
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
