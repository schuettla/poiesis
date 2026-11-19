import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import * as api from "../../lib/api";
import "./SoulEditor.css";

/**
 * Standing instructions and the proposals to change them (SOUL-UI-1).
 *
 * The agent can only ever *propose* an edit here; accepting is a user action
 * outside the agent loop. The diff is two plain blocks on purpose — seeing the
 * whole before and after is more honest than a coloured line-level guess.
 */
export default function SoulEditor() {
  const soul = useAppStore((s) => s.memoryContext.soul);
  const refreshMemoryContext = useAppStore((s) => s.refreshMemoryContext);
  const proposals = useAppStore((s) => s.changeProposals);
  const resolveProposal = useAppStore((s) => s.resolveChangeProposal);

  const [text, setText] = useState(soul);
  const [saved, setSaved] = useState(false);

  // Follow the stored soul when it changes underneath us — accepting a
  // proposal rewrites it, and the textarea should show the result.
  useEffect(() => {
    setText(soul);
  }, [soul]);

  if (!api.inTauri()) return null;

  const pending = proposals.filter((p) => p.target === "soul");
  const dirty = text !== soul;

  async function save() {
    await api.setSoul(text);
    await refreshMemoryContext();
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  }

  return (
    <div className="soul-section">
      <textarea
        className="system-prompt"
        rows={4}
        spellCheck={false}
        value={text}
        placeholder="e.g. Answer in German unless I write in English."
        onChange={(e) => setText(e.target.value)}
      />
      {(dirty || saved) && (
        <div className="setting-actions">
          <button className="btn-primary" onClick={save} disabled={!dirty}>
            {saved && !dirty ? "Saved" : "Save"}
          </button>
          {dirty && (
            <button className="btn-text" onClick={() => setText(soul)}>
              Revert
            </button>
          )}
        </div>
      )}

      {pending.map((p) => (
        <div className="soul-proposal" key={p.id}>
          <p className="soul-proposal-why">{p.rationale}</p>
          <div className="soul-diff">
            <div className="soul-diff-side">
              <span className="soul-diff-label">now</span>
              <pre>{soul || "(empty)"}</pre>
            </div>
            <div className="soul-diff-side">
              <span className="soul-diff-label">proposed</span>
              <pre>{p.proposed_text}</pre>
            </div>
          </div>
          <div className="setting-actions">
            <button className="btn-primary" onClick={() => resolveProposal(p.id, true)}>
              Accept
            </button>
            <button className="btn-text" onClick={() => resolveProposal(p.id, false)}>
              Dismiss
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
