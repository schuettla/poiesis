import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../../lib/store";
import DiffView, { changeLabel } from "./DiffView";
import RecentChanges from "./RecentChanges";
import "./Changes.css";

/** Files past this many start collapsed. */
const OPEN_BY_DEFAULT = 3;

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/**
 * `PRJ-UI-3`: the Changes sub-view — where "the agent says it edited things"
 * becomes "here is the patch, put any of it back".
 *
 * An overview, so it lives in the sidebar: every file the agent changed, with
 * its patch folded under it. One file's patch is a single item, so its name
 * opens that patch as a tab, full width, where it is actually readable.
 *
 * `RecentChanges` is absorbed here as the history underneath: kept and undone
 * operations still have somewhere to be found.
 */
export default function ChangesPanel({ conversationId }: { conversationId: string }) {
  const set = useAppStore((s) => s.changeSets[conversationId]);
  const undoChanges = useAppStore((s) => s.undoChanges);
  const keepChanges = useAppStore((s) => s.keepChanges);
  const openItem = useAppStore((s) => s.openItem);
  const changesFocus = useAppStore((s) => s.changesFocus);
  const [opened, setOpened] = useState<Record<string, boolean>>({});
  const [confirmUndoAll, setConfirmUndoAll] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rows = useRef<Record<string, HTMLLIElement | null>>({});

  // A tab's dirty dot asked for this file: open it and bring it into view.
  useEffect(() => {
    if (!changesFocus) return;
    setOpened((o) => ({ ...o, [changesFocus]: true }));
    const row = rows.current[changesFocus];
    if (row && typeof row.scrollIntoView === "function") row.scrollIntoView({ block: "nearest" });
    useAppStore.setState({ changesFocus: null });
  }, [changesFocus, set]);

  const files = set?.files ?? [];
  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <section className="wb-section-block chg-panel" aria-label="Changes">
      {files.length > 0 ? (
        <>
          <div className="chg-head">
            <span className="chg-count">{plural(files.length, "file")}</span>
            <span className="chg-added">+{set?.added ?? 0}</span>
            <span className="chg-removed">−{set?.removed ?? 0}</span>
            <span className="chg-scope">{set?.this_run ? "this run" : "not yet kept"}</span>
            <span className="chg-head-actions">
              <button className="wb-link" onClick={() => setConfirmUndoAll(true)}>
                Undo all
              </button>
              <button
                className="wb-link"
                title="Keep these changes and clear this list. Nothing on disk changes."
                onClick={() => run(() => keepChanges(conversationId))}
              >
                Keep all
              </button>
            </span>
          </div>
          {confirmUndoAll && (
            <div className="wb-confirm">
              <p>
                Put all {plural(files.length, "file")} back the way they were before I changed them? New files I
                made are removed.
              </p>
              <div className="wb-confirm-actions">
                <button
                  className="wb-primary"
                  onClick={() => {
                    setConfirmUndoAll(false);
                    void run(() => undoChanges(conversationId));
                  }}
                >
                  Undo all
                </button>
                <button className="wb-link" onClick={() => setConfirmUndoAll(false)}>
                  Cancel
                </button>
              </div>
            </div>
          )}
          <ul className="chg-files">
            {files.map((f, i) => {
              const isOpen = opened[f.path] ?? i < OPEN_BY_DEFAULT;
              const label = changeLabel(f);
              return (
                <li key={f.path} className="chg-file" ref={(el) => (rows.current[f.path] = el)}>
                  <div className="chg-file-row">
                    <button
                      className="chg-caret"
                      aria-expanded={isOpen}
                      aria-label={`${isOpen ? "Hide" : "Show"} the patch for ${f.display}`}
                      onClick={() => setOpened((o) => ({ ...o, [f.path]: !isOpen }))}
                    >
                      {isOpen ? "▾" : "▸"}
                    </button>
                    <button
                      className="chg-path"
                      title={`Open the patch for ${f.display} as a tab`}
                      onClick={() => openItem({ kind: "diff", id: f.path, conversationId })}
                    >
                      {f.display}
                    </button>
                    <span className="chg-stat">
                      <span className="chg-added">+{f.added}</span> / <span className="chg-removed">−{f.removed}</span>
                    </span>
                    <button className="wb-link" onClick={() => run(() => undoChanges(conversationId, f.path))}>
                      Undo
                    </button>
                  </div>
                  {label && <p className="chg-status">{label}</p>}
                  {isOpen && <DiffView file={f} />}
                </li>
              );
            })}
          </ul>
        </>
      ) : (
        <div className="wb-empty">
          <p className="wb-empty-title">Nothing waiting for review</p>
          <p className="wb-empty-blurb">
            When I edit files in this folder, each change shows up here so you can keep it or put it back.
          </p>
        </div>
      )}
      {error && <p className="wb-error">{error}</p>}
      <RecentChanges />
    </section>
  );
}
