import { useEffect, useRef, useState } from "react";
import { useSelectedModel } from "../../lib/store";
import { isMediaModel } from "../../lib/modelPrefs";
import { ModelList } from "../ModelPicker/ModelPicker";
import { ChevronIcon } from "../Icons/Icons";
import { EFFORTS, Meter, useEffort } from "./EffortPicker";
import { useContextUsage } from "./ContextMeter";
import "../ModelPicker/ModelPicker.css";
import "./EffortPicker.css";
import "./ModelControl.css";

/** The word the trigger spends on effort. The model is the decision; this only
 * qualifies it, so it gets one short word, not a sentence. */
const SHORT: Record<string, string> = {
  off: "No thinking",
  low: "Brief",
  medium: "Think",
  high: "Hard",
  provider: "Default",
};

const compact = (n: number) =>
  n >= 1000 ? `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}k` : String(n);

/** A small ring filled to how much of the window is used — drawn in CSS, since
 * it is a reading, not an icon. */
function Ring({ fill }: { fill: number }) {
  return (
    <span
      className={`mc-ring ${fill >= 0.85 ? "high" : ""}`}
      style={{ "--mc-fill": fill } as React.CSSProperties}
      aria-hidden="true"
    />
  );
}

/**
 * Which model answers, how hard it thinks, and how full its window is — one
 * control under the composer instead of three. They are all properties of the
 * answer you are about to ask for, so they open together: the model list on
 * top, thinking and context below it, nearest the trigger.
 *
 * The context ring stays silent below half (CTX-UI-1); inside the panel the
 * number is always there for whoever goes looking.
 */
export default function ModelControl({
  draft,
  effortChip,
  onEffortPick,
}: {
  draft: string;
  /** `DEF-1`: a `/effort` chip for the next message only. */
  effortChip?: string;
  onEffortPick?: () => void;
}) {
  const selected = useSelectedModel();
  const media = isMediaModel(selected);
  const { current, standing, chipped, choose } = useEffort(effortChip, onEffortPick);
  const usage = useContextUsage(draft);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const showRing = !!usage && usage.fill >= 0.5;
  const effortLabel = `${current.label}${chipped ? " (just this message)" : ""}`;

  return (
    <div className="model-control" ref={ref}>
      <button
        className="mc-trigger"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`Model: ${selected.name}${media ? "" : `, ${effortLabel}`}${showRing ? `, context ${usage!.label}` : ""}`}
        title={[
          `Model: ${selected.name}`,
          media ? null : `Thinking: ${effortLabel}`,
          usage ? `Context: ${usage.label}` : null,
        ]
          .filter(Boolean)
          .join("\n")}
        onClick={() => setOpen((o) => !o)}
      >
        {showRing && <Ring fill={usage!.fill} />}
        <span className={`provenance-dot ${selected.provenance}`} aria-hidden="true" />
        <span className="mc-model">{selected.name}</span>
        {!media && (
          <span className="mc-effort">
            {SHORT[current.value]}
            {chipped && <span className="effort-once"> ·</span>}
          </span>
        )}
        <span className="caret" aria-hidden="true">
          <ChevronIcon dir="up" size={10} strokeWidth={1.8} />
        </span>
      </button>

      {open && (
        <div className="mc-panel" role="dialog" aria-label="Model, thinking and context">
          <div className="mc-list" role="listbox" aria-label="Choose a model">
            <ModelList onDone={() => setOpen(false)} />
          </div>

          {/* Hidden for an image or video model: there is nothing to think about. */}
          {!media && (
            <div className="mc-section">
              <div className="mc-section-head">
                <span>Thinking</span>
                <span className="mc-section-hint">
                  {chipped ? `Just this message · default ${standing.label.toLowerCase()}` : current.hint}
                </span>
              </div>
              <div className="mc-efforts" role="radiogroup" aria-label="How hard to think">
                {EFFORTS.map((e) => (
                  <button
                    key={e.value}
                    className={`mc-effort-opt ${e.value === current.value ? "selected" : ""}`}
                    role="radio"
                    aria-checked={e.value === current.value}
                    title={`${e.label}. ${e.hint}`}
                    onClick={() => choose(e)}
                  >
                    <Meter bars={e.bars} />
                    <span>{SHORT[e.value]}</span>
                  </button>
                ))}
              </div>
            </div>
          )}

          {usage && (
            <div className="mc-section mc-context" title={usage.label}>
              <div className="mc-section-head">
                <span>Context</span>
                <span className="mc-section-hint">
                  ~{compact(usage.used)} / {compact(usage.budget)} tokens
                </span>
              </div>
              <div className="mc-bar">
                <div
                  className={`mc-bar-fill ${usage.fill >= 0.85 ? "high" : ""}`}
                  style={{ width: `${Math.max(1, Math.round(usage.fill * 100))}%` }}
                />
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
