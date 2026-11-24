import { useEffect, useRef, useState } from "react";
import ApprovalCard from "../components/ApprovalCard";
import Art from "../components/Art";
import Composer from "../components/Composer";
import { CloseIcon } from "../components/Icons";
import Mark from "../components/Mark";
import MessageView, { Answer, ArtifactCard } from "../components/Message";
import TopBar from "../components/TopBar";
import VoiceBar from "../components/VoiceBar";
import { useStore } from "../store";
import { statusLine, type PhoneArtifact } from "../types";
import { openVoice } from "../voice/controller";
import { useVoiceStore } from "../voice/voiceStore";

/** One chat: the messages the desktop kept, the turn that is arriving, questions to answer, and at the bottom the
 * composer, or the voice bar while you talk. */
export default function Chat({ onBack }: { onBack: () => void }) {
  const { active, chats, messages, artifacts, live, approvals, link, desktop, send, stop, answer } = useStore();
  const talking = useVoiceStore((v) => v.shown);
  const [reading, setReading] = useState<PhoneArtifact | null>(null);
  const end = useRef<HTMLDivElement>(null);
  const chat = active ?? "";
  const list = messages[chat] ?? [];
  const turn = live[chat];
  const mine = approvals.filter((a) => a.conversationId === chat);
  const title = chats.find((c) => c.id === active)?.title || "New chat";

  useEffect(() => {
    end.current?.scrollIntoView?.({ block: "end", behavior: "smooth" });
  }, [list.length, turn?.text, turn?.notice, mine.length]);

  const out = link !== "connected";
  const working = !!turn?.running;
  return (
    <section className="screen chat">
      <TopBar
        title={title}
        line={out ? statusLine(link, desktop) : undefined}
        link={link}
        working={working}
        onBack={onBack}
      />
      <div className="scroll chat-scroll">
        {list.length === 0 && !turn && (
          <div className="empty-art">
            <Art name="penelope" />
            <h2>What should I work on?</h2>
            <p>Type it, or tap the mic and say it. The work happens on your desktop.</p>
          </div>
        )}
        {list.map((m) => (
          <MessageView key={m.id} message={m} />
        ))}
        {(artifacts[chat] ?? []).map((a) => (
          <ArtifactCard key={a.id} artifact={a} onOpen={() => setReading(a)} />
        ))}
        {turn && (
          <article className="message message-assistant live">
            {turn.text ? (
              <Answer text={turn.text} />
            ) : (
              turn.running &&
              !turn.error && (
                <p className="working-line">
                  <Mark state="working" size={18} />
                  {turn.helper ? "A helper is working on this" : "Working on it"}
                </p>
              )
            )}
            {turn.text && turn.helper && <p className="chat-note">A helper is working on this.</p>}
            {turn.notes.map((n) => (
              <p key={n} className="chat-note">
                {n}: open this on your desktop to see it.
              </p>
            ))}
            {turn.artifacts.map((a) => (
              <ArtifactCard key={a.id} artifact={a} onOpen={() => setReading({ ...a, created_at: 0 })} />
            ))}
            {turn.notice && <p className="chat-note">{turn.notice}</p>}
            {turn.error && <p className="chat-note chat-error">{turn.error}</p>}
          </article>
        )}
        {mine.map((card) => (
          <ApprovalCard key={card.id} card={card} onAnswer={(allow) => void answer(card.id, allow)} />
        ))}
        <div ref={end} className="scroll-end" />
      </div>
      {talking ? (
        <VoiceBar />
      ) : (
        <Composer
          disabled={out}
          reason={link === "connecting" ? "Looking for your desktop ..." : "Your desktop is asleep or Poiesis is not running."}
          busy={working}
          onSend={(text, photos) => void send(text, photos)}
          onStop={() => void stop()}
          onVoice={() => void openVoice()}
        />
      )}
      {reading && (
        <div className="sheet-backdrop" onClick={() => setReading(null)}>
          <div className="sheet sheet-tall" role="dialog" aria-label={reading.title} onClick={(e) => e.stopPropagation()}>
            <span className="sheet-grip" aria-hidden />
            <div className="sheet-head">
              <h2>{reading.title}</h2>
              <button className="bar-btn" aria-label="Close" onClick={() => setReading(null)}>
                <CloseIcon size={20} />
              </button>
            </div>
            <div className="sheet-text">
              {reading.kind === "code" ? <pre className="sheet-code">{reading.content}</pre> : <Answer text={reading.content} />}
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
