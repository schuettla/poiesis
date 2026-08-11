import { useEffect } from "react";
import { useAppStore } from "./lib/store";
import TopBar from "./components/TopBar/TopBar";
import Rail from "./components/Rail/Rail";
import Workbench from "./components/Workbench/Workbench";
import Chat from "./routes/Chat";
import SettingsHub from "./routes/SettingsHub";
import Library from "./routes/Library";
import PermissionPanel from "./components/SidePanel/PermissionPanel";
import ContextPanel from "./components/Context/ContextPanel";
import MediaConsentDialog from "./components/Confirm/MediaConsentDialog";
import ImageLightbox from "./components/Conversation/ImageLightbox";
import "./App.css";

export default function App() {
  const view = useAppStore((s) => s.view);
  const bootstrap = useAppStore((s) => s.bootstrap);
  const railCollapsed = useAppStore((s) => s.railCollapsed);
  const dockOpen = useAppStore((s) => s.dockOpen);
  const toggleDock = useAppStore((s) => s.toggleDock);
  const dockWidth = useAppStore((s) => s.dockWidth);
  const dockDragging = useAppStore((s) => s.dockDragging);

  useEffect(() => {
    bootstrap();
  }, [bootstrap]);

  // Ctrl+\ mirrors the header toggle, matching the rail's place in the shell.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "\\") {
        e.preventDefault();
        toggleDock();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleDock]);

  // The Workbench belongs to the conversation, so it only exists in chat.
  const showDock = view === "chat";
  // Driving the width from here (rather than a CSS class) is what lets the
  // same property serve both the show/hide animation and the drag.
  const width = showDock && dockOpen ? dockWidth : 0;

  return (
    <div
      className={[
        "app",
        railCollapsed ? "rail-collapsed" : "",
        showDock ? "" : "no-dock",
        showDock && !dockOpen ? "dock-collapsed" : "",
        dockDragging ? "dock-dragging" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      style={{ "--dock-w": `${width}px` } as React.CSSProperties}
    >
      <TopBar />
      <Rail />
      {view === "chat" && <Chat />}
      {(view === "models" ||
        view === "engine" ||
        view === "apps" ||
        view === "skills" ||
        view === "self" ||
        view === "tasks" ||
        view === "activity" ||
        view === "settings") && (
        <SettingsHub />
      )}
      {view === "library" && <Library />}
      {/* Stays mounted while collapsed so the column can animate shut — an
          unmount would blank it instantly and leave the grid sliding over
          nothing. */}
      {showDock && <Workbench />}
      <PermissionPanel />
      <ContextPanel />
      <MediaConsentDialog />
      <ImageLightbox />
    </div>
  );
}
