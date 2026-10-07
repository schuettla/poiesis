import { useEffect, useMemo, useRef, useState } from "react";
import { stillWorking } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import type { SubRun } from "../../lib/types";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import "./RunBar.css";

/**
 * `RUN`: one line above the composer that gathers what a run is doing, and
 * after it, what it did. The plan, the agents, the changed files and the cost
 * used to live in four different places; this links to each and replaces none.
 *
 * What it is not: it never repeats the meter (activity, clock and context stay
 * in the turn), and it never shows a step count or a budget. A limit is not a
 * status, and the run-meter test holds the same line for the turn. No gauges,
 * no progress bar, no colour for good or bad.
 *
 * Each segment is shown only when it has something true to say. A plain chat
 * answer has no plan, no agents, no files and often no price, so the composer
 * looks exactly as it did before this existed.
 */

/** Said to a screen reader no more often than this, so a busy run does not
 * read itself out loud every time a number moves. */
const ANNOUNCE_MS = 2000;
/** Below this width, segments past the second fold into `+N`. */
const NARROW_PX = 520;

function money(usd: number): string {
  return usd < 0.01 ? "under $0.01" : `$${usd.toFixed(2)}`;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

interface Segment {
  key: string;
  text: string;
  title?: string;
  onClick?: () => void;
  /** Undo and Keep, for the files segment once the run is over. */
  extra?: { label: string; onClick: () => void }[];
}

/** Hold a value back so it changes at most once per `ms`. */
function useThrottled<T>(value: T, ms: number): T {
  const [held, setHeld] = useState(value);
  const last = useRef(0);
  useEffect(() => {
    const wait = last.current + ms - Date.now();
    if (wait <= 0) {
      last.current = Date.now();
      setHeld(value);
      return;
    }
    const t = setTimeout(() => {
      last.current = Date.now();
      setHeld(value);
    }, wait);
    return () => clearTimeout(t);
  }, [value, ms]);
  return held;
}

/** The cost segment's text, or `null` when there is nothing true to say.
 *
 * The lead's own spend plus every agent it started this run. If the lead is
 * priced but an agent is not, the sum is not the total, so it says so rather
 * than presenting a floor as a figure. */
export function costSegment(
  lead: number | null,
  localRun: boolean,
  agents: SubRun[],
  live: boolean
): { text: string; title?: string } | null {
  if (lead === null) {
    // A run on this machine is free and says so once, not as `$0.00`.
    return localRun && agents.every((a) => a.costUsd === null || a.costUsd === undefined)
      ? { text: "on this machine" }
      : null;
  }
  const unpriced = agents.some((a) => a.costUsd === null || a.costUsd === undefined);
  if (unpriced) {
    return {
      text: `${money(lead)}${live ? " so far" : ""} + agents`,
      title: "I can't price what one of my agents used.",
    };
  }
  const total = lead + agents.reduce((sum, a) => sum + (a.costUsd ?? 0), 0);
  return { text: `${money(total)}${live ? " so far" : ""}` };
}

export default function RunBar() {
  const convId = useAppStore((s) => s.activeConversationId);
  const summary = useAppStore((s) => (s.activeConversationId ? s.runSummaries[s.activeConversationId] : undefined));
  const subRuns = useAppStore((s) => s.subRuns);
  const changes = useAppStore((s) => (s.activeConversationId ? s.changeSets[s.activeConversationId] : undefined));
  const dismiss = useAppStore((s) => s.dismissRunSummary);
  const setDockView = useAppStore((s) => s.setDockView);
  const setDockOpen = useAppStore((s) => s.setDockOpen);
  const openUsage = useAppStore((s) => s.openUsage);
  const keepChanges = useAppStore((s) => s.keepChanges);
  const undoChanges = useAppStore((s) => s.undoChanges);
  const resumeLastRun = useAppStore((s) => s.resumeLastRun);
  const busy = useAppStore((s) => s.busy);
  // `GOL-UI-1`: the goal this chat is working toward, or ended with.
  const goal = useAppStore((s) => (s.activeConversationId ? s.goals[s.activeConversationId] : undefined));
  const stopGoal = useAppStore((s) => s.stopGoal);
  // `AGC-3`: the run is paused on a question for this chat.
  const asking = useAppStore((s) => !!s.pendingQuestion && s.pendingQuestion.convId === s.activeConversationId);

  const [confirmUndo, setConfirmUndo] = useState(false);
  const [narrow, setNarrow] = useState(false);
  const [overflowOpen, setOverflowOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(([entry]) => setNarrow(entry.contentRect.width < NARROW_PX));
    watch.observe(el);
    return () => watch.disconnect();
  }, [summary?.runId]);

  const segments = useMemo<Segment[]>(() => {
    if (!summary || !convId) return [];
    const out: Segment[] = [];
    const live = summary.live;

    // First, because nothing else matters while I am waiting on you.
    if (asking) {
      out.push({
        key: "asking",
        text: "I asked you something ↑",
        title: "Go to my question",
        onClick: () => {
          const cards = document.querySelectorAll(".question-card");
          cards[cards.length - 1]?.scrollIntoView?.({ behavior: "smooth", block: "center" });
        },
      });
    }

    // The goal, the user's own: where I am in it, or how it ended.
    if (goal) {
      const said = goal.text.length > 60 ? `${goal.text.slice(0, 59).trimEnd()}…` : goal.text;
      const toLatest = () => {
        const notes = document.querySelectorAll(".command-note");
        notes[notes.length - 1]?.scrollIntoView?.({ behavior: "smooth", block: "center" });
      };
      if (goal.status === "active") {
        out.push({
          key: "goal",
          text: goal.checking ? `◎ ${said} · checking` : `◎ ${said} · round ${goal.round} of ${goal.maxRounds}`,
          title: goal.lastCheck?.next || "Show where I am in this goal",
          onClick: toLatest,
          extra: [{ label: "Stop goal", onClick: () => stopGoal(convId) }],
        });
      } else if (goal.status === "met") {
        out.push({ key: "goal", text: `◆ Done: ${goal.lastCheck?.evidence ?? said}`, title: said, onClick: toLatest });
      } else {
        out.push({
          key: "goal",
          text:
            goal.status === "exhausted"
              ? `◎ ${said} · not reached in ${goal.maxRounds} rounds`
              : `◎ ${said} · stopped`,
          title: goal.lastCheck?.next || said,
          onClick: toLatest,
        });
      }
    }

    // Plan. Dropped items are not part of the count: a plan that dropped two of
    // eight is a plan of six.
    const items = summary.plan?.items.filter((i) => i.status !== "dropped") ?? [];
    if (items.length) {
      const done = items.filter((i) => i.status === "done").length;
      const interrupted = summary.stopReason && summary.stopReason !== "completed";
      const text = live
        ? `plan ${done} of ${items.length}`
        : done === items.length
          ? "plan done"
          : interrupted
            ? `stopped at ${done} of ${items.length}`
            : `plan ${done} of ${items.length}`;
      out.push({
        key: "plan",
        text,
        title: "Show the plan",
        onClick: () => {
          const cards = document.querySelectorAll(".plan-card");
          cards[cards.length - 1]?.scrollIntoView?.({ behavior: "smooth", block: "center" });
        },
      });
    }

    // Agents this conversation started during this run.
    const agents = Object.values(subRuns).filter(
      (r) => r.parentConversationId === convId && r.startedAt >= summary.startedAt - 1000
    );
    const working = agents.filter((r) => stillWorking(r.status)).length;
    const reported = agents.length - working;
    if (agents.length) {
      const text = live
        ? working > 0
          ? `${plural(working, "agent", "agents")} working`
          : `${plural(reported, "agent", "agents")} reported back`
        : [
            reported > 0 ? `${plural(reported, "agent", "agents")} reported back` : "",
            working > 0 ? `${working} still working` : "",
          ]
            .filter(Boolean)
            .join(" · ");
      out.push({
        key: "agents",
        text,
        title: "Show what my agents are doing",
        onClick: () => {
          setDockView("agents");
          setDockOpen(true);
        },
      });
    }

    // Files. Live, the paths the run touched; after it, the Changes view's own
    // count so the two can never disagree.
    const fileCount = live ? summary.files.length : (changes?.files.length ?? summary.files.length);
    if (fileCount > 0) {
      out.push({
        key: "files",
        text: `I changed ${plural(fileCount, "file", "files")}`,
        title: "Show what I changed",
        onClick: () => {
          setDockView("changes");
          setDockOpen(true);
        },
        extra: live
          ? undefined
          : [
              { label: "Undo", onClick: () => setConfirmUndo(true) },
              { label: "Keep", onClick: () => void keepChanges(convId) },
            ],
      });
    }

    const cost = costSegment(summary.costUsd, summary.localRun, agents, live);
    if (cost) {
      out.push({
        key: "cost",
        text: cost.text,
        title: cost.title ?? "What this chat has cost",
        onClick: () => openUsage(convId),
      });
    }
    return out;
  }, [summary, convId, subRuns, changes, asking, goal, stopGoal, setDockView, setDockOpen, openUsage, keepChanges]);

  const interrupted =
    !!summary && !summary.live && !!summary.stopReason && summary.stopReason !== "completed";
  const announced = useThrottled(segments.map((s) => s.text).join(", "), ANNOUNCE_MS);

  if (!summary || !convId || (segments.length === 0 && !interrupted)) return null;

  const shown = narrow ? segments.slice(0, 2) : segments;
  const folded = narrow ? segments.slice(2) : [];
  const undoCount = changes?.files.length ?? 0;

  const render = (seg: Segment) => (
    <span className="run-bar-segment" key={seg.key}>
      <button className="run-bar-link" title={seg.title} onClick={seg.onClick}>
        {seg.text}
      </button>
      {seg.extra?.map((x) => (
        <span key={x.label}>
          <span className="run-bar-sep" aria-hidden="true"> · </span>
          <button className="run-bar-link run-bar-act" onClick={x.onClick}>
            {x.label}
          </button>
        </span>
      ))}
    </span>
  );

  return (
    <div className="run-bar" ref={ref}>
      <div className="run-bar-line">
        {shown.map((seg, i) => (
          <span key={seg.key}>
            {i > 0 && <span className="run-bar-sep" aria-hidden="true"> · </span>}
            {render(seg)}
          </span>
        ))}
        {folded.length > 0 && (
          <span className="run-bar-more">
            <span className="run-bar-sep" aria-hidden="true"> · </span>
            <button
              className="run-bar-link"
              aria-expanded={overflowOpen}
              onClick={() => setOverflowOpen((v) => !v)}
            >
              +{folded.length}
            </button>
            {overflowOpen && <div className="run-bar-popover">{folded.map(render)}</div>}
          </span>
        )}
        {interrupted && !busy && (
          <span className="run-bar-segment">
            <span className="run-bar-sep" aria-hidden="true"> · </span>
            <button className="run-bar-link run-bar-act" onClick={() => void resumeLastRun()}>
              Continue where I stopped
            </button>
          </span>
        )}
      </div>
      <button
        className="run-bar-x"
        aria-label="Dismiss this summary"
        title="Dismiss"
        onClick={() => dismiss(convId)}
      >
        ×
      </button>
      {/* Said, not drawn, and not more than once in a while. */}
      <div className="run-bar-live" role="status" aria-live="polite">
        {announced}
      </div>
      {confirmUndo && (
        <ConfirmDialog
          title={`Take back my changes to ${plural(undoCount, "file", "files")}?`}
          body="The files go back to how they were before I touched them."
          confirmLabel="Take back"
          onCancel={() => setConfirmUndo(false)}
          onConfirm={() => {
            setConfirmUndo(false);
            void undoChanges(convId);
          }}
        />
      )}
    </div>
  );
}
