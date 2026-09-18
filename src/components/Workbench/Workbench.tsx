import { useEffect, useMemo, useRef, useState } from "react";
import { useActiveConversation, useAppStore } from "../../lib/store";
import { stillWorking } from "../../lib/api";
import type { DockView } from "../../lib/types";
import AgentsPanel from "./AgentsPanel";
import BrowserPanel from "./BrowserPanel";
import ChangesPanel from "./ChangesPanel";
import FolderHeader, { FolderIndexStatus } from "./FolderHeader";
import Tree from "./Tree";
import Artifacts from "./Artifacts";
import Duplicates from "./Duplicates";
import "./Workbench.css";

/** Drag the dock's inner edge to resize it. Width comes from the distance to the
 * window's right edge, so the divider tracks the pointer exactly rather than
 * accumulating drift from deltas. */
function DockResizer() {
  const setDockWidth = useAppStore((s) => s.setDockWidth);
  const setDockDragging = useAppStore((s) => s.setDockDragging);
  const dockWidth = useAppStore((s) => s.dockWidth);

  const startDrag = (e: React.PointerEvent) => {
    e.preventDefault();
    setDockDragging(true);
    const move = (ev: PointerEvent) => setDockWidth(window.innerWidth - ev.clientX);
    const up = () => {
      setDockDragging(false);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };

  return (
    <div
      className="wb-resizer"
      onPointerDown={startDrag}
      onDoubleClick={() => setDockWidth(340)}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the Workbench"
      aria-valuenow={dockWidth}
      aria-valuemin={260}
      aria-valuemax={720}
      tabIndex={0}
      title="Drag to resize · double-click to reset"
      onKeyDown={(e) => {
        // Keyboard parity: the panel must be resizable without a pointer.
        if (e.key === "ArrowLeft") setDockWidth(dockWidth + 24);
        if (e.key === "ArrowRight") setDockWidth(dockWidth - 24);
      }}
    />
  );
}

/**
 * Move the sidebar to whichever sub-view the agent just did something in
 * (`SHL-23`).
 *
 * Only *transitions* move it — browsing starting, a child agent starting —
 * never the mere fact that a session or a subrun exists. A steady state must
 * not keep yanking the sidebar back while the user is reading a different
 * sub-view. Switching conversations re-arms both, since the new chat's state
 * isn't a transition the user watched happen.
 *
 * The hook is handed `setDockView` and nothing else, so it cannot focus a tab:
 * the strip holds the chat you are typing in, and the agent may never move
 * you off it. A new artifact is the store's concern (it lands on its stream
 * event), and it follows the same rule.
 */
function useFollowTheAgent({
  convId,
  browsing,
  subRunCount,
  setDockView,
}: {
  convId: string | null;
  browsing: boolean;
  subRunCount: number;
  setDockView: (view: DockView) => void;
}) {
  const prev = useRef({ convId, browsing, subRunCount });

  useEffect(() => {
    const was = prev.current;
    prev.current = { convId, browsing, subRunCount };
    // A different chat: adopt its state as the baseline rather than reading
    // the difference between two unrelated conversations as activity.
    if (was.convId !== convId) return;
    // `SUB-UI-2`: a new agent starting is the strongest "come look" there is —
    // work has just left the turn you were reading and gone somewhere else.
    if (subRunCount > was.subRunCount) setDockView("agents");
    else if (browsing && !was.browsing) setDockView("browser");
  }, [convId, browsing, subRunCount, setDockView]);
}

/**
 * The Workbench: the agent's side of the desk.
 *
 * It holds overviews and navigates them itself (`SHL-21`), from the row at its
 * top, the way Settings navigates its sections. A single thing picked out of
 * an overview — a file, an artifact, one agent, one patch — opens as a tab in
 * its own pane to the left of this one (`SHL-24`), so the list and the thing
 * you took out of it are both on screen. This panel never changes when an item
 * opens.
 */
/** One empty state for every sub-view: what will show up here, and — when
 * there is one — the single thing that makes it show up. */
function Empty({ title, children, action }: { title: string; children: React.ReactNode; action?: React.ReactNode }) {
  return (
    <div className="wb-empty">
      <p className="wb-empty-title">{title}</p>
      <p className="wb-empty-blurb">{children}</p>
      {action}
    </div>
  );
}

export default function Workbench() {
  const conversation = useActiveConversation();
  const dockView = useAppStore((s) => s.dockView);
  const setDockView = useAppStore((s) => s.setDockView);
  const attachFolder = useAppStore((s) => s.attachFolder);
  const refreshTrash = useAppStore((s) => s.refreshTrash);
  const artifactsMap = useAppStore((s) => s.artifacts);
  const convId = conversation?.id ?? null;
  const folder = conversation?.folderPath ?? null;

  const [filter, setFilter] = useState("");
  const browserSession = useAppStore((s) => (convId ? s.browserSessions[convId] : undefined));
  const refreshBrowserSession = useAppStore((s) => s.refreshBrowserSession);

  // Saved artifacts have become files — they belong in the tree, not here.
  const artifacts = useMemo(
    () => (convId ? (artifactsMap[convId] ?? []).filter((a) => !a.saved_path) : []),
    [artifactsMap, convId]
  );

  // `SUB-UI-2`: the children this conversation started, live or finished.
  const subRunMap = useAppStore((s) => s.subRuns);
  const subRuns = useMemo(
    () => Object.values(subRunMap).filter((r) => r.parentConversationId === convId),
    [subRunMap, convId]
  );
  const agentsWorking = subRuns.some((r) => stillWorking(r.status));

  const browsing = !!browserSession && !browserSession.closed;

  // `PRJ-UI-3`: what the agent changed and nobody has kept yet. The history
  // underneath (`RecentChanges`) keeps the view reachable after a Keep all.
  const changedFiles = useAppStore((s) => (convId ? (s.changeSets[convId]?.files.length ?? 0) : 0));

  // Files, Artifacts and Changes are always in the row, in the same places,
  // so the row can be learned: a sub-view with nothing in it says what will
  // appear there instead of vanishing. Agents and Browser are the exception —
  // they only exist while this chat has used them, and arrive with a live dot.
  const views: { id: DockView; label: string; count?: number; live?: boolean }[] = [
    { id: "files", label: "Files" },
    { id: "artifacts", label: "Artifacts", count: artifacts.length || undefined },
    { id: "changes", label: "Changes", count: (folder && changedFiles) || undefined },
    ...(subRuns.length
      ? [{ id: "agents" as DockView, label: "Agents", count: subRuns.length, live: agentsWorking }]
      : []),
    ...(browserSession ? [{ id: "browser" as DockView, label: "Browser", live: browsing }] : []),
  ];

  // `SHL-13`'s rule as the default: a chat without a folder lands on its
  // artifacts, not on an empty Files view — unless Files is what you just
  // pressed. That choice is this chat's only; the next one starts over.
  const [pickedFiles, setPickedFiles] = useState(false);
  useEffect(() => setPickedFiles(false), [convId]);
  const available = views.some((v) => v.id === dockView) ? dockView : folder ? "files" : "artifacts";
  const shown: DockView = available === "files" && !folder && !pickedFiles ? "artifacts" : available;
  const pick = (id: DockView) => {
    setPickedFiles(id === "files");
    setDockView(id);
  };

  useFollowTheAgent({ convId, browsing, subRunCount: subRuns.length, setDockView });

  useEffect(() => {
    refreshTrash().catch(() => {});
  }, [convId, refreshTrash]);

  // Asked for here rather than inside `BrowserPanel`, which only mounts once
  // its sub-view is showing — and the sub-view only exists once this has
  // answered. Left in the panel it was a deadlock: a re-opened chat never got
  // its session back because nothing was mounted to ask for it.
  useEffect(() => {
    if (convId) refreshBrowserSession(convId);
  }, [convId, refreshBrowserSession]);

  return (
    <aside className="workbench" aria-label="Workbench">
      <DockResizer />
      <FolderHeader />

      {/* A real tablist: these switch what this panel shows. While an item
          covers the main column none of them changes — the list stays beside
          the thing you took out of it. */}
      <div className="wb-tabs" role="tablist" aria-label="Workbench sections">
        {views.map((v) => (
          <button
            key={v.id}
            role="tab"
            aria-selected={shown === v.id}
            className={`wb-tab ${shown === v.id ? "active" : ""}`}
            onClick={() => pick(v.id)}
          >
            {v.label}
            {v.live && <span className="wb-tab-live" aria-label="live" />}
            {!!v.count && <span className="wb-tab-count">{v.count}</span>}
          </button>
        ))}
      </div>

      <div className="wb-tabpanel" role="tabpanel">
        {shown === "files" ? (
          folder ? (
            <section className="wb-section-block wb-files">
              <div className="wb-filter-wrap">
                <input
                  className="wb-filter"
                  placeholder="Filter files"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  aria-label="Filter files"
                />
              </div>
              <Tree filter={filter} />
              <Duplicates />
              <FolderIndexStatus />
            </section>
          ) : (
            <Empty
              title="Give Poiesis a folder to work in"
              action={
                <button className="wb-primary" onClick={attachFolder}>
                  Choose folder…
                </button>
              }
            >
              It can read, search and edit files there — you choose how much it may change.
            </Empty>
          )
        ) : shown === "changes" ? (
          folder && convId ? (
            <ChangesPanel conversationId={convId} />
          ) : (
            <Empty title="No changes to review">
              When I edit files in this chat's folder, each change shows up here so you can keep it or put it back.
            </Empty>
          )
        ) : shown === "browser" && convId ? (
          <BrowserPanel conversationId={convId} />
        ) : shown === "agents" && convId ? (
          <AgentsPanel conversationId={convId} />
        ) : artifacts.length > 0 ? (
          <section className="wb-section-block wb-artifacts-block open">
            <Artifacts artifacts={artifacts} canSave={!!folder} />
          </section>
        ) : (
          <Empty title="Nothing made yet">
            Documents, code and pages I make in this chat collect here
            {folder ? ", until you save one into the folder." : "."}
          </Empty>
        )}
      </div>
    </aside>
  );
}
