import { useEffect, useRef } from "react";
import { useActiveConversation, useAppStore } from "../lib/store";
import UserTurn from "../components/Conversation/UserTurn";
import AgentRun from "../components/Conversation/AgentRun";
import CompactDivider from "../components/Conversation/CompactDivider";
import Introduction from "../components/Conversation/Introduction";
import FolderInvite from "../components/Conversation/FolderInvite";
import Composer from "../components/Composer/Composer";
import MemoryToast from "../components/Memory/MemoryToast";
import RecallOffer from "../components/RecallRuntime/RecallOffer";
import SessionStrip from "../components/Blocks/SessionStrip";
import SessionMenu from "../components/Conversation/SessionMenu";
import Workspace from "./Workspace";
import "../components/Conversation/Conversation.css";
import "./Chat.css";
import type { Message } from "../lib/types";

/** One user turn plus every turn that follows it up to (not including) the
 * next user turn — a leading run of non-user messages before the first user
 * turn (rare, but possible) forms its own group. This is the unit `.turn-group`
 * wraps in the JSX below: it's what a sticky `.turn-user` needs to stay
 * pinned within so it holds for the whole exchange, not just its own height. */
function groupTurns(messages: Message[]): Message[][] {
  const groups: Message[][] = [];
  for (const m of messages) {
    if (m.role === "user" || groups.length === 0) groups.push([m]);
    else groups[groups.length - 1].push(m);
  }
  return groups;
}

export default function Chat() {
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const conversation = useActiveConversation();
  const sendMessage = useAppStore((s) => s.sendMessage);
  const stopGenerating = useAppStore((s) => s.stopGenerating);
  const busy = useAppStore((s) => s.busy);
  const scrollRef = useRef<HTMLDivElement>(null);

  const msgs = conversation?.messages ?? [];
  const lastMessage = msgs.length ? msgs[msgs.length - 1] : undefined;
  const messageIndex = new Map(msgs.map((m, i) => [m.id, i]));

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
                {groupTurns(conversation!.messages).map((group) => (
                  // `.turn-user` (the first, direct child here for any group
                  // that opens with one) is `position: sticky` — this wrapper
                  // is what it stays pinned *within*, so it holds for the
                  // whole exchange rather than unsticking the instant its own
                  // small box scrolls past.
                  <div className="turn-group" key={group[0].id}>
                    {group.map((m) => {
                      const i = messageIndex.get(m.id)!;
                      const turn =
                        m.role === "user" ? (
                          <UserTurn key={m.id} message={m} />
                        ) : (
                          <AgentRun
                            key={m.id}
                            message={m}
                            last={i === conversation!.messages.length - 1}
                          />
                        );
                      // The boundary sits *after* the last summarized turn, so
                      // the divider goes before the message that follows it.
                      const isFirstUnsummarized =
                        i > 0 &&
                        conversation!.messages[i - 1].id === conversation!.summaryUptoMessageId;
                      if (!isFirstUnsummarized || !conversation!.summary) return turn;
                      return (
                        <div key={`div-${m.id}`}>
                          <CompactDivider
                            summary={conversation!.summary}
                            conversationId={conversation!.id}
                          />
                          {turn}
                        </div>
                      );
                    })}
                  </div>
                ))}
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
