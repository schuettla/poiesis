import { useEffect, useMemo, useRef, useState } from "react";
import { useAppStore } from "../lib/store";
import { NEW_PROJECT_NAME } from "../lib/types";
import type { FolderTrust } from "../lib/types";
import ConfirmDialog from "../components/Confirm/ConfirmDialog";
import "./ProjectView.css";

const TRUST_LABELS: { value: FolderTrust; label: string; detail: string }[] = [
  { value: "read-only", label: "Read only", detail: "Look, never touch." },
  { value: "confirm", label: "Ask first", detail: "Every write and delete raises a prompt." },
  { value: "auto", label: "Full access", detail: "Writes go through; deletes still ask." },
];

/** Edited in place, as a heading — no label, no Save button. Blur or Enter
 * commits, Escape reverts. The same gesture as renaming anything else. */
function NameField({
  name,
  fresh,
  onCommit,
}: {
  name: string;
  fresh: boolean;
  onCommit: (next: string) => void;
}) {
  const [draft, setDraft] = useState(name);
  const ref = useRef<HTMLInputElement>(null);
  // A rename from elsewhere (the Rail's own menu) has to reach this field, but
  // not while the user is typing into it — that would fight them mid-word.
  useEffect(() => {
    if (document.activeElement !== ref.current) setDraft(name);
  }, [name]);

  // `PRJ-UI-1a`: a project that still carries the placeholder name opens with
  // it selected, so the first thing you do after `New project` is type what
  // this project is — the same shape as making a new anything. Once it has a
  // real name, opening the view must not put the cursor in the field and
  // invite an accidental overwrite.
  useEffect(() => {
    if (!fresh) return;
    // Focus *and* select. Selecting alone leaves the caret nowhere the
    // keyboard can reach it, so the user would still have to click before
    // typing — which is the whole thing this is meant to save.
    ref.current?.focus();
    ref.current?.select();
  }, [fresh]);

  return (
    <input
      ref={ref}
      className="pv-name"
      value={draft}
      aria-label="Project name"
      placeholder="Name this project"
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        const next = draft.trim();
        if (next && next !== name) onCommit(next);
        else setDraft(name);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          setDraft(name);
          e.currentTarget.blur();
        }
      }}
    />
  );
}

/** `PRJ-7`. Autosaves on blur rather than on every keystroke: this is prose
 * someone is composing, and a save per character would be a write per
 * character for a field nothing reads until the next turn. */
function InstructionsField({
  instructions,
  onCommit,
}: {
  instructions: string;
  onCommit: (next: string) => void;
}) {
  const [draft, setDraft] = useState(instructions);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (document.activeElement !== ref.current) setDraft(instructions);
  }, [instructions]);

  return (
    <textarea
      ref={ref}
      className="pv-instructions"
      value={draft}
      rows={7}
      aria-label="Project instructions"
      placeholder="What is this project, and how should I work on it? Anything here is carried into every chat in the project."
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        if (draft.trim() !== instructions.trim()) onCommit(draft);
      }}
    />
  );
}

/**
 * `PRJ-UI-4`: the project view.
 *
 * One column, four sections — name, instructions, working folder, sessions —
 * each editable where it is shown. Deliberately not a dashboard: no counts, no
 * charts, no activity feed. It is the four things a project *is*.
 */
export default function ProjectView() {
  const projectId = useAppStore((s) => s.activeProjectId);
  const project = useAppStore((s) => s.projects.find((p) => p.id === s.activeProjectId));
  // Filtered and sorted outside the selector: a selector that builds a new
  // array every call never compares equal, and zustand re-renders forever.
  const allConversations = useAppStore((s) => s.conversations);
  const sessions = useMemo(
    () =>
      allConversations
        .filter((c) => c.projectId === projectId && !c.parentConversationId)
        .sort((a, b) => b.updatedAt - a.updatedAt),
    [allConversations, projectId]
  );
  const renameProject = useAppStore((s) => s.renameProject);
  const setProjectInstructions = useAppStore((s) => s.setProjectInstructions);
  const setProjectFolder = useAppStore((s) => s.setProjectFolder);
  const setFolderTrust = useAppStore((s) => s.setFolderTrust);
  const archiveProject = useAppStore((s) => s.archiveProject);
  const openSession = useAppStore((s) => s.openSession);
  const openProject = useAppStore((s) => s.openProject);
  const moveSessionToProject = useAppStore((s) => s.moveSessionToProject);
  const setView = useAppStore((s) => s.setView);
  const folderError = useAppStore((s) => s.folderError);
  const [confirmingArchive, setConfirmingArchive] = useState(false);

  // The project was archived, or the tab outlived it. Say so rather than
  // rendering an empty column that looks like a loading state forever.
  if (!project || !projectId) {
    return (
      <main className="project-view">
        <p className="pv-gone">
          That project is no longer here. <button onClick={() => setView("chat")}>Close</button>
        </p>
      </main>
    );
  }

  return (
    <main className="project-view">
      <div className="pv-column">
        <NameField
          name={project.name}
          fresh={project.name === NEW_PROJECT_NAME}
          onCommit={(next) => renameProject(projectId, next)}
        />

        <section className="pv-section">
          <h2 className="pv-label">Instructions</h2>
          <p className="pv-hint">Added to every chat in this project.</p>
          <InstructionsField
            instructions={project.instructions ?? ""}
            onCommit={(next) => setProjectInstructions(projectId, next)}
          />
        </section>

        <section className="pv-section">
          <h2 className="pv-label">Working folder</h2>
          {project.rootPath ? (
            <>
              <p className="pv-path" title={project.rootPath}>
                {project.rootPath}
              </p>
              <div className="pv-trust" role="radiogroup" aria-label="What I may do in this folder">
                {TRUST_LABELS.map((t) => (
                  <button
                    key={t.value}
                    className={`pv-trust-btn ${project.trust === t.value ? "active" : ""}`}
                    role="radio"
                    aria-checked={project.trust === t.value}
                    title={t.detail}
                    onClick={() => setFolderTrust(t.value)}
                  >
                    {t.label}
                  </button>
                ))}
              </div>
              <button className="pv-quiet" onClick={() => setProjectFolder(projectId, false)}>
                Remove folder
              </button>
            </>
          ) : (
            <>
              {/* Said plainly, because the whole point of `PRJ-1a` is that a
                  project without a folder is a normal project, not an
                  unfinished one. */}
              <p className="pv-hint">
                A project doesn’t need one. Add a folder if this project is about files on disk.
              </p>
              <button className="pv-action" onClick={() => setProjectFolder(projectId, true)}>
                Add a folder
              </button>
            </>
          )}
          {folderError && <p className="pv-error">{folderError}</p>}
        </section>

        <section className="pv-section">
          <h2 className="pv-label">Sessions</h2>
          {sessions.length === 0 ? (
            <p className="pv-hint">No chats in this project yet.</p>
          ) : (
            <ul className="pv-sessions">
              {sessions.map((c) => (
                <li key={c.id}>
                  <button className="pv-session" onClick={() => openSession(c.id)} title={c.title}>
                    {c.title}
                  </button>
                  {/* Removing a session from a project never deletes it — it
                      goes back to being a loose chat, where it started. */}
                  <button
                    className="pv-quiet pv-session-remove"
                    aria-label={`Remove ${c.title} from this project`}
                    onClick={() => moveSessionToProject(c.id, null)}
                  >
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
          <button className="pv-action" onClick={() => openProject(projectId)}>
            New session in this project
          </button>
        </section>

        <section className="pv-section pv-footer">
          <button className="pv-quiet" onClick={() => setConfirmingArchive(true)}>
            Archive project
          </button>
          <p className="pv-hint">Hides it and its chats. Nothing on disk is touched.</p>
        </section>
      </div>

      {confirmingArchive && (
        <ConfirmDialog
          title="Archive this project?"
          body={`“${project.name}” and its chats will be hidden from the rail. Nothing on disk is touched, and nothing is deleted.`}
          onCancel={() => setConfirmingArchive(false)}
          onConfirm={() => {
            setConfirmingArchive(false);
            archiveProject(projectId);
            setView("chat");
          }}
        />
      )}
    </main>
  );
}
