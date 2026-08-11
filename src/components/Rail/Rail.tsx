import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import { inTauri, searchConversations } from "../../lib/api";
import type { Conversation } from "../../lib/types";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import "./Rail.css";

const DAY = 86400_000;

function LibraryIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden="true">
      <path
        d="M5.5 3.5h9a1 1 0 0 1 1 1V17l-5.5-3.2L4.5 17V4.5a1 1 0 0 1 1-1z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function MessageIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden="true">
      <path
        d="M3.5 5.5a1 1 0 0 1 1-1h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H8.5l-3.6 2.8a.5.5 0 0 1-.8-.4V13.5h-.6a1 1 0 0 1-1-1v-7z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden="true">
      <circle cx="10" cy="10" r="2.6" stroke="currentColor" strokeWidth="1.3" />
      <path
        d="M10 2.8v2.3M10 14.9v2.3M17.2 10h-2.3M5.1 10H2.8M15.1 4.9l-1.6 1.6M6.5 13.5l-1.6 1.6M15.1 15.1l-1.6-1.6M6.5 6.5 4.9 4.9"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinecap="round"
      />
    </svg>
  );
}

function groupConversations(convs: Conversation[]): { label: string; items: Conversation[] }[] {
  const now = Date.now();
  const today: Conversation[] = [];
  const earlier: Conversation[] = [];
  for (const c of convs) {
    if (now - c.updatedAt < DAY) today.push(c);
    else earlier.push(c);
  }
  const groups: { label: string; items: Conversation[] }[] = [];
  if (today.length) groups.push({ label: "Today", items: today });
  if (earlier.length) groups.push({ label: "Earlier", items: earlier });
  return groups;
}


/** One conversation row, with its digestion state (PRES-2) and the manual
 * "reflect now" affordance (REF-UI-2). The `◆` is one element in three states:
 * an offer, a pulse while I'm reading the conversation back, and a quiet mark
 * once it taught me something. */
function ChatRow({ c, active }: { c: Conversation; active: boolean }) {
  const setActive = useAppStore((s) => s.setActiveConversation);
  const reflect = useAppStore((s) => s.reflectConversation);
  const reflecting = useAppStore((s) => s.reflectingIds.includes(c.id));
  const digested = useAppStore((s) => s.digestedIds.includes(c.id));
  const deleteConversation = useAppStore((s) => s.deleteConversation);
  // A chat is a real thing the user made, so removing it takes two steps: a
  // menu (right-click, or the ⋯ that appears on hover) and then a confirmation.
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);

  return (
    <li
      className={active ? "active" : ""}
      title={c.title}
      tabIndex={0}
      onClick={() => setActive(c.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenuOpen(true);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") setActive(c.id);
      }}
    >
      {c.workspace && (
        <span className="chat-workspace" aria-label="Workspace session" title="Workspace session">
          ▦
        </span>
      )}
      <span className="chat-title">{c.title}</span>
      {reflecting ? (
        <span className="chat-digest reflecting" role="status" aria-label="I'm reflecting on this conversation">
          ◆
        </span>
      ) : digested ? (
        <span
          className="chat-digest learned"
          role="img"
          aria-label="I learned something from this conversation"
          title="I learned something from this conversation"
        >
          ◆
        </span>
      ) : (
        <button
          className="chat-digest offer"
          title="Reflect on this conversation"
          aria-label={`Reflect on ${c.title}`}
          onClick={(e) => {
            e.stopPropagation();
            reflect(c.id);
          }}
        >
          ◆
        </button>
      )}

      <div className="chat-menu-wrap">
        <button
          className="chat-more"
          aria-label={`More actions for ${c.title}`}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          title="More"
          onClick={(e) => {
            e.stopPropagation();
            setMenuOpen((v) => !v);
          }}
        >
          ⋯
        </button>
        {menuOpen && (
          <>
            <div
              className="row-menu-backdrop"
              onClick={(e) => {
                e.stopPropagation();
                setMenuOpen(false);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                setMenuOpen(false);
              }}
            />
            <div className="row-menu" role="menu">
              <button
                className="row-menu-item danger"
                role="menuitem"
                onClick={(e) => {
                  e.stopPropagation();
                  setMenuOpen(false);
                  setConfirming(true);
                }}
              >
                Delete chat
              </button>
            </div>
          </>
        )}
      </div>

      {confirming && (
        // The dialog renders inside the row, so its clicks must not fall
        // through to the row's "select this chat" handler.
        <span onClick={(e) => e.stopPropagation()}>
        <ConfirmDialog
          title="Delete this chat?"
          body={`“${c.title}” and everything said in it will be removed. This can't be undone.`}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            deleteConversation(c.id);
          }}
        />
        </span>
      )}
    </li>
  );
}

export default function Rail() {
  const conversations = useAppStore((s) => s.conversations);
  const activeId = useAppStore((s) => s.activeConversationId);
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const newConversation = useAppStore((s) => s.newConversation);
  const collapsed = useAppStore((s) => s.railCollapsed);
  // Something about the agent's self is waiting on an answer (SOUL-UI-3). The
  // dot goes where the answer is given: soul edits are reviewed in Settings →
  // Personas, everything else in the Self panel.
  const soulPending = useAppStore((s) =>
    s.changeProposals.some((p) => p.target === "soul")
  );
  const selfPending = useAppStore((s) =>
    s.changeProposals.some((p) => p.target !== "soul")
  );
  const consolidationPending = useAppStore((s) => s.consolidationPending);
  // SCH-UI-4: a scheduled job runs unattended, but the user must always be
  // able to see that it's happening, and end it.
  const runningJob = useAppStore((s) => s.runningJob);
  const stopScheduledJob = useAppStore((s) => s.stopScheduledJob);
  // Models, Engine, Apps, Self and Settings now live together in one hub
  // (behind the cog, below) — one badge covers whatever's waiting in any of them.
  const settingsPending = soulPending || selfPending || consolidationPending;
  const inSettingsHub = [
    "models",
    "engine",
    "apps",
    "skills",
    "self",
    "tasks",
    "activity",
    "settings",
  ].includes(view);

  const setActive = useAppStore((s) => s.setActiveConversation);
  const [query, setQuery] = useState("");
  const [resultIds, setResultIds] = useState<string[] | null>(null);
  const [sessionMenuOpen, setSessionMenuOpen] = useState(false);

  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setResultIds(null);
      return;
    }
    let active = true;
    const run = setTimeout(() => {
      if (inTauri()) {
        searchConversations(q)
          .then((rows) => active && setResultIds(rows.map((r) => r.id)))
          .catch(() => active && setResultIds([]));
      } else {
        const lower = q.toLowerCase();
        setResultIds(
          conversations.filter((c) => c.title.toLowerCase().includes(lower)).map((c) => c.id)
        );
      }
    }, 180);
    return () => {
      active = false;
      clearTimeout(run);
    };
  }, [query, conversations]);

  const searching = resultIds !== null;
  const results = searching
    ? conversations.filter((c) => resultIds!.includes(c.id))
    : [];
  const groups = groupConversations(conversations);

  return (
    <nav className={`rail ${collapsed ? "collapsed" : ""}`} aria-label="Conversations and sections">
      <div className="rail-top-actions">
        <button
          className={`rail-top-btn library-btn ${view === "library" ? "active" : ""}`}
          onClick={() => setView("library")}
          title="Library"
        >
          <span className="nav-icon" aria-hidden="true"><LibraryIcon /></span>
          <span className="nav-label">Library</span>
        </button>
        <button className="rail-top-btn new-chat" onClick={newConversation} title="New chat">
          <span className="nav-icon" aria-hidden="true">+</span>
          <span className="nav-label">New chat</span>
        </button>
      </div>

      {collapsed && (
        <div className="rail-session-collapsed">
          <button
            className="rail-session-btn"
            aria-haspopup="menu"
            aria-expanded={sessionMenuOpen}
            title="Chats"
            onClick={() => setSessionMenuOpen((v) => !v)}
          >
            <MessageIcon />
          </button>
          {sessionMenuOpen && (
            <>
              <div
                className="row-menu-backdrop"
                onClick={() => setSessionMenuOpen(false)}
              />
              <div className="row-menu rail-session-menu" role="menu">
                {conversations.length === 0 && (
                  <span className="row-menu-empty">No chats yet</span>
                )}
                {conversations.map((c) => (
                  <button
                    key={c.id}
                    className={`row-menu-item ${
                      c.id === activeId && view === "chat" ? "active" : ""
                    }`}
                    role="menuitem"
                    title={c.title}
                    onClick={() => {
                      setActive(c.id);
                      setSessionMenuOpen(false);
                    }}
                  >
                    <span className="row-menu-label">{c.title}</span>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      )}

      <input
        className="rail-search"
        type="search"
        placeholder="Search chats…"
        aria-label="Search chats"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />

      {runningJob && (
        <div className="rail-job-row" role="status">
          <span className="rail-job-dot" aria-hidden="true" />
          <span className="rail-job-label">Running “{runningJob.job_name}”…</span>
          <button
            className="rail-job-stop"
            aria-label={`Stop ${runningJob.job_name}`}
            title="Stop"
            onClick={() => stopScheduledJob()}
          >
            ■
          </button>
        </div>
      )}

      {searching ? (
        <div>
          <p className="rail-label">{results.length ? "Results" : "No matches"}</p>
          <ul className="chat-list">
            {results.map((c) => (
              <ChatRow key={c.id} c={c} active={c.id === activeId && view === "chat"} />
            ))}
          </ul>
        </div>
      ) : (
        groups.map((g) => (
        <div key={g.label}>
          <p className="rail-label">{g.label}</p>
          <ul className="chat-list">
            {g.items.map((c) => (
              <ChatRow key={c.id} c={c} active={c.id === activeId && view === "chat"} />
            ))}
          </ul>
        </div>
        ))
      )}

      <hr className="rail-divider" />

      <ul className="rail-nav rail-nav-footer">
        <li
          className={inSettingsHub ? "active" : ""}
          tabIndex={0}
          onClick={() => setView("settings")}
          onKeyDown={(e) => {
            if (e.key === "Enter") setView("settings");
          }}
          title="Settings"
        >
          <span className="nav-icon" aria-hidden="true"><SettingsIcon /></span>
          <span className="nav-label">Settings</span>
          {settingsPending && (
            <span
              className="nav-badge"
              role="img"
              aria-label="Changes waiting for review"
              title="Changes waiting for review"
            />
          )}
        </li>
      </ul>
    </nav>
  );
}
