import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { itemKey, useActiveConversation, useAppStore, useLiveItems } from "../../lib/store";
import { stillWorking } from "../../lib/api";
import type { ItemRef } from "../../lib/types";
import { AgentIcon, ChevronIcon, CloseIcon, DiffIcon, FileIcon, MessageIcon, SparkleIcon } from "../Icons/Icons";
import "./TabStrip.css";

/** `SHL-24`/`SHL-27`: the header's tab area, and the only one.
 *
 * It holds the single things picked out of the right sidebar — a file, an
 * artifact, one child agent, one patch — and each of them fills the whole
 * main column when selected, which is the space a document actually needs.
 *
 * In front of them sits one session tab: the conversation those items came
 * out of. It appears only once something else is open, because with nothing
 * open there is nothing to switch between and a lone tab labelled with the
 * chat you are already reading says nothing. It cannot be closed — a chat is
 * a destination, not something you hold open — so the way back is always
 * there, which is what lets an item take the full width in the first place.
 *
 * Other chats, projects and Settings are not here. Each is a destination
 * reached from the Rail; filing them into the strip as well produced two
 * competing lists of where you might be (`SHL-3`, `SHL-5`, `SHL-14`, all
 * withdrawn). Everything in this strip belongs to one chat, and switching
 * chats takes the whole strip with it.
 */

/** A plain vertical wheel over a horizontal-only strip does nothing by
 * default — the same reason every browser's own tab bar redirects it. */
function onWheelHorizontal(e: React.WheelEvent<HTMLDivElement>) {
  if (e.deltaY === 0) return;
  e.currentTarget.scrollLeft += e.deltaY;
}

/**
 * `SHL-8`: how a tab bar that can't wrap and won't show a scrollbar still
 * lets you reach whatever's off-screen. Chevrons appear only when there's
 * something to scroll to in that direction — nothing to look at when a
 * handful of tabs already fit, which is the common case.
 */
function ScrollZone({
  className,
  ariaLabel,
  children,
}: {
  className: string;
  ariaLabel: string;
  children: React.ReactNode;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [canLeft, setCanLeft] = useState(false);
  const [canRight, setCanRight] = useState(false);

  /** `SHL-19`: a tablist is one stop in the page's tab order, not one per tab.
   * Arrow keys move between the tabs inside it and `Home`/`End` jump to its
   * ends, which is what makes the strip reachable without a mouse at all.
   * Roving focus is read off the DOM rather than tracked in state — the tab
   * elements are the source of truth for what is in this zone and in what
   * order, and mirroring that into React state is how the two drift. */
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
    if (!keys.includes(e.key)) return;
    const el = scrollRef.current;
    if (!el) return;
    const tabs = Array.from(el.querySelectorAll<HTMLElement>('[role="tab"]'));
    if (tabs.length === 0) return;
    const here = tabs.findIndex((t) => t === document.activeElement || t.contains(document.activeElement));
    let next: number;
    if (e.key === "Home") next = 0;
    else if (e.key === "End") next = tabs.length - 1;
    else if (here === -1) next = 0;
    else next = (here + (e.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
    e.preventDefault();
    tabs[next]?.focus();
  };

  const measure = () => {
    const el = scrollRef.current;
    if (!el) return;
    setCanLeft(el.scrollLeft > 1);
    setCanRight(el.scrollLeft < el.scrollWidth - el.clientWidth - 1);
  };

  // Re-measured on scroll, on resize, and whenever the tab set itself
  // changes shape (opening or closing a tab can flip either chevron).
  useLayoutEffect(measure);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const scrollBy = (dir: 1 | -1) => scrollRef.current?.scrollBy({ left: dir * 160, behavior: "smooth" });

  return (
    <div className={`ts-scrollzone ${className}`}>
      {canLeft && (
        <button className="ts-nav ts-nav-left" aria-label="Scroll left" onClick={() => scrollBy(-1)}>
          <ChevronIcon dir="left" size={12} />
        </button>
      )}
      <div
        className="ts-zone"
        ref={scrollRef}
        role="tablist"
        aria-label={ariaLabel}
        onWheel={onWheelHorizontal}
        onScroll={measure}
        onKeyDown={onKeyDown}
      >
        {children}
      </div>
      {canRight && (
        <button className="ts-nav ts-nav-right" aria-label="Scroll right" onClick={() => scrollBy(1)}>
          <ChevronIcon dir="right" size={12} />
        </button>
      )}
    </div>
  );
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** `SHL-8`: the active tab is scrolled into view on focus — the strip's own
 * answer to "how do I reach the one that's off-screen". */
function useScrollIntoView(active: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // jsdom (the test environment) doesn't implement this at all.
    if (active && typeof ref.current?.scrollIntoView === "function") {
      ref.current.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }, [active]);
  return ref;
}

/**
 * The tab shell, so the behaviour that makes a tab a tab is written once.
 *
 * `SHL-18`: middle-click closes, and it belongs on the *tab*, not on the close
 * button — middle-clicking a button that a plain click already closes is not a
 * shortcut, it is the same gesture spelled harder. `preventDefault` stops the
 * browser's autoscroll cursor appearing over the strip.
 *
 * `SHL-19`: the tab itself is the focusable element, roving so that the whole
 * strip is a single stop in the page's tab order. The label is a `span` rather
 * than the `button` it used to be: a `role="tab"` wrapped around a real button
 * is two conflicting things in one place, and screen readers announce it as
 * such. Only the close control stays a button, because it is a second,
 * genuinely separate action on the same row.
 */
function Tab({
  className,
  active,
  label,
  title,
  icon,
  onOpen,
  onClose,
  closeSlot,
  children,
}: {
  className?: string;
  active: boolean;
  label: string;
  title?: string;
  icon?: React.ReactNode;
  onOpen: () => void;
  /** Omitted by the session tab: there is nothing to close, so it grows no ×
   * and middle-click does nothing over it either. */
  onClose?: () => void;
  /** Drawn where the close button sits, with the button shown on hover in
   * its place — how an unsaved editor buffer wears its dot (`PRJ-UI-2`). */
  closeSlot?: React.ReactNode;
  children?: React.ReactNode;
}) {
  const ref = useScrollIntoView(active);
  return (
    <div
      ref={ref}
      className={`ts-tab ${className ?? ""} ${active ? "active" : ""} ${closeSlot ? "has-slot" : ""}`}
      role="tab"
      aria-selected={active}
      tabIndex={active ? 0 : -1}
      title={title ?? label}
      onClick={onOpen}
      onMouseDown={(e) => {
        if (e.button === 1 && onClose) {
          e.preventDefault();
          onClose();
        }
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen();
        }
      }}
    >
      <span className="ts-tab-main">
        {icon && (
          <span className="ts-tab-icon" aria-hidden="true">
            {icon}
          </span>
        )}
        <span className="ts-tab-label">{label}</span>
        {children}
      </span>
      {closeSlot}
      {onClose && (
        <button
          className="ts-tab-close"
          aria-label={`Close ${label}`}
          title="Close"
          tabIndex={active ? 0 : -1}
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
        >
          <CloseIcon size={10} strokeWidth={1.6} />
        </button>
      )}
    </div>
  );
}

function ItemTab({ item, active }: { item: ItemRef; active: boolean }) {
  const openItem = useAppStore((s) => s.openItem);
  const closeItem = useAppStore((s) => s.closeItem);
  // Looked up in the item's own chat, not the live one: the strip is global,
  // and an artifact from another chat still has a name.
  const convId = item.conversationId;
  const artifactTitle = useAppStore((s) => {
    if (item.kind !== "artifact" || !convId) return undefined;
    return (s.artifacts[convId] ?? []).find((a) => a.id === item.id)?.title;
  });
  const run = useAppStore((s) => (item.kind === "run" ? s.subRuns[item.id] : undefined));
  // `PRJ-UI-2`: a file the agent changed carries the dot a changed document
  // always does, read from the change set of the tab's *own* chat — the strip
  // is global, and the tab may belong to a chat that is not live.
  const change = useAppStore((s) => {
    if ((item.kind !== "file" && item.kind !== "diff") || !convId) return undefined;
    return s.changeSets[convId]?.files.find((f) => f.path === item.id);
  });
  const focusChange = useAppStore((s) => s.focusChange);
  // `EDT-1`: edits of the user's own that aren't on disk yet. This outranks
  // the agent's dot below — "you have unsaved work" is the more urgent of the
  // two things a dot on a file tab can mean, and it is the one that is lost if
  // ignored.
  const unsaved = useAppStore((s) => item.kind === "file" && !!s.unsavedFiles[item.id]);
  const dirty = item.kind === "file" && !!change;
  const live = !!run && stillWorking(run.status);
  const label =
    item.kind === "file" || item.kind === "diff"
      ? fileName(item.id)
      : item.kind === "artifact"
        ? artifactTitle || item.id
        : run?.agent || "Agent";
  const key = itemKey(item);
  // One icon per origin, matching the ones `Tree`/`Artifacts`/`AgentsPanel`
  // already sit under, so a tab says where it came from before it is read.
  const icon =
    item.kind === "file" ? (
      <FileIcon size={12} />
    ) : item.kind === "diff" ? (
      <DiffIcon size={12} />
    ) : item.kind === "artifact" ? (
      <SparkleIcon size={12} />
    ) : (
      <AgentIcon size={12} />
    );
  const title =
    item.kind === "file"
      ? item.id
      : item.kind === "diff"
        ? `${item.id}${change ? `  +${change.added} / −${change.removed}` : ""}`
        : item.kind === "run"
          ? run?.task
          : label;
  return (
    <Tab
      className={item.kind === "file" || item.kind === "diff" ? "ts-mono" : ""}
      active={active}
      label={label}
      title={title}
      icon={icon}
      onOpen={() => openItem(item)}
      onClose={() => closeItem(key)}
      closeSlot={
        unsaved ? (
          <span className="ts-dirty unsaved" aria-label={`${label} has unsaved changes`} title="Unsaved changes" />
        ) : dirty && convId ? (
          <button
            className="ts-dirty"
            aria-label={`${label} was changed — show the patch`}
            title="Changed by me — show the patch"
            tabIndex={active ? 0 : -1}
            onClick={(e) => {
              e.stopPropagation();
              focusChange(convId, item.id);
            }}
          />
        ) : undefined
      }
    >
      {live && <span className="ts-live" aria-label="working" />}
    </Tab>
  );
}

/** `SHL-27`: the conversation, as the strip's first and permanent tab.
 *
 * Serif, like the chat's title everywhere else in the shell, and with no ×:
 * the one tab in the strip that is a place rather than a thing held open. */
function SessionTab({ active }: { active: boolean }) {
  const conversation = useActiveConversation();
  const showConversation = useAppStore((s) => s.showConversation);
  const label = conversation?.title || "New chat";
  return (
    <Tab
      className="ts-session"
      active={active}
      label={label}
      title={`${label} — back to the conversation`}
      icon={<MessageIcon size={12} />}
      onOpen={showConversation}
    />
  );
}

export default function TabStrip() {
  const view = useAppStore((s) => s.view);
  const { items, activeKey } = useLiveItems();

  // Nothing open, or any route other than chat: no strip at all. The header
  // names where you are instead (`Location` in TopBar.tsx) — a single tab
  // labelled with the chat already in front of you is a control with nowhere
  // to go, and a strip standing empty reads as a broken layout.
  if (view !== "chat" || items.length === 0) return null;

  return (
    <ScrollZone className="ts-zone-items" ariaLabel="The conversation and its open files, artifacts and agents">
      <SessionTab active={activeKey === null} />
      {items.map((ref) => (
        <ItemTab key={itemKey(ref)} item={ref} active={itemKey(ref) === activeKey} />
      ))}
    </ScrollZone>
  );
}
