import { useState } from "react";
import { useActiveConversation, useAppStore } from "../lib/store";
import type { BlockView, Message, UINode } from "../lib/types";
import RunText from "../components/Conversation/RunText";
import PlanCard from "../components/Conversation/PlanCard";
import { RunAsks, RunMeter, StoppedNote } from "../components/Conversation/AgentRun";
import SessionStrip from "../components/Blocks/SessionStrip";
import BlockRenderer from "../components/Blocks/BlockRenderer";
import SurfaceRenderer from "../components/Surface/SurfaceRenderer";
import Orb from "../components/Orb/Orb";
import { orbForPresence, orbForStep } from "../components/Orb/orbState";
import { LayoutIcon, MessageIcon } from "../components/Icons/Icons";
import "../components/Conversation/Conversation.css";
import "./Workspace.css";

/** The Workspace view of a chat that is in Workspace mode. The composed
 * interface (render_ui) is the thing to work with; the chat's own composer sits
 * under it exactly where it does in the message stream, so switching between the
 * two never moves the box you type in.
 *
 * The stream is one click away (`Show chat`, the top bar's switch, the mode chip)
 * and holds the full record. What cannot wait there is shown here: a question the
 * run is paused on, a plan to approve, what the agent is doing and its latest
 * reply. */
export default function Workspace() {
  const conversation = useActiveConversation();
  const busy = useAppStore((s) => s.busy);
  const surface = useAppStore((s) =>
    s.activeConversationId ? s.surfaces[s.activeConversationId] : undefined
  );
  const setSurfaceState = useAppStore((s) => s.setSurfaceState);
  const sendSurfaceAction = useAppStore((s) => s.sendSurfaceAction);
  const setWorkspaceShown = useAppStore((s) => s.setWorkspaceShown);
  const setWorkspaceMode = useAppStore((s) => s.setWorkspaceMode);
  const pendingCount = useAppStore((s) =>
    s.activeConversationId ? (s.pendingActions[s.activeConversationId]?.length ?? 0) : 0
  );

  const msgs = conversation?.messages ?? [];
  const lastAssistant = [...msgs].reverse().find((m) => m.role === "assistant");

  // Blocks that are not the surface (a build's diagnostics, chats from before
  // `render_ui` was the only interface tool); shown with it, not instead of it.
  const blocks: BlockView[] = msgs.flatMap((m) => m.blocks ?? []);

  return (
    <div className="workspace-body">
      <header className="ws-head">
        <div className="ws-col ws-head-row">
          <span className="ws-mode" title="This chat is in Workspace mode">
            <LayoutIcon size={15} />
            Workspace
          </span>
          {/* RCP-UI-3: this workspace came from a procedure we kept. */}
          {conversation?.skillName && (
            <span className="ws-recipe" title={`Started from the ${conversation.skillName} skill`}>
              from the {conversation.skillName} skill
            </span>
          )}
          <span className="ws-head-actions">
            <button
              className="btn-text ws-head-btn"
              onClick={() => setWorkspaceShown(false)}
              title="Show the message stream. Workspace mode stays on."
            >
              <MessageIcon size={14} />
              Show chat
            </button>
            <button
              className="btn-text ws-head-btn"
              onClick={() => setWorkspaceMode(false)}
              title="Turn Workspace mode off for this chat. The chat stays as it is."
            >
              Turn off
            </button>
          </span>
        </div>
        <div className="ws-col">
          <SessionStrip />
        </div>
      </header>

      <div className="ws-canvas">
        <div className="ws-col">
          {surface && (
            <SurfaceRenderer
              tree={surface.data as UINode}
              ctx={{
                state: (surface.state as Record<string, unknown>) ?? {},
                disabled: busy,
                onBind: (key, value) =>
                  setSurfaceState({
                    ...((surface.state as Record<string, unknown>) ?? {}),
                    [key]: value,
                  }),
                onAction: (action, payload, humanText) =>
                  sendSurfaceAction(humanText, { action, ...payload }),
              }}
            />
          )}
          {/* Other blocks of this chat: below the composed interface, never hidden by it. */}
          {blocks.map((b) => (
            <div key={b.id} id={`ws-block-${b.id}`} className="ws-block">
              <BlockRenderer block={b} />
            </div>
          ))}
          {!surface && blocks.length === 0 && (
            <div className="empty-state ws-empty">
              <Orb state={orbForPresence(busy ? "active" : "idle")} size={64} />
              <p className="empty-line">{busy ? "Building your interface…" : "Nothing here yet."}</p>
              <p className="ws-empty-hint">
                Ask for anything. I will build a live interface for it here, such as a board, a
                picker or a dashboard.
              </p>
            </div>
          )}
        </div>
      </div>

      {(lastAssistant || pendingCount > 0) && (
        <div className="ws-dock">
          <div className="ws-dock-col">
            {/* Keyed by message: "Show more" belongs to one reply, not the next. */}
            {lastAssistant && <WorkspaceReply key={lastAssistant.id} message={lastAssistant} />}
            {pendingCount > 0 && (
              <p className="ws-pending" aria-live="polite">
                {pendingCount} change{pendingCount > 1 ? "s" : ""} will be shared with your next
                message
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/** What the stream would have shown that cannot wait for a switch of view: the
 * decisions the run is paused on, then what it is doing, then its latest words.
 * The full turn (steps, files, media) is in the chat. */
function WorkspaceReply({ message }: { message: Message }) {
  const busy = useAppStore((s) => s.busy);
  const setWorkspaceShown = useAppStore((s) => s.setWorkspaceShown);
  const [expanded, setExpanded] = useState(false);

  const running = message.steps?.find((s) => s.status === "running");
  const orb = running ? orbForStep(running.verb) : message.text ? "composing" : "working";
  const long = message.text.length > 280;
  const approving = !!message.plan?.awaiting_approval && !message.streaming && !busy;

  return (
    <>
      {/* `PLF-4`: a plan to approve is a decision, so it cannot sit behind the stream. */}
      {approving && message.plan && <PlanCard plan={message.plan} actionable />}
      <RunAsks message={message} />

      {message.streaming && <RunMeter steps={message.steps} orb={orb} idle />}

      {message.text && (
        <div className={`ws-reply${long ? " long" : ""}${expanded ? " expanded" : ""}`}>
          <RunText text={message.text} streaming={message.streaming} />
          {long && (
            <div className="ws-reply-more">
              <button className="btn-text" onClick={() => setExpanded((v) => !v)}>
                {expanded ? "Show less" : "Show more"}
              </button>
              <button className="btn-text" onClick={() => setWorkspaceShown(false)}>
                Open in chat
              </button>
            </div>
          )}
        </div>
      )}
      {!message.streaming && message.stopReason && <StoppedNote reason={message.stopReason} />}
    </>
  );
}
