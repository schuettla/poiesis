import { useEffect, useMemo, useRef, useState } from "react";
import { useActiveConversation, useAppStore } from "../../lib/store";
import BrowserPanel from "./BrowserPanel";
import FolderHeader from "./FolderHeader";
import Tree from "./Tree";
import Artifacts from "./Artifacts";
import Duplicates from "./Duplicates";
import RecentChanges from "./RecentChanges";
import { ArtifactView, FileView } from "./Viewer";
import { downloadArtifact } from "./artifactFiles";
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

/** The Workbench's three places, each a whole thing rather than a slice of a
 * scroll: what the agent made, the folder on disk, and the live page. */
type Tab = "artifacts" | "files" | "browser";

/**
 * Move the panel to whichever tab the agent just did something in.
 *
 * Only *transitions* switch the tab — browsing starting, an artifact count
 * going up — never the mere fact that a session or an artifact exists. A
 * steady state must not keep yanking the panel back while the user is reading
 * a different tab. Switching conversations re-arms both, since the new chat's
 * state isn't a transition the user watched happen.
 */
function useFollowTheAgent({
  convId,
  browsing,
  artifactCount,
  setTab,
}: {
  convId: string | null;
  browsing: boolean;
  artifactCount: number;
  setTab: (t: Tab) => void;
}) {
  const prev = useRef({ convId, browsing, artifactCount });

  useEffect(() => {
    const was = prev.current;
    prev.current = { convId, browsing, artifactCount };
    // A different chat: adopt its state as the baseline rather than reading
    // the difference between two unrelated conversations as activity.
    if (was.convId !== convId) return;
    if (browsing && !was.browsing) setTab("browser");
    else if (artifactCount > was.artifactCount) setTab("artifacts");
  }, [convId, browsing, artifactCount, setTab]);
}

/**
 * The Workbench: the agent's side of the desk.
 *
 * **One tab at a time, not a stack.** Artifacts, the folder and the browser
 * each want the full height of a ~340px column; stacked, every one of them was
 * a sliver and the interesting one was usually scrolled off. Tabs also give
 * the panel somewhere to *point*: when the agent starts browsing or makes
 * something, `useFollowTheAgent` moves to that tab, so the panel tracks what's
 * happening instead of waiting to be searched.
 *
 * Selecting a file or artifact opens the viewer, which takes over the whole
 * panel — the same reason the tabs exist.
 */
export default function Workbench() {
  const conversation = useActiveConversation();
  const selected = useAppStore((s) => s.selected);
  const selectNode = useAppStore((s) => s.selectNode);
  const viewerExpanded = useAppStore((s) => s.viewerExpanded);
  const setViewerExpanded = useAppStore((s) => s.setViewerExpanded);
  const revealInSystem = useAppStore((s) => s.revealInSystem);
  const refreshTree = useAppStore((s) => s.refreshTree);
  const scheduleConversation = useAppStore((s) => s.scheduleConversation);
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
  // Media artifacts are the deliberate exception to `useFollowTheAgent`
  // (`ART-2`): they're already visible inline in the stream, so a new one
  // appearing shouldn't yank the panel to this tab the way any other artifact
  // does. Still counted in the tab label — just not a "come look" transition.
  const nonMediaArtifactCount = useMemo(
    () => artifacts.filter((a) => a.kind !== "image" && a.kind !== "video").length,
    [artifacts]
  );

  const browsing = !!browserSession && !browserSession.closed;
  const tabs: { id: Tab; label: string; count?: number; live?: boolean }[] = [
    { id: "artifacts", label: "Artifacts", count: artifacts.length },
    ...(folder ? [{ id: "files" as Tab, label: "Files" }] : []),
    ...(browserSession ? [{ id: "browser" as Tab, label: "Browser", live: browsing }] : []),
  ];

  // Default to whichever place actually has something in it, so a fresh chat
  // with a folder opens on Files rather than an empty Artifacts.
  const [tab, setTab] = useState<Tab>(() =>
    artifacts.length > 0 ? "artifacts" : folder ? "files" : "artifacts"
  );

  // A tab can disappear (the folder is detached, the browser panel dismissed).
  // Landing on a tab that no longer exists would blank the panel.
  const available = tabs.map((t) => t.id);
  const activeTab: Tab = available.includes(tab) ? tab : (available[0] ?? "artifacts");

  useFollowTheAgent({ convId, browsing, artifactCount: nonMediaArtifactCount, setTab });
  const activeArtifact =
    selected?.kind === "artifact" ? artifacts.find((a) => a.id === selected.id) : undefined;

  useEffect(() => {
    refreshTrash().catch(() => {});
  }, [convId, refreshTrash]);

  // Asked for here rather than inside `BrowserPanel`, which only mounts once
  // the Browser tab exists — and the tab only exists once this has answered.
  // Left in the panel it was a deadlock: a re-opened chat never got its
  // session back because nothing was mounted to ask for it.
  useEffect(() => {
    if (convId) refreshBrowserSession(convId);
  }, [convId, refreshBrowserSession]);

  useEffect(() => {
    if (!viewerExpanded) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setViewerExpanded(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [viewerExpanded, setViewerExpanded]);

  // A selection that no longer resolves shouldn't leave a ghost pane behind.
  useEffect(() => {
    if (selected?.kind === "artifact" && !activeArtifact) selectNode(null);
  }, [selected, activeArtifact, selectNode]);

  const title =
    selected?.kind === "file" ? selected.id.split(/[\\/]/).pop() : activeArtifact?.title ?? "";

  const body =
    selected?.kind === "file" ? (
      <FileView path={selected.id} />
    ) : activeArtifact ? (
      <ArtifactView kind={activeArtifact.kind} content={activeArtifact.content} />
    ) : null;

  return (
    <>
      <aside className="workbench" aria-label="Workbench">
        <DockResizer />
        {selected ? (
          /* The viewer takes the whole panel — a 340px column can't usefully
             hold a file preview and two lists at the same time. */
          <div className="wb-viewer">
            <div className="wb-viewer-head">
              <button className="wb-back" onClick={() => selectNode(null)} aria-label="Back to files">
                <svg width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                  <path
                    d="M12 4.5 6.5 10l5.5 5.5"
                    stroke="currentColor"
                    strokeWidth="1.4"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                </svg>
              </button>
              <span className="wb-viewer-title" title={selected.id}>
                {title}
              </span>
              <div className="wb-viewer-actions">
                {activeArtifact && (
                  <button
                    className="wb-icon"
                    title="Save a copy…"
                    aria-label={`Save a copy of ${activeArtifact.title}`}
                    onClick={() => downloadArtifact(activeArtifact)}
                  >
                    <svg width="15" height="15" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                      <path
                        d="M10 3v9m0 0-3.5-3.5M10 12l3.5-3.5M4 14.5v1a1.5 1.5 0 0 0 1.5 1.5h9a1.5 1.5 0 0 0 1.5-1.5v-1"
                        stroke="currentColor"
                        strokeWidth="1.3"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  </button>
                )}
                {selected.kind === "file" && (
                  <button
                    className="wb-icon"
                    title="Show in file manager"
                    aria-label="Show in file manager"
                    onClick={() => revealInSystem(selected.id)}
                  >
                    <svg width="15" height="15" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                      <path
                        d="M2.5 5.5A1.5 1.5 0 0 1 4 4h3.2l1.4 1.8H16a1.5 1.5 0 0 1 1.5 1.5v7.2A1.5 1.5 0 0 1 16 16H4a1.5 1.5 0 0 1-1.5-1.5z"
                        stroke="currentColor"
                        strokeWidth="1.3"
                        strokeLinejoin="round"
                      />
                    </svg>
                  </button>
                )}
                <button
                  className="wb-icon"
                  title="Expand"
                  aria-label="Expand the preview"
                  onClick={() => setViewerExpanded(true)}
                >
                  <svg width="15" height="15" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                    <path
                      d="M12 3h5v5M8 17H3v-5M17 3l-6 6M3 17l6-6"
                      stroke="currentColor"
                      strokeWidth="1.3"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                </button>
              </div>
            </div>
            <div className="wb-viewer-body">{body}</div>
          </div>
        ) : (
          <>
            <FolderHeader />

            {/* Turn this chat into something I do on a schedule. It sits here
                because this panel is already "everything about this chat" —
                and because the moment you want a task is usually just after
                you've had the agent do the thing once, by hand. */}
            {convId && (
              <button
                className="wb-schedule"
                onClick={() => scheduleConversation(convId)}
                title="Do this again on a schedule"
              >
                <span className="wb-schedule-glyph" aria-hidden="true">
                  ◷
                </span>
                Schedule this
              </button>
            )}

            {tabs.length > 1 && (
              <div className="wb-tabs" role="tablist" aria-label="Workbench sections">
                {tabs.map((t) => (
                  <button
                    key={t.id}
                    role="tab"
                    id={`wb-tab-${t.id}`}
                    aria-selected={activeTab === t.id}
                    aria-controls={`wb-panel-${t.id}`}
                    className={`wb-tab ${activeTab === t.id ? "active" : ""}`}
                    onClick={() => setTab(t.id)}
                  >
                    {t.label}
                    {/* A live browser reads as a state, not a quantity — the
                        one dot the panel is allowed, per "no badges, counts
                        and words instead of gauges". */}
                    {t.live && <span className="wb-tab-live" aria-label="live" />}
                    {!!t.count && <span className="wb-tab-count">{t.count}</span>}
                  </button>
                ))}
              </div>
            )}

            <div
              className="wb-tabpanel"
              role="tabpanel"
              id={`wb-panel-${activeTab}`}
              aria-labelledby={`wb-tab-${activeTab}`}
            >
              {activeTab === "browser" && convId && <BrowserPanel conversationId={convId} />}

              {activeTab === "artifacts" &&
                (artifacts.length > 0 ? (
                  <section className="wb-section-block wb-artifacts-block open">
                    <Artifacts artifacts={artifacts} canSave={!!folder} />
                  </section>
                ) : (
                  <p className="wb-hint">Artifacts the agent makes will show up here.</p>
                ))}

              {activeTab === "files" && folder && (
                <section className="wb-section-block wb-files">
                  <div className="wb-section">
                    Files
                    <button
                      className="wb-icon wb-section-action"
                      title="Refresh"
                      aria-label="Refresh the file list"
                      onClick={() => refreshTree()}
                    >
                      <svg width="13" height="13" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                        <path
                          d="M16 10a6 6 0 1 1-1.8-4.3M16 3v3h-3"
                          stroke="currentColor"
                          strokeWidth="1.3"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        />
                      </svg>
                    </button>
                  </div>
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
                  {/* Both are about the folder, so they belong with it rather
                      than under every tab as they were when this was a stack. */}
                  <Duplicates />
                  <RecentChanges />
                </section>
              )}
            </div>
          </>
        )}
      </aside>

      {viewerExpanded && selected && (
        <div
          className="wb-overlay"
          role="dialog"
          aria-modal="true"
          aria-label={title}
          onClick={(e) => e.target === e.currentTarget && setViewerExpanded(false)}
        >
          <div className="wb-overlay-panel">
            <div className="wb-viewer-head">
              <span className="wb-viewer-title">{title}</span>
              <button
                className="wb-icon"
                title="Close"
                aria-label="Close the preview"
                onClick={() => setViewerExpanded(false)}
              >
                <svg width="16" height="16" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                  <path d="M5.5 5.5l9 9M14.5 5.5l-9 9" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
                </svg>
              </button>
            </div>
            <div className="wb-viewer-body">
              {selected.kind === "file" ? (
                <FileView key={`x-${selected.id}`} path={selected.id} />
              ) : activeArtifact ? (
                <ArtifactView kind={activeArtifact.kind} content={activeArtifact.content} />
              ) : null}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
