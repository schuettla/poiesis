import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";
import "./Memory.css";

/** A quiet relative date, as the rest of the Self page writes it. */
function ago(ts: number): string {
  const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** How many sources one card lists before it says "and N more". */
const SHOWN = 3;

/**
 * `BLF-UI-1`, F1 (source amnesia): where a belief came from, as a list. Each line
 * opens the conversation it was said in. A conversation that is gone still counts.
 */
export function Evidence({ ids }: { ids: string[] | undefined }) {
  const conversations = useAppStore((s) => s.conversations);
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);
  const setView = useAppStore((s) => s.setView);
  if (!ids || ids.length === 0) return null;
  // Newest source first: that is the one a person is most likely to remember.
  const sources = [...ids].reverse();
  return (
    <ul className="memory-evidence" aria-label="Where this came from">
      {sources.slice(0, SHOWN).map((id) => {
        const c = conversations.find((x) => x.id === id);
        if (!c) return <li key={id}>from a chat that is gone</li>;
        return (
          <li key={id}>
            <button
              className="btn-text"
              onClick={() => {
                setActiveConversation(id);
                setView("chat");
              }}
            >
              from {c.title || "a chat"}, {new Date(c.updatedAt).toLocaleDateString()}
            </button>
          </li>
        );
      })}
      {sources.length > SHOWN && <li>and {sources.length - SHOWN} more</li>}
    </ul>
  );
}

/** What I last did to what I know, in the first person (IV.5 item 3). */
export function RecentChanges({ collection, version }: { collection: "facts" | "lessons"; version: number }) {
  const [changes, setChanges] = useState<api.RecentChange[]>([]);
  useEffect(() => {
    if (!api.inTauri()) return;
    api
      .listRecentChanges(40)
      .then((all) => setChanges(all.filter((c) => c.collection === collection).slice(0, 6)))
      .catch(() => setChanges([]));
  }, [collection, version]);
  if (changes.length === 0) return null;
  return (
    <section className="memory-recent" aria-label="Recent changes">
      <h3 className="memory-section-title">Recent changes</h3>
      <ul>
        {changes.map((c, i) => (
          <li key={`${c.at}-${i}`}>
            {c.line} <span className="memory-created">{ago(c.at)}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * Contested facts: two notes that disagree, both shown with their sources, one button
 * per side and one for "both, in different situations" (IV.5 item 4).
 */
export function Unsure({ version, onChanged }: { version: number; onChanged: () => void }) {
  const [groups, setGroups] = useState<api.Fact[][]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!api.inTauri()) return;
    api.listUnsure().then(setGroups).catch(() => setGroups([]));
  }, [version]);

  async function settle(keep: string, both: boolean) {
    setError("");
    try {
      await api.resolveUnsure(keep, both);
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  }

  if (groups.length === 0) return null;
  return (
    <section className="memory-unsure" aria-label="Unsure">
      <h3 className="memory-section-title">Unsure</h3>
      <p className="memory-earlier-note">
        These notes may not both be true. I still use them, and I say I am not sure when I do.
      </p>
      {error && <p className="memory-error">{error}</p>}
      {groups.map((group) => (
        <div className="memory-card memory-unsure-group" key={group.map((f) => f.name).join("|")}>
          {group.map((f) => (
            <div className="memory-unsure-side" key={f.name}>
              <p className="memory-desc">{f.description}</p>
              <p className="memory-body">{f.body}</p>
              <Evidence ids={f.evidence} />
              <button className="btn-secondary" onClick={() => settle(f.name, false)}>
                {group.length > 1 ? "This one is right" : "Still true"}
              </button>
            </div>
          ))}
          {group.length > 1 && (
            <button className="btn-text" onClick={() => settle(group[0].name, true)}>
              Both, in different situations
            </button>
          )}
        </div>
      ))}
    </section>
  );
}

/**
 * `INT-UI-1`: what I said I would do later, each with when in words. Drop sets it
 * aside like any note, so it can be brought back under Let go.
 */
export function ThingsIWillDo({ version, onChanged }: { version: number; onChanged: () => void }) {
  const [items, setItems] = useState<api.IntentionRow[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!api.inTauri()) return;
    api.listIntentions().then(setItems).catch(() => setItems([]));
  }, [version]);

  async function drop(name: string) {
    setError("");
    try {
      await api.forgetMemoryFact(name);
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  }

  if (items.length === 0) return null;
  return (
    <section className="memory-todo" aria-label="Things I'll do">
      <h3 className="memory-section-title">Things I'll do</h3>
      {error && <p className="memory-error">{error}</p>}
      {items.map((i) => (
        <div className="memory-card" key={i.name}>
          <p className="memory-desc">Remind you: {i.task}</p>
          <p className="memory-last-used">{i.when}</p>
          <div className="memory-card-actions">
            <button className="btn-text danger" onClick={() => void drop(i.name)}>
              Drop
            </button>
          </div>
        </div>
      ))}
    </section>
  );
}

/** What was forgotten or pruned, still on disk and still recoverable (G9). */
export function LetGo({
  collection,
  version,
  onChanged,
}: {
  collection: "facts" | "lessons";
  version: number;
  onChanged: () => void;
}) {
  const [items, setItems] = useState<api.LetGo[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    if (!api.inTauri()) return;
    api
      .listLetGo()
      .then((all) => setItems(all.filter((i) => i.collection === collection)))
      .catch(() => setItems([]));
  }, [collection, version]);

  async function restore(file: string) {
    setError("");
    try {
      await api.restoreMemoryFact(file);
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  }

  if (items.length === 0) return null;
  const needle = query.trim().toLowerCase();
  const shown = needle
    ? items.filter((i) => i.name.toLowerCase().includes(needle) || i.description.toLowerCase().includes(needle))
    : items;
  return (
    <details className="memory-earlier memory-letgo">
      <summary>Let go ({items.length})</summary>
      <p className="memory-earlier-note">
        I no longer use these. They stay on this device until you remove them, and you can bring one back.
      </p>
      <input
        className="field-input memory-search"
        type="search"
        placeholder="Search what I let go…"
        aria-label="Search what I let go"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {error && <p className="memory-error">{error}</p>}
      {shown.map((i) => (
        <div className="memory-card memory-card-earlier" key={i.file}>
          <div className="memory-card-head">
            <span className="memory-name">{i.name}</span>
            <span className="memory-created">set aside {ago(i.at)}</span>
          </div>
          <p className="memory-desc">{i.description}</p>
          <div className="memory-card-actions">
            <button className="btn-text" onClick={() => restore(i.file)}>
              Bring it back
            </button>
          </div>
        </div>
      ))}
    </details>
  );
}
