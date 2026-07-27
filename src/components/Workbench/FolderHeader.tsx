import { useEffect, useRef, useState } from "react";
import { useActiveConversation, useAppStore } from "../../lib/store";
import type { FolderTrust } from "../../lib/types";

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

export default function FolderHeader() {
  const conversation = useActiveConversation();
  const attachFolder = useAppStore((s) => s.attachFolder);
  const detachFolder = useAppStore((s) => s.detachFolder);
  const setFolderTrust = useAppStore((s) => s.setFolderTrust);
  const revealInSystem = useAppStore((s) => s.revealInSystem);
  const showHidden = useAppStore((s) => s.showHidden);
  const toggleShowHidden = useAppStore((s) => s.toggleShowHidden);
  const folderError = useAppStore((s) => s.folderError);

  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmDetach, setConfirmDetach] = useState(false);
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
