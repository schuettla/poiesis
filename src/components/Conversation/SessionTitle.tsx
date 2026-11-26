import { useRef, useState } from "react";
import { useActiveConversation, useAppStore } from "../../lib/store";

/**
 * The chat's name, editable where it is shown (the header). Enter or leaving the
 * field keeps the new name, Escape drops it, and an empty name changes nothing.
 *
 * The header shows the name in two places that must not behave alike: the lone
 * label (nothing to switch to, so one click renames) and the session tab (a click
 * means "back to the chat", so renaming takes a double click).
 */
export default function SessionTitle({
  className,
  activate,
  fallback,
}: {
  className: string;
  activate: "click" | "dblclick";
  /** Shown when there is no chat yet. */
  fallback: string;
}) {
  const conversation = useActiveConversation();
  const renameConversation = useAppStore((s) => s.renameConversation);
  const [draft, setDraft] = useState<string | null>(null);
  // Escape and Enter both end the edit and the field then loses focus: the blur must not commit twice.
  const finished = useRef(false);

  if (!conversation) return <span className={className}>{fallback}</span>;

  const begin = () => {
    finished.current = false;
    setDraft(conversation.title);
  };
  const finish = (keep: boolean) => {
    if (finished.current) return;
    finished.current = true;
    const title = (draft ?? "").trim();
    setDraft(null);
    if (keep && title && title !== conversation.title) void renameConversation(conversation.id, title);
  };

  if (draft !== null) {
    return (
      <input
        className={`${className} session-title-input`}
        value={draft}
        autoFocus
        aria-label="Chat name"
        onFocus={(e) => e.currentTarget.select()}
        onClick={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => finish(true)}
        onKeyDown={(e) => {
          // Typing must not reach the tab strip's arrow-key and Enter handling.
          e.stopPropagation();
          if (e.key === "Enter") finish(true);
          if (e.key === "Escape") finish(false);
        }}
      />
    );
  }

  const label = conversation.title || fallback;
  if (activate === "click") {
    // A button, not a span: the header is the window's drag surface and only controls are exempt from it.
    return (
      <button className={`${className} session-title-btn`} title={`${label} (click to rename)`} onClick={begin}>
        {label}
      </button>
    );
  }
  return (
    <span className={className} onDoubleClick={begin}>
      {label}
    </span>
  );
}
