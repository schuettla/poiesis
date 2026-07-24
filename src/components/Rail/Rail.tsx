import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import { inTauri, searchConversations } from "../../lib/api";
import type { Conversation, View } from "../../lib/types";
import "./Rail.css";

const DAY = 86400_000;

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

const NAV: { label: string; view: View; icon: string }[] = [
  { label: "Models", view: "models", icon: "▤" },
  { label: "Engine", view: "engine", icon: "◧" },
  { label: "Library", view: "library", icon: "□" },
  { label: "Apps", view: "apps", icon: "◇" },
  { label: "Settings", view: "settings", icon: "⚙" },
];

export default function Rail() {
  const conversations = useAppStore((s) => s.conversations);
  const activeId = useAppStore((s) => s.activeConversationId);
  const view = useAppStore((s) => s.view);
  const setActive = useAppStore((s) => s.setActiveConversation);
  const setView = useAppStore((s) => s.setView);
  const newConversation = useAppStore((s) => s.newConversation);
  const collapsed = useAppStore((s) => s.railCollapsed);
  const toggleRail = useAppStore((s) => s.toggleRail);

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
      <button
        className="rail-collapse"
        onClick={toggleRail}
        aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
      >
        {collapsed ? "»" : "«"}
      </button>

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

      {searching ? (
        <div>
          <p className="rail-label">{results.length ? "Results" : "No matches"}</p>
          <ul className="chat-list">
            {results.map((c) => (
              <li
                key={c.id}
                className={c.id === activeId && view === "chat" ? "active" : ""}
                title={c.title}
                tabIndex={0}
                onClick={() => setActive(c.id)}
                onKeyDown={(e) => e.key === "Enter" && setActive(c.id)}
              >
                {c.workspace && (
                  <span className="chat-workspace" aria-label="Workspace session" title="Workspace session">
                    ▦
                  </span>
                )}
                {c.title}
              </li>
            ))}
          </ul>
        </div>
      ) : (
        groups.map((g) => (
        <div key={g.label}>
          <p className="rail-label">{g.label}</p>
          <ul className="chat-list">
            {g.items.map((c) => (
              <li
                key={c.id}
                className={c.id === activeId && view === "chat" ? "active" : ""}
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
                {c.title}
              </li>
            ))}
          </ul>
        </div>
        ))
      )}

      <hr className="rail-divider" />

      <ul className="rail-nav">
        {NAV.map((n) => (
          <li
            key={n.view}
            className={view === n.view ? "active" : ""}
            tabIndex={0}
            onClick={() => setView(n.view)}
            onKeyDown={(e) => {
              if (e.key === "Enter") setView(n.view);
            }}
            title={n.label}
          >
            <span className="nav-icon" aria-hidden="true">
              {n.icon}
            </span>
            <span className="nav-label">{n.label}</span>
          </li>
        ))}
      </ul>
    </nav>
  );
}
