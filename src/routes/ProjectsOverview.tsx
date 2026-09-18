import { useMemo, useState } from "react";
import { useAppStore } from "../lib/store";
import type { FolderTrust, Project } from "../lib/types";
import ConfirmDialog from "../components/Confirm/ConfirmDialog";
import { FolderIcon } from "../components/Icons/Icons";
import "./Surface.css";
import "./ProjectsOverview.css";

const TRUST_LABELS: Record<FolderTrust, string> = {
  "read-only": "Read only",
  confirm: "Ask first",
  auto: "Full access",
};

function formatDate(ts: number): string {
  return new Date(ts).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/**
 * `PRJ-3`: the projects overview. Every project as a card — the destination
 * the Rail's "Projects" button now opens, in place of creating one on the
 * spot. Quick create moved to the `+` beside that button; this page is where
 * you pick among the ones you already have.
 */
export default function ProjectsOverview() {
  const projects = useAppStore((s) => s.projects);
  const conversations = useAppStore((s) => s.conversations);
  const newProject = useAppStore((s) => s.newProject);
  const openProjectView = useAppStore((s) => s.openProjectView);
  const openProject = useAppStore((s) => s.openProject);
  const renameProject = useAppStore((s) => s.renameProject);
  const archiveProject = useAppStore((s) => s.archiveProject);

  const sessionCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const c of conversations) {
      if (!c.projectId || c.parentConversationId) continue;
      counts.set(c.projectId, (counts.get(c.projectId) ?? 0) + 1);
    }
    return counts;
  }, [conversations]);

  const sorted = useMemo(
    () => [...projects].sort((a, b) => b.updatedAt - a.updatedAt),
    [projects]
  );

  return (
    <div className="surface">
      <div className="surface-inner">
        <div className="projects-head">
          <div>
            <h1>Projects</h1>
            <p className="lede">
              A named group of chats, with its own instructions and — if it's about files on disk
              — a working folder.
            </p>
          </div>
          <button className="btn-primary" onClick={newProject}>
            New project
          </button>
        </div>

        {sorted.length === 0 ? (
          <div className="placeholder-note">
            No projects yet. Start one, or attach a folder to a chat — that makes one too.
          </div>
        ) : (
          <div className="project-card-grid">
            {sorted.map((p) => (
              <ProjectCard
                key={p.id}
                project={p}
                sessionCount={sessionCounts.get(p.id) ?? 0}
                onOpen={() => openProjectView(p.id)}
                onOpenChat={() => openProject(p.id)}
                onRename={(next) => renameProject(p.id, next)}
                onArchive={() => archiveProject(p.id)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function ProjectCard({
  project,
  sessionCount,
  onOpen,
  onOpenChat,
  onRename,
  onArchive,
}: {
  project: Project;
  sessionCount: number;
  onOpen: () => void;
  onOpenChat: () => void;
  onRename: (next: string) => void;
  onArchive: () => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [draftName, setDraftName] = useState<string | null>(null);
  const [confirmingArchive, setConfirmingArchive] = useState(false);

  return (
    <article
      className="project-card"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key === "Enter") onOpen();
      }}
      aria-label={`Open ${project.name}`}
    >
      <div className="project-card-head">
        <span
          className={`project-dot ${project.rootPath ? "has-folder" : ""}`}
          aria-hidden="true"
        />
        {draftName === null ? (
          <h3 className="project-card-title" title={project.name}>
            {project.name}
          </h3>
        ) : (
          <input
            className="project-card-rename"
            value={draftName}
            autoFocus
            aria-label={`Rename ${project.name}`}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => setDraftName(e.target.value)}
            onBlur={() => {
              onRename(draftName);
              setDraftName(null);
            }}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") setDraftName(null);
            }}
          />
        )}

        <div className="chat-menu-wrap" onClick={(e) => e.stopPropagation()}>
          <button
            className="project-card-more"
            aria-label={`More actions for ${project.name}`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            title="More"
            onClick={() => setMenuOpen((v) => !v)}
          >
            ⋯
          </button>
          {menuOpen && (
            <>
              <div className="row-menu-backdrop" onClick={() => setMenuOpen(false)} />
              <div className="row-menu" role="menu">
                <button
                  className="row-menu-item"
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    setDraftName(project.name);
                  }}
                >
                  Rename project
                </button>
                <button
                  className="row-menu-item"
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    setConfirmingArchive(true);
                  }}
                >
                  Archive project
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      <p className="project-card-path" title={project.rootPath ?? undefined}>
        {project.rootPath ?? "No working folder"}
      </p>

      <div className="project-card-meta">
        <span>{sessionCount} session{sessionCount === 1 ? "" : "s"}</span>
        <span>Updated {formatDate(project.updatedAt)}</span>
        {project.rootPath && (
          <span className="project-card-trust">{TRUST_LABELS[project.trust]}</span>
        )}
      </div>

      <div className="project-card-actions">
        <button
          className="btn-secondary"
          onClick={(e) => {
            e.stopPropagation();
            onOpenChat();
          }}
        >
          Open chat
        </button>
        {!project.rootPath && (
          <span className="project-card-hint">
            <FolderIcon size={12} /> no folder
          </span>
        )}
      </div>

      {confirmingArchive && (
        <span onClick={(e) => e.stopPropagation()}>
          <ConfirmDialog
            title="Archive this project?"
            body={`“${project.name}” and its chats will be hidden from the rail. Nothing on disk is touched, and nothing is deleted.`}
            onCancel={() => setConfirmingArchive(false)}
            onConfirm={() => {
              setConfirmingArchive(false);
              onArchive();
            }}
          />
        </span>
      )}
    </article>
  );
}
