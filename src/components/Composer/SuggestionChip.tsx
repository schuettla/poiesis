import { useMemo } from "react";
import { useAppStore } from "../../lib/store";
import { commandViews } from "../../lib/commands";
import "./SuggestionChip.css";

/**
 * `AGC-2`: one slash command I think would clearly help, offered once.
 *
 * One at a time: a newer suggestion replaces this one, and the backend never
 * offers the same command twice in a conversation. There is no badge, no count
 * and no animation. A command that cannot run right now is not offered at all,
 * because a suggestion you cannot follow is only noise.
 */
export default function SuggestionChip() {
  const suggestion = useAppStore((s) => s.activeSuggestion);
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const accept = useAppStore((s) => s.acceptSuggestion);
  const dismiss = useAppStore((s) => s.dismissSuggestion);
  const conversations = useAppStore((s) => s.conversations);
  const busy = useAppStore((s) => s.busy);
  const changeSets = useAppStore((s) => s.changeSets);

  // A fresh array each time, so it lives in a memo over the slices it reads.
  const view = useMemo(
    () =>
      suggestion
        ? commandViews(useAppStore.getState()).find((v) => v.name === suggestion.command)
        : undefined,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [suggestion, conversations, busy, changeSets]
  );

  if (!suggestion || suggestion.convId !== activeConversationId) return null;
  if (!view || view.disabledReason) return null;

  return (
    <div className="suggestion-chip" role="group" aria-label="A suggestion from me">
      <span className="suggestion-chip-text">
        <span aria-hidden="true">◆ </span>
        {suggestion.reason}
        <span className="suggestion-chip-command"> · /{suggestion.command}</span>
      </span>
      <span className="suggestion-chip-actions">
        <button className="btn-text" onClick={accept}>
          Do it
        </button>
        <button className="btn-text" onClick={dismiss}>
          Not now
        </button>
      </span>
    </div>
  );
}
