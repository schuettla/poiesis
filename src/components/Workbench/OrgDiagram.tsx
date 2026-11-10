import { useId, useState } from "react";
import * as api from "../../lib/api";
import { itemDot, orgWords } from "../../lib/orgState";
import { useAppStore } from "../../lib/store";
import StyleGlyph from "../Mark/StyleGlyph";
import "./OrgDiagram.css";

/**
 * `WRK-15`: Coordination, how this chat is being worked, as a living diagram. Each
 * style has its own shape, because the shape is the point:
 *
 * - Command & Control: you at the top, me under you, every item under me, and a line
 *   back to you after each one.
 * - Lean: one belt, one piece at a time.
 * - Mission Command: you set the goal, I work it, a check looks at it, and the result
 *   is what comes out.
 * - Swarm: the items on a board, me and the helpers around it, no boss.
 * - Ad hoc, or between tasks: you and me, a question and an answer.
 *
 * It draws only what the run's own events say (`ORG_LOOP_PLAN` IX.6): the plan the
 * backend last sent, the helpers that were started, the line being stopped, a question
 * waiting for you. Before there is a plan the places for items are drawn faint, so the
 * shape reads from the start. No colour for good or bad, no counts, and every state is
 * also in the sentence under the diagram, which is all there is under reduced motion.
 *
 * It uses a fixed box and scales inside it: nothing here scrolls.
 */

const W = 320;
const H = 176;
const BIG = 13;

interface Node {
  key: string;
  kind: "you" | "lead" | "item" | "gate" | "result" | "helper";
  x: number;
  y: number;
  r: number;
  label?: string;
  /** Where the label sits: under the node (default) or inside it. */
  inside?: boolean;
  title: string;
  state?: string;
  halt?: boolean;
  working?: boolean;
  onClick?: () => void;
}

interface Edge {
  d: string;
  /** A line that comes back (a check-in) or ties a helper on: drawn dashed, not counted
   * as the flow. */
  back?: boolean;
  arrow?: boolean;
}

/** `n` places between `from` and `to`: none for none, the middle for one. */
const spread = (n: number, from: number, to: number): number[] =>
  n <= 0 ? [] : n === 1 ? [(from + to) / 2] : Array.from({ length: n }, (_, i) => from + ((to - from) * i) / (n - 1));

/** A straight line from the rim of one circle to the rim of another. */
function link(a: { x: number; y: number; r: number }, b: { x: number; y: number; r: number }, gap = 3): string {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len = Math.hypot(dx, dy) || 1;
  const ux = dx / len;
  const uy = dy / len;
  const x1 = a.x + ux * (a.r + gap);
  const y1 = a.y + uy * (a.r + gap);
  const x2 = b.x - ux * (b.r + gap);
  const y2 = b.y - uy * (b.r + gap);
  return `M${x1.toFixed(1)} ${y1.toFixed(1)} L${x2.toFixed(1)} ${y2.toFixed(1)}`;
}

interface Thing {
  text: string;
  word: string;
  state: string;
  halt: boolean;
}

/** The class an item's state draws with. */
function stateOf(item: api.PlanItem): string {
  if (item.review?.state === "checking") return "checking";
  if (item.review?.state === "sent_back" && item.status === "doing") return "sent-back";
  return item.status;
}

const short = (s: string, n: number) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/** The plan the chat's last run left on its message: what is known about how it
 * was worked once the live summary is gone, after a restart. */
function useLastPlan(conversationId: string | null) {
  return useAppStore((s) => {
    const conv = conversationId ? s.conversations.find((c) => c.id === conversationId) : undefined;
    if (!conv) return undefined;
    for (let i = conv.messages.length - 1; i >= 0; i--) {
      const plan = conv.messages[i].plan;
      if (plan) return plan;
    }
    return undefined;
  });
}

/**
 * The style this chat is coordinated by, or null for Ad hoc: the live run's, else
 * the one its last plan was worked under, else the chat's own. Shared by the
 * Coordination tab and the drawing, so the tab never disappears while there is
 * something to draw, and stays after the app is opened again.
 */
export function useCoordinationStyle(conversationId: string | null): string | null {
  const live = useAppStore((s) => (conversationId ? s.runSummaries[conversationId]?.style?.preset : undefined));
  const own = useAppStore((s) => (conversationId ? s.conversations.find((c) => c.id === conversationId)?.style : undefined));
  const last = useLastPlan(conversationId)?.style;
  const slug = live ?? last ?? own ?? null;
  return slug && slug !== "ad-hoc" ? slug : null;
}

export default function OrgDiagram({ conversationId }: { conversationId: string }) {
  const summary = useAppStore((s) => s.runSummaries[conversationId]);
  const lastPlan = useLastPlan(conversationId);
  const presets = useAppStore((s) => s.presets);
  const subRuns = useAppStore((s) => s.subRuns);
  const asking = useAppStore((s) => !!s.pendingQuestion && s.pendingQuestion.convId === conversationId);
  const openItem = useAppStore((s) => s.openItem);
  const [gateNote, setGateNote] = useState<string | null>(null);
  const arrowId = `org-arrow-${useId().replace(/:/g, "")}`;

  const slug = useCoordinationStyle(conversationId);
  const preset = slug ? presets.find((p) => p.slug === slug) : undefined;
  // A style of your own, or an offspring, is drawn the way its parent is.
  const shape = slug ? (preset?.glyph ?? slug) : "ad-hoc";
  const plan = summary?.plan ?? lastPlan;
  const live = !!summary?.live;
  const stopped = !!summary?.stopped;

  const helpers = summary
    ? Object.values(subRuns)
        .filter((r) => r.parentConversationId === conversationId && r.startedAt >= summary.startedAt - 1000)
        .sort((a, b) => a.startedAt - b.startedAt)
    : [];
  const out = helpers.filter((h) => api.stillWorking(h.status)).length;

  // What moves through it: the items, or for a goal the criteria.
  const things: Thing[] = (plan?.items.length ? plan.items : [])
    .map((item) => ({
      text: item.text,
      word: itemDot(item).word,
      state: stateOf(item),
      halt: stopped && item.status === "doing",
    }))
    .concat(
      !plan?.items.length && shape === "mission-command" && !plan?.awaiting_approval
        ? (plan?.criteria ?? []).map((c) => ({
            text: c.text,
            state: c.met ? "done" : "todo",
            halt: false,
            word: c.met ? "met" : c.met === false ? "not met" : "not yet",
          }))
        : []
    );
  const inHand = plan?.items.find((i) => i.status === "doing");

  const toBoard = () => document.querySelector(".plan-card")?.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
  const n = things.length;
  // Before there is a plan, three faint places show where the items will go.
  const ghosts = n === 0 && shape !== "ad-hoc" ? 3 : 0;
  const slots = n || ghosts;
  const r = Math.max(4, Math.min(8, 96 / Math.max(slots, 1)));

  const nodes: Node[] = [];
  const edges: Edge[] = [];
  const faint: { x: number; y: number; r: number }[] = [];
  const notes: { x: number; y: number; text: string; anchor?: "start" | "middle" | "end" }[] = [];
  let board: { x: number; y: number; w: number; h: number } | null = null;

  /** Item `i` at a place, or a faint place when there is no item yet. */
  const place = (i: number, x: number, y: number): Node | null => {
    if (i >= n) {
      faint.push({ x, y, r });
      return null;
    }
    const t = things[i];
    const node: Node = {
      key: `item-${i}`,
      kind: "item",
      x,
      y,
      r,
      title: `${t.text} — ${t.word}`,
      state: t.state,
      halt: t.halt,
      onClick: toBoard,
    };
    nodes.push(node);
    return node;
  };
  const helper = (h: (typeof helpers)[number], x: number, y: number, named: boolean): Node => {
    const node: Node = {
      key: `helper-${h.runId}`,
      kind: "helper",
      x,
      y,
      r: 6,
      label: named ? short(h.agent, 10) : undefined,
      working: api.stillWorking(h.status),
      title: `${h.agent}: ${h.task}`,
      onClick: () => openItem({ kind: "run", id: h.runId }),
    };
    nodes.push(node);
    return node;
  };
  const you = (x: number, y: number): Node => {
    const node: Node = { key: "you", kind: "you", x, y, r: BIG, label: "You", inside: true, title: asking ? "I'm waiting for you" : "You" };
    nodes.push(node);
    return node;
  };
  const me = (x: number, y: number, label = "Me"): Node => {
    const node: Node = { key: "lead", kind: "lead", x, y, r: BIG, label, title: "Me" };
    nodes.push(node);
    return node;
  };
  const named = helpers.length <= 4;

  if (shape === "command-and-control") {
    const top = you(W / 2, 24);
    const lead = me(W / 2, 74);
    edges.push({ d: link(top, lead), arrow: true });
    spread(slots, 48, W - 48).forEach((x, i) => {
      const it = place(i, x, 128);
      if (it) edges.push({ d: link(lead, it, 2), arrow: true });
    });
    // After each item the line goes back to you.
    edges.push({ d: `M${W - 34} 122 C ${W - 8} 90, ${W - 40} 30, ${top.x + BIG + 6} 24`, back: true, arrow: true });
    notes.push({ x: W - 12, y: 74, text: "checks in", anchor: "end" });
    spread(helpers.length, 60, W - 60).forEach((x, i) => {
      const h = helper(helpers[i], x, 162, false);
      edges.push({ d: link(lead, h), back: true });
    });
  } else if (shape === "lean") {
    const lead = me(30, 70);
    board = { x: 58, y: 54, w: W - 70, h: 32 };
    let prev: { x: number; y: number; r: number } = lead;
    spread(slots, 80, W - 30).forEach((x, i) => {
      const it = place(i, x, 70);
      if (it) {
        edges.push({ d: link(prev, it, 2), arrow: true });
        prev = it;
      }
    });
    notes.push({ x: 58, y: 100, text: "one at a time", anchor: "start" });
    spread(helpers.length, 90, W - 40).forEach((x, i) => {
      const h = helper(helpers[i], x, 132, named);
      edges.push({ d: link(lead, h), back: true });
    });
  } else if (shape === "mission-command") {
    const goal = you(28, 56);
    const lead = me(108, 56);
    const gate: Node = {
      key: "gate",
      kind: "gate",
      x: 200,
      y: 56,
      r: 10,
      label: "check",
      title: "A second look before the answer goes out. Press to see what it said.",
      onClick: () => setGateNote(gateWords(plan) ?? "Nothing has been checked yet."),
    };
    const result: Node = { key: "result", kind: "result", x: 288, y: 56, r: 11, label: "done", title: "What comes out" };
    nodes.push(gate, result);
    edges.push({ d: link(goal, lead), arrow: true }, { d: link(lead, gate), arrow: true }, { d: link(gate, result), arrow: true });
    notes.push({ x: 68, y: 46, text: "goal" });
    spread(slots, 64, 176).forEach((x, i) => {
      const it = place(i, x, 116);
      if (it) edges.push({ d: link(lead, it, 2), back: true });
    });
    spread(helpers.length, 220, 300).forEach((x, i) => {
      const h = helper(helpers[i], x, 128, false);
      edges.push({ d: link(lead, h), back: true });
    });
  } else if (shape === "swarm") {
    board = { x: 84, y: 22, w: 150, h: 128 };
    const cols = Math.max(1, Math.min(4, slots));
    const rows = Math.max(1, Math.ceil(slots / cols));
    const xs = spread(cols, board.x + 26, board.x + board.w - 26);
    const ys = spread(rows, board.y + 32, board.y + board.h - 26);
    for (let i = 0; i < slots; i++) place(i, xs[i % cols], ys[Math.floor(i / cols)]);
    notes.push({ x: board.x + board.w / 2, y: board.y + 14, text: "shared board" });
    // Me and the helpers around the board, as peers: no one above anyone.
    const workers = 1 + helpers.length;
    const left = Math.ceil(workers / 2);
    const leftYs = spread(left, 40, 140);
    const rightYs = spread(workers - left, 40, 140);
    const lead = me(40, leftYs[0]);
    edges.push({ d: `M${lead.x + BIG + 3} ${lead.y} L${board.x - 3} ${lead.y}`, back: true });
    helpers.forEach((h, i) => {
      const k = i + 1;
      const onLeft = k < left;
      const y = onLeft ? leftYs[k] : rightYs[k - left];
      const x = onLeft ? 44 : W - 44;
      helper(h, x, y, named);
      const b = board!;
      edges.push({ d: onLeft ? `M${x + 9} ${y} L${b.x - 3} ${y}` : `M${x - 9} ${y} L${b.x + b.w + 3} ${y}`, back: true });
    });
  } else {
    const a = you(96, 78);
    const b = me(224, 78);
    edges.push(
      { d: `M${a.x + 12} ${a.y - 8} Q ${(a.x + b.x) / 2} ${a.y - 40}, ${b.x - 12} ${b.y - 8}`, arrow: true },
      { d: `M${b.x - 12} ${b.y + 8} Q ${(a.x + b.x) / 2} ${a.y + 40}, ${a.x + 12} ${a.y + 8}`, arrow: true }
    );
    notes.push({ x: W / 2, y: 46, text: "you ask" }, { x: W / 2, y: 120, text: "I answer" });
  }
  const waiting = asking;

  const name = slug ? (summary?.style?.name ?? preset?.name ?? slug) : "Ad hoc";
  const words = orgWords({
    preset: slug ? name : null,
    live,
    helpers: out,
    looking: !!summary?.looking,
    branching: !!summary?.branching,
    stopped,
  });
  const sentence = live
    ? [...words, ...(waiting ? ["waiting for you"] : [])].join(" · ") || "working"
    : slug && plan
      ? `Worked the ${name} way`
      : "Resting";
  const line = slug ? preset?.line : "Just answer";

  return (
    <section className="org-diagram" aria-label="How this chat is coordinated">
      <div className="org-head">
        <StyleGlyph glyph={shape} size={18} branch={!!preset?.parent} />
        <span className="org-head-name">{name}</span>
        {line && <span className="org-head-line">{line}</span>}
      </div>
      <svg className={`org-svg shape-${shape}${live ? " live" : ""}${stopped ? " stopped" : ""}`} viewBox={`0 0 ${W} ${H}`} role="group" aria-label={sentence}>
        <defs>
          <marker id={arrowId} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path className="org-arrow" d="M0 0.8 L7 4 L0 7.2 Z" />
          </marker>
        </defs>
        {board && <rect className={`org-board${shape === "lean" ? " belt" : ""}`} x={board.x} y={board.y} width={board.w} height={board.h} rx={shape === "lean" ? 16 : 12} />}
        {edges.map((e, i) => (
          <path key={i} className={`org-edge${e.back ? " back" : ""}`} d={e.d} markerEnd={e.arrow ? `url(#${arrowId})` : undefined} />
        ))}
        {faint.map((f, i) => (
          <circle key={`ghost-${i}`} className="org-ghost" cx={f.x} cy={f.y} r={f.r} />
        ))}
        {notes.map((t, i) => (
          <text key={`note-${i}`} className="org-note" x={t.x} y={t.y} textAnchor={t.anchor ?? "middle"}>
            {t.text}
          </text>
        ))}
        {nodes.map((nd) => {
          const act = nd.onClick;
          const props = act
            ? {
                role: "button" as const,
                tabIndex: 0,
                onClick: act,
                onKeyDown: (e: React.KeyboardEvent) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    act();
                  }
                },
              }
            : {};
          return (
            <g
              key={nd.key}
              className={`org-node ${nd.kind}${nd.state ? ` ${nd.state}` : ""}${nd.halt ? " halt" : ""}${nd.working ? " working" : ""}${nd.kind === "you" && waiting ? " waiting" : ""}`}
              {...props}
            >
              <title>{nd.title}</title>
              {nd.kind === "gate" ? (
                <path className="org-shape" d={`M${nd.x} ${nd.y - nd.r} L${nd.x + nd.r} ${nd.y} L${nd.x} ${nd.y + nd.r} L${nd.x - nd.r} ${nd.y} Z`} />
              ) : (
                <circle className="org-shape" cx={nd.x} cy={nd.y} r={nd.r} />
              )}
              {nd.kind === "lead" && <circle className="org-nucleus" cx={nd.x} cy={nd.y} r={4} />}
              {nd.kind === "result" && <path className="org-tick" d={`M${nd.x - 4.5} ${nd.y} L${nd.x - 1} ${nd.y + 3.5} L${nd.x + 5} ${nd.y - 3.5}`} />}
              {nd.kind === "item" && nd.state === "doing" && <circle className="org-nucleus" cx={nd.x} cy={nd.y} r={Math.max(1.6, nd.r / 3)} />}
              {nd.label && (
                <text
                  className={nd.inside ? "org-name inside" : "org-name"}
                  x={nd.x}
                  y={nd.inside ? nd.y : nd.y + nd.r + 12}
                  textAnchor="middle"
                  dominantBaseline={nd.inside ? "central" : undefined}
                >
                  {nd.label}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      <p className="org-sentence" role="status">
        {sentence}
      </p>
      {live && inHand && <p className="org-now">Now: {inHand.text}</p>}
      {gateNote && (
        <p className="org-gate-note">
          {gateNote}{" "}
          <button className="btn-text" onClick={() => setGateNote(null)}>
            Close
          </button>
        </p>
      )}
    </section>
  );
}

/** What the second look said, in words: the criteria that were met and those that were
 * not, or the verdicts on the items. `null` when nothing has been looked at. */
function gateWords(plan: api.PlanView | undefined): string | null {
  if (!plan) return null;
  const lines: string[] = [];
  for (const c of plan.criteria ?? []) {
    if (c.met === true) lines.push(`Met: ${c.text}.`);
    else if (c.met === false) lines.push(`Not met: ${c.text}${c.why ? ` (${c.why})` : ""}.`);
  }
  plan.items.forEach((i, n) => {
    if (i.review?.state === "checked") lines.push(`Item ${n + 1} checked${i.review.reason ? `: ${i.review.reason}` : "."}`);
    if (i.review?.state === "sent_back") lines.push(`Item ${n + 1} sent back: ${i.review.reason ?? "not done"}`);
  });
  return lines.length ? lines.join(" ") : null;
}
