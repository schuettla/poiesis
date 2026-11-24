import type { ReactNode } from "react";
import type { LinkState } from "../types";
import { BackIcon } from "./Icons";
import Mark from "./Mark";

/** The one bar at the top of every screen: back (or the mark), a title, one quiet line under it, and one action. */
export default function TopBar({
  title,
  line,
  link,
  working,
  onBack,
  action,
  serif,
}: {
  title: string;
  /** Under the title, small. */
  line?: string;
  link: LinkState;
  working: boolean;
  onBack?: () => void;
  action?: ReactNode;
  /** The app's own name is set in the serif, as on the desktop. */
  serif?: boolean;
}) {
  const state = link !== "connected" ? "away" : working ? "working" : "resting";
  return (
    <header className="top-bar">
      {onBack ? (
        <button className="bar-btn" aria-label="Back to the chats" onClick={onBack}>
          <BackIcon size={22} />
        </button>
      ) : (
        <span className="bar-mark">
          <Mark state={state} size={28} />
        </span>
      )}
      <div className="top-bar-text" role="status">
        <h1 className={serif ? "top-bar-title is-serif" : "top-bar-title"}>{title}</h1>
        {line && <p className={`top-bar-line link-${link}`}>{line}</p>}
      </div>
      {onBack && (
        <span className="bar-mark">
          <Mark state={state} size={22} />
        </span>
      )}
      {action}
    </header>
  );
}
