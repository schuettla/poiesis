import { useAppStore } from "../../lib/store";
import { inTauri } from "../../lib/api";
import ModelPicker from "../ModelPicker/ModelPicker";
import PoiesisMark from "../Mark/PoiesisMark";
import "./TopBar.css";

/** Sidebar collapse/expand toggle, lives in the header so it reads as a
 * property of the whole window rather than of the rail itself. */
function SidebarToggle() {
  const collapsed = useAppStore((s) => s.railCollapsed);
  const toggleRail = useAppStore((s) => s.toggleRail);
  return (
    <button
      className="sidebar-toggle"
      onClick={toggleRail}
      aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
      title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
    >
      <svg width="17" height="17" viewBox="0 0 20 20" fill="none" aria-hidden="true">
        <rect x="2.5" y="3.5" width="15" height="13" rx="2.5" stroke="currentColor" strokeWidth="1.3" />
        <line x1="7.7" y1="3.5" x2="7.7" y2="16.5" stroke="currentColor" strokeWidth="1.3" />
      </svg>
    </button>
  );
}

/** Workbench show/hide, mirroring the sidebar toggle on the opposite edge. The
 * dot marks work waiting behind a closed panel — a file the agent changed or an
 * artifact it made — so closing it never means missing what happened. */
function WorkbenchToggle() {
  const view = useAppStore((s) => s.view);
  const dockOpen = useAppStore((s) => s.dockOpen);
  const toggleDock = useAppStore((s) => s.toggleDock);
  const unseen = useAppStore((s) => {
    if (s.dockOpen || s.view !== "chat") return false;
    const artifacts = s.activeConversationId
      ? (s.artifacts[s.activeConversationId]?.length ?? 0)
      : 0;
    return artifacts > 0 || Object.keys(s.touchedFiles).length > 0;
  });
  if (view !== "chat") return null;

  const label = dockOpen ? "Hide files and canvas" : "Show files and canvas";
  return (
    <button
      className={`sidebar-toggle workbench-toggle ${dockOpen ? "on" : ""}`}
      onClick={toggleDock}
      aria-pressed={dockOpen}
      aria-label={label}
      title={`${label} (Ctrl+\\)`}
    >
      <svg width="17" height="17" viewBox="0 0 20 20" fill="none" aria-hidden="true">
        <rect x="2.5" y="3.5" width="15" height="13" rx="2.5" stroke="currentColor" strokeWidth="1.3" />
        <line x1="12.3" y1="3.5" x2="12.3" y2="16.5" stroke="currentColor" strokeWidth="1.3" />
      </svg>
      {unseen && <span className="toggle-badge" aria-hidden="true" />}
    </button>
  );
}

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
      <div className="topbar-left">
        <SidebarToggle />
        <div className="brand">
          <PoiesisMark />
          <span>Poiesis Agent</span>
        </div>
      </div>
      <div className="topbar-right">
        <EngineStatus />
        <ModelPicker />
        <WorkbenchToggle />
      </div>
    </div>
  );
}
