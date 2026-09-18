import { useEffect, useState } from "react";
import * as api from "../../lib/api";

/**
 * Marks where the model stops seeing turns verbatim (CTX-UI-2). Everything
 * above it is still here and still yours — it just reaches the model as the
 * summary this divider reveals.
 *
 * `CTX-5`: the conversation carries only the newest summary, because that is
 * what gets sent. When a chat is compacted a second time, that summary is a
 * summary of a summary, and the wording of the first is gone from the column.
 * The session log keeps every pass, so opening this divider can show how the
 * beginning of the conversation actually got compressed rather than only where
 * it ended up.
 *
 * The history is fetched on open, not on render: most chats are never compacted
 * and none of them should pay for a query that would come back empty.
 */
export default function CompactDivider({
  summary,
  conversationId,
}: {
  summary: string;
  conversationId: string;
}) {
  const [open, setOpen] = useState(false);
  const [history, setHistory] = useState<api.Compaction[] | null>(null);

  useEffect(() => {
    if (!open || history || !api.inTauri()) return;
    let live = true;
    api
      .conversationSummaries(conversationId)
      // An unreadable history must not hide the summary itself, which is the
      // thing the user opened this for.
      .then((rows) => live && setHistory(rows))
      .catch(() => live && setHistory([]));
    return () => {
      live = false;
    };
  }, [open, history, conversationId]);

  // History is closed on the conversation only once its last row's boundary is
  // the one being shown; until then the newest pass may not have been logged
  // (an older chat, or a write that failed), and the count would be wrong.
  const latest = history?.[history.length - 1];
  const earlier = history ? history.slice(0, -1) : [];
  const replaced = history?.reduce((n, c) => n + c.replaced, 0) ?? 0;

  return (
    <div className="compact-divider-wrap">
      <button
        className="compact-divider"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        title="Show what the model sees in place of these turns"
      >
        · · · earlier turns are summarized for the model · · ·
      </button>
      {open && (
        <div className="compact-detail">
          {replaced > 0 && (
            <p className="compact-note">
              {replaced} {replaced === 1 ? "message stands" : "messages stand"} behind this
              summary. They are all still above, unchanged — this is only what the model is
              sent in their place.
            </p>
          )}
          <pre className="compact-summary">{summary}</pre>
          {latest?.merged_earlier && (
            <p className="compact-note">
              This was written by summarizing an earlier summary, so anything that pass
              dropped is not in it. The earlier versions are below.
            </p>
          )}
          {earlier.length > 0 && (
            <details className="compact-history">
              <summary>
                {earlier.length} earlier {earlier.length === 1 ? "version" : "versions"}
              </summary>
              {earlier
                .slice()
                .reverse()
                .map((c) => (
                  <div className="compact-past" key={c.at}>
                    <span className="compact-when">
                      {new Date(c.at).toLocaleString()} · {c.replaced}{" "}
                      {c.replaced === 1 ? "message" : "messages"}
                    </span>
                    <pre className="compact-summary">{c.text}</pre>
                  </div>
                ))}
            </details>
          )}
        </div>
      )}
    </div>
  );
}
