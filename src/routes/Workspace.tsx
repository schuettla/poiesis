import ContextMeter from "../components/Composer/ContextMeter";
import { useEffect, useRef, useState } from "react";
import { useActiveConversation, useAppStore } from "../lib/store";
import type { BlockView, Message, UINode } from "../lib/types";
import UserTurn from "../components/Conversation/UserTurn";
import RunText from "../components/Conversation/RunText";
import Composer from "../components/Composer/Composer";
import SessionStrip from "../components/Blocks/SessionStrip";
import BlockRenderer from "../components/Blocks/BlockRenderer";
import SurfaceRenderer from "../components/Surface/SurfaceRenderer";
import "../components/Conversation/Conversation.css";
import "./Workspace.css";

/** The Workspace view — decoupled from the chat stream. The composed interface
 * (render_ui) IS the interaction point: the user works the UI and types into
 * one input; the agent's latest reply appears as a single status line. The
 * message stream exists only as an optional, read-only log drawer. */
export default function Workspace() {
  const conversation = useActiveConversation();
  const sendMessage = useAppStore((s) => s.sendMessage);
  const stopGenerating = useAppStore((s) => s.stopGenerating);
  const busy = useAppStore((s) => s.busy);
  const surface = useAppStore((s) =>
    s.activeConversationId ? s.surfaces[s.activeConversationId] : undefined
  );
  const setSurfaceState = useAppStore((s) => s.setSurfaceState);
  const sendSurfaceAction = useAppStore((s) => s.sendSurfaceAction);
  const pendingCount = useAppStore((s) =>
    s.activeConversationId ? (s.pendingActions[s.activeConversationId]?.length ?? 0) : 0
  );
  const [showLog, setShowLog] = useState(false);
  const turnsRef = useRef<HTMLDivElement>(null);

  const msgs = conversation?.messages ?? [];
  const lastAssistant = [...msgs].reverse().find((m) => m.role === "assistant");

  useEffect(() => {
    if (!showLog) return;
    turnsRef.current?.scrollTo({ top: turnsRef.current.scrollHeight, behavior: "smooth" });
  }, [showLog, msgs.length, lastAssistant?.text]);

  // Legacy fallback only: pre-surface conversations still show their blocks.
  const blocks: BlockView[] = msgs.flatMap((m) => m.blocks ?? []);

  return (
    <div className="workspace-body">
      <main className="ws-main" aria-label="Workspace">
        <header className="ws-head">
          <span className="ws-badge" title="This session is pinned to Workspace mode">
            ▦ Workspace
          </span>
          {conversation?.title && <span className="ws-title">{conversation.title}</span>}
          {/* RCP-UI-3: this workspace came from a procedure we kept. */}
          {conversation?.recipeName && (
            <span className="ws-recipe" title={`Started from the recipe ${conversation.recipeName}`}>
              · from recipe {conversation.recipeName}
            </span>
          )}
          <SessionStrip />
          <ContextMeter />
          <button
            className="ws-log-toggle"
            onClick={() => setShowLog((v) => !v)}
            aria-pressed={showLog}
            title="Show the conversation log (read-only)"
          >
            {showLog ? "hide log" : "log"}
          </button>
        </header>

        <div className="ws-canvas">
          {surface ? (
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
          ) : blocks.length ? (
            blocks.map((b) => (
              <div key={b.id} id={`ws-block-${b.id}`} className="ws-block">
                <BlockRenderer block={b} />
              </div>
            ))
          ) : (
            <div className="ws-empty">
              <p className="empty-line">Nothing composed yet.</p>
              <p className="ws-empty-hint">
                The agent builds the interface for your task here — ask for anything and it will
                compose a live UI (a board, a picker, a dashboard) instead of a wall of chat.
              </p>
            </div>
          )}
        </div>

        <footer className="ws-foot">
          {pendingCount > 0 && (
            <p className="ws-pending" aria-live="polite">
              {pendingCount} change{pendingCount > 1 ? "s" : ""} will be shared with your next
              message
            </p>
          )}
          <Composer onSend={sendMessage} busy={busy} onStop={stopGenerating} />
        </footer>
      </main>

      {showLog && (
        <aside className="ws-log" aria-label="Conversation log (read-only)">
          <div className="ws-turns" ref={turnsRef}>
            {msgs.length === 0 && <p className="ws-empty-line">No activity yet.</p>}
            {msgs.map((m) =>
              m.role === "user" ? (
                <UserTurn key={m.id} message={m} />
              ) : (
                <WsLogTurn key={m.id} message={m} />
              )
            )}
          </div>
        </aside>
      )}
    </div>
  );
}

/** A log entry for an assistant turn: model tag and prose only — pure record,
 * no interaction. The interface itself lives on the canvas. */
function WsLogTurn({ message }: { message: Message }) {
  const model = message.model;
  return (
    <div className="agent-run ws-run">
      {model && (
        <div className="run-header">
          <span className={`provenance-dot ${model.provenance}`} aria-hidden="true" />
          <span className="model-tag">{model.name}</span>
        </div>
      )}
      {message.text && <RunText text={message.text} streaming={message.streaming} />}
      {!message.text && message.streaming && <RunText text="" streaming />}
    </div>
  );
}
