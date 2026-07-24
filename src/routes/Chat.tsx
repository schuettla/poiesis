import { useEffect, useRef } from "react";
import { useActiveConversation, useAppStore } from "../lib/store";
import UserTurn from "../components/Conversation/UserTurn";
import AgentRun from "../components/Conversation/AgentRun";
import Composer from "../components/Composer/Composer";
import CanvasPanel from "../components/Canvas/CanvasPanel";
import SessionStrip from "../components/Blocks/SessionStrip";
import Workspace from "./Workspace";
import "../components/Conversation/Conversation.css";
import "./Chat.css";

export default function Chat() {
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const conversation = useActiveConversation();
  const sendMessage = useAppStore((s) => s.sendMessage);
  const stopGenerating = useAppStore((s) => s.stopGenerating);
  const busy = useAppStore((s) => s.busy);
  const canvasOpen = useAppStore((s) => s.canvasOpen);
  const openCanvas = useAppStore((s) => s.openCanvas);
  const artifactCount = useAppStore((s) =>
    s.activeConversationId ? (s.artifacts[s.activeConversationId]?.length ?? 0) : 0
  );
  const scrollRef = useRef<HTMLDivElement>(null);

  const msgs = conversation?.messages ?? [];
  const lastMessage = msgs.length ? msgs[msgs.length - 1] : undefined;

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [conversation?.messages.length, lastMessage?.text]);

  const isEmpty = !conversation || conversation.messages.length === 0;

  // Workspace mode: same session, inverted layout — the composed interface is
  // the interaction point, the message stream demotes to an optional log.
  if (workspaceMode) return <Workspace />;

  return (
    <>
      <div className="chat-body">
        <div className="main" ref={scrollRef}>
          <div className="conversation">
            {!isEmpty && <SessionStrip />}
            {isEmpty ? (
              <div className="empty-state">
                <p className="empty-line">No messages yet — say hello to get started.</p>
              </div>
            ) : (
              conversation!.messages.map((m) =>
                m.role === "user" ? (
                  <UserTurn key={m.id} message={m} />
                ) : (
                  <AgentRun key={m.id} message={m} />
                )
              )
            )}
          </div>
          {!canvasOpen && artifactCount > 0 && (
            <button className="canvas-reopen" onClick={() => openCanvas()}>
              Canvas · {artifactCount}
            </button>
          )}
        </div>
        <CanvasPanel />
      </div>
      <Composer onSend={sendMessage} busy={busy} onStop={stopGenerating} />
    </>
  );
}
