import { useAppStore } from "../../lib/store";
import { inTauri } from "../../lib/api";
import ModelPicker from "../ModelPicker/ModelPicker";
import "./TopBar.css";

/**
 * Makes the local runtime visible: a model isn't usable until llama-server is
 * actually running it. Shows starting / ready / idle so it's never ambiguous
 * that an engine must be in place to chat.
 */
function EngineStatus() {
  const engineReady = useAppStore((s) => s.engineReady);
  const loadingModel = useAppStore((s) => s.loadingModel);
  if (!inTauri()) return null;

  let state = "idle";
  let label = "Engine idle";
  if (loadingModel) {
    state = "starting";
    label = loadingModel.label || "Starting engine…";
  } else if (engineReady) {
    state = "ready";
    label = "Engine ready";
  }

  return (
    <div
      className={`engine-status ${state}`}
      title="The local model engine (llama-server) runs on your PC to power chats. It starts automatically when you use a model."
      aria-label={`Local engine: ${label}`}
    >
      <span className="engine-dot" aria-hidden="true" />
      <span className="engine-label">{label}</span>
    </div>
  );
}

export default function TopBar() {
  return (
    <div className="topbar">
      <div className="brand">Poiesis</div>
      <div className="topbar-right">
        <EngineStatus />
        <ModelPicker />
      </div>
    </div>
  );
}
