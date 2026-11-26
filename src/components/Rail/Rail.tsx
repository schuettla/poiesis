import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
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
  FilterIcon,
  FolderIcon,
  KebabIcon,
  LayoutIcon,
  PaperPlaneIcon,
  PhoneIcon,
  MessageIcon,
  PlusIcon,
  SearchIcon,
  SettingsIcon,
} from "../Icons/Icons";
import { updateIsWaiting } from "../../lib/updates";
import { sessionOutputs, stillWorking } from "../../lib/api";
import { chatNow, chatState, runOfChat, type ChatState } from "../../lib/chatState";
import {
  folderName,
  folderOf,
  groupChats,
  hasOutput,
  hasRailFilters,
  matchesRailFilters,
  NO_PROJECT,
  OUTPUT_ORDER,
  OUTPUT_WORD,
  RAIL_GROUPS,
  STATUS_ORDER,
  STATUS_WORD,
  type OutputKind,
  type RailContext,
  type RailFilters,
  type RailGroup,
} from "../../lib/railView";
import { agentLogoUrl } from "../../lib/engines";
import { clockText } from "../../lib/liveWords";
import AppLogo from "../Apps/AppLogo";
import { ChatNow, ChatSlot } from "./ChatState";
import { leaderOf } from "../../lib/leaders";
import PersonaGlyph, { PoiesisGlyph, TeamGlyph } from "../Personas/PersonaGlyph";
import WaitingDialog from "../Waiting/WaitingDialog";
import { useSessionHover } from "./SessionHoverCard";
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

/** Drag the rail's right edge to resize it, like the Workbench's left edge. The width comes from
 * the pointer's distance to the window's left edge, so the divider tracks it exactly. */
function RailResizer() {
  const railWidth = useAppStore((s) => s.railWidth);
  const setRailWidth = useAppStore((s) => s.setRailWidth);
  const setRailDragging = useAppStore((s) => s.setRailDragging);

  const startDrag = (e: React.PointerEvent) => {
    e.preventDefault();
    setRailDragging(true);
    const move = (ev: PointerEvent) => setRailWidth(ev.clientX);
    const up = () => {
      setRailDragging(false);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };

  return (
    <div
      className="rail-resizer"
      onPointerDown={startDrag}
      onDoubleClick={() => setRailWidth(232)}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the sidebar"
      aria-valuenow={railWidth}
      aria-valuemin={200}
      aria-valuemax={420}
      tabIndex={0}
      title="Drag to resize · double-click to reset"
      onKeyDown={(e) => {
        if (e.key === "ArrowRight") setRailWidth(railWidth + 24);
        if (e.key === "ArrowLeft") setRailWidth(railWidth - 24);
      }}
    />
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

/** `UI-3`: the helpers of each chat, found once by `Rail` and read by the rows. A context,
 * because a chat row sits in three lists (loose, in a project, in a filter) and each would
 * otherwise need the same three props. */
interface RailTreeValue {
  kids: Map<string, Conversation[]>;
  /** What the person chose for each chat: open (true) or closed (false). */
  open: Record<string, boolean>;
  setOpen: (convId: string, to: boolean) => void;
}
const RailTree = createContext<RailTreeValue>({ kids: new Map(), open: {}, setOpen: () => {} });

const HELPERS_OPEN_KEY = "poiesis.rail.helpers";

function loadHelpersOpen(): Record<string, boolean> {
  if (typeof window === "undefined") return {};
  try {
    const raw = JSON.parse(window.localStorage.getItem(HELPERS_OPEN_KEY) ?? "{}");
    return raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
  } catch {
    return {};
  }
}
function saveHelpersOpen(open: Record<string, boolean>) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(HELPERS_OPEN_KEY, JSON.stringify(open));
  } catch {
    /* ignore */
  }
}

/** The helper list shows this many entries, then one row "{n} more". */
const HELPERS_SHOWN = 5;

/** Re-render each second while something runs, for the clock. */
function useSecondTick(on: boolean): void {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!on) return;
    const timer = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(timer);
  }, [on]);
}

/** `UI-3`: one helper under its chat: a job of a helper, an agent app or a team member. A click
 * opens what the Fleet card and the Coordination panel open: the job's own stream, as a tab of the
 * chat that started it. Its conversation alone has no stream to read. */
function HelperRow({ c, now }: { c: Conversation; now: number }) {
  const hover = useSessionHover(c);
  const openSession = useAppStore((s) => s.openSession);
  const openItem = useAppStore((s) => s.openItem);
  const stopSubRun = useAppStore((s) => s.stopSubRun);
  const openTurnOf = useAppStore((s) => s.openTurnOf);
  const state = useAppStore((s) => chatState(s, c.id));
  const doing = useAppStore((s) => chatNow(s, c.id));
  const run = useAppStore((s) => runOfChat(s, c.id));
  const active = useAppStore((s) =>
    s.view === "chat" && (run ? s.activeItemId === `run:${run.runId}` : s.activeConversationId === c.id)
  );
  const open = () =>
    run ? openItem({ kind: "run", id: run.runId, conversationId: run.parentConversationId }) : openSession(c.id);
  const persona = useAppStore((s) => {
    const leader = leaderOf(c);
    if (leader.startsWith("persona:")) return s.personas.find((p) => `persona:${p.id}` === leader);
    return run && !run.engine ? s.personas.find((p) => p.name === run.agent) : undefined;
  });
  const working = !!run && stillWorking(run.status);
  useSecondTick(working);
  const [menuOpen, setMenuOpen] = useState(false);
  const title = run?.member ? `${run.member}: ${c.title}` : c.title;

  return (
    <li
      className={`helper-row ${active ? "active" : ""}`}
      tabIndex={0}
      aria-current={active ? "page" : undefined}
      {...hover.rowProps}
      onClick={open}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenuOpen(true);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" && e.target === e.currentTarget) open();
      }}
    >
      <div className="helper-line">
        {/* Marks go where a chat row keeps them: who it is before the title, what it is doing after,
            so no helper is indented for a mark it does not have. */}
        {run?.engine ? (
          <span className="helper-mark">
            <AppLogo slug={run.engine.id} name={run.engine.name} src={agentLogoUrl(run.engine.logo)} />
          </span>
        ) : persona ? (
          <span className="chat-leader" role="img" aria-label={`Led by ${persona.name}`}>
            <PersonaGlyph name={persona.name} look={persona.look} size={14} />
          </span>
        ) : null}
        <span className="chat-title">{title}</span>
        {state !== "idle" && <ChatSlot state={state} />}
        <span className="row-slot">
          {working && run ? (
            <span className="row-stamp helper-clock">{clockText(Date.now() - run.startedAt)}</span>
          ) : (
            <time className="row-stamp" dateTime={new Date(c.updatedAt).toISOString()}>
              {shortTime(c.updatedAt, now)}
            </time>
          )}
          {run && (
            <RowMenu label={title} open={menuOpen} setOpen={setMenuOpen}>
              {working && (
                <button
                  className="row-menu-item"
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    void stopSubRun(run.runId);
                  }}
                >
                  Stop
                </button>
              )}
              <button
                className="row-menu-item"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  void openTurnOf(run.runId);
                }}
              >
                Open the chat it came from
              </button>
            </RowMenu>
          )}
        </span>
      </div>
      <ChatNow text={doing} indent={run?.engine || persona ? 20 : 0} />
      {hover.cardPortal}
    </li>
  );
}

function HelperList({ kids, now }: { kids: Conversation[]; now: number }) {
  const [all, setAll] = useState(false);
  const shown = all ? kids : kids.slice(0, HELPERS_SHOWN);
  return (
    <ul className="helper-list">
      {shown.map((k) => (
        <HelperRow key={k.id} c={k} now={now} />
      ))}
      {!all && kids.length > HELPERS_SHOWN && (
        <li className="helper-more-row">
          <button className="helper-more" onClick={() => setAll(true)}>
            {kids.length - HELPERS_SHOWN} more
          </button>
        </li>
      )}
    </ul>
  );
}

/** One conversation. At rest: the title, the `◆` if reflection taught me
 * something (PRES-2), and when it was last touched. On hover the time gives
 * its slot to the ⋯, which holds everything you can *do* to the chat —
 * including "reflect now" (REF-UI-2), which used to be a second hover button
 * reserving its own width on every row.
 *
 * `UI-1`: the slot at the start says what the chat is doing; `UI-2`: while it works
 * or waits, a second line says what; `UI-3`: its helpers hang under it. */
function ChatRow({ c, active, now }: { c: Conversation; active: boolean; now: number }) {
  const hover = useSessionHover(c);
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
  // `TEAM-19`/`TEAM-20`/`UI-1`: something waits for the person here, a run is going, it failed,
  // or it changed while they were away. One word, decided in one place.
  const state = useAppStore((s) => chatState(s, c.id));
  const doing = useAppStore((s) => chatNow(s, c.id));
  const waiting = useAppStore((s) => s.waiting.some((p) => p.session_id === c.id));
  const tree = useContext(RailTree);
  const kids = tree.kids.get(c.id);
  const kidActive = useAppStore((s) => !!kids && s.view === "chat" && kids.some((k) => k.id === s.activeConversationId));
  const helpersOpen = tree.open[c.id] ?? (state === "running" || state === "needs-you" || active || kidActive);
  // Removing a chat takes two steps: the menu, then a confirmation.
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);
  // The second line starts under the title, which a leader's face pushes right (14px and the 6px gap).
  const indent = leader || team || c.remoteSender ? 20 : 0;

  return (
    <li className="chat-group">
      <div
        className={`chat-row ${active ? "active" : ""} ${waiting ? "waiting" : ""}`}
        tabIndex={0}
        aria-current={active ? "page" : undefined}
        {...hover.rowProps}
        onClick={() => openSession(c.id)}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenuOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && e.target === e.currentTarget) openSession(c.id);
        }}
      >
        <div className="chat-line">

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
          {/* `REM-UI-3`: a chat a phone or a chat app started says where it came from. */}
          {c.remoteSender && (
            <span
              className="chat-leader"
              role="img"
              aria-label={`Started from ${c.remoteSender}`}
              title={`Started from ${c.remoteSender}`}
            >
              {c.remoteTransport === "telegram" ? <PaperPlaneIcon size={13} /> : <PhoneIcon size={13} />}
            </span>
          )}
          <span className="chat-title">{c.title}</span>
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
          {/* `UI-1`: what the chat is doing, after the title where the row always kept its marks, so
              no row is indented for it. `UI-3`: a chat with helpers has its chevron in the same
              place: alone while the chat is idle, in place of the state glyph on hover. */}
          {(state !== "idle" || kids) && (
            <ChatSlot
              state={state}
              twisty={
                kids && (
                  <button
                    className="chat-twisty"
                    aria-label={helpersOpen ? `Hide helpers of ${c.title}` : `Show helpers of ${c.title}`}
                    aria-expanded={helpersOpen}
                    onClick={(e) => {
                      e.stopPropagation();
                      tree.setOpen(c.id, !helpersOpen);
                    }}
                  >
                    <ChevronIcon dir="right" size={12} strokeWidth={1.6} />
                  </button>
                )
              }
            />
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
        </div>
        <ChatNow text={doing} indent={indent} />

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
      </div>
      {kids && helpersOpen && <HelperList kids={kids} now={now} />}
      {hover.cardPortal}
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

/** What each filter option would leave, so an option that leaves nothing can be greyed. */
interface FilterStats {
  status: Map<string, number>;
  output: Record<OutputKind, number>;
  folders: { path: string; count: number }[];
  projects: { id: string; name: string; count: number }[];
  noProject: number;
}

const POPUP_WIDTH = 200;

/** A popup of the funnel menu. Fixed and out of the rail: the rail clips whatever leaves it, and
 * a flyout is meant to leave it. Kept inside the window once its height is known. */
function Popup({
  top,
  left,
  className = "",
  label,
  children,
  onPointerEnter,
}: {
  top: number;
  left: number;
  className?: string;
  label?: string;
  children: ReactNode;
  onPointerEnter?: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.top = `${Math.max(8, Math.min(top, window.innerHeight - el.offsetHeight - 8))}px`;
    el.style.left = `${Math.max(8, Math.min(left, window.innerWidth - el.offsetWidth - 8))}px`;
  }, [top, left, children]);
  return (
    <div
      ref={ref}
      className={`row-menu rail-popup ${className}`}
      role="menu"
      aria-label={label}
      style={{ top, left }}
      onPointerEnter={onPointerEnter}
    >
      {children}
    </div>
  );
}

/** `TEAM-6`: the funnel in the sidebar: how the chats are grouped, and which of them to show
 * (led by, status, what they made, folder, project). Each choice opens a flyout beside the menu.
 * When something is on, one line says so and takes it all off. It owns no row: it sits at the
 * end of the first group's label. */
function RailFilter({
  filter,
  filterName,
  counts,
  filters,
  group,
  stats,
  onOpen,
}: {
  filter: string | null;
  filterName: string | null;
  counts: Map<string, number>;
  filters: RailFilters;
  group: RailGroup;
  stats: FilterStats;
  onOpen: () => void;
}) {
  const personas = useAppStore((s) => s.personas);
  const teams = useAppStore((s) => s.teams);
  const setLeaderFilter = useAppStore((s) => s.setLeaderFilter);
  const setRailFilters = useAppStore((s) => s.setRailFilters);
  const setRailGroup = useAppStore((s) => s.setRailGroup);
  const clearRailFilters = useAppStore((s) => s.clearRailFilters);
  const buttonRef = useRef<HTMLButtonElement>(null);
  // Where the menu opens (under the funnel), and which flyout is out beside it.
  const [at, setAt] = useState<{ top: number; left: number } | null>(null);
  const [fly, setFly] = useState<{ id: string; top: number; left: number } | null>(null);
  const open = at !== null;
  const leaders = [
    { id: "poiesis", name: "Poiesis", glyph: <PoiesisGlyph size={14} /> },
    ...personas.map((p) => ({ id: `persona:${p.id}`, name: p.name, glyph: <PersonaGlyph name={p.name} look={p.look} size={14} /> })),
    ...teams.map((t) => ({
      id: `team:${t.slug}`,
      name: t.name,
      glyph: <TeamGlyph coordinator={personas.find((p) => `persona:${p.id}` === t.coordinator)} size={14} />,
    })),
  ];
  const hasLeaders = personas.length > 0;
  const projectName = (id: string) => (id === NO_PROJECT ? "No project" : (stats.projects.find((p) => p.id === id)?.name ?? ""));
  const active = [
    filter ? `Led by ${filterName}` : null,
    filters.status ? STATUS_WORD[filters.status] : null,
    filters.output ? OUTPUT_WORD[filters.output] : null,
    filters.folder ? `In ${folderName(filters.folder)}` : null,
    filters.project ? projectName(filters.project) : null,
  ].filter((x): x is string => !!x);
  const close = () => {
    setAt(null);
    setFly(null);
  };
  // Escape takes back one level: the flyout first, then the menu.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      if (fly) setFly(null);
      else close();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, fly]);
  // Picking what is already picked takes it off.
  const pick = <K extends keyof RailFilters>(key: K, value: RailFilters[K]) => {
    close();
    setRailFilters({ [key]: filters[key] === value ? null : value } as Partial<RailFilters>);
  };
  const opt = (o: { key: string; label: string; checked: boolean; count?: number; glyph?: ReactNode; cls?: string; onPick: () => void }) => (
    <button
      key={o.key}
      className={`row-menu-item rail-opt ${o.cls ?? ""} ${o.checked ? "active" : ""}`}
      role="menuitemradio"
      aria-checked={o.checked}
      disabled={o.count === 0 && !o.checked}
      onClick={o.onPick}
    >
      {o.glyph}
      <span className="row-menu-label">{o.label}</span>
      {o.count !== undefined && <span className="row-stamp">{o.count}</span>}
    </button>
  );

  // The flyouts, by id: what each parent opens.
  const subs: Record<string, { label: string; items: ReactNode[] }> = {
    group: {
      label: "Group by",
      items: RAIL_GROUPS.filter((g) => g.id !== "leader" || hasLeaders).map((g) =>
        opt({
          key: g.id,
          label: g.label,
          checked: group === g.id,
          onPick: () => {
            close();
            setRailGroup(g.id);
          },
        })
      ),
    },
    leader: {
      label: "Leaders",
      items: leaders.map((l) =>
        opt({
          key: l.id,
          label: l.name,
          glyph: l.glyph,
          cls: "rail-leader-item",
          checked: filter === l.id,
          count: counts.get(l.id) ?? 0,
          onPick: () => {
            close();
            setLeaderFilter(l.id);
          },
        })
      ),
    },
    status: {
      label: "Status",
      items: STATUS_ORDER.map((s) =>
        opt({ key: s, label: STATUS_WORD[s], checked: filters.status === s, count: stats.status.get(s) ?? 0, onPick: () => pick("status", s) })
      ),
    },
    output: {
      label: "Has output",
      items: OUTPUT_ORDER.map((k) =>
        opt({ key: k, label: OUTPUT_WORD[k], checked: filters.output === k, count: stats.output[k], onPick: () => pick("output", k) })
      ),
    },
    folder: {
      label: "Folder",
      items: stats.folders.map((f) =>
        opt({ key: f.path, label: folderName(f.path), checked: filters.folder === f.path, count: f.count, onPick: () => pick("folder", f.path) })
      ),
    },
    project: {
      label: "Project",
      items: [
        ...stats.projects.map((p) =>
          opt({ key: p.id, label: p.name, checked: filters.project === p.id, count: p.count, onPick: () => pick("project", p.id) })
        ),
        opt({ key: NO_PROJECT, label: "No project", checked: filters.project === NO_PROJECT, count: stats.noProject, onPick: () => pick("project", NO_PROJECT) }),
      ],
    },
  };
  const flyOpen = (id: string, row: HTMLElement) => {
    const m = row.closest(".rail-popup")?.getBoundingClientRect();
    const r = row.getBoundingClientRect();
    // Level with the row (less the menu's padding), against the menu's right edge.
    setFly({ id, top: r.top - 5, left: (m?.right ?? r.right) - 2 });
  };
  // A parent opens its flyout on hover, click or →; the rest of the menu closes it.
  const parent = (id: string, label: string, now?: string) => (
    <button
      className="row-menu-item row-menu-parent"
      role="menuitem"
      aria-haspopup="menu"
      aria-expanded={fly?.id === id}
      onPointerEnter={(e) => flyOpen(id, e.currentTarget)}
      onClick={(e) => (fly?.id === id ? setFly(null) : flyOpen(id, e.currentTarget))}
      onKeyDown={(e) => {
        if (e.key === "ArrowRight") {
          e.preventDefault();
          flyOpen(id, e.currentTarget);
        }
      }}
    >
      <span className="row-menu-label">{label}</span>
      {now && <span className="row-stamp">{now}</span>}
      <ChevronIcon dir="right" size={12} className="row-menu-caret" />
    </button>
  );

  return (
    <div className="rail-filter">
      {active.length > 0 && (
        <button
          className="rail-filter-chip"
          title="Show every chat"
          aria-label={`${active.join(", ")}. Show every chat`}
          onClick={clearRailFilters}
        >
          <span className="row-menu-label">{active.length === 1 ? active[0] : `${active.length} filters`}</span>
          <span aria-hidden="true">×</span>
        </button>
      )}
      <div className="rail-filter-menu">
        <button
          ref={buttonRef}
          className="chat-more rail-more"
          aria-label="More chat options"
          aria-haspopup="menu"
          aria-expanded={open}
          title="Group and filter"
          onClick={() => {
            if (open) return close();
            onOpen();
            const r = buttonRef.current?.getBoundingClientRect();
            if (r) setAt({ top: r.bottom + 4, left: r.right - POPUP_WIDTH });
          }}
        >
          <FilterIcon size={14} />
        </button>
        {at &&
          createPortal(
            <>
              <div className="row-menu-backdrop" onClick={close} onContextMenu={(e) => { e.preventDefault(); close(); }} />
                <Popup top={at.top} left={at.left} className="rail-filter-popup">
                  {parent("group", "Group by", RAIL_GROUPS.find((g) => g.id === group)?.label)}
                  <hr className="row-menu-sep" />
                  {hasLeaders && parent("leader", "Show chats led by…")}
                  {parent("status", "Status…")}
                  {parent("output", "Has output…")}
                  {stats.folders.length > 0 && parent("folder", "Folder…")}
                  {stats.projects.length > 0 && parent("project", "Project…")}
                  {active.length > 0 && (
                    <>
                      <hr className="row-menu-sep" />
                      <button
                        className="row-menu-item"
                        role="menuitem"
                        onPointerEnter={() => setFly(null)}
                        onClick={() => {
                          close();
                          clearRailFilters();
                        }}
                      >
                        <span className="row-menu-label">Show every chat</span>
                      </button>
                    </>
                  )}
                </Popup>
              {fly && subs[fly.id] && (
                <Popup top={fly.top} left={fly.left} className="rail-flyout" label={subs[fly.id].label}>
                  {subs[fly.id].items}
                </Popup>
              )}
            </>,
            document.body
          )}
      </div>
    </div>
  );
}

export default function Rail() {
  // `SUB-3` kept a delegated child out of the list: it belongs to the turn that started it.
  // `UI-3` reverses that, as omher does: a child is listed under its chat, one click away. The
  // Fleet card and the Agents tab still open it too.
  const allConversations = useAppStore((s) => s.conversations);
  // Filtered outside the selector: a selector that builds a new array every
  // call never compares equal, and zustand re-renders forever.
  const topLevel = useMemo(
    () => allConversations.filter((c) => !c.parentConversationId),
    [allConversations]
  );
  // Each chat's helpers, newest first (`UI-3`). Nothing is loaded for this: they are already here.
  const kids = useMemo(() => {
    const byParent = new Map<string, Conversation[]>();
    for (const c of allConversations) {
      if (!c.parentConversationId) continue;
      const list = byParent.get(c.parentConversationId);
      if (list) list.push(c);
      else byParent.set(c.parentConversationId, [c]);
    }
    for (const list of byParent.values()) list.sort((a, b) => b.updatedAt - a.updatedAt);
    return byParent;
  }, [allConversations]);
  // The person's own open and closed choices for the helper lists, kept across restarts.
  const [helpersOpen, setHelpersOpen] = useState(loadHelpersOpen);
  const setOpen = useCallback((convId: string, to: boolean) => {
    setHelpersOpen((o) => {
      const next = { ...o, [convId]: to };
      saveHelpersOpen(next);
      return next;
    });
  }, []);
  // A chat that is gone takes its choice with it. Not before chats are loaded at all.
  useEffect(() => {
    if (allConversations.length === 0) return;
    const ids = new Set(allConversations.map((c) => c.id));
    setHelpersOpen((o) => {
      const gone = Object.keys(o).filter((id) => !ids.has(id));
      if (gone.length === 0) return o;
      const next = { ...o };
      for (const id of gone) delete next[id];
      saveHelpersOpen(next);
      return next;
    });
  }, [allConversations]);
  const tree = useMemo(() => ({ kids, open: helpersOpen, setOpen }), [kids, helpersOpen, setOpen]);
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
  const projects = useAppStore((s) => s.projects);
  const newProject = useAppStore((s) => s.newProject);
  // The funnel's other filters and the grouping (`railView`). What each chat is doing is read once
  // as a string, so the Rail re-renders when a state changes and not on every store update.
  const group = useAppStore((s) => s.railGroup);
  const railFilters = useAppStore((s) => s.railFilters);
  const statusKey = useAppStore((s) =>
    s.conversations
      .filter((c) => !c.parentConversationId)
      .map((c) => `${c.id}\t${chatState(s, c.id)}`)
      .join("\n")
  );
  const statusOf = useMemo(() => {
    const byChat = new Map<string, ChatState>();
    for (const line of statusKey ? statusKey.split("\n") : []) {
      const [id, state] = line.split("\t");
      byChat.set(id, state as ChatState);
    }
    return (id: string): ChatState => byChat.get(id) ?? "idle";
  }, [statusKey]);
  // What each chat made is read from the database, and only once someone asks for it.
  const [outputs, setOutputs] = useState<Record<string, [number, number]>>({});
  const refreshOutputs = useCallback(() => {
    sessionOutputs().then(setOutputs).catch(() => undefined);
  }, []);
  const outputFilterOn = railFilters.output !== null;
  useEffect(() => {
    if (outputFilterOn) refreshOutputs();
  }, [outputFilterOn, allConversations, refreshOutputs]);
  const ctx = useMemo<RailContext>(
    () => ({ projects, status: statusOf, outputs, withHelpers: (id) => kids.has(id) }),
    [projects, statusOf, outputs, kids]
  );
  // A folder or project that is gone leaves its filter off, as a leader that is gone does.
  const folderCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const c of topLevel) {
      const f = folderOf(c, projects);
      if (f) counts.set(f, (counts.get(f) ?? 0) + 1);
    }
    return counts;
  }, [topLevel, projects]);
  const filters = useMemo<RailFilters>(
    () => ({
      ...railFilters,
      folder: railFilters.folder && folderCounts.has(railFilters.folder) ? railFilters.folder : null,
      project:
        railFilters.project && (railFilters.project === NO_PROJECT || projects.some((p) => p.id === railFilters.project))
          ? railFilters.project
          : null,
    }),
    [railFilters, folderCounts, projects]
  );
  const anyFilter = !!filter || hasRailFilters(filters);
  // With a leader filter on, a chat also stays when one of its helpers matches (`UI-3`).
  const conversations = useMemo(
    () =>
      topLevel.filter(
        (c) =>
          (!filter || leaderOf(c) === filter || (kids.get(c.id) ?? []).some((k) => leaderOf(k) === filter)) &&
          matchesRailFilters(c, filters, ctx)
      ),
    [topLevel, filter, kids, filters, ctx]
  );
  const filterStats = useMemo<FilterStats>(() => {
    const status = new Map<string, number>();
    const output: Record<OutputKind, number> = { artifacts: 0, files: 0, helpers: 0 };
    const perProject = new Map<string, number>();
    let noProject = 0;
    for (const c of topLevel) {
      const s = statusOf(c.id);
      status.set(s, (status.get(s) ?? 0) + 1);
      for (const kind of OUTPUT_ORDER) if (hasOutput(c, kind, ctx)) output[kind] += 1;
      if (c.projectId) perProject.set(c.projectId, (perProject.get(c.projectId) ?? 0) + 1);
      else noProject += 1;
    }
    return {
      status,
      output,
      folders: [...folderCounts]
        .map(([path, count]) => ({ path, count }))
        .sort((a, b) => folderName(a.path).localeCompare(folderName(b.path))),
      projects: projects.map((p) => ({ id: p.id, name: p.name, count: perProject.get(p.id) ?? 0 })),
      noProject,
    };
  }, [topLevel, statusOf, ctx, folderCounts, projects]);
  const leaderList = useMemo(
    () => [
      { id: "poiesis", name: "Poiesis" },
      ...personas.map((p) => ({ id: `persona:${p.id}`, name: p.name })),
      ...teams.map((t) => ({ id: `team:${t.slug}`, name: t.name })),
    ],
    [personas, teams]
  );
  // Grouped any other way than by date, the Projects section steps aside: the groups hold every chat.
  const byDate = group === "date" || (group === "leader" && personas.length === 0);
  // `PRJ-UI-1`: sessions in a project are listed under it, so the date groups
  // below hold only the loose chats.
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
  const shownProjects = !byDate ? [] : anyFilter ? projects.filter((p) => sessionsByProject.has(p.id)) : projects;
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
    "remote",
    "tools",
    "about",
  ].includes(view);

  const setActive = useAppStore((s) => s.openSession);
  const [sessionMenuOpen, setSessionMenuOpen] = useState(false);
  useMinuteTick();
  const now = Date.now();
  const groups: { label: string; items: Conversation[] }[] = byDate
    ? groupByBucket(looseConversations, now)
    : groupChats(conversations, group as Exclude<RailGroup, "date">, { projects, status: statusOf, leaders: leaderList });

  return (
    <RailTree.Provider value={tree}>
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

        {(topLevel.length > 0 || anyFilter) && (
          <RailFilter
            filter={filter}
            filterName={filterName}
            counts={ledCounts}
            filters={filters}
            group={byDate ? "date" : group}
            stats={filterStats}
            onOpen={refreshOutputs}
          />
        )}
        {anyFilter && conversations.length === 0 && (
          <p className="rail-filter-empty">
            {filter && !hasRailFilters(filters) ? `No chats led by ${filterName}.` : "No chats match."}
          </p>
        )}

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
      {!collapsed && <RailResizer />}
    </nav>
    </RailTree.Provider>
  );
}
