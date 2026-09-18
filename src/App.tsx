import { useEffect } from "react";
import { useAppStore, useLiveItems } from "./lib/store";
import TopBar from "./components/TopBar/TopBar";
import Rail from "./components/Rail/Rail";
import Workbench from "./components/Workbench/Workbench";
import ItemView from "./components/Workbench/ItemView";
import Chat from "./routes/Chat";
import SettingsHub, { isHubView } from "./routes/SettingsHub";
import Library from "./routes/Library";
import ProjectView from "./routes/ProjectView";
import ProjectsOverview from "./routes/ProjectsOverview";
import PermissionPanel from "./components/SidePanel/PermissionPanel";
import ContextPanel from "./components/Context/ContextPanel";
import MediaConsentDialog from "./components/Confirm/MediaConsentDialog";
import ImageLightbox from "./components/Conversation/ImageLightbox";
import OnboardingGuide from "./components/Onboarding/OnboardingGuide";
import CommandPalette from "./components/CommandPalette/CommandPalette";
import "./App.css";

export default function App() {
  const view = useAppStore((s) => s.view);
  const bootstrap = useAppStore((s) => s.bootstrap);
  const railCollapsed = useAppStore((s) => s.railCollapsed);
  const dockOpen = useAppStore((s) => s.dockOpen);
  const toggleDock = useAppStore((s) => s.toggleDock);
  const dockWidth = useAppStore((s) => s.dockWidth);
  const dockDragging = useAppStore((s) => s.dockDragging);
  // `SHL-27`: the open item, in the conversation's own column.
  const { activeKey } = useLiveItems();

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

  // `SHL-18`/`SHL-24`: what is left of the strip's keyboard bindings. Reads
  // live state via `getState()` on each keydown rather than subscribing, so
  // this effect never needs to re-run — and skips entirely while a text field
  // holds focus, the Composer included, so none of this fights with typing.
  //
  // Only `Ctrl+W`. Cycling chats with `Ctrl+Tab` and reaching the nth one with
  // `Ctrl+1`..`Ctrl+9` both addressed a list of open sessions, and there is no
  // such list any more: a chat is a destination, reached from the Rail.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      if (e.key !== "w" && e.key !== "W") return;
      const el = document.activeElement;
      const typing =
        el instanceof HTMLElement &&
        (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
      if (typing) return;
      const s = useAppStore.getState();
      // Closing is only ever about the sidebar's item tabs; nothing else in
      // the shell is a thing you hold open. With none, the binding does
      // nothing rather than closing the window out from under the user.
      if (s.activeItemId) {
        e.preventDefault();
        s.closeItem(s.activeItemId);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // A route takes the whole window, so it takes the sidebar with it.
  const showDock = view === "chat";
  // Driving the width from here (rather than a CSS class) is what lets the
  // same property serve both the show/hide animation and the drag.
  const width = showDock && dockOpen ? dockWidth : 0;
  // An item belongs to a conversation, so a route takes it away too. `null`
  // here is the session tab being the selected one — tabs may still be open.
  const showItem = view === "chat" && activeKey !== null;

  return (
    <div
      className={[
        "app",
        railCollapsed ? "rail-collapsed" : "",
        showDock ? "" : "no-dock",
        showDock && !dockOpen ? "dock-collapsed" : "",
        dockDragging ? "dock-dragging" : "",
        showItem ? "item-open" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      style={{ "--dock-w": `${width}px` } as React.CSSProperties}
    >
      <TopBar />
      <Rail />
      {view === "chat" && <Chat />}
      {showItem && <ItemView />}
      {/* Asked of the hub rather than restated here. The copy of this list that
          used to live in this file was missing "usage", so that tab rendered an
          empty window instead of the panel. */}
      {isHubView(view) && <SettingsHub />}
      {view === "library" && <Library />}
      {view === "projects" && <ProjectsOverview />}
      {view === "project" && <ProjectView />}
      {/* Stays mounted while collapsed so the column can animate shut — an
          unmount would blank it instantly and leave the grid sliding over
          nothing. */}
      {showDock && <Workbench />}
      <PermissionPanel />
      <ContextPanel />
      <MediaConsentDialog />
      <ImageLightbox />
      <OnboardingGuide />
      <CommandPalette />
    </div>
  );
}
