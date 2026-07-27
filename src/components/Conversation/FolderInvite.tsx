import { useActiveConversation, useAppStore } from "../../lib/store";
import { inTauri } from "../../lib/api";

/**
 * The way in, shown on an empty chat.
 *
 * The Workbench's own empty state says the same thing, but only to someone who
 * already opened the panel. This puts the capability in the conversation itself,
 * which is where the user is looking.
 */
export default function FolderInvite() {
  const conversation = useActiveConversation();
  const attachFolder = useAppStore((s) => s.attachFolder);
  const setDockOpen = useAppStore((s) => s.setDockOpen);

  if (!inTauri() || conversation?.folderPath) return null;

  return (
    <button
      className="folder-invite"
      onClick={() => {
        setDockOpen(true);
        attachFolder();
      }}
    >
      <span className="folder-invite-icon" aria-hidden="true">
        <svg width="15" height="15" viewBox="0 0 20 20" fill="none">
          <path
            d="M2.5 5.5A1.5 1.5 0 0 1 4 4h3.2l1.4 1.8H16a1.5 1.5 0 0 1 1.5 1.5v7.2A1.5 1.5 0 0 1 16 16H4a1.5 1.5 0 0 1-1.5-1.5z"
            stroke="currentColor"
            strokeWidth="1.3"
            strokeLinejoin="round"
          />
        </svg>
      </span>
      Give me a folder to work in — I can read, search and edit files there
      <span className="folder-invite-arrow" aria-hidden="true">
        →
      </span>
    </button>
  );
}
