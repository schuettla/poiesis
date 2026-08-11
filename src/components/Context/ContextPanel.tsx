import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";
import "./Context.css";

/** Which route a layer's content is edited from (WHY-6) — the panel explains,
 * it never becomes a second place to configure things, so this is a link out
 * rather than an inline control. `undefined` for layers nothing edits yet
 * (About you and From your files aren't built; Learned/Procedures have no
 * single edit surface). */
function editRouteFor(label: string): "self" | "settings" | undefined {
  if (label.startsWith("Soul") || label.startsWith("Remembered")) return "self";
  if (label.startsWith("Persona")) return "settings";
  return undefined;
}

const EDIT_LABEL: Record<string, string> = {
  self: "Edit in Memory",
  settings: "Edit in Personas",
};

/**
 * The shared "what I'm working from" panel (WHY-1/3): every entry point
 * (composer chip, "why this answer?", the Self row) opens the same dialog
 * through `contextPanelTarget`, so there is exactly one place this ever
 * renders from.
 */
export default function ContextPanel() {
  const target = useAppStore((s) => s.contextPanelTarget);
  const close = useAppStore((s) => s.closeContextPanel);
  const setView = useAppStore((s) => s.setView);
  const expert = useAppStore((s) => s.expert);
  const [manifest, setManifest] = useState<api.ContextManifest | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  useEffect(() => {
    if (!target) return;
    let cancelled = false;
    setManifest(null);
    api
      .contextManifest(target.conversationId, target.messageId)
      .then((m) => {
        if (!cancelled) setManifest(m);
      })
      .catch(() => {
        if (!cancelled) setManifest({ recorded: false, layers: [] });
      });
    return () => {
      cancelled = true;
    };
  }, [target]);

  useEffect(() => {
    if (!target) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [target, close]);

  if (!target) return null;

  const raw = manifest?.layers ?? [];
  // Learned/Procedures are the retrieval layers (SEM-3): worth showing only
  // once they actually surfaced something, or expert mode wants the full
  // machine visible regardless (SMP-1d exempts consequences, not internals).
  const layers = raw.filter((l) => l.always_on || l.text.trim() || expert);

  return (
    <>
      <div className="context-panel-backdrop" onClick={close} />
      <div className="context-panel" role="dialog" aria-modal="true" aria-label="What I'm working from">
        <div className="context-panel-head">
          <span>What I'm working from</span>
          <button className="btn-text" onClick={close}>
            Close
          </button>
        </div>

        {!manifest ? (
          <p className="empty-hint">Loading…</p>
        ) : !manifest.recorded ? (
          <p className="empty-hint">I didn't record this one.</p>
        ) : (
          <div role="list" aria-label="What shaped this answer">
            {layers.map((l, i) => {
              const isOpen = expanded[l.label] ?? i === 0;
              const editRoute = editRouteFor(l.label);
              return (
                <details
                  className="context-layer"
                  key={l.label}
                  open={isOpen}
                  onToggle={(e) => {
                    const nowOpen = e.currentTarget.open;
                    setExpanded((prev) => ({ ...prev, [l.label]: nowOpen }));
                  }}
                >
                  <summary role="listitem" aria-expanded={isOpen}>
                    <span className="context-layer-label">{l.label}</span>
                    <span className="context-layer-badge">
                      {l.always_on ? "in every answer" : "brought in for this question"}
                    </span>
                  </summary>
                  {l.text.trim() ? (
                    <>
                      {expert && l.sources.length > 0 && (
                        <p className="context-layer-sources">{l.sources.join(", ")}</p>
                      )}
                      <pre className="context-layer-text">{l.text}</pre>
                    </>
                  ) : (
                    <p className="context-layer-empty">nothing from here</p>
                  )}
                  {editRoute && (
                    <button
                      className="context-layer-edit"
                      onClick={() => {
                        close();
                        setView(editRoute);
                      }}
                    >
                      {EDIT_LABEL[editRoute]}
                    </button>
                  )}
                </details>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
}
