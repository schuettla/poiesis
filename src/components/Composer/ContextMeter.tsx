import { useAppStore } from "../../lib/store";
import { estimateTokens } from "../../lib/context";
import "./ContextMeter.css";

/**
 * How full the model's context window is (CTX-UI-1). Silent until it matters:
 * nothing renders below half. Estimated locally each render — no backend call
 * per keystroke.
 */
export default function ContextMeter({ draft = "" }: { draft?: string }) {
  const budget = useAppStore((s) => s.contextBudget);
  const conv = useAppStore((s) => s.conversations.find((c) => c.id === s.activeConversationId));
  const systemPrompt = useAppStore((s) => s.systemPrompt);

  if (!conv || !budget) return null;

  // Turns the summary already stands in for don't get sent, so don't count them.
  const boundary = conv.summaryUptoMessageId;
  const cut = boundary ? conv.messages.findIndex((m) => m.id === boundary) : -1;
  const sent = conv.messages.slice(cut + 1);

  const used =
    estimateTokens(systemPrompt) +
    estimateTokens(conv.summary ?? "") +
    estimateTokens(draft) +
    sent.reduce((n, m) => n + estimateTokens(m.text), 0);

  const fill = Math.min(1, used / budget);
  if (fill < 0.5) return null;

  const label = `~${used.toLocaleString()} / ${budget.toLocaleString()} tokens${
    conv.summary ? " · older turns summarized" : ""
  }`;

  return (
    <div className="context-meter" title={label} role="img" aria-label={label}>
      <div className="context-meter-fill" style={{ width: `${Math.round(fill * 100)}%` }} />
    </div>
  );
}
