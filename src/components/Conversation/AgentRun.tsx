import { useEffect, useMemo, useState } from "react";
import type { Message } from "../../lib/types";
import type { StopReason } from "../../lib/api";
import { isPersistedId, sayRefused, useAppStore } from "../../lib/store";
import Timeline, { hasVisibleSteps } from "./Timeline";
import CheckedAnswer from "./VerifyPanel";
import RunText from "./RunText";
import { experimentOn } from "../../lib/experiments";
import { canVerify } from "../../lib/verify";
import ChatMedia, { ChatMediaPending } from "./ChatMedia";
import FleetCard from "./FleetCard";
import PlanCard from "./PlanCard";
import { ThoughtReceipt, ThoughtStrip } from "./ThoughtStrip";
import BlockRenderer from "../Blocks/BlockRenderer";
import ProposalCard from "./ProposalCard";
import QuestionCard from "./QuestionCard";
import HarnessProposal from "./HarnessProposal";
import Orb from "../Orb/Orb";
import { ChevronIcon, DownloadIcon, ForkIcon, KebabIcon, RefreshIcon, RewindIcon } from "../Icons/Icons";
import CopyButton from "./CopyButton";
import { fullTime, turnTime } from "../../lib/time";
import { artifactKind, downloadArtifact } from "../Workbench/artifactFiles";
import { SpokenGlyph, StoppedHere } from "./SpokenMark";
import { orbForStep } from "../Orb/orbState";
import { liveActivity, stepWords, thinkingIsLive, writingCount } from "../../lib/liveWords";
import { estimateTokens } from "../../lib/context";
import { runWaitsForYou } from "../../lib/chatState";
import Ticker from "../Ticker/Ticker";
import RollingNumber from "../Ticker/RollingNumber";
import type { OrbState } from "thinking-orbs";
import { busyIn, openQuestions, shownRun } from "../../lib/runs";
import "../Context/Context.css";

/** The working orb, shown while the turn is live but nothing else on screen is
 * moving. Without this the turn looks stalled — the composer's stop button was
 * the only sign anything was happening.
 *
 * `label` names what it's waiting on once some steps have already run: after a
 * browse-click-read sequence, a bare animation doesn't distinguish "thinking
 * about the next move" from "hung". */
function Thinking({ label }: { label?: string }) {
  return (
    <div className="thinking" role="status" aria-label={label ?? "Agent is working"}>
      <Orb state="working" />
      {label && <span className="thinking-label">{label}</span>}
    </div>
  );
}

/** A turn that ended having said nothing at all.
 *
 * Small models do this routinely after a run of tool calls — especially one
 * that ended in failures. An empty bubble is indistinguishable from a turn
 * that's still going, which is the worst thing it could look like, so the end
 * of the turn is stated outright. */
function SaidNothing() {
  return (
    <p className="run-empty">
      I stopped without saying anything. Ask me again, or tell me what to do with what I found.
    </p>
  );
}

/** A turn that ran its steps and then wrote no answer. A different failure
 * from saying nothing at all: the work is on screen, only the report is
 * missing. It happens when a model returns an empty reply after its tool
 * results, even after the loop has asked it once more for the answer. */
function NoAnswer() {
  return (
    <p className="run-no-answer">
      I did the steps above but ended without writing an answer. Ask me what they showed.
    </p>
  );
}

/** `HRN-UI-3`: a quiet line under the timeline while the run is live, so the
 * time between steps reads as work rather than as a hang.
 *
 * It used to read "step 1 of 12". That was the step *budget*, and as a live
 * status line it was worse than useless: it told you nothing about what the run
 * was doing, and the 12 invited the reading that the app had planned twelve
 * steps — which it had not. The budget still exists and is still settable
 * (Settings → Tools), but it belongs where a limit belongs: in the setting, and
 * in the message you get if a run ever hits it. What goes here is what the run
 * is doing now.
 *
 * Only rendered for the turn that is actually streaming — a finished turn's
 * meter would be a number about nothing.
 *
 * `orb` is the animation for what the line says. Once the meter is drawn the
 * orb lives in its slot and the bare `Thinking` orb is gone: drawn above the
 * line it left the slot empty, and the line looked indented under nothing. The
 * slot is kept even without an orb so the text does not slide sideways. */
export function RunMeter({
  steps,
  orb,
  idle = false,
  mayLight = true,
}: {
  steps?: Message["steps"];
  orb: OrbState | null;
  /** Before the run's first step, show a plain "what it is doing" line instead
   * of nothing. For views (the Workspace dock) with no other sign of life. */
  idle?: boolean;
  /** False while something else in the turn carries the moving light. */
  mayLight?: boolean;
}) {
  const run = useAppStore(shownRun);
  const asking = useAppStore((s) => runWaitsForYou(s, s.activeConversationId));
  const [, tick] = useState(0);
  useEffect(() => {
    if (!run) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [run]);
  if (!run || run.step < 1) {
    if (!idle) return null;
    const pending = steps?.find((s) => s.status === "running");
    return (
      <div className="run-meter-row" role="status">
        <span className="run-meter-orb">{orb && <Orb state={orb} />}</span>
        <p className="run-meter">
          <span className="run-meter-words">
            <Ticker text={pending ? stepWords(pending) : "working"} live={mayLight} />
          </span>
        </p>
      </div>
    );
  }
  const secs = Math.floor((Date.now() - run.startedAt) / 1000);
  const clock = `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
  // `OBS-3`: the window's fill is read on the model control's ring under the
  // composer. The meter says it only once it is tight, the one moment it changes
  // what to expect from the run.
  const tight = !!run.contextWindow && run.contextTokens / run.contextWindow > 0.8;
  const context = tight
    ? `context ${Math.min(100, Math.round((run.contextTokens / run.contextWindow!) * 100))}%`
    : null;
  // What is happening right now: a tool that is running, then thinking we can see arriving,
  // then the honest fallback, with the plan item in front (`liveActivity`).
  const doing = liveActivity(steps, run);
  const thinkingNow = thinkingIsLive(steps, run.thinking);
  // A long tool call being written: the count is what shows it is still going.
  const words = writingCount(steps, run);
  return (
    <>
      <div className="run-meter-row">
        <span className="run-meter-orb">{orb && <Orb state={orb} />}</span>
        <p className={`run-meter ${tight ? "tight" : ""}`}>
          <span className="run-meter-words">
            <Ticker text={doing} live={mayLight && !thinkingNow && !asking} />
          </span>
          <span className="run-meter-rest">
            {words !== null && (
              <>
                {" · "}
                <RollingNumber value={words} />
                {" words"}
              </>
            )}
            {` · ${clock}`}
            {context ? ` · ${context}` : ""}
          </span>
        </p>
      </div>
      <ThinkingTrace text={run.thinking} live={thinkingNow} />
    </>
  );
}

/** The model's thinking while it is thinking.
 *
 * A reasoning model can spend minutes here before its first word, and the meter
 * above it was the only sign of life — a clock ticking beside an empty screen,
 * which reads as a hang. This is folded shut by default because thinking is not
 * the answer and should not compete with it, but it is openable, because the
 * only thing worse than a wall of reasoning is no evidence at all.
 *
 * While the thought is arriving (`live`) the word carries the moving light, and the
 * token count flaps upward. The count is an estimate, hence the "~". The reasoning
 * itself never moves: it is there to be read. */
function ThinkingTrace({ text, live }: { text: string; live: boolean }) {
  const [open, setOpen] = useState(false);
  if (!text) return null;
  const tokens = estimateTokens(text);
  return (
    <details className="run-thinking" open={open} onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>
        <span className="run-thinking-twisty" aria-hidden="true">
          <ChevronIcon dir="right" size={11} strokeWidth={1.6} />
        </span>
        <span className={live ? "live-words" : undefined}>Thinking</span>
        <span className="run-thinking-count">
          <span>~</span>
          <RollingNumber value={tokens} />
          <span>{" tokens"}</span>
        </span>
      </summary>
      {/* The tail, not the head: what it is thinking about now is the part
          that says whether it is still going anywhere. */}
      <pre>{text.slice(-4000)}</pre>
    </details>
  );
}

/** The model's thinking under a timeline that is live: the meter used to carry it. */
function LiveThinking({ steps }: { steps: Message["steps"] }) {
  const text = useAppStore((s) => shownRun(s)?.thinking ?? "");
  return <ThinkingTrace text={text} live={thinkingIsLive(steps, text)} />;
}

/** `HRN-3`: why a turn stopped, when it stopped for anything but finishing.
 * The text above is what the run had in hand, and saying so is the difference
 * between a short answer and a truthful one. */
export function StoppedNote({ reason, canContinue = false }: { reason: StopReason; canContinue?: boolean }) {
  const resumeLastRun = useAppStore((s) => s.resumeLastRun);
  const said: Record<StopReason, string> = {
    completed: "",
    aborted: "You stopped me. This is what I had.",
    timeout: "I ran out of time. This is what I had.",
    max_steps: "I stopped at my step limit. This is what I had.",
    stuck: "I stopped because I was not getting anywhere. This is what I had.",
    budget: "The team reached its limit and stopped. This is what was done.",
    error: "The model call failed partway. This is what I had.",
    // `XA-11`: only a job of an agent app ends this way; its card says so, not this note.
    plan_limit: "",
    // `VTN-6`: a spoken reply the user cut off has its own mark (`SpokenMark`).
    interrupted: "",
  };
  const text = said[reason];
  if (!text) return null;
  // `HRN-UI-5`: the way on sits on the line that says why it stopped. Only a run
  // that ended at a limit or was interrupted, and only the last one, because
  // "continue" has one meaning and it is the run at the end. A failed call is
  // asked again, not continued.
  const resumable = reason === "aborted" || reason === "timeout" || reason === "max_steps" || reason === "stuck";
  return (
    <p className="run-stopped">
      {text}
      {canContinue && resumable && (
        <button className="run-continue" onClick={() => void resumeLastRun()}>
          Continue where I stopped
        </button>
      )}
    </p>
  );
}

/** Cards for artifacts produced during this turn (CHT-6). The card opens the
 * Workbench straight on that artifact instead of always landing on whichever
 * one is currently selected; the button at its edge saves the file. */
function ArtifactChips({ ids }: { ids: string[] }) {
  const convId = useAppStore((s) => s.activeConversationId);
  const artifacts = useAppStore((s) => (convId ? s.artifacts[convId] ?? [] : []));
  const openArtifact = useAppStore((s) => s.openArtifact);

  return (
    <div className="artifact-chips">
      {ids.map((id) => {
        const artifact = artifacts.find((a) => a.id === id);
        if (!artifact) return null;
        const { Icon, meta } = artifactKind(artifact);
        return (
          <div key={id} className="chat-artifact">
            <button
              className="chat-artifact-open"
              onClick={() => openArtifact(id)}
              title={`Open “${artifact.title}” in the Workbench`}
            >
              <span className="chat-artifact-icon">
                <Icon size={18} />
              </span>
              <span className="chat-artifact-text">
                <span className="chat-artifact-title">{artifact.title}</span>
                <span className="chat-artifact-meta">{meta}</span>
              </span>
            </button>
            <button
              className="chat-artifact-save"
              onClick={() => void downloadArtifact(artifact)}
              title="Download"
              aria-label={`Download “${artifact.title}”`}
            >
              <DownloadIcon size={16} />
            </button>
          </div>
        );
      })}
    </div>
  );
}

/** Files the agent changed during this turn. This is the link that makes the
 * Workbench feel connected to what just happened rather than a separate
 * browser: click to see the file, Undo to put it back. */
function ChangedFiles({ ids }: { ids: string[] }) {
  const trash = useAppStore((s) => s.trash);
  const selectNode = useAppStore((s) => s.selectNode);
  const setDockOpen = useAppStore((s) => s.setDockOpen);
  const undoFileOp = useAppStore((s) => s.undoFileOp);

  const [open, setOpen] = useState(false);
  const entries = ids.map((id) => trash.find((t) => t.id === id)).filter(Boolean);
  if (entries.length === 0) return null;
  // One file is its own row; more fold into one line that opens to the list.
  const folds = entries.length > 1;
  const undone = entries.filter((t) => t!.undone).length;

  return (
    <div className={`changed-files${folds ? " folds" : ""}${open ? " open" : ""}`}>
      {folds && (
        <button className="changed-files-head" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
          <span className="changed-files-twisty" aria-hidden="true">
            <ChevronIcon dir="right" size={11} strokeWidth={1.6} />
          </span>
          Changed {entries.length} files
          {undone > 0 && <span className="changed-files-undone">, {undone} undone</span>}
        </button>
      )}
      {(!folds || open) && (
        <div className="changed-files-list">
      {entries.map((t) => (
        <div key={t!.id} className={`changed-file ${t!.undone ? "undone" : ""}`}>
          <button
            className="changed-file-name"
            title={t!.path}
            onClick={() => {
              setDockOpen(true);
              selectNode({ kind: "file", id: t!.path });
            }}
          >
            {t!.path.split(/[\\/]/).pop()}
          </button>
          {t!.undone ? (
            <span className="changed-file-undone">undone</span>
          ) : (
            <button className="changed-file-undo" onClick={() => void undoFileOp(t!.id).catch(sayRefused)}>
              Undo
            </button>
          )}
        </div>
      ))}
        </div>
      )}
    </div>
  );
}

/**
 * `HRN-UI-5`: what you can do with an answer once it exists.
 *
 * **Copy** takes its text, also while the chat is busy: it changes nothing.
 * **Try again**, on the last answer only, replaces it: the same message is sent
 * once more in this chat. **Rewind** removes your message and what followed.
 * **Fork** carries on from this answer in a new chat and leaves this one alone.
 *
 * **Continue where I stopped** appears only on a run that ended at a limit or
 * was interrupted, and only on the last turn, because "continue" has one
 * meaning and it is the run at the end. It picks that run back up with its tool
 * results intact instead of paying for them twice.
 */
function TurnActions({ message, last }: { message: Message; last: boolean }) {
  const [more, setMore] = useState(false);
  const retryAnswer = useAppStore((s) => s.retryAnswer);
  const busy = useAppStore((s) => busyIn(s, s.activeConversationId));
  const requestRewind = useAppStore((s) => s.requestRewind);
  const forkAt = useAppStore((s) => s.forkAt);
  // `USE-UI-7`, `SEE-5`: only while `fast-slow` is on, and not while this answer is being checked.
  const verifyOn = useAppStore((s) => experimentOn(s, "fast-slow"));
  const verifyMessage = useAppStore((s) => s.verifyMessage);
  const checking = useAppStore((s) => s.verifications[message.id]?.running === true);
  // `RWD-UI-1`: the message you wrote that this answer replied to. A primitive,
  // so the selector stays stable.
  const askedBy = useAppStore((s) => {
    const conv = s.conversations.find((c) => c.id === s.activeConversationId);
    const list = conv?.messages ?? [];
    const at = list.findIndex((m) => m.id === message.id);
    for (let i = at - 1; i >= 0; i--) if (list[i].role === "user" && isPersistedId(list[i].id)) return list[i].id;
    return null;
  });
  if (message.streaming) return null;

  const persisted = isPersistedId(message.id);
  const canAct = !busy && persisted;
  const at = message.createdAt || null;
  // The line says the time alone. Who answered and the full date wait in its tooltip.
  const about = [message.model?.name, at && fullTime(at)].filter(Boolean).join("\n");
  // A spoken reply you cut off shows what you heard; Copy takes the whole of it.
  const copy = message.unspoken ?? message.text;
  if (!at && !canAct && !copy) return null;

  // The rare one waits behind the `⋯`.
  const deeper = canAct && verifyOn && !checking && canVerify(message.text);

  // One line, shown on hover (or keyboard focus) so a finished turn stays as
  // quiet as its text until you reach for it.
  return (
    <div className={`turn-actions${more ? " menu-open" : ""}`}>
      {copy && <CopyButton text={copy} tip="Copy this answer" />}
      {canAct && last && !!askedBy && (
        <button
          className="turn-btn"
          aria-label="Try again"
          data-tip="Try again: replace this answer with a new one"
          onClick={() => void retryAnswer(message.id)}
        >
          <RefreshIcon size={14} strokeWidth={1.5} />
        </button>
      )}
      {canAct && !!askedBy && (
        <button
          className="turn-btn"
          aria-label="Rewind"
          data-tip="Rewind: remove your message and everything after it"
          onClick={() => requestRewind(askedBy)}
        >
          <RewindIcon size={14} strokeWidth={1.5} />
        </button>
      )}
      {canAct && (
        <button
          className="turn-btn"
          aria-label="Fork"
          data-tip="Fork: continue from here in a new chat"
          onClick={() => void forkAt(message.id).catch(sayRefused)}
        >
          <ForkIcon size={14} strokeWidth={1.5} />
        </button>
      )}
      {deeper && (
        <span className="turn-more">
          <button
            className="turn-more-btn"
            aria-label="More for this answer"
            aria-haspopup="menu"
            aria-expanded={more}
            data-tip="More"
            onClick={() => setMore((v) => !v)}
          >
            <KebabIcon size={14} />
          </button>
          {more && (
            <>
              <div className="row-menu-backdrop" onClick={() => setMore(false)} />
              <div className="row-menu" role="menu">
                <button
                  className="row-menu-item"
                  role="menuitem"
                  onClick={() => {
                    setMore(false);
                    void verifyMessage(message.id);
                  }}
                  title="Search the web for this answer's claims (their words leave this computer) and have my slower model review the doubtful ones"
                >
                  Check more deeply
                </button>
              </div>
            </>
          )}
        </span>
      )}
      {at && (
        <span className="turn-time" tabIndex={0} data-tip={about}>
          {message.spoken && <SpokenGlyph />}
          {turnTime(at)}
        </span>
      )}
    </div>
  );
}

/** `AGC-3`/`AGC-4`: what the run is waiting on, and what it asked of you. These
 * are decisions, not decoration — the Workspace view shows them too, because a
 * run paused on a question nobody can see never continues. */
export function RunAsks({ message }: { message: Message }) {
  // Selectors return stable references (zustand v5): the filter is a memo.
  const proposals = useAppStore((s) => s.harnessProposals);
  // `PAR-10`: this turn's own question, also while another chat's waits ahead of it.
  const question = useAppStore((s) => openQuestions(s).find((q) => q.messageId === message.id));
  const asked = useMemo(() => proposals.filter((p) => p.messageId === message.id), [proposals, message.id]);
  return (
    <>
      {message.proposalIds?.map((id) => (
        <ProposalCard key={id} id={id} />
      ))}
      {question && <QuestionCard q={question} />}
      {asked.map((p) => (
        <HarnessProposal key={p.id} proposal={p} />
      ))}
    </>
  );
}

/** `/focus`: an answer as its words and nothing around them. What it made (a
 * picture, a clip) is part of what it said, so that stays. A question the run is
 * waiting on stays too: hidden, the run would wait for ever on something nobody can
 * see. While it works with nothing yet to read, one orb says so. */
function FocusedRun({ message }: { message: Message }) {
  const media = message.attachments?.filter((a) => a.kind === "image" || a.kind === "video") ?? [];
  const waiting = !!message.streaming && !message.text && media.length === 0;
  const blank = !message.streaming && !message.text && media.length === 0 && !message.pendingMedia;
  // Nothing to read: the turn leaves no empty box behind, only what it asks of you.
  if (blank) return <RunAsks message={message} />;
  return (
    <div className="agent-run focus" data-message-id={message.id}>
      {message.text && <RunText text={message.text} streaming={message.streaming} />}
      {media.map((a) => (
        <ChatMedia key={a.id} attachment={a} />
      ))}
      {message.pendingMedia && (
        <ChatMediaPending
          modality={message.pendingMedia.modality}
          aspectRatio={message.pendingMedia.aspectRatio}
          startedAt={message.pendingMedia.startedAt}
          jobId={message.pendingMedia.jobId}
        />
      )}
      {waiting && <Thinking />}
      <RunAsks message={message} />
    </div>
  );
}

export default function AgentRun({
  message,
  last = false,
  focus = false,
}: {
  message: Message;
  last?: boolean;
  focus?: boolean;
}) {
  if (focus) return <FocusedRun message={message} />;
  return <FullRun message={message} last={last} />;
}

function FullRun({ message, last }: { message: Message; last: boolean }) {
  const model = message.model;
  const busyNow = useAppStore((s) => busyIn(s, s.activeConversationId));
  // Who answered is said over the answer only when it changed from the answer
  // before. The time's tooltip at the foot always names it.
  const sameModel = useAppStore((s) => {
    if (!model) return false;
    const list = s.conversations.find((c) => c.id === s.activeConversationId)?.messages ?? [];
    for (let i = list.findIndex((m) => m.id === message.id) - 1; i >= 0; i--) {
      const prev = list[i].role === "assistant" ? list[i].model : undefined;
      if (prev) return prev.name === model.name && prev.provenance === model.provenance;
    }
    return false;
  });
  // Is this the newest turn of the chat that has a plan? Only that one is shown whole.
  const newestPlan = useAppStore((s) => {
    const conv = s.conversations.find((c) => c.id === s.activeConversationId);
    const list = conv?.messages ?? [];
    for (let i = list.length - 1; i >= 0; i--) if (list[i].plan) return list[i].id === message.id;
    return true;
  });
  // `WRK-1`: the check that stopped the line, while this turn is the one running.
  const stopped = useAppStore((s) =>
    message.streaming && s.activeConversationId ? s.runSummaries[s.activeConversationId]?.stopped?.check : undefined
  );
  // The words under a picture or clip being made carry the moving light, until partial
  // frames take their place.
  const making = useAppStore(
    (s) => !!message.pendingMedia && !(message.pendingMedia.jobId && s.mediaPartials[message.pendingMedia.jobId])
  );
  // `WRK-7`/`WRK-6`: while this turn is the one running, the run behind its board
  // (for "Not this time") and the item being tried two ways.
  const liveRun = useAppStore((s) =>
    message.streaming && s.activeConversationId ? s.runSummaries[s.activeConversationId] : undefined
  );
  // Video counts as media here too — filtering to `"image"` used to drop a
  // generated clip on the floor, leaving the turn showing only its timeline
  // step while the MP4 sat in Library.
  const media = message.attachments?.filter((a) => a.kind === "image" || a.kind === "video") ?? [];
  const hasContent =
    !!message.text ||
    media.length > 0 ||
    !!message.pendingMedia ||
    !!message.steps?.length ||
    !!message.blocks?.length ||
    // A turn that wrote a plan and nothing else has still shown you something.
    !!message.plan?.items.length;

  // Something on screen is already moving when a step is in flight (its dot
  // pulses) or when prose is arriving (it carries a blinking caret). The gap
  // this closes is the third case: steps have finished and the model is
  // deciding what to do next, with nothing changing anywhere. That is most of
  // the wall-clock time in a browsing run, and it used to look identical to a
  // hang — the old test keyed on *any* step existing, so the indicator
  // switched off permanently the moment the first one landed.
  const runningStep = message.steps?.find((s) => s.status === "running");
  const anyStepRunning = !!runningStep;
  const isThinking = !!message.streaming && !anyStepRunning && !message.text;
  // The orb that goes with the meter line. Where `Thinking` is showing its own
  // there is none here; where prose is arriving the work is composing it.
  const meterOrb: OrbState | null = runningStep
    ? orbForStep(runningStep.verb)
    : message.text
      ? "composing"
      : null;
  const thinkingLabel = message.steps?.length ? "still working" : undefined;
  // `UI-4`: a turn with steps has one live line, the head of its timeline, with the orb and
  // the clock in it. The meter and the bare `Thinking` orb are for a turn that has none yet.
  const timelineShown = hasVisibleSteps(message.steps);
  const headOrb: OrbState | null = meterOrb ?? (isThinking ? "working" : null);
  // Whether the meter draws anything yet. Until the run's first step the bare orb is the
  // only sign of life; from then on the orb is in the meter's line, never above it.
  const metered = useAppStore((s) => (shownRun(s)?.step ?? 0) >= 1);
  // A run that stopped at a limit already says why it has nothing; the generic
  // "I said nothing" line under it would be the same news told worse.
  const saidNothing = !message.streaming && !hasContent && !message.stopReason;
  const noAnswer =
    !message.streaming &&
    !message.text &&
    !!message.steps?.length &&
    !media.length &&
    !message.pendingMedia &&
    !message.blocks?.length &&
    !message.artifactIds?.length &&
    (!message.stopReason || message.stopReason === "completed");

  return (
    <div className="agent-run" data-message-id={message.id}>
      {model && !sameModel && (
        <div className="run-header">
          <span className={`provenance-dot ${model.provenance}`} aria-hidden="true" />
          <span className="agent-label">Agent</span>
          <span className="model-tag">{model.name}</span>
        </div>
      )}
      {/* `PLN-UI-1`: above the timeline, because the plan is what the steps
          below it are for. */}
      {/* `WRK-9`: how I am working rides in the plan's own header. */}
      {message.plan && (
        <PlanCard
          plan={message.plan}
          live={!!message.streaming}
          actionable={last && !message.streaming && !busyNow}
          stopped={stopped}
          runId={liveRun?.live ? liveRun.runId : undefined}
          branching={liveRun?.branching?.item}
          folded={!newestPlan}
        />
      )}
      {/* `SEE-2`: what the quick decisions understood, before the first word. */}
      <ThoughtStrip message={message} />
      {message.steps && (
        <Timeline steps={message.steps} live={!!message.streaming} orb={headOrb} mayLight={!making} />
      )}
      {message.subRunIds && message.subRunIds.length > 0 && (
        <FleetCard runIds={message.subRunIds} />
      )}
      {message.blocks?.map((b) => (
        <BlockRenderer key={b.id} block={b} />
      ))}
      {message.text && <CheckedAnswer message={message} />}
      {media.map((a) => (
        <ChatMedia
          key={a.id}
          attachment={a}
          alt={message.steps?.length ? message.steps[message.steps.length - 1].target : undefined}
        />
      ))}
      {message.pendingMedia && (
        <ChatMediaPending
          modality={message.pendingMedia.modality}
          aspectRatio={message.pendingMedia.aspectRatio}
          startedAt={message.pendingMedia.startedAt}
          jobId={message.pendingMedia.jobId}
        />
      )}
      {isThinking && !timelineShown && !metered && <Thinking label={thinkingLabel} />}
      {message.streaming && !timelineShown && (
        <RunMeter steps={message.steps} orb={headOrb} mayLight={!making} />
      )}
      {message.streaming && timelineShown && <LiveThinking steps={message.steps} />}
      {!message.streaming && message.stopReason && (
        <StoppedNote
          reason={message.stopReason}
          canContinue={last && !busyNow && isPersistedId(message.id)}
        />
      )}
      {!message.streaming && <StoppedHere message={message} />}
      {saidNothing && <SaidNothing />}
      {noAnswer && <NoAnswer />}
      {/* What the turn made, before what you can do with it. */}
      {message.artifactIds && message.artifactIds.length > 0 && (
        <ArtifactChips ids={message.artifactIds} />
      )}
      {message.fileChangeIds && message.fileChangeIds.length > 0 && (
        <ChangedFiles ids={message.fileChangeIds} />
      )}
      {/* `SEE-2` receipt on the left, the hover actions and time on the right:
          one line at the foot of the answer. */}
      <div className="turn-foot">
        <ThoughtReceipt message={message} />
        <TurnActions message={message} last={last} />
      </div>
      <RunAsks message={message} />
    </div>
  );
}
