import { useEffect, useRef, useState } from "react";
import type { OrbState } from "thinking-orbs";
import type { AgentStep } from "../../lib/types";
import { clockText, liveActivity, stepsDuration, stepTime, thinkingIsLive, writingCount } from "../../lib/liveWords";
import Orb from "../Orb/Orb";
import Ticker from "../Ticker/Ticker";
import RollingNumber from "../Ticker/RollingNumber";
import { saveKeptResult, type SearchHit } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import { runWaitsForYou } from "../../lib/chatState";
import DiagnosticsList, { locateDiagnostic } from "./Diagnostics";
import RecallMemoryRow from "./RecallMemoryRow";
import { ChevronIcon } from "../Icons/Icons";
import { shownRun } from "../../lib/runs";

/** Screen-reader sentence for a step (§5.5: "searched project files, 3 matches"). */
function announce(step: AgentStep): string {
  const result = step.result ? `, ${step.result.replace(/^—\s*/, "")}` : "";
  const state = step.status === "running" ? " (in progress)" : "";
  // `RPC-1`: indentation carries this for a sighted reader, and nothing does
  // for anyone else, so the sentence says it outright.
  const from = step.nestedUnder ? "from the script, " : "";
  return `${from}${step.verb} ${step.target}${result}${state}`;
}

function asDate(ms: number): string {
  if (!ms) return "saved";
  return new Date(ms).toLocaleDateString();
}

/** First-person, kind-specific label for a recall chip (SEM-UI-2) — a lesson
 * reads differently from a fact, even though all
 * three arrive as the same `source: "memory"`. */
function chipLabel(m: SearchHit): string {
  if (m.source === "chat") return "earlier chat";
  if (m.source === "file") return "from your files";
  if (m.kind === "lesson") return "◆ learned";
  if (m.kind === "intention") return "◆ reminder";
  return "◆ remembered";
}

/** Where a recalled answer actually came from (RCL-UI). Chat rows jump to the
 *  source conversation; memory rows name the entry that was matched. */
function Provenance({ matches }: { matches: SearchHit[] }) {
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);
  const selectNode = useAppStore((s) => s.selectNode);

  return (
    <div className="recall-matches">
      {matches.map((m, i) => {
        // `CUE-UI-1`: what I remembered or learned says why it came and can be corrected.
        if (m.source === "memory") {
          return <RecallMemoryRow key={`${m.source}-${i}`} hit={m} label={chipLabel(m)} date={asDate(m.created_at)} />;
        }
        const body = (
          <>
            <span className={`recall-chip ${m.source}`}>{chipLabel(m)}</span>
            <span className="recall-title">{m.title}</span>
            <span className="recall-date">{asDate(m.created_at)}</span>
            <span className="recall-snippet">{m.snippet}</span>
          </>
        );
        const convId = m.conversation_id;
        const filePath = m.source === "file" ? m.path : null;
        if (convId) {
          return (
            <button
              key={`${m.source}-${i}`}
              className="recall-row link"
              onClick={() => setActiveConversation(convId)}
              title="Open this conversation"
            >
              {body}
            </button>
          );
        }
        if (filePath) {
          return (
            <button
              key={`${m.source}-${i}`}
              className="recall-row link"
              onClick={() => selectNode({ kind: "file", id: filePath })}
              title="Open this file"
            >
              {body}
            </button>
          );
        }
        return (
          <div key={`${m.source}-${i}`} className="recall-row">
            {body}
          </div>
        );
      })}
    </div>
  );
}

/** The snippet behind a Code Execution step (`DAT-UI-1`) — plain, unstyled
 * source, revealed only when the user asks for it. */
function CodeDisclosure({ code }: { code: { language: string; code: string } }) {
  return (
    <div className="code-disclosure">
      <span className="code-disclosure-lang">{code.language}</span>
      <pre className="code-disclosure-source">{code.code}</pre>
    </div>
  );
}

type UntrustedSource = NonNullable<AgentStep["untrusted"]>[number];

/** `TRU-UI-1`: the quiet marker that a step's content came from outside the
 * model's own knowledge, not a warning. Clicking it opens the same
 * step-detail disclosure the `⌄` control does. */
function UntrustedChip({
  sources,
  open,
  onClick,
}: {
  sources: UntrustedSource[];
  open: boolean;
  onClick: () => void;
}) {
  const scanRisk = Math.max(...sources.map((u) => u.risk));
  // `USE-8`: the quick check found orders the pattern scan did not. It is drawn
  // like a scan of risk 2: the text was already fenced as data either way.
  const readsLikeOrders = sources.some((u) => u.readsAsInstructions !== undefined);
  const maxRisk = readsLikeOrders ? Math.max(scanRisk, 2) : scanRisk;
  const labels = Array.from(new Set(sources.map((u) => u.label))).join(", ");
  const flags = Array.from(new Set(sources.flatMap((u) => u.flags)));
  const text =
    scanRisk >= 2
      ? "◇ from outside: I ignored its instructions"
      : readsLikeOrders
        ? "◇ from outside: it reads like orders to me"
        : "◇ from outside";
  const ariaLabel = scanRisk >= 2 && flags.length > 0 ? `${text} (${flags.join(", ")})` : text;

  return (
    <button
      className={`untrusted-chip risk-${maxRisk}`}
      onClick={onClick}
      aria-expanded={open}
      aria-label={ariaLabel}
      title={labels}
    >
      {text}
    </button>
  );
}

/** The raw text behind an `◇ from outside` chip, grouped by where each piece
 * came from — revealed only on demand, same as `CodeDisclosure`. */
function UntrustedDisclosure({ sources }: { sources: UntrustedSource[] }) {
  return (
    <div className="untrusted-disclosure">
      {sources.map((u, i) => (
        <div key={`${u.label}-${i}`} className="untrusted-source">
          <span
            className="untrusted-source-label"
            // `SEE-4`: words on the line, the number on the hover.
            title={u.readsAsInstructions !== undefined ? `quick check, sure ${u.readsAsInstructions.toFixed(2)}` : undefined}
          >
            {u.label}
            {u.readsAsInstructions !== undefined && " · reads like orders to the assistant"}
          </span>
          {u.text && <pre className="untrusted-source-text">{u.text}</pre>}
        </div>
      ))}
    </div>
  );
}

function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** `HRN-UI-4`: the whole of a result that was too big to paste. The model only
 * ever saw the first couple of kilobytes of this, which is exactly why it is
 * worth being able to open. */
function KeptDisclosure({ kept }: { kept: NonNullable<AgentStep["kept"]> }) {
  const convId = useAppStore((s) => s.activeConversationId);
  const folder = useAppStore(
    (s) => s.conversations.find((c) => c.id === s.activeConversationId)?.folderPath ?? null
  );
  const [saved, setSaved] = useState(false);
  const save = async () => {
    if (!convId) return;
    try {
      await saveKeptResult(convId, kept.reference, kept.text);
      setSaved(true);
    } catch {
      setSaved(false);
    }
  };
  return (
    <div className="kept-disclosure">
      <div className="kept-head">
        <span className="kept-ref">{kept.reference}</span>
        <span className="kept-size">{bytes(kept.bytes)}, I kept the full result</span>
        {folder && !saved && (
          <button className="kept-save" onClick={save}>
            Save to the working folder
          </button>
        )}
        {saved && <span className="kept-saved">Saved as {kept.reference}.txt</span>}
      </div>
      <pre className="kept-text">{kept.text}</pre>
    </div>
  );
}

type StepTask = NonNullable<AgentStep["task"]>;

/** `COD-UI-2`: what a task found, diagnostics first and the raw tail below.
 * Every `line:col` opens that file as a tab, scrolled to the line — one file
 * is one item, so it is a tab and not a sidebar view. */
function TaskDisclosure({ task }: { task: StepTask }) {
  const openItem = useAppStore((s) => s.openItem);
  const root = useAppStore(
    (s) => s.conversations.find((c) => c.id === s.activeConversationId)?.folderPath ?? null
  );
  const diagnostics = task.diagnostics ?? [];
  return (
    <div className="task-disclosure">
      <code className="task-command">
        {task.argv.join(" ")}
        {task.cwd ? `  (in ${task.cwd})` : ""}
      </code>
      <DiagnosticsList
        items={diagnostics}
        onOpen={(d) => {
          const path = locateDiagnostic(d.file, root, task.cwd);
          if (path) openItem({ kind: "file", id: path, line: d.line ?? undefined });
        }}
      />
      {task.tail && <pre className="task-tail">{task.tail}</pre>}
    </div>
  );
}

/** A result reads as a reading, not a sentence: the backend's "— 34 lines"
 * becomes "34 lines" in the row's stamp column. */
function stamp(result: string): string {
  return result.replace(/^\s*—\s*/, "");
}

function Step({ step, lit }: { step: AgentStep; lit: boolean }) {
  const [open, setOpen] = useState(false);
  const matches = step.matches ?? [];
  const code = step.code;
  const kept = step.kept;
  const task = step.task;
  const untrusted = step.untrusted ?? [];
  const fired = step.fired ?? [];
  const taskHasDetail = !!task && (!!task.tail || (task.diagnostics?.length ?? 0) > 0);
  const expandable = matches.length > 0 || !!code || !!kept || untrusted.length > 0 || fired.length > 0 || taskHasDetail;
  const expandLabel = taskHasDetail
    ? "what the task reported"
    : code
    ? "the code behind this step"
    : kept
      ? "the full result I kept"
      : matches.length > 0
        ? `the ${matches.length} sources for this recall`
        : fired.length > 0 && untrusted.length === 0
          ? "what I checked here"
          : "what this step read from outside";
  const toggle = () => setOpen((v) => !v);

  // One grid row: status dot, verb, target, then the readings. Nothing in the
  // head wraps — the target clips, the stamp clips, and everything the row can
  // say at more length opens *under* it, full width, never beside it.
  return (
    <div
      className={`step ${step.status}${step.nestedUnder ? " nested" : ""}${expandable ? " expandable" : ""}${open ? " open" : ""}`}
      role="listitem"
      aria-label={announce(step)}
    >
      <div className="step-head" onClick={expandable ? toggle : undefined}>
        <span className="step-dot" aria-hidden="true" />
        <span className="verb" aria-hidden="true">
          {lit && step.status === "running" ? <span className="live-words">{step.verb}</span> : step.verb}
        </span>
        <span className="target" aria-hidden="true" title={task ? task.argv.join(" ") : step.target}>
          {step.target}
        </span>
        <span className="step-meta" onClick={(e) => e.stopPropagation()}>
          {untrusted.length > 0 && <UntrustedChip sources={untrusted} open={open} onClick={toggle} />}
          {step.result && (
            <span className="result" aria-hidden="true" title={stamp(step.result)}>
              {stamp(step.result)}
            </span>
          )}
          {step.status !== "running" && step.startedAt !== undefined && step.endedAt !== undefined && (
            <span className="step-time" aria-hidden="true">
              {stepTime(step.endedAt - step.startedAt)}
            </span>
          )}
          {expandable && (
            <button
              className="step-expand"
              onClick={toggle}
              aria-expanded={open}
              aria-label={`${open ? "Hide" : "Show"} ${expandLabel}`}
            >
              <ChevronIcon dir="right" size={11} strokeWidth={1.6} />
            </button>
          )}
        </span>
      </div>
      {/* A four-minute build is not four minutes of silence: the newest line
          it printed stands under the step until it finishes. */}
      {task && step.status === "running" && task.lastLine && (
        <span className="task-live-line" aria-live="off">
          {task.lastLine}
        </span>
      )}
      {open && (
        <>
          {task && taskHasDetail && <TaskDisclosure task={task} />}
          {code && <CodeDisclosure code={code} />}
          {kept && <KeptDisclosure kept={kept} />}
          {!code && matches.length > 0 && <Provenance matches={matches} />}
          {untrusted.length > 0 && <UntrustedDisclosure sources={untrusted} />}
          {fired.length > 0 && (
            <div className="fired-disclosure">
              {fired.map((f) => (
                <p key={f.lesson}>{f.line}</p>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** `HRN-UI-2`: consecutive steps that share a parallel batch, folded into one
 * run so they can be drawn as a band. Everything else stays a lone step. */
export function bands(steps: AgentStep[]): { group: string | null; steps: AgentStep[] }[] {
  const out: { group: string | null; steps: AgentStep[] }[] = [];
  for (const step of steps) {
    const group = step.parallelGroup ?? null;
    const last = out[out.length - 1];
    if (group && last && last.group === group) last.steps.push(step);
    else out.push({ group, steps: [step] });
  }
  return out;
}

/** Several steps running at once, drawn as one thing. A stack that fills a row
 * at a time reads as sequential work, which is precisely what this is not. */
function ParallelBand({ steps, lit }: { steps: AgentStep[]; lit: boolean }) {
  const working = steps.some((s) => s.status === "running");
  return (
    <div className={`step-band ${working ? "running" : "settled"}`}>
      <span className="band-header" aria-hidden="true">
        {steps.length} things at once
      </span>
      {steps.map((step) => (
        <Step key={step.id} step={step} lit={lit} />
      ))}
    </div>
  );
}

/** `"planned"` and `"updated the plan"` steps are the backend's own log of
 * every change it made to `message.plan` — the exact same information
 * `PlanCard` already renders above the timeline, item by item, with its
 * checkbox marks and drop reasons. Left in here too, they turned every plan
 * revision into three or four near-duplicate lines repeating the same item
 * text, which was most of what made a working run look cluttered. Nothing is
 * lost by dropping them: the plan's current state is never only on the
 * timeline. */
function isPlanNote(step: AgentStep): boolean {
  return step.verb === "planned" || step.verb === "updated the plan";
}

/** The folded run in one line: the verbs it used most, counted, in the order
 * they first happened — "searched 4 · fetched 2 · created 1". Three is as many
 * as a glance holds; the rest are counted, not named. */
export function summarize(steps: AgentStep[]): string {
  const counts = new Map<string, number>();
  for (const s of steps) counts.set(s.verb, (counts.get(s.verb) ?? 0) + 1);
  const ranked = [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 3);
  const named = new Set(ranked.map(([verb]) => verb));
  const parts = [...counts.entries()]
    .filter(([verb]) => named.has(verb))
    .map(([verb, n]) => `${verb} ${n}`);
  const rest = steps.filter((s) => !named.has(s.verb)).length;
  if (rest > 0) parts.push(`${rest} more`);
  return parts.join(" · ");
}

/** While a run is live and long, only its newest rows stay on screen — forty
 * steps of scrolling history would push the part that is moving out of view. */
const LIVE_TAIL = 6;

/** `WRK-2`/`WRK-5`: a line where an item ended or the run started fresh. The
 * fresh start carries the note it wrote itself, which a click opens. */
function Divider({ step }: { step: AgentStep }) {
  const [open, setOpen] = useState(false);
  if (!step.note) {
    return (
      <div className="item-divider" role="listitem">
        {step.divider}
      </div>
    );
  }
  return (
    <div className="item-divider has-note" role="listitem">
      <button className="item-divider-btn" aria-expanded={open} onClick={() => setOpen(!open)}>
        {step.divider}
      </button>
      {open && <pre className="item-divider-note">{step.note}</pre>}
    </div>
  );
}

/** Do these steps make a timeline? The same test `Timeline` uses to draw nothing, so the
 * turn can tell whether it has one (`UI-4`). */
export function hasVisibleSteps(steps: AgentStep[] | undefined): boolean {
  return !!steps && steps.some((s) => !isPlanNote(s) && !s.divider);
}

/** `UI-4`: the head of a live timeline: the orb, "Working", what it does now and the clock. One
 * moving line for the whole turn. The words change in place (`Ticker`), and "Working" carries
 * the moving light unless something else in the turn has it (`mayLight` is false), the
 * thinking line below has it, or the run is waiting for you. */
function LiveHead({ steps, orb, mayLight }: { steps: AgentStep[]; orb: OrbState | null; mayLight: boolean }) {
  const run = useAppStore(shownRun);
  const asking = useAppStore((s) => runWaitsForYou(s, s.activeConversationId));
  const lit = mayLight && !asking && !thinkingIsLive(steps, run?.thinking);
  // A long tool call being written: the count is what shows it is still going.
  const words = writingCount(steps, run);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!run) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [run]);
  return (
    <>
      <span className="timeline-orb">{orb && <Orb state={orb} />}</span>
      <span className="timeline-title">
        <span className={lit ? "live-words" : undefined}>Working</span>
      </span>
      <span className="timeline-sep" aria-hidden="true">
        ·
      </span>
      <span className="timeline-activity">
        <Ticker text={liveActivity(steps, run)} />
      </span>
      {words !== null && (
        <>
          <span className="timeline-sep" aria-hidden="true">
            ·
          </span>
          <span className="timeline-clock">
            <RollingNumber value={words} />
            {" words"}
          </span>
        </>
      )}
      {run && (
        <>
          <span className="timeline-sep" aria-hidden="true">
            ·
          </span>
          <span className="timeline-clock">{clockText(Date.now() - run.startedAt)}</span>
        </>
      )}
    </>
  );
}

/**
 * What the agent did, as one line that opens into its steps. While the work runs the line says
 * what is happening now (`UI-4`); once it is done it says how many steps and how long. Closed
 * either way: the answer is what you came for, and the steps are how it was got. Your own
 * choice wins: open it and it stays open.
 */
export default function Timeline({
  steps,
  live = false,
  orb = null,
  mayLight = true,
}: {
  steps: AgentStep[];
  live?: boolean;
  /** The orb that goes with the live head, or none. There is only ever one orb in a turn. */
  orb?: OrbState | null;
  /** False while something else in the turn carries the moving light. */
  mayLight?: boolean;
}) {
  const [chosen, setChosen] = useState<boolean | null>(null);
  const [showAll, setShowAll] = useState(false);
  const asking = useAppStore((s) => live && runWaitsForYou(s, s.activeConversationId));
  // The count and the time of a run that ended in front of you flap in, once. A turn read
  // back from the chat is simply there.
  const ran = useRef(false);
  if (live) ran.current = true;
  // `WRK-2`: a divider is where an item ended, not a step. It is drawn between
  // the steps and never counted among them.
  const noted = steps.filter((s) => !isPlanNote(s));
  const visible = noted.filter((s) => !s.divider);
  if (!visible.length) return null;

  // `UI-4`: closed while live. The head says what is happening; the rows are one click away.
  const open = chosen ?? false;
  const failed = visible.filter((s) => s.status === "error").length;
  const hidden = live && !showAll ? Math.max(0, visible.length - LIVE_TAIL) : 0;
  const first = hidden ? noted.findIndex((s) => s.id === visible[hidden].id) : 0;
  const rows = noted.slice(Math.max(0, first));
  const count = `${visible.length} ${visible.length === 1 ? "step" : "steps"}`;
  const took = live ? null : stepsDuration(visible);
  const stepsLit = mayLight && !asking;
  // Open, a running step is on screen: the light is on its row, not on the head.
  const onRow = open && visible.some((s) => s.status === "running");
  const arrived = !live && ran.current;

  return (
    <div className={`timeline ${live ? "live" : "settled"}${open ? " open" : ""}${arrived ? " arrived" : ""}`}>
      <button
        className="timeline-head"
        onClick={() => setChosen(!open)}
        aria-expanded={open}
      >
        <span className="timeline-twisty" aria-hidden="true">
          <ChevronIcon dir="right" size={12} strokeWidth={1.6} />
        </span>
        {live ? (
          <LiveHead steps={visible} orb={orb} mayLight={mayLight && !onRow} />
        ) : (
          <>
            <span className="timeline-title">{count}</span>
            <span className="timeline-summary">
              {[took === null ? "" : stepTime(took), summarize(visible)].filter(Boolean).join(" · ")}
            </span>
          </>
        )}
        {failed > 0 && <span className="timeline-failed">{failed} failed</span>}
      </button>
      {open && (
        <div className="timeline-rows" role="list" aria-label="Steps the agent took">
          {hidden > 0 && (
            <button className="timeline-earlier" onClick={() => setShowAll(true)}>
              {hidden} earlier {hidden === 1 ? "step" : "steps"}
            </button>
          )}
          {bands(rows).map((band, i) =>
            band.group && band.steps.length > 1 ? (
              <ParallelBand key={band.group + i} steps={band.steps} lit={stepsLit} />
            ) : (
              band.steps.map((step) =>
                step.divider ? (
                  <Divider key={step.id} step={step} />
                ) : (
                  <Step key={step.id} step={step} lit={stepsLit} />
                )
              )
            )
          )}
        </div>
      )}
    </div>
  );
}
