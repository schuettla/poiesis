import { useActiveConversation, useAppStore, useLiveItems } from "../../lib/store";
import { HUB_SECTIONS } from "../../lib/types";
import { SidebarIcon } from "../Icons/Icons";
import PoiesisMark from "../Mark/PoiesisMark";
import TabStrip from "./TabStrip";
import WindowControls, { framelessWindow, onTitleBarMouseDown } from "./WindowControls";
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
      <SidebarIcon side="left" size={17} />
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
      <SidebarIcon side="right" size={17} />
      {unseen && <span className="toggle-badge" aria-hidden="true" />}
    </button>
  );
}

/** Where you are, in words (`SHL-24`).
 *
 * With chats and routes out of the strip, the header would otherwise say
 * nothing about the thing filling most of the window. This is a label, not a
 * tab: there is nothing to press and nothing to close, because a conversation
 * and a route are places you are rather than things you hold open.
 *
 * `SHL-27`: it yields to the strip the moment anything is open, because the
 * session tab then names the same chat — two names for one place, one of them
 * pressable and one not, is worse than either alone. */
function Location() {
  const view = useAppStore((s) => s.view);
  const conversation = useActiveConversation();
  const projectName = useAppStore((s) =>
    view === "project" ? s.projects.find((p) => p.id === s.activeProjectId)?.name : undefined
  );
  const chat = view === "chat";
  const label = chat
    ? conversation?.title || "New chat"
    : view === "project"
      ? (projectName ?? "Project")
      : view === "library"
        ? "Library"
        : (HUB_SECTIONS.find((s) => s.view === view)?.label ?? "Settings");

  return (
    <div className={`topbar-where ${chat ? "is-chat" : "is-route"}`} title={label}>
      <span className="topbar-where-label">{label}</span>
    </div>
  );
}

/**
 * `SHL-27`: one header across the whole window.
 *
 * It used to be four segments, each pinned to the width of the column beneath
 * it so every divider in the header continued a divider in the shell. That
 * only pays while each of those columns holds something — with the item pane
 * gone, the segments were dividing the header into boxes that no longer
 * matched anything, and the strip's box was the narrowest of them. The header
 * is now the brand, the strip, and the toggles: the strip gets every pixel
 * between them, which is what a file tab's path needs.
 */
export default function TopBar() {
  const view = useAppStore((s) => s.view);
  const { items } = useLiveItems();
  // Exactly the condition `TabStrip` draws under, asked here so the two cannot
  // both claim the row — the strip names this chat when it is showing.
  const strip = view === "chat" && items.length > 0;

  return (
    // On Windows the native frame is off and this row is the title bar: empty
    // space drags the window, and the caption buttons close the row.
    <div className={`topbar ${framelessWindow() ? "is-titlebar" : ""}`} onMouseDown={onTitleBarMouseDown}>
      <div className="topbar-left">
        <div className="brand">
          <PoiesisMark />
          {/* Matches the mockup's compact wordmark — the fuller "Poiesis
              Agent" name still applies everywhere outside this cramped
              232px-wide segment (About, window title, and so on). */}
          <span>Poiesis</span>
        </div>
        <SidebarToggle />
      </div>
      {strip ? <TabStrip /> : <Location />}
      <div className="topbar-right">
        <WorkbenchToggle />
      </div>
      <WindowControls />
    </div>
  );
}
