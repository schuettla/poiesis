import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import { inTauri, searchConversations } from "../../lib/api";
import type { Conversation } from "../../lib/types";
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

  return (
    <li
      className={active ? "active" : ""}
      title={c.title}
      tabIndex={0}
      onClick={() => setActive(c.id)}
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
  // Models, Engine, Apps, Self and Settings now live together in one hub
  // (behind the cog, below) — one badge covers whatever's waiting in any of them.
  const settingsPending = soulPending || selfPending || consolidationPending;
  const inSettingsHub = ["models", "engine", "apps", "self", "settings"].includes(view);

  const [query, setQuery] = useState("");
  const [resultIds, setResultIds] = useState<string[] | null>(null);

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
      <button className="new-chat" onClick={newConversation} title="New chat">
        {collapsed ? "+" : "+ New chat"}
      </button>

      <input
        className="rail-search"
        type="search"
        placeholder="Search chats…"
        aria-label="Search chats"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />

      <ul className="rail-nav rail-nav-top">
        <li
          className={view === "library" ? "active" : ""}
          tabIndex={0}
          onClick={() => setView("library")}
          onKeyDown={(e) => {
            if (e.key === "Enter") setView("library");
          }}
          title="Library"
        >
          <span className="nav-icon" aria-hidden="true"><LibraryIcon /></span>
          <span className="nav-label">Library</span>
        </li>
      </ul>

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
