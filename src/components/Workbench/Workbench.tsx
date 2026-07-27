import { useEffect, useMemo, useState } from "react";
import { useActiveConversation, useAppStore } from "../../lib/store";
import FolderHeader from "./FolderHeader";
import Tree from "./Tree";
import Artifacts from "./Artifacts";
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

/**
 * The Workbench: the agent's side of the desk.
 *
 * Two stacked sections, plainly separate. **Files** is the folder on disk.
 * **Artifacts** is what the agent made in this chat — not on disk until you
 * save one, which writes it into the folder and moves it up into the tree.
 * Selecting either opens it in the viewer, which takes over the panel rather
 * than squeezing the lists into a sliver.
 */
export default function Workbench() {
  const conversation = useActiveConversation();
  const selected = useAppStore((s) => s.selected);
  const selectNode = useAppStore((s) => s.selectNode);
  const viewerExpanded = useAppStore((s) => s.viewerExpanded);
  const setViewerExpanded = useAppStore((s) => s.setViewerExpanded);
  const revealInSystem = useAppStore((s) => s.revealInSystem);
  const refreshTree = useAppStore((s) => s.refreshTree);
  const refreshTrash = useAppStore((s) => s.refreshTrash);
  const artifactsMap = useAppStore((s) => s.artifacts);
  const convId = conversation?.id ?? null;
  const folder = conversation?.folderPath ?? null;

  const [filter, setFilter] = useState("");
  const [artifactsOpen, setArtifactsOpen] = useState(true);

  // Saved artifacts have become files — they belong in the tree, not here.
  const artifacts = useMemo(
    () => (convId ? (artifactsMap[convId] ?? []).filter((a) => !a.saved_path) : []),
    [artifactsMap, convId]
  );
  const activeArtifact =
    selected?.kind === "artifact" ? artifacts.find((a) => a.id === selected.id) : undefined;

  useEffect(() => {
    refreshTrash().catch(() => {});
  }, [convId, refreshTrash]);

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

            {/* What the agent just made sits at the top, where you look first —
                it's the newest thing in the panel and the thing you asked for.
                The folder is the standing context underneath it. */}
            {artifacts.length > 0 && (
              <section className={`wb-section-block wb-artifacts-block ${artifactsOpen ? "open" : ""}`}>
                <button
                  className="wb-section wb-section-toggle"
                  aria-expanded={artifactsOpen}
                  onClick={() => setArtifactsOpen((o) => !o)}
                >
                  <span className="wb-row-caret" aria-hidden="true">
                    {artifactsOpen ? "▾" : "▸"}
                  </span>
                  Made in this chat
                  <span className="wb-section-count">{artifacts.length}</span>
                </button>
                {artifactsOpen && <Artifacts artifacts={artifacts} canSave={!!folder} />}
              </section>
            )}

            {folder && (
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
              </section>
            )}

            {!folder && artifacts.length === 0 && (
              <p className="wb-hint">Artifacts the agent makes will show up here.</p>
            )}

            <RecentChanges />
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
