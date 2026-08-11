import { useState } from "react";
import { useAppStore } from "../../lib/store";
import ImageByPath from "../Conversation/ImageByPath";

/**
 * `BRW-UI-1`: the browser as a felt presence, not a background process — a
 * live page, a screenshot, and a plain-past-tense action trail, shown while a
 * session is open for this conversation and for as long as it takes to read
 * that it closed.
 */
export default function BrowserPanel({ conversationId }: { conversationId: string }) {
  const session = useAppStore((s) => s.browserSessions[conversationId]);
  const stopBrowsing = useAppStore((s) => s.stopBrowsing);
  const dismiss = useAppStore((s) => s.dismissBrowserPanel);
  const [expanded, setExpanded] = useState(false);

  // The fetch lives in `Workbench`, not here: this component only mounts once
  // the Browser tab exists, and the tab only exists once the fetch has come
  // back — asking from inside it could never have populated an empty store.
  if (!session) return null;
  const closed = session.closed;

  return (
    <section className="wb-section-block wb-browser">
      <div className="wb-section wb-browser-head">
        <span className="wb-browser-glyph" aria-hidden="true">
          ◇
        </span>
        <span className="wb-browser-title">
          {closed
            ? "I closed the page."
            : session.domain
              ? `I'm looking at ${session.domain}.`
              : "Opening a page…"}
        </span>
        <button
          className="wb-browser-stop"
          onClick={() => (closed ? dismiss(conversationId) : stopBrowsing(conversationId))}
        >
          {closed ? "Dismiss" : "Stop browsing"}
        </button>
      </div>
      {!closed && session.title && <p className="wb-browser-page-title">{session.title}</p>}
      {!closed && session.screenshot && (
        <button
          className={`wb-browser-shot ${expanded ? "expanded" : ""}`}
          onClick={() => setExpanded((e) => !e)}
          aria-label={expanded ? "Shrink the screenshot" : "View the screenshot full size"}
        >
          <ImageByPath path={session.screenshot} alt="Browser screenshot" />
        </button>
      )}
      {session.trail.length > 0 && (
        <p className="wb-browser-trail">{session.trail.join(" · ")}</p>
      )}
    </section>
  );
}
