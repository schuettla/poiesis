import { useAppStore } from "../../lib/store";
import { TIER_LABEL, tokensLabel, windowSteps } from "../../lib/tier";
import type { Model, ModelTier } from "../../lib/types";
import Select from "../Select/Select";
import "./TierControls.css";

/** `TIER-UI-3`: what a model may do. One choice per model; Auto follows the
 * model's own facts, and the other three pin it. */
export function FeaturesSelect({ model }: { model: Model }) {
  const setTier = useAppStore((s) => s.setModelTier);
  const auto = model.autoTier ? `Auto · ${TIER_LABEL[model.autoTier]}` : "Auto";
  return (
    <label className="tier-control">
      <span className="tier-control-label">Features</span>
      <Select
        label={`Features for ${model.name}`}
        value={model.tierSet ?? "auto"}
        options={[
          { value: "auto", label: auto },
          { value: "full", label: TIER_LABEL.full },
          { value: "basic", label: TIER_LABEL.basic },
          { value: "chat", label: TIER_LABEL.chat },
        ]}
        onChange={(v) => void setTier(model, v === "auto" ? null : (v as ModelTier))}
      />
    </label>
  );
}

/** `TIER-UI-3`: how much a local model holds in memory at once. Only the steps
 * the model was trained for are offered. */
export function WindowSelect({ model }: { model: Model }) {
  const setWindow = useAppStore((s) => s.setModelWindow);
  // Compared with what the load was meant to use, not what the engine reports:
  // a start that had to fall back to less memory is not a change still waiting.
  const meant = useAppStore((s) => (s.loadedModelId === model.id ? s.loadedWindow : null));
  const waiting = meant !== null && model.window !== undefined && meant !== model.window;

  const auto = model.autoWindow ? `Auto · ${tokensLabel(model.autoWindow)}` : "Auto";
  return (
    <label className="tier-control">
      <span className="tier-control-label">Memory window</span>
      <Select
        label={`Memory window for ${model.name}`}
        value={model.windowSet ? String(model.windowSet) : "auto"}
        options={[
          { value: "auto", label: auto },
          ...windowSteps(model).map((n) => ({ value: String(n), label: tokensLabel(n) })),
        ]}
        onChange={(v) => void setWindow(model, v === "auto" ? null : Number(v))}
      />
      {waiting && <span className="tier-control-note">Used next time the model loads</span>}
    </label>
  );
}

/** The strip under a chat model's row. */
export function TierControls({ model }: { model: Model }) {
  return (
    <div className="tier-controls">
      <FeaturesSelect model={model} />
      {model.provenance === "local" && <WindowSelect model={model} />}
    </div>
  );
}
