import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { CloseIcon } from "../Icons/Icons";
import "./BranchDialog.css";

/**
 * `WRK-6`: the other way. When an item was tried two ways, one was kept and the
 * folder holds its changes; this is how the way that was not kept stays readable:
 * what the helper said it did, and what it changed, as a patch. Read-only. It is
 * read from a record, because the copy it was made in is gone.
 *
 * Opens on the way that was not kept, because that is what "See the other way"
 * asked for; the kept one is a tab away.
 */
export default function BranchDialog({
  id,
  kept,
  onClose,
}: {
  id: string;
  kept: "a" | "b";
  onClose: () => void;
}) {
  const [view, setView] = useState<api.BranchView | null | undefined>(undefined);
  const [side, setSide] = useState<0 | 1>(kept === "a" ? 1 : 0);

  useEffect(() => {
    let cancelled = false;
    api
      .branchView(id)
      .then((v) => !cancelled && setView(v))
      .catch(() => !cancelled && setView(null));
    return () => {
      cancelled = true;
    };
  }, [id]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const keptSide = kept === "a" ? 0 : 1;
  return (
    <>
      <div className="branch-backdrop" onClick={onClose} />
      <div className="branch-dialog" role="dialog" aria-modal="true" aria-label="The two ways">
        <div className="branch-head">
          <h2 className="branch-title">{view ? view.text : "The two ways"}</h2>
          <button className="branch-close" aria-label="Close" onClick={onClose}>
            <CloseIcon size={14} strokeWidth={1.4} />
          </button>
        </div>
        {view === undefined && <p className="branch-empty">Loading…</p>}
        {view === null && <p className="branch-empty">I no longer have this one.</p>}
        {view && (
          <>
            <div className="branch-tabs" role="tablist" aria-label="Which way">
              {([0, 1] as const).map((i) => (
                <button
                  key={i}
                  role="tab"
                  aria-selected={side === i}
                  className={`branch-tab${side === i ? " on" : ""}`}
                  onClick={() => setSide(i)}
                >
                  Way {i === 0 ? "A" : "B"}
                  {i === keptSide ? " · kept" : ""}
                </button>
              ))}
            </div>
            <p className="branch-said">{view.ways[side] || "It did not say."}</p>
            <p className="branch-meta">
              {view.files[side] === 1 ? "1 file changed" : `${view.files[side]} files changed`}
              {side === keptSide ? ` · kept: ${view.reason}` : " · not kept"}
            </p>
            <pre className="branch-patch" tabIndex={0}>
              {view.patches[side] || "It changed no files."}
            </pre>
          </>
        )}
      </div>
    </>
  );
}
