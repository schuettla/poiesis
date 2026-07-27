import { useState } from "react";
import { useActiveConversation, useAppStore } from "../../lib/store";

function ago(ts: number): string {
  const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

const VERB: Record<string, string> = {
  write: "wrote",
  edit: "edited",
  delete: "deleted",
  move: "moved",
  save: "saved",
};

/**
 * The honest answer to "the agent has write access to my real disk": a running
 * list of everything that changed, each row reversible. Undo restores the exact
 * prior bytes from the snapshot taken before the change.
 */
export default function RecentChanges() {
  const trash = useAppStore((s) => s.trash);
  const undoFileOp = useAppStore((s) => s.undoFileOp);
  const selectNode = useAppStore((s) => s.selectNode);
  const conversation = useActiveConversation();
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (trash.length === 0) return null;

  const pending = trash.filter((t) => !t.undone).length;
  const shown = open ? trash.slice(0, 12) : trash.slice(0, 3);

  return (
    <div className={`wb-changes ${open ? "open" : ""}`}>
      <button
        className="wb-changes-head"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="wb-changes-caret" aria-hidden="true">
          {open ? "▾" : "▸"}
        </span>
        Recent changes
        {pending > 0 && <span className="wb-section-count">{pending}</span>}
      </button>

      <ul className="wb-changes-list">
        {shown.map((t) => {
          const name = t.path.split(/[\\/]/).pop();
          const relative =
            conversation?.folderPath && t.path.startsWith(conversation.folderPath)
              ? t.path.slice(conversation.folderPath.length + 1).replace(/\\/g, "/")
              : t.path;
          return (
            <li key={t.id} className={t.undone ? "undone" : ""}>
              <span className="wb-change-op">{VERB[t.op] ?? t.op}</span>
              <button
                className="wb-change-name"
                title={relative}
                onClick={() => selectNode({ kind: "file", id: t.path })}
              >
                {name}
              </button>
              <span className="wb-change-when">{ago(t.created_at)}</span>
              {t.undone ? (
                <span className="wb-change-undone">undone</span>
              ) : (
                <button
                  className="wb-link"
                  onClick={() =>
                    undoFileOp(t.id).catch((e) => setError(String(e)))
                  }
                >
                  Undo
                </button>
              )}
            </li>
          );
        })}
      </ul>
      {error && <p className="wb-error">{error}</p>}
    </div>
  );
}
