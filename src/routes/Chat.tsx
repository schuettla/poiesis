import { Fragment, useEffect, useRef } from "react";
import { useActiveConversation, useAppStore } from "../lib/store";
import UserTurn from "../components/Conversation/UserTurn";
import AgentRun from "../components/Conversation/AgentRun";
import CompactDivider from "../components/Conversation/CompactDivider";
import CommandNote from "../components/Conversation/CommandNote";
import RewindDialog from "../components/Conversation/RewindDialog";
import Introduction from "../components/Conversation/Introduction";
import FolderInvite from "../components/Conversation/FolderInvite";
import Orb from "../components/Orb/Orb";
import { orbForPresence } from "../components/Orb/orbState";
import Composer from "../components/Composer/Composer";
import VoiceMode from "../components/Voice/VoiceMode";
import { useVoiceStore } from "../lib/voice/voiceStore";
import MemoryToast from "../components/Memory/MemoryToast";
import RecallOffer from "../components/RecallRuntime/RecallOffer";
import SessionStrip from "../components/Blocks/SessionStrip";
import SessionMenu from "../components/Conversation/SessionMenu";
import Workspace from "./Workspace";
import "../components/Conversation/Conversation.css";
import "./Chat.css";
import type { CommandNoteView, Message } from "../lib/types";

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

/** Where each traced command sits: just before the first message made after it,
 * or at the end. A skill command (`messageId` set) is the chip on its own bubble
 * and has no line of its own. */
function placeNotes(notes: CommandNoteView[], messages: Message[]) {
  const before = new Map<string, CommandNoteView[]>();
  const end: CommandNoteView[] = [];
  for (const n of notes) {
    if (n.messageId) continue;
    const next = messages.find((m) => m.createdAt !== undefined && m.createdAt > n.at);
    if (!next) end.push(n);
    else before.set(next.id, [...(before.get(next.id) ?? []), n]);
  }
  return { before, end };
}

export default function Chat() {
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const conversation = useActiveConversation();
  const sendMessage = useAppStore((s) => s.sendMessage);
  const stopGenerating = useAppStore((s) => s.stopGenerating);
  const busy = useAppStore((s) => s.busy);
  const presence = useAppStore((s) => s.presence);
  const checkingMyself = useAppStore((s) => s.checkupRunning);
  const voiceShown = useVoiceStore((s) => s.shown);
  const scrollRef = useRef<HTMLDivElement>(null);

  const msgs = conversation?.messages ?? [];
  const lastMessage = msgs.length ? msgs[msgs.length - 1] : undefined;
  const messageIndex = new Map(msgs.map((m, i) => [m.id, i]));

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [conversation?.messages.length, lastMessage?.text]);

  const isEmpty = !conversation || conversation.messages.length === 0;
  const allNotes = useAppStore((s) => (conversation ? s.commandNotes[conversation.id] : undefined));
  const notes = placeNotes(allNotes ?? [], msgs);

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
                {/* The same presence the mark in the top bar carries, at
                    avatar size: breathing at rest, and the kind of work when
                    reflection or recovery is running in the background. */}
                <Orb state={orbForPresence(presence)} size={64} />
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
                      const lines = (notes.before.get(m.id) ?? []).map((n) => (
                        <CommandNote key={n.id} note={n} />
                      ));
                      if (!isFirstUnsummarized || !conversation!.summary) {
                        // A fragment, not a wrapper: a user turn is sticky within
                        // its group, and a div around it would become the box it
                        // sticks within.
                        return lines.length ? (
                          <Fragment key={`notes-${m.id}`}>
                            {lines}
                            {turn}
                          </Fragment>
                        ) : (
                          turn
                        );
                      }
                      return (
                        <div key={`div-${m.id}`}>
                          {lines}
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
                {notes.end.map((n) => (
                  <CommandNote key={n.id} note={n} />
                ))}
                {/* `CHK-UI-2`: said up front, because the last check is a real
                    model call and would otherwise look like nothing happened. */}
                {checkingMyself && (
                  <div className="command-note" role="status">
                    <span aria-hidden="true">◆ </span>I'm checking myself. The last check asks my model something, so
                    it takes a moment.
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
        {voiceShown && <VoiceMode />}
      </div>
      {/* The voice surface has its own bar, so the box is not shown under it. */}
      {!voiceShown && <Composer onSend={sendMessage} busy={busy} onStop={stopGenerating} />}
      <RewindDialog />
      <MemoryToast />
      <RecallOffer />
    </>
  );
}
