import { useState } from "react";
import ApprovalCard from "../components/ApprovalCard";
import { ComposeIcon, MoreIcon } from "../components/Icons";
import Art from "../components/Art";
import TopBar from "../components/TopBar";
import { useStore } from "../store";
import { statusLine, type ChatRow, type Waiting } from "../types";

/** When a chat last moved, in words. */
export function when(ms: number, now = Date.now()): string {
  const min = Math.floor((now - ms) / 60000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.floor(h / 24);
  return d === 1 ? "yesterday" : `${d} days ago`;
}

function waitingText(w: Waiting): string {
  if (typeof w.summary === "string") return w.summary;
  if (typeof w.title === "string") return w.title;
  return "Something needs your answer.";
}

function ChatList({ label, rows, onOpen }: { label: string; rows: ChatRow[]; onOpen: (id: string) => void }) {
  if (rows.length === 0) return null;
  return (
    <section className="list-group" aria-label={label}>
      <h2 className="group-label">{label}</h2>
      <ul className="chat-list">
        {rows.map((c) => (
          <li key={c.id}>
            <button className="chat-row" onClick={() => onOpen(c.id)}>
              <span className="chat-row-title">{c.title || "New chat"}</span>
              <span className="chat-row-meta">
                {c.remote_sender ? `${c.remote_sender} · ` : ""}
                {when(c.updated_at)}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Home: what waits for an answer, then the chats, the ones started from a phone first. */
export default function Chats() {
  const { chats, waiting, approvals, live, link, desktop, openChat, newChat, unpair, resolveWaiting, answer } = useStore();
  const [sheet, setSheet] = useState<"closed" | "desktop" | "forget">("closed");
  const working = Object.values(live).some((l) => l.running);
  // A question with no chat to open is answered here.
  const loose = approvals.filter((a) => !a.conversationId);
  const fromPhone = chats.filter((c) => c.remote_sender);
  const onDesktop = chats.filter((c) => !c.remote_sender);

  return (
    <section className="screen chats">
      <TopBar
        title="Poiesis"
        serif
        line={statusLine(link, desktop)}
        link={link}
        working={working}
        action={
          <button className="bar-btn" aria-label="Your desktop" onClick={() => setSheet("desktop")}>
            <MoreIcon size={22} />
          </button>
        }
      />
      <div className="scroll home">
        {loose.map((card) => (
          <ApprovalCard key={card.id} card={card} onAnswer={(allow) => void answer(card.id, allow)} />
        ))}
        {waiting.length > 0 && (
          <section className="list-group" aria-label="Waiting for you">
            <h2 className="group-label">Waiting for you</h2>
            {waiting.map((w) => (
              <div key={w.id} className="card waiting-card">
                <p>{waitingText(w)}</p>
                <div className="card-actions">
                  <button className="btn btn-primary" onClick={() => void resolveWaiting(w.id, true)}>
                    Yes
                  </button>
                  <button className="btn btn-secondary" onClick={() => void resolveWaiting(w.id, false)}>
                    Not now
                  </button>
                </div>
              </div>
            ))}
          </section>
        )}
        {chats.length === 0 ? (
          <div className="empty-art">
            <Art name="athena" />
            <h2>Nothing here yet</h2>
            <p>Start a chat and Poiesis does the work on your desktop, even while you are away from it.</p>
          </div>
        ) : (
          <>
            <ChatList label="From your phone" rows={fromPhone} onOpen={(id) => void openChat(id)} />
            <ChatList label="On your desktop" rows={onDesktop} onOpen={(id) => void openChat(id)} />
          </>
        )}
      </div>
      <button className="fab" onClick={newChat}>
        <ComposeIcon size={20} />
        New chat
      </button>
      {sheet !== "closed" && (
        <div className="sheet-backdrop" onClick={() => setSheet("closed")}>
          <div className="sheet sheet-fixed" role="dialog" aria-label="Your desktop" onClick={(e) => e.stopPropagation()}>
            <span className="sheet-grip" aria-hidden />
            {sheet === "desktop" ? (
              <>
                <h2>{desktop ?? "Your desktop"}</h2>
                <p>{statusLine(link, desktop)}</p>
                <p>Everything runs on your desktop. This phone only shows it and sends what you ask.</p>
                <div className="sheet-actions">
                  <button className="btn btn-danger" onClick={() => setSheet("forget")}>
                    Forget this desktop
                  </button>
                  <button className="btn btn-secondary" onClick={() => setSheet("closed")}>
                    Close
                  </button>
                </div>
              </>
            ) : (
              <>
                <h2>Forget {desktop ?? "this desktop"}?</h2>
                <p>To reach it again, pair this phone with a new code from the desktop.</p>
                <div className="sheet-actions">
                  <button className="btn btn-danger" onClick={() => void unpair()}>
                    Forget
                  </button>
                  <button className="btn btn-secondary" onClick={() => setSheet("desktop")}>
                    Cancel
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
