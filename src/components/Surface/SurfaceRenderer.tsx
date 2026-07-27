import { useEffect, useRef } from "react";
import { Component, type ReactNode } from "react";
import type { UINode } from "../../lib/types";
import "./Surface.css";

/** The dynamic Workspace surface: one recursive renderer that walks the
 * interface tree the agent composed with `render_ui`. There are no prebuilt
 * widgets here — every interface is an arrangement of ~14 primitives, so the
 * agent can project any UI (dashboard, board, wizard, picker) it needs.
 * All fields are treated leniently; an unknown node type degrades to its
 * children or a raw dump, never a crash. */

export interface SurfaceCtx {
  /** The user's bound values (inputs, choices, toggles), keyed by `bind`. */
  state: Record<string, unknown>;
  /** True while a turn is streaming — action nodes disable, binds stay live. */
  disabled: boolean;
  /** A `bind` field changed: persist locally, no model turn. */
  onBind: (key: string, value: unknown) => void;
  /** An `action` node was activated: send a turn to the agent. */
  onAction: (action: string, payload: Record<string, unknown>, humanText: string) => void;
}

export default function SurfaceRenderer({ tree, ctx }: { tree: UINode; ctx: SurfaceCtx }) {
  // PRES-7: a surface hatches the first time it appears — including one seeded
  // from a recipe, so a workspace born from a procedure visibly comes to life.
  // Only the first render: revisions should feel continuous, not restarted.
  const hatched = useRef(false);
  const entering = !hatched.current;
  useEffect(() => {
    hatched.current = true;
  }, []);

  return (
    <div
      className={`surface ${entering ? "surface-enter" : ""}`}
      role="region"
      aria-label="Workspace surface"
    >
      <SurfaceErrorBoundary tree={tree}>
        <Node node={tree} ctx={ctx} />
      </SurfaceErrorBoundary>
    </div>
  );
}

// ---- lenient field readers ----

const str = (v: unknown): string => (typeof v === "string" ? v : typeof v === "number" ? String(v) : "");
const num = (v: unknown, fallback = 0): number => (typeof v === "number" && Number.isFinite(v) ? v : fallback);
const obj = (v: unknown): Record<string, unknown> =>
  v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
const arr = (v: unknown): UINode[] => (Array.isArray(v) ? (v as UINode[]) : []);

function Children({ node, ctx }: { node: UINode; ctx: SurfaceCtx }) {
  return (
    <>
      {arr(node.children).map((child, i) => (
        <Node key={str(child.id) || i} node={child} ctx={ctx} />
      ))}
    </>
  );
}

function Node({ node, ctx }: { node: UINode; ctx: SurfaceCtx }) {
  if (!node || typeof node !== "object") return null;
  switch (node.type) {
    case "stack": {
      const row = str(node.direction) === "row";
      return (
        <div className={`sf-stack${row ? " row" : ""}`} data-node={str(node.id) || undefined}>
          <Children node={node} ctx={ctx} />
        </div>
      );
    }
    case "grid": {
      const cols = Math.max(1, Math.min(6, num(node.columns, 2)));
      return (
        <div
          className="sf-grid"
          style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}
          data-node={str(node.id) || undefined}
        >
          <Children node={node} ctx={ctx} />
        </div>
      );
    }
    case "section":
      return (
        <section className="sf-section" data-node={str(node.id) || undefined}>
          {str(node.title) && <header className="sf-section-title">{str(node.title)}</header>}
          <div className="sf-section-body">
            <Children node={node} ctx={ctx} />
          </div>
        </section>
      );
    case "divider":
      return <hr className="sf-rule" />;

    case "text": {
      const variant = str(node.variant) || "body";
      return <p className={`sf-text ${variant}`}>{str(node.value)}</p>;
    }
    case "metric": {
      const intent = str(node.intent);
      return (
        <div className="sf-metric">
          <span className="sf-metric-label">{str(node.label)}</span>
          <span className={`sf-metric-value${intent ? ` ${intent}` : ""}`}>
            {str(node.value)}
            {str(node.unit) && <span className="sf-metric-unit"> {str(node.unit)}</span>}
          </span>
          {str(node.delta) && <span className="sf-metric-delta">{str(node.delta)}</span>}
        </div>
      );
    }
    case "badge": {
      const intent = str(node.intent);
      return <span className={`sf-badge${intent ? ` ${intent}` : ""}`}>{str(node.value)}</span>;
    }
    case "progress": {
      const max = num(node.max, 100) || 100;
      const value = Math.max(0, Math.min(max, num(node.value)));
      return (
        <div className="sf-progress" role="progressbar" aria-valuenow={value} aria-valuemax={max}>
          {str(node.label) && <span className="sf-progress-label">{str(node.label)}</span>}
          <span className="sf-progress-track">
            <span className="sf-progress-fill" style={{ width: `${(value / max) * 100}%` }} />
          </span>
          <span className="sf-progress-nums">
            {value} / {max}
          </span>
        </div>
      );
    }
    case "link":
      return (
        <a className="sf-link" href={str(node.url)} target="_blank" rel="noreferrer">
          {str(node.value) || str(node.url)} ↗
        </a>
      );

    case "item": {
      const action = str(node.action);
      const title = str(node.title);
      const body = (
        <>
          <span className="sf-item-main">
            <span className="sf-item-title">{title}</span>
            {str(node.subtitle) && <span className="sf-item-subtitle">{str(node.subtitle)}</span>}
          </span>
          {str(node.meta) && <span className="sf-item-meta">{str(node.meta)}</span>}
        </>
      );
      if (!action) {
        return <div className={`sf-item${node.selected ? " selected" : ""}`}>{body}</div>;
      }
      return (
        <button
          className={`sf-item actionable${node.selected ? " selected" : ""}`}
          disabled={ctx.disabled}
          onClick={() =>
            ctx.onAction(
              action,
              { item: title, ...obj(node.payload) },
              `Selected “${title}” in the workspace.`
            )
          }
        >
          {body}
        </button>
      );
    }
    case "choice": {
      const bind = str(node.bind);
      const multi = node.multi === true;
      const current = ctx.state[bind];
      const selected = multi
        ? new Set(Array.isArray(current) ? (current as unknown[]).map(String) : [])
        : new Set(current != null ? [String(current)] : []);
      const options = arr(node.options);
      return (
        <div className="sf-choice" role={multi ? "group" : "radiogroup"}>
          {options.map((o, i) => {
            const oid = str(o.id) || String(i);
            const isOn = selected.has(oid);
            return (
              <button
                key={oid}
                className={`sf-choice-option${isOn ? " on" : ""}`}
                role={multi ? "checkbox" : "radio"}
                aria-checked={isOn}
                onClick={() => {
                  if (!bind) return;
                  if (multi) {
                    const next = new Set(selected);
                    if (isOn) next.delete(oid);
                    else next.add(oid);
                    ctx.onBind(bind, [...next]);
                  } else {
                    ctx.onBind(bind, isOn ? null : oid);
                  }
                }}
              >
                <span className="sf-choice-dot" aria-hidden="true">
                  {isOn ? "●" : "○"}
                </span>
                <span className="sf-choice-label">{str(o.label) || oid}</span>
                {str(o.detail) && <span className="sf-choice-detail">{str(o.detail)}</span>}
              </button>
            );
          })}
        </div>
      );
    }
    case "input": {
      const bind = str(node.bind);
      const type = str(node.type) === "number" ? "number" : "text";
      const value = ctx.state[bind];
      return (
        <label className="sf-input">
          {str(node.label) && <span className="sf-input-label">{str(node.label)}</span>}
          <input
            type={type}
            placeholder={str(node.placeholder) || undefined}
            value={value == null ? "" : String(value)}
            onChange={(e) => {
              if (!bind) return;
              const raw = e.target.value;
              ctx.onBind(bind, type === "number" && raw !== "" ? Number(raw) : raw);
            }}
          />
        </label>
      );
    }
    case "toggle": {
      const bind = str(node.bind);
      const on = ctx.state[bind] === true;
      return (
        <button
          className={`sf-toggle${on ? " on" : ""}`}
          role="switch"
          aria-checked={on}
          onClick={() => bind && ctx.onBind(bind, !on)}
        >
          <span className="sf-toggle-box" aria-hidden="true">
            {on ? "✓" : ""}
          </span>
          <span>{str(node.label)}</span>
        </button>
      );
    }
    case "button": {
      const action = str(node.action);
      const label = str(node.label) || action;
      return (
        <button
          className={`sf-button${str(node.style) === "primary" ? " primary" : ""}`}
          disabled={ctx.disabled || !action}
          onClick={() =>
            ctx.onAction(action, obj(node.payload), `Chose “${label}” in the workspace.`)
          }
        >
          {label}
        </button>
      );
    }

    case "form": {
      // A composite the model reaches for naturally: a list of typed fields.
      // Each field maps onto a leaf primitive (choice/input/toggle) bound by
      // its own id, so the whole form's values travel with the next action.
      const fields = arr(node.fields);
      return (
        <div className="sf-form" data-node={str(node.id) || undefined}>
          {fields.map((f, i) => {
            const fid = str(f.id) || String(i);
            const label = str(f.label) || fid;
            const req = f.required === true ? " *" : "";
            const ftype = str(f.type);
            if (ftype === "select" || ftype === "multiselect") {
              return (
                <div className="sf-field" key={fid}>
                  <span className="sf-input-label">{label}{req}</span>
                  <Node
                    node={{ type: "choice", bind: fid, multi: ftype === "multiselect", options: f.options } as UINode}
                    ctx={ctx}
                  />
                </div>
              );
            }
            if (ftype === "toggle" || ftype === "boolean") {
              return (
                <div className="sf-field" key={fid}>
                  <Node node={{ type: "toggle", bind: fid, label } as UINode} ctx={ctx} />
                </div>
              );
            }
            const isNum = ftype === "number";
            const value = ctx.state[fid];
            return (
              <label className="sf-input sf-field" key={fid}>
                <span className="sf-input-label">{label}{req}</span>
                <input
                  type={isNum ? "number" : "text"}
                  placeholder={str(f.placeholder) || undefined}
                  value={value == null ? "" : String(value)}
                  onChange={(e) => {
                    const raw = e.target.value;
                    ctx.onBind(fid, isNum && raw !== "" ? Number(raw) : raw);
                  }}
                />
              </label>
            );
          })}
          {str(node.submit_label) && (
            <Node
              node={{
                type: "button",
                label: str(node.submit_label),
                action: str(node.action) || "submit",
                style: "primary",
                payload: { form: str(node.id) },
              } as UINode}
              ctx={ctx}
            />
          )}
        </div>
      );
    }

    default:
      // Unknown primitive: render its children if it has any (forward
      // compatible with layout-ish types), else a quiet raw dump.
      if (arr(node.children).length) {
        return (
          <div className="sf-stack">
            <Children node={node} ctx={ctx} />
          </div>
        );
      }
      return (
        <details className="sf-raw">
          <summary>{str(node.type) || "node"}</summary>
          <pre>{JSON.stringify(node, null, 2)}</pre>
        </details>
      );
  }
}

/** A render error in one composed tree must not take down the workspace. */
class SurfaceErrorBoundary extends Component<
  { tree: UINode; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidUpdate(prev: { tree: UINode }) {
    // A new tree from the agent gets a fresh chance to render.
    if (prev.tree !== this.props.tree && this.state.failed) {
      this.setState({ failed: false });
    }
  }
  render() {
    if (this.state.failed) {
      return (
        <details className="sf-raw" open>
          <summary>The surface couldn't be rendered — raw tree</summary>
          <pre>{JSON.stringify(this.props.tree, null, 2)}</pre>
        </details>
      );
    }
    return this.props.children;
  }
}
