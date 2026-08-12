import { useEffect, useRef } from "react";
import { useActiveConversation, useAppStore } from "../lib/store";
import UserTurn from "../components/Conversation/UserTurn";
import AgentRun from "../components/Conversation/AgentRun";
import CompactDivider from "../components/Conversation/CompactDivider";
import Introduction from "../components/Conversation/Introduction";
import FolderInvite from "../components/Conversation/FolderInvite";
import Composer from "../components/Composer/Composer";
import MemoryToast from "../components/Memory/MemoryToast";
import RecallOffer from "../components/EmbedEngine/RecallOffer";
import SessionStrip from "../components/Blocks/SessionStrip";
import SessionMenu from "../components/Conversation/SessionMenu";
import Workspace from "./Workspace";
import "../components/Conversation/Conversation.css";
import "./Chat.css";

export default function Chat() {
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const conversation = useActiveConversation();
  const sendMessage = useAppStore((s) => s.sendMessage);
  const stopGenerating = useAppStore((s) => s.stopGenerating);
  const busy = useAppStore((s) => s.busy);
  const scrollRef = useRef<HTMLDivElement>(null);

  const msgs = conversation?.messages ?? [];
  const lastMessage = msgs.length ? msgs[msgs.length - 1] : undefined;

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [conversation?.messages.length, lastMessage?.text]);

  const isEmpty = !conversation || conversation.messages.length === 0;

  // Workspace mode: same session, inverted layout — the composed interface is
  // the interaction point, the message stream demotes to an optional log.
  if (workspaceMode) {
    return (
      <>
        <Workspace />
        <MemoryToast />
        <RecallOffer />
      </>
    );
  }

  return (
    <>
      <div className="chat-body">
        <SessionMenu />
        <div className="main" ref={scrollRef}>
          <div className="conversation">
            {!isEmpty && <SessionStrip />}
            {isEmpty ? (
              <div className="empty-state">
                <p className="empty-line">No messages yet — say hello to get started.</p>
                <Introduction />
                <FolderInvite />
              </div>
            ) : (
              <div className="message-stream" data-selectable="true">
                {conversation!.messages.map((m, i) => {
                  const turn =
                    m.role === "user" ? (
                      <UserTurn key={m.id} message={m} />
                    ) : (
                      <AgentRun key={m.id} message={m} />
                    );
                  // The boundary sits *after* the last summarized turn, so the
                  // divider goes before the message that follows it.
                  const isFirstUnsummarized =
                    i > 0 && conversation!.messages[i - 1].id === conversation!.summaryUptoMessageId;
                  if (!isFirstUnsummarized || !conversation!.summary) return turn;
                  return (
                    <div key={`div-${m.id}`}>
                      <CompactDivider summary={conversation!.summary} />
                      {turn}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>
      <Composer onSend={sendMessage} busy={busy} onStop={stopGenerating} />
      <MemoryToast />
      <RecallOffer />
    </>
  );
}
