import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import * as api from "../../lib/api";
import { leaderOf } from "../../lib/leaders";
import { useAppStore } from "../../lib/store";
import type { Conversation } from "../../lib/types";
import {
  CodeIcon,
  DiffIcon,
  DocumentIcon,
  FileIcon,
  GlobeIcon,
  ImageIcon,
  TeamIcon,
  VideoIcon,
} from "../Icons/Icons";
import PersonaGlyph, { TeamGlyph } from "../Personas/PersonaGlyph";
import { extFor, slugify } from "../Workbench/artifactFiles";

/** How long the pointer rests on a row before its card opens: soon, but not on a pass-through. */
const SHOW_DELAY_MS = 350;
const CARD_WIDTH = 272;

/** One read per session until it is touched again: moving back over a row costs nothing. */
const cache = new Map<string, { updatedAt: number; card: api.SessionCard }>();

const KIND_ICONS: Record<string, ReactNode> = {
  html: <GlobeIcon size={13} />,
  svg: <ImageIcon size={13} />,
  image: <ImageIcon size={13} />,
  video: <VideoIcon size={13} />,
  markdown: <DocumentIcon size={13} />,
  code: <CodeIcon size={13} />,
};
/** How many artifacts are named before "+N more". */
const NAMED = 4;

function Made({ icon, title, children }: { icon: ReactNode; title?: string; children: ReactNode }) {
  return (
    <span className="session-card-made" title={title}>
      {icon}
      <span className="session-card-made-name">{children}</span>
    </span>
  );
}

/** The artifact as a file: the name it was saved under, the generated file's own name, or the
 * name the Workbench would offer for it. */
function fileNameOf(a: api.SessionCard["artifacts"][number]): string {
  const real = a.saved_path ?? a.path;
  if (real) return folderName(real);
  return `${slugify(a.title)}.${extFor({ kind: a.kind, content: "" })}`;
}

/** "5 min ago", "3 days ago", then a date: what a person says, not a timestamp. */
function ago(ts: number, now = Date.now()): string {
  const mins = Math.floor((now - ts) / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return days === 1 ? "yesterday" : `${days} days ago`;
  return `on ${day(ts, now)}`;
}
function day(ts: number, now = Date.now()): string {
  const sameYear = new Date(ts).getFullYear() === new Date(now).getFullYear();
  return new Date(ts).toLocaleDateString(undefined, sameYear ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" });
}
function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function SessionHoverCard({
  c,
  card,
  top,
  left,
}: {
  c: Conversation;
  card: api.SessionCard | null;
  top: number;
  left: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const projects = useAppStore((s) => s.projects);
  const personas = useAppStore((s) => s.personas);
  const teams = useAppStore((s) => s.teams);
  const project = c.projectId ? projects.find((p) => p.id === c.projectId) : undefined;
  const leaderId = leaderOf(c);
  const persona = leaderId.startsWith("persona:") ? personas.find((p) => `persona:${p.id}` === leaderId) : undefined;
  const team = leaderId.startsWith("team:") ? teams.find((t) => `team:${t.slug}` === leaderId) : undefined;
  const coordinator = team ? personas.find((p) => `persona:${p.id}` === team.coordinator) : undefined;
  const folder = c.folderPath ?? project?.rootPath ?? null;
  const where = project?.name ?? (folder ? folderName(folder) : null);

  // Keep the card inside the window: its height is only known once it is laid out.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.top = `${Math.max(8, Math.min(top, window.innerHeight - el.offsetHeight - 8))}px`;
    el.style.left = `${Math.max(8, Math.min(left, window.innerWidth - CARD_WIDTH - 8))}px`;
  }, [top, left, card]);

  const messages = card ? card.user_messages + card.assistant_messages : 0;
  const shown = (card?.artifacts ?? []).slice(0, NAMED);
  const made = shown.map((a, i) => {
    const name = fileNameOf(a);
    return (
      <Made key={`${i}:${name}`} icon={KIND_ICONS[a.kind] ?? <FileIcon size={13} />} title={name}>
        {name}
      </Made>
    );
  });
  const more = (card?.artifact_count ?? 0) - shown.length;
  if (more > 0) made.push(<Made key="more" icon={null}>+{more} more</Made>);
  const hasMade = made.length > 0 || (card?.files_changed ?? 0) > 0 || (card?.helpers ?? 0) > 0;

  return (
    <div ref={ref} className="session-card" role="tooltip" style={{ top, left, width: CARD_WIDTH }}>
      <p className="session-card-title">{c.title}</p>

      {(persona || team || where) && (
        <p className="session-card-where">
          {persona && <PersonaGlyph name={persona.name} look={persona.look} size={14} />}
          {team && <TeamGlyph coordinator={coordinator} size={14} />}
          <span>{[persona?.name ?? team?.name, where].filter(Boolean).join(" · ")}</span>
        </p>
      )}

      {hasMade && (
        <p className="session-card-made-row">
          {made}
          {(card?.files_changed ?? 0) > 0 && (
            <Made icon={<DiffIcon size={13} />}>
              {card!.files_changed} {card!.files_changed === 1 ? "file" : "files"} changed
            </Made>
          )}
          {(card?.helpers ?? 0) > 0 && (
            <Made icon={<TeamIcon size={13} />}>
              {card!.helpers} {card!.helpers === 1 ? "helper" : "helpers"}
            </Made>
          )}
        </p>
      )}

      <p className="session-card-foot">
        {[
          card && messages > 0 ? `${messages} ${messages === 1 ? "message" : "messages"}` : null,
          card && card.created_at > 0 ? `started ${day(card.created_at)}` : null,
          `active ${ago(c.updatedAt)}`,
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>
    </div>
  );
}

/** `Rail` rows: a hover card with where the session stands. The row spreads `rowProps` and
 * renders `cardPortal`. The card opens after a short rest, and only then reads the counts (and
 * opens once it has them, so it never changes size under the pointer). Keyboard focus opens it
 * the same way; a click, a scroll or leaving closes it. */
export function useSessionHover(c: Conversation) {
  const [at, setAt] = useState<{ top: number; left: number; card: api.SessionCard | null } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const ticket = useRef(0);

  const hide = useCallback(() => {
    ticket.current += 1;
    window.clearTimeout(timer.current);
    setAt(null);
  }, []);

  const show = useCallback(
    (row: HTMLElement) => {
      window.clearTimeout(timer.current);
      const mine = ++ticket.current;
      timer.current = window.setTimeout(async () => {
        const hit = cache.get(c.id);
        let card: api.SessionCard | null = hit && hit.updatedAt === c.updatedAt ? hit.card : null;
        if (!card) {
          try {
            card = await api.sessionCard(c.id);
            cache.set(c.id, { updatedAt: c.updatedAt, card });
          } catch {
            card = null; // outside the app, or unreadable: the card still shows what the list knows
          }
        }
        if (ticket.current !== mine) return;
        const r = row.getBoundingClientRect();
        const rail = row.closest(".rail")?.getBoundingClientRect();
        setAt({ top: r.top, left: (rail?.right ?? r.right) + 8, card });
      }, SHOW_DELAY_MS);
    },
    [c.id, c.updatedAt]
  );

  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    if (!at) return;
    // The list moving under a resting pointer leaves the card pointing at the wrong row.
    window.addEventListener("scroll", hide, true);
    return () => window.removeEventListener("scroll", hide, true);
  }, [at, hide]);

  const rowProps = {
    onPointerEnter: (e: React.PointerEvent<HTMLElement>) => {
      if (e.pointerType !== "touch") show(e.currentTarget);
    },
    onPointerLeave: hide,
    onPointerDown: hide, // also covers a right-click, which opens the row menu
    onFocus: (e: React.FocusEvent<HTMLElement>) => {
      if (e.target === e.currentTarget && e.currentTarget.matches(":focus-visible")) show(e.currentTarget);
    },
    onBlur: hide,
  };

  const cardPortal = at
    ? createPortal(<SessionHoverCard c={c} card={at.card} top={at.top} left={at.left} />, document.body)
    : null;

  return { rowProps, cardPortal };
}
