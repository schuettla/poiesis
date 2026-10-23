import { useState } from "react";
import {
  setConnectorPolicy,
  setToolKind,
  type CallKind,
  type ConnectorPolicy,
  type ConnectorView,
  type ToolView,
} from "../../lib/api";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import "./ConnectorGate.css";

const RANK: Record<CallKind, number> = { read: 0, change: 1, send: 2 };
const KINDS: CallKind[] = ["read", "change", "send"];
const MARK: Record<CallKind, string> = { read: "R", change: "C", send: "S" };
const WORD: Record<CallKind, string> = { read: "reads", change: "changes", send: "sends" };

/** `MCG-7`: a tool is loosened when it is held to less than its name says. */
export function isLoosened(tool: Pick<ToolView, "kind" | "auto_kind">): boolean {
  return RANK[tool.kind] < RANK[tool.auto_kind];
}

/** `MCG-UI-2`: the collapsed line, in the person's own terms. */
export function policyLine(p: ConnectorPolicy): string {
  const read = p.read_policy === "ask" ? "ask" : "without asking";
  const change = p.change_policy === "allow" ? "always allowed" : "ask";
  const send = p.send_policy === "allow" ? `up to ${p.send_cap} a day` : "ask";
  return `Read: ${read} · Change: ${change} · Send: ${send}`;
}

function Segments<T extends string>({
  label,
  value,
  options,
  onPick,
}: {
  label: string;
  value: T;
  options: { value: T; text: string }[];
  onPick: (v: T) => void;
}) {
  return (
    <div className="gate-row">
      <span className="gate-row-label">{label}</span>
      <div className="transport-toggle gate-segments" role="group" aria-label={label}>
        {options.map((o) => (
          <button
            key={o.value}
            className={`seg ${o.value === value ? "on" : ""}`}
            aria-pressed={o.value === value}
            onClick={() => o.value !== value && onPick(o.value)}
          >
            {o.text}
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * `MCG-UI-2`: what a connector's tools may do without asking. One quiet line
 * until opened; the controls are three small choices and the daily send cap.
 */
export function WhatIMayDo({
  connector,
  app,
  onChanged,
  onError,
}: {
  connector: Pick<ConnectorView, "id" | "policy">;
  /** One Composio app's slug, when the policy is that app's and not the connector's. */
  app?: string;
  onChanged: () => void | Promise<void>;
  onError: (message: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [capText, setCapText] = useState<string | null>(null);
  const p = connector.policy;

  async function save(patch: Partial<Omit<ConnectorPolicy, "sent_today">>) {
    const { sent_today: _sent, ...current } = p;
    void _sent;
    try {
      await setConnectorPolicy(connector.id, { ...current, ...patch }, app);
      await onChanged();
    } catch (e) {
      onError(String(e));
    }
  }

  function commitCap() {
    if (capText === null) return;
    const n = Math.round(Number(capText));
    setCapText(null);
    if (!Number.isFinite(n) || n === p.send_cap) return;
    void save({ send_cap: Math.min(1000, Math.max(1, n)) });
  }

  return (
    <div className="gate">
      <button className="gate-toggle" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <span className="gate-title">What I may do</span>
        <span className="gate-line">{policyLine(p)}</span>
      </button>
      {open && (
        <div className="gate-body">
          <Segments
            label="Read"
            value={p.read_policy}
            options={[
              { value: "run", text: "Without asking" },
              { value: "ask", text: "Ask" },
            ]}
            onPick={(v) => void save({ read_policy: v })}
          />
          <Segments
            label="Change"
            value={p.change_policy}
            options={[
              { value: "ask", text: "Ask" },
              { value: "allow", text: "Always allowed" },
            ]}
            onPick={(v) => void save({ change_policy: v })}
          />
          <Segments
            label="Send"
            value={p.send_policy}
            options={[
              { value: "ask", text: "Ask" },
              { value: "allow", text: "Up to a daily limit" },
            ]}
            onPick={(v) => void save({ send_policy: v })}
          />
          <label className="gate-row">
            <span className="gate-row-label">Daily limit</span>
            <input
              className="field-input gate-cap"
              type="number"
              min={1}
              max={1000}
              value={capText ?? String(p.send_cap)}
              onChange={(e) => setCapText(e.target.value)}
              onBlur={commitCap}
              onKeyDown={(e) => e.key === "Enter" && commitCap()}
            />
            <span className="gate-cap-note">
              {p.sent_today} of {p.send_cap} used today. At the limit I ask again.
            </span>
          </label>
        </div>
      )}
    </div>
  );
}

/**
 * The connector's tools, each marked R / C / S for what it does, so the
 * classification can be inspected. A wrong guess is fixed from the chip itself
 * (`MCG-7`): stricter is one click, looser says what it gives up first.
 *
 * A Composio app has no tool list of its own, so it passes the tools I can name
 * for it (those that ran, or those in an approval) with its `app` slug.
 */
export function ToolChips({
  connector,
  app,
  lead,
  onChanged,
  onError,
}: {
  connector: Pick<ConnectorView, "id" | "tools">;
  /** A Composio app's slug: the corrections are kept with that app. */
  app?: string;
  /** Said before the chips in place of "N tools:". */
  lead?: string;
  /** Told which tool changed and to what (`null`: back to its name). */
  onChanged: (tool: string, kind: CallKind | null) => void | Promise<void>;
  onError: (message: string) => void;
}) {
  const [all, setAll] = useState(false);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [pending, setPending] = useState<{ tool: ToolView; kind: CallKind | null } | null>(null);
  const tools = connector.tools;
  const shown = all ? tools : tools.slice(0, 8);

  async function apply(tool: ToolView, kind: CallKind | null) {
    setMenuFor(null);
    setPending(null);
    try {
      await setToolKind(connector.id, tool.name, kind, app);
      await onChanged(tool.name, kind);
    } catch (e) {
      onError(String(e));
    }
  }

  /** Stricter applies at once; looser asks first. */
  function choose(tool: ToolView, kind: CallKind | null) {
    const target = kind ?? tool.auto_kind;
    if (RANK[target] < RANK[tool.kind]) setPending({ tool, kind });
    else void apply(tool, kind);
  }

  if (tools.length === 0) return <span className="tool-empty">No tools discovered</span>;

  return (
    <>
      <span className="tool-count">
        {lead ?? `${tools.length} ${tools.length === 1 ? "tool" : "tools"}:`}
      </span>
      {shown.map((t) => (
        <span className="tool-chip-wrap" key={t.name}>
          <button
            className={`tool-chip tool-chip-button kind-${t.kind} ${isLoosened(t) ? "loosened" : ""}`}
            aria-haspopup="menu"
            aria-expanded={menuFor === t.name}
            title={`${t.name} ${WORD[t.kind]}${t.overridden ? ` (you set this; its name says ${WORD[t.auto_kind]})` : ""}. ${t.description}`}
            onClick={() => setMenuFor(menuFor === t.name ? null : t.name)}
          >
            <b className="tool-kind" aria-label={WORD[t.kind]}>
              {MARK[t.kind]}
            </b>
            {t.name}
            {isLoosened(t) && <span className="tool-loosened">loosened</span>}
          </button>
          {menuFor === t.name && (
            <div className="tool-menu" role="menu">
              {KINDS.map((k) => (
                <button
                  key={k}
                  role="menuitemradio"
                  aria-checked={k === t.kind}
                  className="tool-menu-item"
                  disabled={k === t.kind}
                  onClick={() => choose(t, k === t.auto_kind ? null : k)}
                >
                  {k === t.kind ? "✓ " : ""}Treat as {k}
                </button>
              ))}
              {t.overridden && (
                <button role="menuitem" className="tool-menu-item" onClick={() => choose(t, null)}>
                  Back to what its name says
                </button>
              )}
            </div>
          )}
        </span>
      ))}
      {tools.length > 8 && (
        <button className="tool-chip tool-chip-button" onClick={() => setAll((v) => !v)}>
          {all ? "Show fewer" : `+${tools.length - 8}`}
        </button>
      )}
      {pending && (
        <ConfirmDialog
          title="Stop asking?"
          body={`I'll stop asking before ${pending.tool.name} runs.`}
          confirmLabel="Stop asking"
          onConfirm={() => void apply(pending.tool, pending.kind)}
          onCancel={() => setPending(null)}
        />
      )}
    </>
  );
}
