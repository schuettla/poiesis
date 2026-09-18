import { useEffect, useRef, useState } from "react";
import { useActiveConversation, useAppStore, useExpert } from "../../lib/store";
import type { FolderTrust, Project } from "../../lib/types";
import type { ExecPolicy } from "../../lib/api";
import { ChevronIcon, FolderIcon, RefreshIcon } from "../Icons/Icons";

/** The three access levels, in order of how much they let the agent do. Reads
 * are free at every level — this only governs what changes bytes. `short` is
 * the badge on the header line, where there is room for a word or two. */
const TRUST_LEVELS: { id: FolderTrust; label: string; short: string; blurb: string }[] = [
  { id: "read-only", label: "Read only", short: "Read only", blurb: "It can look at files but never change them." },
  { id: "confirm", label: "Ask first", short: "Asks first", blurb: "You approve each change before it happens." },
  { id: "auto", label: "Full", short: "Full access", blurb: "Changes apply straight away. Deleting still asks." },
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

function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/**
 * `IDX-UI-1`'s never-built / building / built / stale line. A plain counting
 * line while building — no bar, no percentage (per the plan's own rule).
 *
 * It sits at the foot of the Files view rather than in the panel head: what I
 * have read of the folder is a fact about its files, and it used to sit above
 * every sub-view, Artifacts and Browser included, where it meant nothing.
 */
export function FolderIndexStatus() {
  const indexState = useAppStore((s) => s.indexState);
  const indexProgress = useAppStore((s) => s.indexProgress);
  const indexError = useAppStore((s) => s.indexError);
  const indexExplained = useAppStore((s) => s.indexExplained);
  const onBuild = useAppStore((s) => s.buildFolderIndex);
  const onCancel = useAppStore((s) => s.cancelFolderIndex);
  const [skippedOpen, setSkippedOpen] = useState(false);
  const building = indexProgress !== null || indexState?.state === "building";

  return (
    <div className="wb-files-foot">
      <div className="wb-index-row">
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
              I've read {plural(indexState.file_count, "file")} · {timeAgo(indexState.updated_at)}
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
      {indexError && <p className="wb-error">{indexError}</p>}
    </div>
  );
}

/**
 * `PRJ-UI-5`: the project this chat belongs to, inside the context popover.
 *
 * The name is a button rather than a label, because the project view is where
 * its instructions, folder and other sessions all are — and being told which
 * project you are in without a way to reach it is a dead end. Moving the chat
 * (`PRJ-9`'s chat-side half) folds open in place rather than stacking a second
 * menu on top of the popover. Neither move ever deletes the chat or touches a
 * byte on disk.
 */
function ProjectSection({ project, onDone }: { project: Project; onDone: () => void }) {
  const projects = useAppStore((s) => s.projects);
  const openProjectView = useAppStore((s) => s.openProjectView);
  const moveSessionToProject = useAppStore((s) => s.moveSessionToProject);
  const conversationId = useAppStore((s) => s.activeConversationId);
  const [moving, setMoving] = useState(false);
  if (!conversationId) return null;
  const others = projects.filter((p) => p.id !== project.id);

  return (
    <section className="wb-pop-section wb-project-row" aria-label="Project">
      <p className="wb-pop-label">Project</p>
      <button
        className="wb-pop-item wb-project-name"
        title={`Open ${project.name}`}
        onClick={() => {
          onDone();
          openProjectView(project.id);
        }}
      >
        {project.name}
      </button>
      {others.length > 0 && (
        <>
          <button className="wb-pop-item wb-pop-quiet" aria-expanded={moving} onClick={() => setMoving((m) => !m)}>
            <span>Move this chat to…</span>
            <ChevronIcon dir="right" size={12} className={`wb-pop-caret ${moving ? "open" : ""}`} />
          </button>
          {moving && (
            <div className="wb-pop-sublist" role="group" aria-label="Move this chat to">
              {others.map((p) => (
                <button
                  key={p.id}
                  className="wb-pop-item"
                  title={p.rootPath ?? p.name}
                  onClick={() => {
                    onDone();
                    moveSessionToProject(conversationId, p.id);
                  }}
                >
                  {p.name}
                </button>
              ))}
            </div>
          )}
        </>
      )}
      {/* Not destructive, and worded so it cannot be read as one: the chat
          goes back to being a loose chat, where it started. */}
      <button
        className="wb-pop-item wb-pop-quiet"
        onClick={() => {
          onDone();
          moveSessionToProject(conversationId, null);
        }}
      >
        Remove from project
      </button>
    </section>
  );
}

const POLICIES: { id: ExecPolicy | "inherit"; label: string; blurb: string }[] = [
  { id: "inherit", label: "Default", blurb: "Follow the setting in Settings → Tools." },
  { id: "off", label: "Off", blurb: "No task runs in this project." },
  { id: "ask", label: "Ask", blurb: "Every run asks first, unless you always allowed that task." },
  { id: "allow", label: "Allow", blurb: "Declared tasks run without asking. Other commands still ask." },
];

const POLICY_LABEL: Record<ExecPolicy, string> = { off: "tasks off", ask: "tasks ask", allow: "tasks allowed" };

/**
 * `COD-UI-1`: what Poiesis knows about the project's code, as one quiet row of
 * chips, and a `Tasks` disclosure with the per-project choices. Nothing
 * detected draws nothing: no empty section.
 */
function ProjectCodeRow({ projectId, trust }: { projectId: string; trust: FolderTrust }) {
  const view = useAppStore((s) => s.projectCards[projectId] ?? null);
  const ownPolicy = useAppStore((s) => s.projects.find((p) => p.id === projectId)?.execPolicy ?? "inherit");
  const refreshProjectCard = useAppStore((s) => s.refreshProjectCard);
  const setProjectExecPolicy = useAppStore((s) => s.setProjectExecPolicy);
  const setProjectTaskAllowed = useAppStore((s) => s.setProjectTaskAllowed);
  const setProjectCommands = useAppStore((s) => s.setProjectCommands);
  const expert = useExpert();
  const [open, setOpen] = useState(false);
  const [detecting, setDetecting] = useState(false);

  useEffect(() => {
    void refreshProjectCard(projectId);
  }, [projectId, refreshProjectCard]);

  if (!view) return null;
  const { card, allow } = view;
  const empty = card.languages.length === 0 && card.tasks.length === 0 && !card.git && !card.instructions_file;
  if (empty) return null;

  const readOnly = trust === "read-only";
  const policy: ExecPolicy = readOnly ? "off" : view.policy;
  const redetect = async () => {
    setDetecting(true);
    try {
      await refreshProjectCard(projectId, true);
    } finally {
      setDetecting(false);
    }
  };

  return (
    <section className="wb-pop-section wb-code" aria-label="Code">
      <p className="wb-pop-label">Code</p>
      <div className="wb-chips">
        {card.languages.length > 0 && (
          <span className="wb-chip" title="Languages, most files first">
            {card.languages.slice(0, 3).join(" + ")}
          </span>
        )}
        {card.git && (
          <span className="wb-chip wb-chip-mono" title="Git branch">
            {card.branch ?? "git"}
          </span>
        )}
        {card.tasks.length > 0 && (
          <button
            className={`wb-chip wb-chip-button ${open ? "on" : ""}`}
            aria-expanded={open}
            onClick={() => setOpen((o) => !o)}
          >
            Tasks {card.tasks.length}
            {view.tasks_enabled && <span className="wb-chip-note"> · {POLICY_LABEL[policy]}</span>}
          </button>
        )}
      </div>

      {open && (
        <div className="wb-tasks">
          {!view.tasks_enabled ? (
            <p className="wb-tasks-note">Running project tasks is off in Settings → Tools.</p>
          ) : readOnly ? (
            <p className="wb-tasks-note">This folder is read only, so no task runs here. A build writes files.</p>
          ) : (
            <div className="wb-trust" role="group" aria-label="Running tasks">
              <span className="wb-trust-label">Run</span>
              <div className="wb-segments">
                {POLICIES.map((p) => (
                  <button
                    key={p.id}
                    className={`wb-segment ${ownPolicy === p.id ? "on" : ""}`}
                    aria-pressed={ownPolicy === p.id}
                    title={p.blurb}
                    onClick={() => setProjectExecPolicy(projectId, p.id)}
                  >
                    {p.label}
                  </button>
                ))}
              </div>
            </div>
          )}

          <ul className="wb-task-list">
            {card.tasks.map((t) => {
              const allowed = allow.tasks.includes(t.name);
              return (
                <li key={t.name} className="wb-task">
                  <div className="wb-task-text">
                    <span className="wb-task-name">{t.name}</span>
                    <code className="wb-task-cmd" title={t.cwd}>
                      {t.argv.join(" ")}
                    </code>
                  </div>
                  {view.tasks_enabled && !readOnly && (
                    <label className="wb-task-allow" title="Run this task without asking">
                      <input
                        type="checkbox"
                        checked={allowed}
                        onChange={(e) => setProjectTaskAllowed(projectId, t.name, e.target.checked)}
                      />
                      always allow
                    </label>
                  )}
                </li>
              );
            })}
          </ul>

          {expert && view.tasks_enabled && !readOnly && (
            <div className="wb-commands">
              <label className="wb-task-allow">
                <input
                  type="checkbox"
                  checked={allow.run_command}
                  onChange={(e) => setProjectCommands(projectId, e.target.checked)}
                />
                Let it ask to run other commands here
              </label>
              {allow.commands.length > 0 && (
                <ul className="wb-task-list">
                  {allow.commands.map((c) => (
                    <li key={c} className="wb-task">
                      <code className="wb-task-cmd">{c}</code>
                      <button className="wb-link" onClick={() => setProjectCommands(projectId, allow.run_command, c)}>
                        Forget
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}

          <div className="wb-tasks-foot">
            <span className="wb-index-status">
              {card.instructions_file
                ? `${card.instructions_file} is read into every chat here`
                : "No AGENTS.md or CLAUDE.md in this folder"}
            </span>
            <button className="wb-link" onClick={redetect} disabled={detecting}>
              {detecting ? "Looking…" : "Re-detect"}
            </button>
          </div>
        </div>
      )}
    </section>
  );
}

/** Everything about where this chat works, behind the header line: project,
 * folder, what the agent may change there, and what it knows about the code.
 * It used to be seven stacked rows above the tabs, always open; almost all of
 * it is set once and then only read, so it waits here until asked for. */
function ContextPopover({ onClose }: { onClose: () => void }) {
  const conversation = useActiveConversation();
  const project = useAppStore((s) =>
    conversation?.projectId ? s.projects.find((p) => p.id === conversation.projectId) : undefined
  );
  const attachFolder = useAppStore((s) => s.attachFolder);
  const detachFolder = useAppStore((s) => s.detachFolder);
  const setFolderTrust = useAppStore((s) => s.setFolderTrust);
  const revealInSystem = useAppStore((s) => s.revealInSystem);
  const showHidden = useAppStore((s) => s.showHidden);
  const toggleShowHidden = useAppStore((s) => s.toggleShowHidden);
  const [confirmDetach, setConfirmDetach] = useState(false);

  const folder = conversation?.folderPath ?? null;
  const trust: FolderTrust = conversation?.folderTrust ?? "confirm";
  const level = TRUST_LEVELS.find((l) => l.id === trust);

  return (
    <div className="wb-popover" role="dialog" aria-label="Folder and project">
      {project && <ProjectSection project={project} onDone={onClose} />}

      <section className="wb-pop-section" aria-label="Folder">
        <p className="wb-pop-label">Folder</p>
        {folder ? (
          <>
            <p className="wb-folder-path" title={folder}>
              {shortPath(folder)}
            </p>
            {confirmDetach ? (
              <div className="wb-confirm">
                {/* Say plainly what detaching does — the word sounds
                    destructive and isn't. With a project it does one thing
                    more (`PRJ-3a`): this chat leaves the project, while the
                    project and its other sessions keep the folder. */}
                <p>
                  {project
                    ? "Stop working in this folder? This chat also leaves the project — the project and its other chats keep it. Nothing on disk is deleted or changed."
                    : "Stop working in this folder? Nothing on disk is deleted or changed."}
                </p>
                <div className="wb-confirm-actions">
                  <button
                    className="wb-primary"
                    onClick={() => {
                      setConfirmDetach(false);
                      onClose();
                      detachFolder();
                    }}
                  >
                    Detach
                  </button>
                  <button className="wb-link" onClick={() => setConfirmDetach(false)}>
                    Cancel
                  </button>
                </div>
              </div>
            ) : (
              <>
                <button className="wb-pop-item" onClick={() => { onClose(); revealInSystem(folder); }}>
                  Show in file manager
                </button>
                <button className="wb-pop-item" onClick={() => { onClose(); attachFolder(); }}>
                  Change folder…
                </button>
                <button className="wb-pop-item" onClick={() => toggleShowHidden()}>
                  {showHidden ? "Hide hidden files" : "Show hidden files"}
                </button>
                <button className="wb-pop-item wb-pop-danger" onClick={() => setConfirmDetach(true)}>
                  Detach folder
                </button>
              </>
            )}
          </>
        ) : (
          <>
            <p className="wb-pop-note">
              Give Poiesis a folder to work in. It can read, search and edit files there — you choose how much it
              may change.
            </p>
            <button className="wb-primary" onClick={() => { onClose(); attachFolder(); }}>
              Choose folder…
            </button>
          </>
        )}
      </section>

      {folder && (
        <section className="wb-pop-section" aria-label="Access">
          <p className="wb-pop-label">What it may change</p>
          <div className="wb-segments wb-segments-fill" role="group" aria-label="File access">
            {TRUST_LEVELS.map((l) => (
              <button
                key={l.id}
                className={`wb-segment ${trust === l.id ? "on" : ""}`}
                aria-pressed={trust === l.id}
                title={l.blurb}
                onClick={() => setFolderTrust(l.id)}
              >
                {l.label}
              </button>
            ))}
          </div>
          {level && <p className="wb-pop-note">{level.blurb}</p>}
        </section>
      )}

      {folder && project && <ProjectCodeRow projectId={project.id} trust={trust} />}
    </div>
  );
}

/**
 * The head of the Workbench: one line saying where this chat works —
 * `project / folder` and how much the agent may change there — and, behind
 * it, a popover with everything you can set about that.
 *
 * `PRJ-UI-5` still holds: a chat in a folderless project names its project
 * here, so being in one never looks like being in none.
 */
export default function FolderHeader() {
  const conversation = useActiveConversation();
  const project = useAppStore((s) =>
    conversation?.projectId ? s.projects.find((p) => p.id === conversation.projectId) : undefined
  );
  const attachFolder = useAppStore((s) => s.attachFolder);
  const refreshTree = useAppStore((s) => s.refreshTree);
  const folderError = useAppStore((s) => s.folderError);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const convId = conversation?.id ?? null;

  // A different chat is a different place; its settings are not the ones
  // that were open.
  useEffect(() => setOpen(false), [convId]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const folder = conversation?.folderPath ?? null;
  const trust: FolderTrust = conversation?.folderTrust ?? "confirm";
  const level = TRUST_LEVELS.find((l) => l.id === trust);

  return (
    <div className="wb-head" ref={ref}>
      <div className="wb-context">
        <button
          className="wb-context-btn"
          aria-haspopup="dialog"
          aria-expanded={open}
          title={folder ?? "Folder and project"}
          onClick={() => setOpen((o) => !o)}
        >
          <span className="wb-folder-icon" aria-hidden="true">
            <FolderIcon size={14} />
          </span>
          <span className="wb-crumbs">
            {project && (
              <>
                <span className="wb-crumb-project">{project.name}</span>
                <span className="wb-crumb-sep" aria-hidden="true">
                  /
                </span>
              </>
            )}
            {folder ? (
              <span className="wb-folder-name">{baseName(folder)}</span>
            ) : (
              <span className="wb-crumb-none">No folder</span>
            )}
          </span>
          {folder && level && (
            <span className={`wb-access wb-access-${trust}`} title={level.blurb}>
              {level.short}
            </span>
          )}
          <ChevronIcon dir="right" size={12} className={`wb-context-caret ${open ? "open" : ""}`} />
        </button>
        {folder ? (
          <button
            className="wb-icon"
            title="Refresh the file list"
            aria-label="Refresh the file list"
            onClick={() => refreshTree()}
          >
            <RefreshIcon size={13} />
          </button>
        ) : (
          <button className="wb-link wb-context-choose" onClick={attachFolder}>
            Choose…
          </button>
        )}
      </div>
      {folderError && <p className="wb-error">{folderError}</p>}
      {open && <ContextPopover onClose={() => setOpen(false)} />}
    </div>
  );
}
