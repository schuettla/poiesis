import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useAppStore } from "../../lib/store";
import type { Conversation, Project, View } from "../../lib/types";
import { groupByBucket, shortTime } from "../../lib/time";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import ProjectMenuItems from "../Conversation/ProjectMenuItems";
import EngineStatus from "../EngineStatus/EngineStatus";
import { PALETTE_SHORTCUT } from "../CommandPalette/CommandPalette";
import {
  BookmarkIcon,
  ChevronIcon,
  FolderIcon,
  KebabIcon,
  LayoutIcon,
  MessageIcon,
  PlusIcon,
  SearchIcon,
  SettingsIcon,
} from "../Icons/Icons";
import { updateIsWaiting } from "../../lib/updates";
import { leaderOf } from "../../lib/leaders";
import PersonaGlyph, { PoiesisGlyph, TeamGlyph } from "../Personas/PersonaGlyph";
import WaitingDialog from "../Waiting/WaitingDialog";
import "./Rail.css";

/** The ⋯ and its menu, shared by chat and project rows. It sits in the row's
 * trailing slot on top of the timestamp (or count), which it replaces on
 * hover — so the actions cost the title no width at rest. */
function RowMenu({
  label,
  open,
  setOpen,
  children,
}: {
  label: string;
  open: boolean;
  setOpen: (open: boolean) => void;
  children: ReactNode;
}) {
  return (
    <div className="chat-menu-wrap">
      <button
        className="chat-more"
        aria-label={`More actions for ${label}`}
        aria-haspopup="menu"
        aria-expanded={open}
        title="More"
        onClick={(e) => {
          e.stopPropagation();
          setOpen(!open);
        }}
      >
        <KebabIcon size={14} />
      </button>
      {open && (
        <>
          <div
            className="row-menu-backdrop"
            onClick={(e) => {
              e.stopPropagation();
              setOpen(false);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setOpen(false);
            }}
          />
          <div className="row-menu" role="menu" onClick={(e) => e.stopPropagation()}>
            {children}
          </div>
        </>
      )}
    </div>
  );
}

/** `PRJ-UI-1`: one project, and its sessions in place of the flat list when it
 * is expanded. A group, not a second navigation model — the row behaves like a
 * chat row (click opens, ⋯ for the rest), because the Rail keeps being the
 * Rail. */
function ProjectRow({
  project,
  sessions,
  activeId,
  activeProjectId,
  view,
  now,
}: {
  project: Project;
  sessions: Conversation[];
  activeId: string | null;
  activeProjectId: string | null;
  view: View;
  now: number;
}) {
  const expanded = useAppStore((s) => s.expandedProjects.includes(project.id));
  const toggle = useAppStore((s) => s.toggleProjectExpanded);
  // `PRJ-UI-4`: clicking a project opens *the project*, not one of its chats.
  const openProject = useAppStore((s) => s.openProjectView);
  const renameProject = useAppStore((s) => s.renameProject);
  const archiveProject = useAppStore((s) => s.archiveProject);
  const [menuOpen, setMenuOpen] = useState(false);
  const [draftName, setDraftName] = useState<string | null>(null);
  const current = view === "project" && activeProjectId === project.id;
  const containsActive = view === "chat" && sessions.some((c) => c.id === activeId);

  return (
    <li className={`project-row-wrap ${expanded ? "expanded" : ""}`}>
      <div
        className={`project-row ${current ? "current" : ""} ${containsActive ? "contains-active" : ""}`}
        title={project.rootPath ?? project.name}
        tabIndex={0}
        aria-current={current ? "page" : undefined}
        onClick={() => openProject(project.id)}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenuOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter") openProject(project.id);
          if (e.key === "ArrowRight" && !expanded) toggle(project.id);
          if (e.key === "ArrowLeft" && expanded) toggle(project.id);
        }}
      >
        {/* Only shows what is inside; the row itself opens the project. */}
        <button
          className="project-twisty"
          aria-label={expanded ? `Collapse ${project.name}` : `Expand ${project.name}`}
          aria-expanded={expanded}
          onClick={(e) => {
            e.stopPropagation();
            toggle(project.id);
          }}
        >
          <ChevronIcon dir="right" size={12} strokeWidth={1.6} />
        </button>
        {/* `PRJ-UI-1a`: filled with a working folder, hollow without. */}
        <span
          className={`project-dot ${project.rootPath ? "has-folder" : ""}`}
          aria-hidden="true"
        />
        {draftName === null ? (
          <span className="chat-title">{project.name}</span>
        ) : (
          <input
            className="project-rename"
            value={draftName}
            autoFocus
            aria-label={`Rename ${project.name}`}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => setDraftName(e.target.value)}
            onBlur={() => {
              renameProject(project.id, draftName);
              setDraftName(null);
            }}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") setDraftName(null);
            }}
          />
        )}
        <span className="row-slot">
          <span className="row-stamp project-count" aria-label={`${sessions.length} sessions`}>
            {sessions.length}
          </span>
          <RowMenu label={project.name} open={menuOpen} setOpen={setMenuOpen}>
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
            {/* Archive, never delete: nothing here touches a byte on disk. */}
            <button
              className="row-menu-item"
              role="menuitem"
              onClick={() => {
                setMenuOpen(false);
                archiveProject(project.id);
              }}
            >
              Archive project
            </button>
          </RowMenu>
        </span>
      </div>

      {expanded && (
        <ul className="chat-list project-sessions">
          {sessions.length === 0 && <li className="project-empty">No sessions yet</li>}
          {sessions.map((c) => (
            <ChatRow key={c.id} c={c} active={c.id === activeId && view === "chat"} now={now} />
          ))}
        </ul>
      )}
    </li>
  );
}

/** One conversation. At rest: the title, the `◆` if reflection taught me
 * something (PRES-2), and when it was last touched. On hover the time gives
 * its slot to the ⋯, which holds everything you can *do* to the chat —
 * including "reflect now" (REF-UI-2), which used to be a second hover button
 * reserving its own width on every row. */
function ChatRow({ c, active, now }: { c: Conversation; active: boolean; now: number }) {
  // `SHL-24`: a chat is a destination. Selecting one here goes to it.
  const openSession = useAppStore((s) => s.openSession);
  const reflect = useAppStore((s) => s.reflectConversation);
  const reflecting = useAppStore((s) => s.reflectingIds.includes(c.id));
  const digested = useAppStore((s) => s.digestedIds.includes(c.id));
  const deleteConversation = useAppStore((s) => s.deleteConversation);
  const scheduleConversation = useAppStore((s) => s.scheduleConversation);
  const leaderId = leaderOf(c);
  const leader = useAppStore((s) =>
    leaderId.startsWith("persona:") ? s.personas.find((p) => `persona:${p.id}` === leaderId) : undefined
  );
  const team = useAppStore((s) => (leaderId.startsWith("team:") ? s.teams.find((t) => `team:${t.slug}` === leaderId) : undefined));
  const coordinator = useAppStore((s) => (team ? s.personas.find((p) => `persona:${p.id}` === team.coordinator) : undefined));
  // `TEAM-19`/`TEAM-20`: something waits for the person here, or a run changed this chat while they were away.
  const waiting = useAppStore((s) => s.waiting.some((p) => p.session_id === c.id));
  const unread = useAppStore((s) => s.unreadIds.includes(c.id));
  // Removing a chat takes two steps: the menu, then a confirmation.
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);

  return (
    <li
      className={`chat-row ${active ? "active" : ""} ${waiting ? "waiting" : ""}`}
      title={c.title}
      tabIndex={0}
      aria-current={active ? "page" : undefined}
      onClick={() => openSession(c.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenuOpen(true);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" && e.target === e.currentTarget) openSession(c.id);
      }}
    >
      {/* `TEAM-6`: a chat a persona leads wears its face; Poiesis-led chats have no
          marker, so the list looks as it always did until personas are used. */}
      {leader && (
        <span className="chat-leader" role="img" aria-label={`Led by ${leader.name}`} title={`Led by ${leader.name}`}>
          <PersonaGlyph name={leader.name} look={leader.look} size={14} />
        </span>
      )}
      {team && (
        <span className="chat-leader" role="img" aria-label={`Led by ${team.name}`} title={`Led by ${team.name}`}>
          <TeamGlyph coordinator={coordinator} size={14} />
        </span>
      )}
      <span className="chat-title">{c.title}</span>
      {waiting && (
        <span className="chat-waiting" role="img" aria-label="Waiting for you" title="Waiting for you">
          ◌
        </span>
      )}
      {unread && !waiting && (
        <span className="chat-unread" role="img" aria-label="Changed while you were away" title="Changed while you were away" />
      )}
      {c.workspace && (
        <span className="chat-workspace" role="img" aria-label="Workspace session" title="Workspace session">
          <LayoutIcon size={12} />
        </span>
      )}
      {reflecting ? (
        <span className="chat-digest reflecting" role="status" aria-label="I'm reflecting on this conversation">
          ◆
        </span>
      ) : (
        digested && (
          <span
            className="chat-digest"
            role="img"
            aria-label="I learned something from this conversation"
            title="I learned something from this conversation"
          >
            ◆
          </span>
        )
      )}
      <span className="row-slot">
        <time className="row-stamp" dateTime={new Date(c.updatedAt).toISOString()}>
          {shortTime(c.updatedAt, now)}
        </time>
        <RowMenu label={c.title} open={menuOpen} setOpen={setMenuOpen}>
          {!reflecting && !digested && (
            <button
              className="row-menu-item"
              role="menuitem"
              onClick={() => {
                setMenuOpen(false);
                reflect(c.id);
              }}
            >
              Reflect on this chat
            </button>
          )}
          <button
            className="row-menu-item"
            role="menuitem"
            onClick={() => {
              setMenuOpen(false);
              scheduleConversation(c.id);
            }}
          >
            Schedule this…
          </button>
          <ProjectMenuItems conversationId={c.id} onDone={() => setMenuOpen(false)} />
          <hr className="row-menu-sep" />
          <button
            className="row-menu-item danger"
            role="menuitem"
            onClick={() => {
              setMenuOpen(false);
              setConfirming(true);
            }}
          >
            Delete chat
          </button>
        </RowMenu>
      </span>

      {confirming && (
        // Rendered inside the row, so its clicks must not reach the row.
        <span onClick={(e) => e.stopPropagation()}>
          <ConfirmDialog
            title="Delete this chat?"
            body={`“${c.title}” and everything said in it will be removed. This can't be undone.`}
            onCancel={() => setConfirming(false)}
            onConfirm={() => {
              setConfirming(false);
              deleteConversation(c.id);
            }}
          />
        </span>
      )}
    </li>
  );
}

/** Re-render once a minute, so `now` and `12m` don't go stale on a rail
 * that nothing else happens to touch. Only a trigger: the time itself is read
 * at render, so it is never older than the data it is compared with. */
function useMinuteTick(): void {
  const [, setTick] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => setTick((t) => t + 1), 60_000);
    return () => clearInterval(timer);
  }, []);
}

/** `TEAM-6`: the sidebar's ⋯ and, when a filter is on, the one line that says so and takes it
 * off. Only there once there is a persona to filter by, so the sidebar stays as it was for
 * anyone who has none. */
function LeaderFilter({
  filter,
  filterName,
  counts,
}: {
  filter: string | null;
  filterName: string | null;
  counts: Map<string, number>;
}) {
  const personas = useAppStore((s) => s.personas);
  const teams = useAppStore((s) => s.teams);
  const setLeaderFilter = useAppStore((s) => s.setLeaderFilter);
  const [open, setOpen] = useState(false);
  const [listed, setListed] = useState(false);
  const leaders = [
    { id: "poiesis", name: "Poiesis", glyph: <PoiesisGlyph size={14} /> },
    ...personas.map((p) => ({ id: `persona:${p.id}`, name: p.name, glyph: <PersonaGlyph name={p.name} look={p.look} size={14} /> })),
    ...teams.map((t) => ({
      id: `team:${t.slug}`,
      name: t.name,
      glyph: <TeamGlyph coordinator={personas.find((p) => `persona:${p.id}` === t.coordinator)} size={14} />,
    })),
  ];
  const close = () => {
    setOpen(false);
    setListed(false);
  };
  return (
    <div className="rail-filter">
      {filter && (
        <button
          className="rail-filter-chip"
          title="Show every chat"
          aria-label={`Led by ${filterName}. Show every chat`}
          onClick={() => setLeaderFilter(null)}
        >
          <span className="row-menu-label">Led by {filterName}</span>
          <span aria-hidden="true">×</span>
        </button>
      )}
      <div className="rail-filter-menu">
        <button
          className="chat-more rail-more"
          aria-label="More chat options"
          aria-haspopup="menu"
          aria-expanded={open}
          title="More"
          onClick={() => (open ? close() : setOpen(true))}
        >
          <KebabIcon size={14} />
        </button>
        {open && (
          <>
            <div className="row-menu-backdrop" onClick={close} onContextMenu={(e) => { e.preventDefault(); close(); }} />
            <div className="row-menu" role="menu">
              <button
                className="row-menu-item row-menu-parent"
                role="menuitem"
                aria-haspopup="true"
                aria-expanded={listed}
                onClick={() => setListed((o) => !o)}
              >
                <span className="row-menu-label">Show chats led by…</span>
                <ChevronIcon dir="right" size={12} className={`row-menu-caret ${listed ? "open" : ""}`} />
              </button>
              {listed && (
                <div className="row-menu-sub" role="group" aria-label="Leaders">
                  {leaders.map((l) => (
                    <button
                      key={l.id}
                      className={`row-menu-item rail-leader-item ${filter === l.id ? "active" : ""}`}
                      role="menuitemradio"
                      aria-checked={filter === l.id}
                      disabled={!counts.get(l.id)}
                      onClick={() => {
                        close();
                        setLeaderFilter(l.id);
                      }}
                    >
                      {l.glyph}
                      <span className="row-menu-label">{l.name}</span>
                      <span className="row-stamp">{counts.get(l.id) ?? 0}</span>
                    </button>
                  ))}
                </div>
              )}
              {filter && (
                <button
                  className="row-menu-item"
                  role="menuitem"
                  onClick={() => {
                    close();
                    setLeaderFilter(null);
                  }}
                >
                  <span className="row-menu-label">Show every chat</span>
                </button>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default function Rail() {
  // `SUB-3`: a delegated child belongs to the turn that started it — the Fleet
  // card and the Agents tab are where it is opened from.
  const allConversations = useAppStore((s) => s.conversations);
  // Filtered outside the selector: a selector that builds a new array every
  // call never compares equal, and zustand re-renders forever.
  const topLevel = useMemo(
    () => allConversations.filter((c) => !c.parentConversationId),
    [allConversations]
  );
  // `TEAM-6`: the filter "Show chats led by…", off when it names someone who is gone.
  const personas = useAppStore((s) => s.personas);
  const leaderFilter = useAppStore((s) => s.leaderFilter);
  const teams = useAppStore((s) => s.teams);
  const filterPersona = leaderFilter?.startsWith("persona:") ? personas.find((p) => `persona:${p.id}` === leaderFilter) : undefined;
  const filterTeam = leaderFilter?.startsWith("team:") ? teams.find((t) => `team:${t.slug}` === leaderFilter) : undefined;
  const filter = leaderFilter === "poiesis" || filterPersona || filterTeam ? leaderFilter : null;
  const filterName = filter === "poiesis" ? "Poiesis" : (filterPersona?.name ?? filterTeam?.name ?? null);
  const ledCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const c of topLevel) counts.set(leaderOf(c), (counts.get(leaderOf(c)) ?? 0) + 1);
    return counts;
  }, [topLevel]);
  const conversations = useMemo(
    () => (filter ? topLevel.filter((c) => leaderOf(c) === filter) : topLevel),
    [topLevel, filter]
  );
  // `PRJ-UI-1`: sessions in a project are listed under it, so the date groups
  // below hold only the loose chats.
  const projects = useAppStore((s) => s.projects);
  const newProject = useAppStore((s) => s.newProject);
  const looseConversations = useMemo(
    () => conversations.filter((c) => !c.projectId),
    [conversations]
  );
  const sessionsByProject = useMemo(() => {
    const byProject = new Map<string, Conversation[]>();
    for (const c of conversations) {
      if (!c.projectId) continue;
      const list = byProject.get(c.projectId);
      if (list) list.push(c);
      else byProject.set(c.projectId, [c]);
    }
    return byProject;
  }, [conversations]);
  // With a filter on, a project with none of the chats it matches is left out.
  const shownProjects = filter ? projects.filter((p) => sessionsByProject.has(p.id)) : projects;
  const activeId = useAppStore((s) => s.activeConversationId);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const newConversation = useAppStore((s) => s.newConversation);
  const setPaletteOpen = useAppStore((s) => s.setPaletteOpen);
  const collapsed = useAppStore((s) => s.railCollapsed);
  // Something about the agent's self is waiting on an answer (SOUL-UI-3).
  const soulPending = useAppStore((s) =>
    s.changeProposals.some((p) => p.target === "soul")
  );
  const selfPending = useAppStore((s) =>
    s.changeProposals.some((p) => p.target !== "soul")
  );
  const consolidationPending = useAppStore((s) => s.consolidationPending);
  // SCH-UI-4: a scheduled job runs unattended, but the user must always be
  // able to see that it's happening, and end it.
  const runningJob = useAppStore((s) => s.runningJob);
  // `TEAM-19`: what runs nobody watched left for the person.
  const waitingCount = useAppStore((s) => s.waiting.length);
  const waitingOpen = useAppStore((s) => s.waitingOpen);
  const setWaitingOpen = useAppStore((s) => s.setWaitingOpen);
  const stopScheduledJob = useAppStore((s) => s.stopScheduledJob);
  // `UPD-UI-3`: one badge covers whatever is waiting in any of Settings'
  // sections — a proposal to review, or a newer version of me to install.
  const updateWaiting = useAppStore((s) => updateIsWaiting(s.updateState));
  const reviewPending = soulPending || selfPending || consolidationPending;
  const settingsPending = reviewPending || updateWaiting;
  const inSettingsHub = [
    "models",
    "providers",
    "runtime",
    "apps",
    "skills",
    "self",
    "personas",
    "tasks",
    "activity",
    "settings",
    "workingdir",
    "mail",
    "tools",
    "about",
  ].includes(view);

  const setActive = useAppStore((s) => s.openSession);
  const [sessionMenuOpen, setSessionMenuOpen] = useState(false);
  useMinuteTick();
  const now = Date.now();
  const groups = groupByBucket(looseConversations, now);

  return (
    <nav className={`rail ${collapsed ? "collapsed" : ""}`} aria-label="Conversations and sections">
      <div className="rail-top-actions">
        <button className="rail-top-btn new-chat" onClick={newConversation} title="New chat">
          <span className="nav-icon" aria-hidden="true"><PlusIcon /></span>
          <span className="nav-label">New chat</span>
        </button>
        {/* Search lives in the palette: chats by title *and* message text,
            projects, the library and commands, in one list. */}
        <button
          className="rail-top-btn search-btn"
          onClick={() => setPaletteOpen(true)}
          title={`Search (${PALETTE_SHORTCUT})`}
        >
          <span className="nav-icon" aria-hidden="true"><SearchIcon /></span>
          <span className="nav-label">Search</span>
          <kbd className="kbd rail-kbd">{PALETTE_SHORTCUT}</kbd>
        </button>
        <button
          className={`rail-top-btn library-btn ${view === "library" ? "active" : ""}`}
          onClick={() => setView("library")}
          title="Library"
        >
          <span className="nav-icon" aria-hidden="true"><BookmarkIcon /></span>
          <span className="nav-label">Library</span>
        </button>
        {/* `PRJ-3`: the overview is the destination; the `+` extension is the
            quick create the button used to be. */}
        <div className="rail-top-btn-row">
          <button
            className={`rail-top-btn projects-btn ${view === "projects" ? "active" : ""}`}
            onClick={() => setView("projects")}
            title="Projects"
          >
            <span className="nav-icon" aria-hidden="true"><FolderIcon /></span>
            <span className="nav-label">Projects</span>
          </button>
          <button
            className="rail-top-btn-add"
            onClick={newProject}
            title="New project"
            aria-label="New project"
          >
            <PlusIcon size={13} />
          </button>
        </div>
      </div>

      {collapsed && (
        <div className="rail-session-collapsed">
          <button
            className="rail-session-btn"
            aria-haspopup="menu"
            aria-expanded={sessionMenuOpen}
            title="Chats"
            onClick={() => setSessionMenuOpen((v) => !v)}
          >
            <MessageIcon />
          </button>
          {sessionMenuOpen && (
            <>
              <div
                className="row-menu-backdrop"
                onClick={() => setSessionMenuOpen(false)}
              />
              <div className="row-menu rail-session-menu" role="menu">
                {conversations.length === 0 && (
                  <span className="row-menu-empty">No chats yet</span>
                )}
                {conversations.map((c) => (
                  <button
                    key={c.id}
                    className={`row-menu-item ${
                      c.id === activeId && view === "chat" ? "active" : ""
                    }`}
                    role="menuitem"
                    title={c.title}
                    onClick={() => {
                      setActive(c.id);
                      setSessionMenuOpen(false);
                    }}
                  >
                    <span className="row-menu-label">{c.title}</span>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      )}

      {/* `SHL-16` is withdrawn: this column lists conversations and projects,
          whatever route is focused. */}
      <div className="rail-scroll">
        {waitingCount > 0 && (
          <button className="rail-waiting-row" onClick={() => setWaitingOpen(true)}>
            <span className="rail-waiting-mark" aria-hidden="true">
              ◌
            </span>
            <span className="rail-waiting-label">Waiting for you · {waitingCount}</span>
          </button>
        )}
        {waitingOpen && <WaitingDialog />}
        {runningJob && (
          <div className="rail-job-row" role="status">
            <span className="rail-job-dot" aria-hidden="true" />
            <span className="rail-job-label">Running “{runningJob.job_name}”…</span>
            <button
              className="rail-job-stop"
              aria-label={`Stop ${runningJob.job_name}`}
              title="Stop"
              onClick={() => stopScheduledJob()}
            >
              ■
            </button>
          </div>
        )}

        {(personas.length > 0 || filter) && <LeaderFilter filter={filter} filterName={filterName} counts={ledCounts} />}
        {filter && conversations.length === 0 && <p className="rail-filter-empty">No chats led by {filterName}.</p>}

        {/* `PRJ-UI-1`: a group above the date-grouped chats. Absent entirely
            with no projects, so a user who never made one sees only chats. */}
        {shownProjects.length > 0 && (
          <div className="rail-group">
            <p className="rail-label">Projects</p>
            <ul className="chat-list project-list">
              {shownProjects.map((p) => (
                <ProjectRow
                  key={p.id}
                  project={p}
                  sessions={sessionsByProject.get(p.id) ?? []}
                  activeId={activeId}
                  activeProjectId={activeProjectId}
                  view={view}
                  now={now}
                />
              ))}
            </ul>
          </div>
        )}

        {groups.map((g) => (
          <div className="rail-group" key={g.label}>
            <p className="rail-label">{g.label}</p>
            <ul className="chat-list">
              {g.items.map((c) => (
                <ChatRow key={c.id} c={c} active={c.id === activeId && view === "chat"} now={now} />
              ))}
            </ul>
          </div>
        ))}
      </div>

      <hr className="rail-divider" />

      <ul className="rail-nav rail-nav-footer">
        <li
          className={inSettingsHub ? "active" : ""}
          tabIndex={0}
          onClick={() => setView("settings")}
          onKeyDown={(e) => {
            if (e.key === "Enter") setView("settings");
          }}
          title="Settings"
        >
          <span className="nav-icon" aria-hidden="true"><SettingsIcon /></span>
          <span className="nav-label">Settings</span>
          {settingsPending && (
            <span
              className="nav-badge"
              role="img"
              aria-label={reviewPending ? "Changes waiting for review" : "An update is waiting"}
              title={reviewPending ? "Changes waiting for review" : "An update is waiting"}
            />
          )}
          {/* The local engine's state, beside the cog: the row you press when
              the engine is what you want to do something about. */}
          <EngineStatus dotOnly={collapsed} />
        </li>
      </ul>
    </nav>
  );
}
