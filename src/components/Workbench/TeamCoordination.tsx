import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import * as api from "../../lib/api";
import { teamNamer } from "../../lib/teamChart";
import { itemsOf, kindWord, nowSentence, pairKey, postLine, postsOf, roleHue, taskPosts, taskRuns, tasksOf, whoIs, workingRoles, type TeamTask, type Who } from "../../lib/teamLive";
import { useAppStore } from "../../lib/store";
import type { SubRun } from "../../lib/types";
import { endedLine, nowDoing } from "../Conversation/FleetCard";
import { PauseIcon, PlayIcon, SkipBackIcon, TeamIcon } from "../Icons/Icons";
import MemberGlyph from "../Personas/MemberGlyph";
import OrgDiagram, { useCoordinationStyle, useLastPlan } from "./OrgDiagram";
import { TaskMark } from "./TeamDiagram";
import { questionOf } from "../../lib/runs";

/**
 * `TEAM-14`: the Coordination view of a session a team leads (`TEAMS_PLAN` II.6). The head says
 * which team and which way of working; under it a fixed box with the tabs. The Chart is the
 * team's own drawing (`OrgDiagram team=`) with what the run's events say: who is working, how
 * much was posted along each line in this task, which member has which item. Members is the
 * helper list grouped by member. `TEAM-16`: Tasks is who was given what and where it is, and
 * under the chart the Replay plays the task's posts in order, each one travelling its line; Live
 * jumps back to now.
 *
 * Nothing here is stored or guessed. It is drawn from the run rows and the plan, so before
 * anything happened the chart is only the team, and under reduced motion the "Now:" sentence
 * is the only thing that changes (the CSS keeps every movement behind `no-preference`).
 */

type Tab = "chart" | "tasks" | "members";
const TABS: { id: Tab; label: string }[] = [
  { id: "chart", label: "Chart" },
  { id: "tasks", label: "Tasks" },
  { id: "members", label: "Members" },
];

const NO_POSTS: api.TeamPost[] = [];
/** How long the replay stays on one post while it plays. */
const REPLAY_MS = 900;

export default function TeamCoordination({ conversationId, team }: { conversationId: string; team: api.Team }) {
  const [tab, setTab] = useState<Tab>("chart");
  const personas = useAppStore((s) => s.personas);
  const subRuns = useAppStore((s) => s.subRuns);
  const summary = useAppStore((s) => s.runSummaries[conversationId]);
  const messages = useAppStore((s) => s.conversations.find((c) => c.id === conversationId)?.messages);
  const presets = useAppStore((s) => s.presets);
  const asking = useAppStore((s) => !!questionOf(s, conversationId));
  const openItem = useAppStore((s) => s.openItem);
  const allPosts = useAppStore((s) => s.posts[conversationId]) ?? NO_POSTS;
  const loadPosts = useAppStore((s) => s.loadPosts);
  const lastPlan = useLastPlan(conversationId);
  // `TEAM-16`: null is Live; a number is how many of the task's posts the replay has shown.
  const [replay, setReplay] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  useEffect(() => {
    loadPosts(conversationId).catch(() => {});
  }, [conversationId, loadPosts]);
  // The way it works: the run's, else the chat's, else the one the team file names.
  const slug = useCoordinationStyle(conversationId) ?? (team.style && team.style !== "ad-hoc" ? team.style : null);

  const runs = Object.values(subRuns)
    .filter((r) => r.parentConversationId === conversationId)
    .sort((a, b) => a.startedAt - b.startedAt);
  const task = taskRuns(runs, summary, messages ?? []);
  const roles = team.members.map((m) => m.role);
  const working = workingRoles(task);
  const plan = summary?.plan ?? lastPlan;
  const preset = slug ? presets.find((p) => p.slug === slug) : undefined;
  const styleName = slug ? (summary?.style?.name ?? preset?.name ?? slug) : null;
  const live = !!summary?.live;
  const stopped = !!summary?.stopped;
  const coordinator = teamNamer(team, personas)("coordinator", true);

  /** A member's newest run, which is its task chat. */
  const latest = (role: string): SubRun | undefined => [...runs].reverse().find((r) => r.member === role);
  const open = (run: SubRun) => openItem({ kind: "run", id: run.runId });

  const posts = taskPosts(allPosts, summary, task);
  const shown = replay === null ? posts.length : Math.min(replay, posts.length);
  const replaying = replay !== null;
  const current = replaying && shown > 0 ? posts[shown - 1] : undefined;

  // The post that travels its line: a new one while live, or the next one the replay steps to.
  // Stepping back, and the posts that were there before, fly nothing.
  const newest = shown > 0 ? posts[shown - 1] : undefined;
  const [step, setStep] = useState<api.TeamPost | null>(null);
  const seen = useRef({ id: newest?.id, shown });
  useEffect(() => {
    const before = seen.current;
    if (newest && newest.id !== before.id && shown > before.shown) setStep(newest);
    seen.current = { id: newest?.id, shown };
  }, [newest, shown]);

  // Playing steps one post at a time and ends on Live.
  useEffect(() => {
    if (!playing) return;
    const timer = setTimeout(() => {
      if (shown >= posts.length) {
        setPlaying(false);
        setReplay(null);
      } else {
        setReplay(shown + 1);
      }
    }, REPLAY_MS);
    return () => clearTimeout(timer);
  }, [playing, shown, posts.length]);

  const goLive = () => {
    setPlaying(false);
    setReplay(null);
  };
  const now = replaying
    ? shown === 0
      ? `Replay: before the first post, ${posts.length} to come.`
      : `Replay, ${shown} of ${posts.length}: ${postLine(posts[shown - 1])}`
    : nowSentence({ runs: task, live, asking, stopped, coordinator });
  const running = live && !stopped;

  return (
    <section className="team-coord" aria-label="How this team works">
      <div className="org-head">
        <TeamIcon size={16} />
        <span className="org-head-name">{team.name}</span>
        {styleName && <span className="org-head-line">{styleName}</span>}
      </div>
      <div className="team-tabs" role="tablist" aria-label="Team views">
        {TABS.map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={tab === t.id}
            className={`team-tab ${tab === t.id ? "active" : ""}`}
            onClick={() => {
              setTab(t.id);
              setStep(null);
            }}
          >
            {t.label}
          </button>
        ))}
      </div>
      {tab === "chart" ? (
        <OrgDiagram
          team={{
            team,
            personas,
            sentence: now || "Resting",
            working: replaying ? [] : working,
            live: {
              posts: postsOf(posts.slice(0, shown)),
              ...(current ? { current: pairKey(current.from_member, current.to_member), currentFrom: current.from_member } : {}),
              ...(step ? { step: { id: step.id, from: step.from_member, to: step.to_member } } : {}),
              ...(replaying ? { tone: "replay" as const } : running ? { tone: "live" as const } : {}),
              items: replaying ? {} : itemsOf(plan, roles),
              open: Object.fromEntries(roles.flatMap((role) => (latest(role) ? [[role, () => open(latest(role)!)]] : []))),
              gate: (preset?.glyph ?? slug) === "mission-command",
              stopped: !replaying && live && stopped,
              waiting: !replaying && asking,
            },
          }}
        />
      ) : tab === "tasks" ? (
        <TasksTab team={team} personas={personas} posts={posts} plan={plan} runs={runs} onOpen={open} />
      ) : (
        <MembersTab team={team} personas={personas} runs={runs} plan={plan} onOpen={open} />
      )}
      {tab === "chart" && posts.length > 0 && (
        <div className={`team-replay${replaying ? " replaying" : ""}`} role="group" aria-label="Replay of this task">
          <button
            className="team-replay-btn"
            aria-label="Back to the first post"
            title="Back to the first post"
            onClick={() => {
              setPlaying(false);
              setReplay(0);
            }}
          >
            <SkipBackIcon size={16} />
          </button>
          <button
            className={`team-replay-play${playing ? " on" : ""}`}
            aria-label={playing ? "Pause" : "Play"}
            title={playing ? "Pause" : "Play the posts in order"}
            onClick={() => {
              if (playing) setPlaying(false);
              else {
                if (!replaying || shown >= posts.length) setReplay(0);
                setPlaying(true);
              }
            }}
          >
            {playing ? <PauseIcon size={16} strokeWidth={1.8} /> : <PlayIcon size={16} strokeWidth={1.6} />}
          </button>
          <div className="team-replay-track" style={{ "--fill": `${(shown / posts.length) * 100}%` } as CSSProperties}>
            <div className="team-replay-rail" aria-hidden="true">
              <div className="team-replay-fill" />
              {/* One tick per post, in the sender's hue: filled once the replay has passed it. */}
              {posts.map((p, i) => (
                <span
                  key={p.id}
                  className={`team-replay-tick${i < shown ? " past" : ""}${replaying && i === shown - 1 ? " at" : ""}`}
                  data-hue={roleHue(team, p.from_member, personas)}
                  style={{ left: `${((i + 1) / posts.length) * 100}%` }}
                />
              ))}
            </div>
            <input
              className="team-replay-range"
              type="range"
              min={0}
              max={posts.length}
              value={shown}
              aria-label="Replay position"
              onChange={(e) => {
                setPlaying(false);
                const n = Number(e.target.value);
                setReplay(n >= posts.length ? null : n);
              }}
            />
          </div>
          <button className={`team-replay-live${replaying ? "" : " on"}${running ? " running" : ""}`} aria-pressed={!replaying} onClick={goLive}>
            <span className="team-replay-live-dot" aria-hidden="true" />
            Live
          </button>
        </div>
      )}
    </section>
  );
}

/** `TEAM-16`: the task as a list of who was given what: each piece of work with where it is, who has
 * it and who gave it, and the newest thing said about it. Pressing one opens the chat of the run
 * that works it. Questions and notes that belong to no piece of work are under it. A piece that is
 * handed over while the list is open slides in. */
function TasksTab({
  team,
  personas,
  posts,
  plan,
  runs,
  onOpen,
}: {
  team: api.Team;
  personas: api.Persona[];
  posts: api.TeamPost[];
  plan: api.PlanView | undefined;
  runs: SubRun[];
  onOpen: (run: SubRun) => void;
}) {
  const { tasks, loose } = tasksOf(posts, plan, runs);
  // The tasks that were there when the tab opened are not new.
  const known = useRef<Set<string> | null>(null);
  known.current ??= new Set(tasks.map((t) => t.key));
  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const runOf = (id: string | null | undefined) => (id ? runs.find((r) => r.runId === id) : undefined);
  const said = (s: string) => s.replace(/\s+/g, " ").trim();

  // The details card: after a short pause on a row (at once on keyboard focus), gone when the
  // pointer or focus leaves or the list scrolls.
  const tipId = useId();
  const [tip, setTip] = useState<{ task: TeamTask; at: DOMRect } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const show = (task: TeamTask, el: HTMLElement, wait = TIP_MS) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setTip({ task, at: el.getBoundingClientRect() }), wait);
  };
  const hide = () => {
    clearTimeout(timer.current);
    setTip(null);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  // The card shows the task as it is now, not as it was when the pointer arrived.
  const tipTask = tip ? (tasks.find((t) => t.key === tip.task.key) ?? tip.task) : null;

  if (tasks.length === 0 && loose.length === 0) {
    return (
      <div className="team-tasks" aria-label="Tasks">
        <p className="team-tasks-empty">Nothing has been handed over yet.</p>
      </div>
    );
  }
  return (
    <div className="team-tasks" aria-label="Tasks" onScroll={hide}>
      {tip && tipTask && (
        <TaskTip
          id={tipId}
          task={tipTask}
          at={tip.at}
          who={whoIs(team, tipTask.to, personas)}
          open={!!runOf(tipTask.runId)}
          time={time}
        />
      )}
      <ul className="team-task-list">
        {tasks.map((t) => {
          const run = runOf(t.runId);
          const body = (
            <>
              <TaskMark state={t.state} />
              <span className="team-task-main">
                <span className="team-task-title">
                  {t.item ? <span className="team-task-n">{t.item}</span> : null}
                  <span className="team-task-text">{t.text}</span>
                </span>
                <span className="team-task-meta">
                  <span className="team-task-who" data-hue={roleHue(team, t.to, personas)}>
                    <MemberGlyph who={whoIs(team, t.to, personas)} size={16} />
                    <span className="team-task-role">{t.to}</span>
                  </span>
                  <span className="team-task-word">{t.word}</span>
                  <span className="team-task-from">from {t.from}</span>
                  {t.at !== null && <time className="team-task-time">{time(t.at)}</time>}
                </span>
                {t.last && (
                  <span className="team-task-last">
                    <span className="team-task-last-who">
                      {t.last.from_member} → {t.last.to_member === "all" ? "everyone" : t.last.to_member}, {kindWord(t.last.kind)}:
                    </span>{" "}
                    {said(t.last.text)}
                  </span>
                )}
              </span>
            </>
          );
          const cls = `team-task ${t.state}${known.current!.has(t.key) ? "" : " fresh"}`;
          const hover = {
            onMouseEnter: (e: React.MouseEvent<HTMLElement>) => show(t, e.currentTarget),
            onMouseLeave: hide,
            onFocus: (e: React.FocusEvent<HTMLElement>) => show(t, e.currentTarget, 0),
            onBlur: hide,
            "aria-describedby": tip?.task.key === t.key ? tipId : undefined,
          };
          return (
            <li key={t.key} className={cls}>
              {run ? (
                <button className="team-task-row" aria-label={`${t.text}, ${t.word}, with ${t.to}. Open the chat`} onClick={() => onOpen(run)} {...hover}>
                  {body}
                </button>
              ) : (
                <div className="team-task-row" tabIndex={0} {...hover}>
                  {body}
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {loose.length > 0 && (
        <section className="team-talk" aria-label="Questions and notes">
          <h4 className="team-talk-head">Questions and notes</h4>
          {loose.map((p) => (
            <p key={p.id} className="team-talk-row">
              <span className="team-talk-who">
                {p.from_member} → {p.to_member === "all" ? "everyone" : p.to_member}
              </span>
              <span className="team-talk-kind">{kindWord(p.kind)}</span>
              <span className="team-talk-text">{said(p.text)}</span>
            </p>
          ))}
        </section>
      )}
    </div>
  );
}

const TIP_MS = 350;
const TIP_W = 320;

/** `TEAM-16`: everything about one piece of work, on hover or focus of its row: where it is, who
 * has it and who gave it, the whole hand-over, and all that was said about it since. Drawn in a
 * layer of its own beside the list (below the row when there is no room), so the scrolling list
 * never cuts it off. */
function TaskTip({
  id,
  task,
  at,
  who,
  open,
  time,
}: {
  id: string;
  task: TeamTask;
  at: DOMRect;
  who: Who;
  open: boolean;
  time: (ms: number) => string;
}) {
  const card = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const h = card.current?.offsetHeight ?? 0;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const room = (v: number, size: number, max: number) => Math.max(8, Math.min(v, max - size - 8));
    // Beside the list, on the side with room; else under the row.
    if (at.left - TIP_W - 12 >= 8) setPos({ left: at.left - TIP_W - 12, top: room(at.top, h, vh) });
    else if (at.right + TIP_W + 12 <= vw - 8) setPos({ left: at.right + 12, top: room(at.top, h, vh) });
    else {
      const below = at.bottom + 6;
      setPos({ left: room(at.left, TIP_W, vw), top: below + h <= vh - 8 ? below : room(at.top - h - 6, h, vh) });
    }
  }, [at, task]);
  const to = (role: string) => (role === "all" ? "everyone" : role);
  return createPortal(
    <div
      ref={card}
      id={id}
      role="tooltip"
      className="team-tip"
      style={{ width: TIP_W, left: pos?.left ?? -9999, top: pos?.top ?? -9999, visibility: pos ? "visible" : "hidden" }}
    >
      <div className="team-tip-state">
        <TaskMark state={task.state} />
        {task.item ? `Item ${task.item} · ` : ""}
        {task.word}
      </div>
      <p className="team-tip-title">{task.text}</p>
      <div className="team-tip-who">
        <MemberGlyph who={who} size={18} />
        <span className="team-tip-role">{task.to}</span>
        {who.name && <span className="team-tip-persona">{who.name}</span>}
        <span className="team-tip-from">
          from {task.from}
          {task.at !== null ? `, ${time(task.at)}` : ""}
        </span>
      </div>
      {task.brief && (
        <section className="team-tip-part">
          <h5 className="team-tip-head">Handed over</h5>
          <p className="team-tip-brief">{task.brief}</p>
        </section>
      )}
      {task.thread.length > 0 && (
        <section className="team-tip-part">
          <h5 className="team-tip-head">Since then</h5>
          <ol className="team-tip-thread">
            {task.thread.map((p) => (
              <li key={p.id}>
                <span className="team-tip-post-head">
                  <span className="team-tip-post-who">
                    {p.from_member} → {to(p.to_member)}
                  </span>
                  <span>{kindWord(p.kind)}</span>
                  <time>{time(p.created_at)}</time>
                </span>
                <span className="team-tip-post-text">{p.text.trim()}</span>
              </li>
            ))}
          </ol>
        </section>
      )}
      {open && <p className="team-tip-foot">Click to open {task.to}'s chat.</p>}
    </div>,
    document.body
  );
}

/** Today's helper list, grouped by member: its face, role and persona, what it is doing, its task
 * chat, and the items it has. A plain helper has the `(·)` face. */
function MembersTab({
  team,
  personas,
  runs,
  plan,
  onOpen,
}: {
  team: api.Team;
  personas: api.Persona[];
  runs: SubRun[];
  plan: api.PlanView | undefined;
  onOpen: (run: SubRun) => void;
}) {
  const roles = team.members.map((m) => m.role);
  // A run of a place the team has since lost is still listed, as a helper.
  const gone = [...new Set(runs.flatMap((r) => (r.member && !roles.includes(r.member) ? [r.member] : [])))];
  const items = itemsOf(plan, [...roles, ...gone]);
  const plain = runs.filter((r) => !r.member);

  const doing = (run: SubRun | undefined) =>
    !run ? "has not started" : api.stillWorking(run.status) ? nowDoing(run) : endedLine(run);

  return (
    <div className="team-members" aria-label="Members">
      {[...roles, ...gone].map((role) => {
        const who = whoIs(team, role, personas);
        const run = [...runs].reverse().find((r) => r.member === role);
        const mine = items[role] ?? [];
        return (
          <div key={role} className="team-member">
            <div className="team-member-head">
              <MemberGlyph who={who} size={18} />
              <span className="team-member-role">{role}</span>
              <span className="team-member-persona">{who.name ?? "helper"}</span>
              {run && (
                <button className="fleet-action" aria-label={`Open ${role}'s task chat`} onClick={() => onOpen(run)}>
                  Open
                </button>
              )}
            </div>
            <p className="team-member-doing">{doing(run)}</p>
            {mine.length > 0 && (
              <ul className="team-member-items" aria-label={`Items ${role} has`}>
                {mine.map((i) => (
                  <li key={i.n} className={`item-${i.state}`}>
                    Item {i.n}, {i.word}: {i.text}
                  </li>
                ))}
              </ul>
            )}
          </div>
        );
      })}
      {plain.length > 0 && (
        <div className="team-member">
          <div className="team-member-head">
            <MemberGlyph who={{ kind: "helper", name: null }} size={18} />
            <span className="team-member-role">helpers</span>
          </div>
          {plain.map((run) => (
            <button key={run.runId} className="agents-row" onClick={() => onOpen(run)}>
              <span className="agents-row-agent">{run.agent}</span>
              <span className="agents-row-task">{run.task}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
