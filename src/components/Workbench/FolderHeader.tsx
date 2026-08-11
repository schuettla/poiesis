import { useEffect, useRef, useState } from "react";
import { useActiveConversation, useAppStore } from "../../lib/store";
import type { FolderTrust } from "../../lib/types";
import type { IndexProgress, IndexRootView } from "../../lib/api";

/** The three access levels, in order of how much they let the agent do. Reads
 * are free at every level — this only governs what changes bytes. */
const TRUST_LEVELS: { id: FolderTrust; label: string; blurb: string }[] = [
  { id: "read-only", label: "Read only", blurb: "It can look at files but never change them." },
  { id: "confirm", label: "Ask first", blurb: "You approve each change before it happens." },
  { id: "auto", label: "Full", blurb: "Changes apply straight away. Deleting still asks." },
];

/** Middle-truncate a long path so both the drive and the folder stay visible. */
function shortPath(path: string, max = 46): string {
  if (path.length <= max) return path;
  const head = path.slice(0, Math.ceil(max / 2) - 1);
  const tail = path.slice(-(Math.floor(max / 2) - 2));
  return `${head}…${tail}`;
}

/** Middle dot before a relative time, matching the rail's own "· 2h ago" style. */
function timeAgo(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** `IDX-UI-1`'s never-built / building / built / stale line. A plain counting
 * line while building — no bar, no percentage (per the plan's own rule). */
function IndexStatusRow({
  indexState,
  indexProgress,
  indexExplained,
  skippedOpen,
  setSkippedOpen,
  onBuild,
  onCancel,
}: {
  indexState: IndexRootView | null;
  indexProgress: IndexProgress | null;
  indexExplained: boolean;
  skippedOpen: boolean;
  setSkippedOpen: (fn: (o: boolean) => boolean) => void;
  onBuild: () => void;
  onCancel: () => void;
}) {
  const building = indexProgress !== null || indexState?.state === "building";

  return (
    <>
      <div className="wb-head-row wb-index-row">
        {building ? (
          <>
            <span className="wb-index-status">
              {indexProgress && indexProgress.files_total > 0
                ? `Reading… ${indexProgress.files_done} of ${indexProgress.files_total}`
                : "Reading…"}
            </span>
            <button className="wb-link" onClick={onCancel}>
              Stop
            </button>
          </>
        ) : indexState?.state === "stale" ? (
          <>
            <span className="wb-index-status">
              {indexState.changed_count
                ? `${plural(indexState.changed_count, "file")} changed since I read this`
                : "This folder may have changed since I read it"}
            </span>
            <button className="wb-link" onClick={onBuild}>
              Read again
            </button>
          </>
        ) : indexState ? (
          <>
            <span className="wb-index-status">
              I've read {plural(indexState.file_count, "file")} here · {timeAgo(indexState.updated_at)}
            </span>
            <button className="wb-link" onClick={onBuild}>
              Read again
            </button>
          </>
        ) : (
          <>
            <span className="wb-index-status">I haven't read this folder yet</span>
            <button className="wb-link" onClick={onBuild}>
              Read it
            </button>
          </>
        )}
      </div>

      {/* SMP-4c: the first read says what reading is for, once. */}
      {building && !indexExplained && (
        <p className="wb-index-explain">
          I read the files you give me so I can answer from them. Everything stays on this machine.
        </p>
      )}

      {!building && indexState && indexState.skipped.length > 0 && (
        <div className="wb-index-skipped">
          <button className="wb-link" onClick={() => setSkippedOpen((o) => !o)}>
            {plural(indexState.skipped.length, "file")} I couldn't read
          </button>
          {skippedOpen && (
            <ul className="wb-index-skipped-list">
              {indexState.skipped.map((f) => (
                <li key={f.path}>
                  <span className="wb-index-skipped-path">{f.path}</span> — {f.reason}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </>
  );
}

export default function FolderHeader() {
  const conversation = useActiveConversation();
  const attachFolder = useAppStore((s) => s.attachFolder);
  const detachFolder = useAppStore((s) => s.detachFolder);
  const setFolderTrust = useAppStore((s) => s.setFolderTrust);
  const revealInSystem = useAppStore((s) => s.revealInSystem);
  const showHidden = useAppStore((s) => s.showHidden);
  const toggleShowHidden = useAppStore((s) => s.toggleShowHidden);
  const folderError = useAppStore((s) => s.folderError);
  const indexState = useAppStore((s) => s.indexState);
  const indexProgress = useAppStore((s) => s.indexProgress);
  const indexError = useAppStore((s) => s.indexError);
  const indexExplained = useAppStore((s) => s.indexExplained);
  const buildFolderIndex = useAppStore((s) => s.buildFolderIndex);
  const cancelFolderIndex = useAppStore((s) => s.cancelFolderIndex);

  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmDetach, setConfirmDetach] = useState(false);
  const [skippedOpen, setSkippedOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [menuOpen]);

  const folder = conversation?.folderPath;
  const trust: FolderTrust = conversation?.folderTrust ?? "confirm";

  if (!folder) {
    return (
      <div className="wb-head wb-head-empty">
        <p className="wb-empty-title">Give Poiesis a folder to work in</p>
        <p className="wb-empty-blurb">
          It can read, search and edit files there — you choose how much it may change.
        </p>
        <button className="wb-primary" onClick={attachFolder}>
          Choose folder…
        </button>
        {folderError && <p className="wb-error">{folderError}</p>}
      </div>
    );
  }

  const name = folder.split(/[\\/]/).filter(Boolean).pop() ?? folder;

  return (
    <div className="wb-head">
      <div className="wb-head-row">
        <span className="wb-folder-icon" aria-hidden="true">
          <svg width="15" height="15" viewBox="0 0 20 20" fill="none">
            <path
              d="M2.5 5.5A1.5 1.5 0 0 1 4 4h3.2l1.4 1.8H16a1.5 1.5 0 0 1 1.5 1.5v7.2A1.5 1.5 0 0 1 16 16H4a1.5 1.5 0 0 1-1.5-1.5z"
              stroke="currentColor"
              strokeWidth="1.3"
              strokeLinejoin="round"
            />
          </svg>
        </span>
        <span className="wb-folder-name" title={folder}>
          {name}
        </span>
        <div className="wb-menu-wrap" ref={menuRef}>
          <button
            className="wb-icon"
            aria-label="Folder options"
            aria-expanded={menuOpen}
            title="Folder options"
            onClick={() => setMenuOpen((o) => !o)}
          >
            <svg width="15" height="15" viewBox="0 0 20 20" aria-hidden="true">
              <circle cx="4.5" cy="10" r="1.4" fill="currentColor" />
              <circle cx="10" cy="10" r="1.4" fill="currentColor" />
              <circle cx="15.5" cy="10" r="1.4" fill="currentColor" />
            </svg>
          </button>
          {menuOpen && (
            <div className="wb-menu" role="menu">
              <button role="menuitem" onClick={() => { revealInSystem(folder); setMenuOpen(false); }}>
                Show in file manager
              </button>
              <button role="menuitem" onClick={() => { setMenuOpen(false); attachFolder(); }}>
                Change folder…
              </button>
              <button role="menuitem" onClick={() => { toggleShowHidden(); setMenuOpen(false); }}>
                {showHidden ? "Hide hidden files" : "Show hidden files"}
              </button>
              <hr />
              <button
                role="menuitem"
                className="wb-menu-danger"
                onClick={() => { setConfirmDetach(true); setMenuOpen(false); }}
              >
                Detach folder
              </button>
            </div>
          )}
        </div>
      </div>

      <p className="wb-folder-path" title={folder}>
        {shortPath(folder)}
      </p>

      <IndexStatusRow
        indexState={indexState}
        indexProgress={indexProgress}
        indexExplained={indexExplained}
        skippedOpen={skippedOpen}
        setSkippedOpen={setSkippedOpen}
        onBuild={buildFolderIndex}
        onCancel={cancelFolderIndex}
      />
      {indexError && <p className="wb-error">{indexError}</p>}

      {confirmDetach ? (
        <div className="wb-confirm">
          {/* Say plainly what detaching does — the word sounds destructive and isn't. */}
          <p>Stop working in this folder? Nothing on disk is deleted or changed.</p>
          <div className="wb-confirm-actions">
            <button
              className="wb-primary"
              onClick={() => { detachFolder(); setConfirmDetach(false); }}
            >
              Detach
            </button>
            <button className="wb-link" onClick={() => setConfirmDetach(false)}>
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div className="wb-trust" role="group" aria-label="File access">
          <span className="wb-trust-label">Access</span>
          <div className="wb-segments">
            {TRUST_LEVELS.map((level) => (
              <button
                key={level.id}
                className={`wb-segment ${trust === level.id ? "on" : ""}`}
                aria-pressed={trust === level.id}
                title={level.blurb}
                onClick={() => setFolderTrust(level.id)}
              >
                {level.label}
              </button>
            ))}
          </div>
        </div>
      )}

      {folderError && <p className="wb-error">{folderError}</p>}
    </div>
  );
}
