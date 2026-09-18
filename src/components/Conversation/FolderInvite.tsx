import { useActiveConversation, useAppStore } from "../../lib/store";
import { inTauri } from "../../lib/api";
import { FolderIcon } from "../Icons/Icons";

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
        <FolderIcon size={15} />
      </span>
      Give me a folder to work in — I can read, search and edit files there
      <span className="folder-invite-arrow" aria-hidden="true">
        →
      </span>
    </button>
  );
}
