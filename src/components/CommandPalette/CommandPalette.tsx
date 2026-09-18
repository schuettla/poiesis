import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useAppStore } from "../../lib/store";
import { inTauri, searchMessages, type MessageHit } from "../../lib/api";
import { HUB_SECTIONS } from "../../lib/types";
import { shortTime } from "../../lib/time";
import {
  BookmarkIcon,
  FolderIcon,
  MessageIcon,
  PlusIcon,
  SearchIcon,
  SettingsIcon,
  SparkleIcon,
} from "../Icons/Icons";
import "./CommandPalette.css";

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
export const PALETTE_SHORTCUT = IS_MAC ? "⌘K" : "Ctrl K";

interface Item {
  key: string;
  icon: ReactNode;
  label: string;
  meta?: string;
  time?: string;
  /** Raw FTS excerpt, match fenced by ``…``. */
  snippet?: string;
  run: () => void;
}

interface Group {
  label: string;
  items: Item[];
}

/** Rendered as text pieces, never as markup: message content is untrusted. */
export function splitSnippet(raw: string): { text: string; hit: boolean }[] {
  const pieces: { text: string; hit: boolean }[] = [];
  let hit = false;
  let buf = "";
  const flush = () => {
    if (buf) pieces.push({ text: buf, hit });
    buf = "";
  };
  for (const ch of raw.replace(/\s+/g, " ")) {
    if (ch === "") {
      flush();
      hit = true;
    } else if (ch === "") {
      flush();
      hit = false;
    } else {
      buf += ch;
    }
  }
  flush();
  return pieces;
}

/** Every word must appear. Lower is better: a word at the start of the text
 * beats one at a word boundary, which beats one mid-word. */
function score(text: string, words: string[]): number | null {
  const t = text.toLowerCase();
  let total = 0;
  for (const w of words) {
    const i = t.indexOf(w);
    if (i < 0) return null;
    total += i === 0 ? 0 : /[\s\-_/\\.:]/.test(t[i - 1]) ? 1 : 2;
  }
  return total;
}

function ranked<T>(items: T[], text: (item: T) => string, words: string[], limit: number): T[] {
  return items
    .map((item, order) => ({ item, order, s: score(text(item), words) }))
    .filter((r): r is { item: T; order: number; s: number } => r.s !== null)
    .sort((a, b) => a.s - b.s || a.order - b.order)
    .slice(0, limit)
    .map((r) => r.item);
}

export default function CommandPalette() {
  const open = useAppStore((s) => s.paletteOpen);
  const setOpen = useAppStore((s) => s.setPaletteOpen);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return;
      if (e.key !== "k" && e.key !== "K") return;
      e.preventDefault();
      const s = useAppStore.getState();
      s.setPaletteOpen(!s.paletteOpen);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!open) return null;
  return <Palette onClose={() => setOpen(false)} />;
}

function Palette({ onClose }: { onClose: () => void }) {
  const conversations = useAppStore((s) => s.conversations);
  const projects = useAppStore((s) => s.projects);
  const artifacts = useAppStore((s) => s.allArtifacts);
  const refreshAllArtifacts = useAppStore((s) => s.refreshAllArtifacts);
  const openSession = useAppStore((s) => s.openSession);
  const openProjectView = useAppStore((s) => s.openProjectView);
  const viewArtifact = useAppStore((s) => s.viewArtifact);
  const newConversation = useAppStore((s) => s.newConversation);
  const newProject = useAppStore((s) => s.newProject);
  const setView = useAppStore((s) => s.setView);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<MessageHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // Hand focus back to wherever it was — the composer, usually.
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    inputRef.current?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);

  useEffect(() => {
    refreshAllArtifacts();
  }, [refreshAllArtifacts]);

  // Message text is searched in the database; titles, projects and the
  // library are matched here, instantly, so the list never waits on it.
  useEffect(() => {
    const q = query.trim();
    if (!inTauri() || q.length < 2) {
      setHits([]);
      setSearching(false);
      return;
    }
    let live = true;
    setSearching(true);
    const timer = setTimeout(() => {
      searchMessages(q)
        .then((h) => live && setHits(h))
        .catch(() => live && setHits([]))
        .finally(() => live && setSearching(false));
    }, 150);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query]);

  const groups = useMemo<Group[]>(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const projectName = new Map(projects.map((p) => [p.id, p.name]));
    const chats = conversations
      .filter((c) => !c.parentConversationId)
      .sort((a, b) => b.updatedAt - a.updatedAt);
    const snippets = new Map(hits.map((h) => [h.conversation_id, h.snippet]));
    const now = Date.now();

    const chatItem = (c: (typeof chats)[number]): Item => ({
      key: `chat:${c.id}`,
      icon: <MessageIcon />,
      label: c.title,
      meta: c.projectId ? projectName.get(c.projectId) : undefined,
      time: shortTime(c.updatedAt, now),
      snippet: snippets.get(c.id),
      run: () => openSession(c.id),
    });
    const projectItem = (p: (typeof projects)[number]): Item => ({
      key: `project:${p.id}`,
      icon: <FolderIcon />,
      label: p.name,
      meta: p.rootPath ?? undefined,
      run: () => openProjectView(p.id),
    });
    const commands: Item[] = [
      { key: "cmd:new-chat", icon: <PlusIcon />, label: "New chat", run: newConversation },
      { key: "cmd:new-project", icon: <PlusIcon />, label: "New project", run: newProject },
      { key: "cmd:library", icon: <BookmarkIcon />, label: "Library", run: () => setView("library") },
      { key: "cmd:projects", icon: <FolderIcon />, label: "Projects", meta: "Overview", run: () => setView("projects") },
    ];

    if (words.length === 0) {
      return [
        { label: "Recent", items: chats.slice(0, 6).map(chatItem) },
        {
          label: "Projects",
          items: [...projects].sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 5).map(projectItem),
        },
        {
          label: "Commands",
          items: [
            ...commands,
            { key: "cmd:settings", icon: <SettingsIcon />, label: "Settings", run: () => setView("settings") },
          ],
        },
      ].filter((g) => g.items.length > 0);
    }

    const byTitle = ranked(chats, (c) => c.title, words, 8);
    const titled = new Set(byTitle.map((c) => c.id));
    const byBody = hits
      .map((h) => chats.find((c) => c.id === h.conversation_id))
      .filter((c): c is (typeof chats)[number] => !!c && !titled.has(c.id))
      .slice(0, 12 - byTitle.length);

    const settings: Item[] = HUB_SECTIONS.map((s) => ({
      key: `settings:${s.view}`,
      icon: <SettingsIcon />,
      label: s.label,
      meta: "Settings",
      run: () => setView(s.view),
    }));

    return [
      { label: "Chats", items: [...byTitle, ...byBody].map(chatItem) },
      {
        label: "Projects",
        items: ranked(projects, (p) => `${p.name} ${p.rootPath ?? ""}`, words, 5).map(projectItem),
      },
      {
        label: "Library",
        items: ranked(artifacts, (a) => a.title, words, 5).map((a) => ({
          key: `artifact:${a.id}`,
          icon: <SparkleIcon />,
          label: a.title,
          meta: a.kind,
          time: shortTime(a.created_at, now),
          run: () => viewArtifact(a),
        })),
      },
      {
        label: "Commands",
        items: ranked([...commands, ...settings], (i) => `${i.label} ${i.meta ?? ""}`, words, 8),
      },
    ].filter((g) => g.items.length > 0);
  }, [
    query,
    hits,
    conversations,
    projects,
    artifacts,
    openSession,
    openProjectView,
    viewArtifact,
    newConversation,
    newProject,
    setView,
  ]);

  const flat = groups.flatMap((g) => g.items);
  const activeIndex = Math.min(active, flat.length - 1);

  useEffect(() => {
    document.getElementById(`palette-opt-${activeIndex}`)?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  const choose = (item: Item | undefined) => {
    if (!item) return;
    onClose();
    item.run();
  };

  let index = 0;
  const trimmed = query.trim();

  return (
    <div
      className="palette-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="palette" role="dialog" aria-modal="true" aria-label="Search">
        <div className="palette-input-row">
          <SearchIcon size={16} />
          <input
            ref={inputRef}
            className="palette-input"
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            aria-autocomplete="list"
            aria-activedescendant={flat.length ? `palette-opt-${activeIndex}` : undefined}
            placeholder="Search chats, projects, library…"
            spellCheck={false}
            autoComplete="off"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActive(0);
            }}
            onKeyDown={(e) => {
              const n = flat.length;
              if (e.key === "ArrowDown" && n) {
                e.preventDefault();
                setActive((activeIndex + 1) % n);
              } else if (e.key === "ArrowUp" && n) {
                e.preventDefault();
                setActive((activeIndex - 1 + n) % n);
              } else if (e.key === "Enter" && !e.nativeEvent.isComposing) {
                e.preventDefault();
                choose(flat[activeIndex]);
              } else if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                onClose();
              } else if (e.key === "Tab") {
                e.preventDefault();
              }
            }}
          />
          <kbd className="kbd">esc</kbd>
        </div>

        <div className="palette-list" id="palette-list" role="listbox" aria-label="Results">
          {groups.map((g) => (
            <div className="palette-group" role="group" aria-label={g.label} key={g.label}>
              <div className="palette-group-label" aria-hidden="true">
                {g.label}
              </div>
              {g.items.map((item) => {
                const i = index++;
                return (
                  <div
                    key={item.key}
                    id={`palette-opt-${i}`}
                    className="palette-option"
                    role="option"
                    aria-selected={i === activeIndex}
                    onMouseMove={() => i !== activeIndex && setActive(i)}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => choose(item)}
                  >
                    <span className="palette-icon" aria-hidden="true">
                      {item.icon}
                    </span>
                    <span className="palette-main">
                      <span className="palette-label">{item.label}</span>
                      {item.snippet && (
                        <span className="palette-snippet">
                          {splitSnippet(item.snippet).map((piece, k) =>
                            piece.hit ? <mark key={k}>{piece.text}</mark> : <span key={k}>{piece.text}</span>
                          )}
                        </span>
                      )}
                    </span>
                    {item.meta && (
                      <span className="palette-meta" title={item.meta}>
                        {item.meta}
                      </span>
                    )}
                    {item.time && <span className="palette-time">{item.time}</span>}
                  </div>
                );
              })}
            </div>
          ))}
          {flat.length === 0 && (
            <p className="palette-empty">
              {searching ? "Searching messages…" : `Nothing matches “${trimmed}”.`}
            </p>
          )}
        </div>

        <div className="palette-foot">
          <span>
            <kbd className="kbd">↑</kbd> <kbd className="kbd">↓</kbd> move
          </span>
          <span>
            <kbd className="kbd">↵</kbd> open
          </span>
          <span className="palette-status" role="status">
            {searching && flat.length > 0
              ? "Searching messages…"
              : trimmed
                ? `${flat.length} result${flat.length === 1 ? "" : "s"}`
                : ""}
          </span>
        </div>
      </div>
    </div>
  );
}
