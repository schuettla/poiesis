import { useAppStore } from "../../lib/store";

/** Flatten the durable session state into `path → label` items for the strip. */
function flatten(state: Record<string, unknown> | undefined): { path: string; label: string }[] {
  if (!state) return [];
  const out: { path: string; label: string }[] = [];
  for (const [cat, val] of Object.entries(state)) {
    if (val && typeof val === "object" && !Array.isArray(val)) {
      for (const [k, v] of Object.entries(val as Record<string, unknown>)) {
        out.push({ path: `${cat}.${k}`, label: `${k}: ${formatVal(v)}` });
      }
    } else {
      out.push({ path: cat, label: `${cat}: ${formatVal(val)}` });
    }
  }
  return out;
}

function formatVal(v: unknown): string {
  if (v == null) return "—";
  if (Array.isArray(v)) return v.map(String).join(", ");
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

/**
 * Does this session key also live in durable memory (MEM-UI-7)? A substring
 * check against the memory index is deliberately cheap and slightly loose —
 * the marker is a hint that something outlives the session, not a claim about
 * a specific file. Very short keys are skipped so they can't match by accident.
 */
function isDurable(path: string, index: string): boolean {
  const leaf = path.split(".").pop() ?? "";
  return leaf.length >= 4 && index.toLowerCase().includes(leaf.toLowerCase());
}

/** The durable session-state strip (Generative UI, Phase C): a quiet running
 * head of the conversation's remembered constraints and decisions. */
export default function SessionStrip() {
  const convId = useAppStore((s) => s.activeConversationId);
  const state = useAppStore((s) => (convId ? s.sessionState[convId] : undefined));
  const clearKey = useAppStore((s) => s.clearSessionStateKey);
  const memoryIndex = useAppStore((s) => s.memoryContext.index);
  const items = flatten(state);
  if (!items.length) return null;
  return (
    <div className="session-strip" aria-label="Session memory">
      <span className="session-label">Session</span>
      {items.map((it) => (
        <span className="session-item" key={it.path}>
          {it.label}
          {isDurable(it.path, memoryIndex) && (
            <span
              className="session-durable"
              title="also saved to durable memory"
              aria-label="also saved to durable memory"
            >
              ◆
            </span>
          )}
          <button
            className="session-clear"
            aria-label={`Forget ${it.label}`}
            onClick={() => clearKey(it.path)}
          >
            ×
          </button>
        </span>
      ))}
    </div>
  );
}
