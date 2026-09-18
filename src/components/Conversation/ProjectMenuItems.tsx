import { useState } from "react";
import { useAppStore } from "../../lib/store";
import { ChevronIcon } from "../Icons/Icons";

/**
 * `PRJ-9` in the session's own menus (the chat's ⋯ and the rail row's ⋯):
 * put this chat into a project, move it to another, or take it out again.
 *
 * The destinations fold open in place under the item rather than as a second
 * flyout — the menu sits at the window's edge in both places it is used, and a
 * flyout would have nowhere to go. Nothing here deletes the chat or touches a
 * byte on disk.
 */
export default function ProjectMenuItems({ conversationId, onDone }: { conversationId: string; onDone: () => void }) {
  const projects = useAppStore((s) => s.projects);
  const currentId = useAppStore((s) => s.conversations.find((c) => c.id === conversationId)?.projectId ?? null);
  const moveSessionToProject = useAppStore((s) => s.moveSessionToProject);
  const [open, setOpen] = useState(false);

  // The project it is in may have been archived; then it is in none we can show.
  const current = projects.find((p) => p.id === currentId);
  const targets = projects.filter((p) => p.id !== current?.id);
  if (!current && targets.length === 0) return null;

  const move = (projectId: string | null) => {
    onDone();
    void moveSessionToProject(conversationId, projectId);
  };

  return (
    <>
      {targets.length > 0 && (
        <button
          className="row-menu-item row-menu-parent"
          role="menuitem"
          aria-haspopup="true"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
        >
          <span className="row-menu-label">{current ? "Move to project…" : "Add to project…"}</span>
          <ChevronIcon dir="right" size={12} className={`row-menu-caret ${open ? "open" : ""}`} />
        </button>
      )}
      {open && (
        <div className="row-menu-sub" role="group" aria-label="Projects">
          {targets.map((p) => (
            <button
              key={p.id}
              className="row-menu-item"
              role="menuitem"
              title={p.rootPath ?? p.name}
              onClick={() => move(p.id)}
            >
              <span className="row-menu-label">{p.name}</span>
            </button>
          ))}
        </div>
      )}
      {current && (
        <button className="row-menu-item" role="menuitem" onClick={() => move(null)}>
          <span className="row-menu-label">Remove from {current.name}</span>
        </button>
      )}
    </>
  );
}
