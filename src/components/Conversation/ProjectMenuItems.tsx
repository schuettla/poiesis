import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useAppStore } from "../../lib/store";
import { ChevronIcon } from "../Icons/Icons";

/**
 * `PRJ-9` in the session's own menus (the chat's chevron in the header and the rail row's ⋯):
 * put this chat into a project, move it to another, or take it out again.
 *
 * The destinations open as a second menu beside the first, the way the rail's filter menu opens
 * its choices: on hover, click or →, with Escape taking back one level. It is fixed and out of the
 * tree, because the rail and the header strip clip whatever is drawn inside them. Nothing here
 * deletes the chat or touches a byte on disk.
 */
export default function ProjectMenuItems({ conversationId, onDone }: { conversationId: string; onDone: () => void }) {
  const projects = useAppStore((s) => s.projects);
  const currentId = useAppStore((s) => s.conversations.find((c) => c.id === conversationId)?.projectId ?? null);
  const moveSessionToProject = useAppStore((s) => s.moveSessionToProject);
  const [fly, setFly] = useState<{ top: number; left: number } | null>(null);
  const parentRef = useRef<HTMLButtonElement>(null);
  const flyRef = useRef<HTMLDivElement>(null);

  // Escape takes back the flyout first; the menu's own Escape only sees it once there is none.
  useEffect(() => {
    if (!fly) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setFly(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [fly]);

  // Pointing at any other line of the menu puts the flyout away.
  useEffect(() => {
    if (!fly) return;
    const onOver = (e: PointerEvent) => {
      const t = e.target as Element | null;
      if (!t || parentRef.current?.contains(t) || flyRef.current?.contains(t)) return;
      if (t.closest(".row-menu")) setFly(null);
    };
    document.addEventListener("pointerover", onOver);
    return () => document.removeEventListener("pointerover", onOver);
  }, [fly]);

  // Kept inside the window once its height is known.
  useLayoutEffect(() => {
    const el = flyRef.current;
    if (!el || !fly) return;
    el.style.top = `${Math.max(8, Math.min(fly.top, window.innerHeight - el.offsetHeight - 8))}px`;
    el.style.left = `${Math.max(8, Math.min(fly.left, window.innerWidth - el.offsetWidth - 8))}px`;
  }, [fly]);

  // The project it is in may have been archived; then it is in none we can show.
  const current = projects.find((p) => p.id === currentId);
  const targets = projects.filter((p) => p.id !== current?.id);
  if (!current && targets.length === 0) return null;

  const move = (projectId: string | null) => {
    setFly(null);
    onDone();
    void moveSessionToProject(conversationId, projectId);
  };
  const flyOpen = (row: HTMLElement) => {
    const menu = row.closest(".row-menu")?.getBoundingClientRect();
    const r = row.getBoundingClientRect();
    // Level with the row (less the menu's padding), against the menu's right edge.
    setFly({ top: r.top - 5, left: (menu?.right ?? r.right) - 2 });
  };

  return (
    <>
      {targets.length > 0 && (
        <button
          ref={parentRef}
          className="row-menu-item row-menu-parent"
          role="menuitem"
          aria-haspopup="menu"
          aria-expanded={fly !== null}
          onPointerEnter={(e) => flyOpen(e.currentTarget)}
          onClick={(e) => (fly ? setFly(null) : flyOpen(e.currentTarget))}
          onKeyDown={(e) => {
            if (e.key === "ArrowRight") {
              e.preventDefault();
              flyOpen(e.currentTarget);
            }
          }}
        >
          <span className="row-menu-label">{current ? "Move to project…" : "Add to project…"}</span>
          <ChevronIcon dir="right" size={12} className="row-menu-caret" />
        </button>
      )}
      {fly &&
        createPortal(
          <div
            ref={flyRef}
            className="row-menu rail-popup rail-flyout"
            role="menu"
            aria-label="Projects"
            style={fly}
            onClick={(e) => e.stopPropagation()}
          >
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
          </div>,
          document.body
        )}
      {current && (
        <button className="row-menu-item" role="menuitem" onClick={() => move(null)}>
          <span className="row-menu-label">Remove from {current.name}</span>
        </button>
      )}
    </>
  );
}
