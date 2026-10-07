import { useMemo, useState } from "react";
import { useAppStore } from "../../lib/store";
import type { Attachment, ContextRef } from "../../lib/types";
import { FolderIcon, MessageIcon, PaperclipIcon, SectionIcon } from "../Icons/Icons";

type Panel = "library" | "chats";

const LIST_LIMIT = 40;

/**
 * `CMP-3`: the `+` adds *content to this message* — files, a folder, something
 * I made, an earlier conversation — and nothing else. Anything that changes a
 * mode, the run, my memory or the conversation is a `/` command now.
 *
 * It lights up only while something is waiting to go out with the message.
 */
export default function PlusMenu({
  onAttachFiles,
  onAddAttachment,
  onAddRef,
  pending,
}: {
  onAttachFiles: () => void;
  onAddAttachment: (a: Attachment) => void;
  onAddRef: (r: ContextRef) => void;
  /** Attachments or references are queued for the next message. */
  pending: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [panel, setPanel] = useState<Panel | null>(null);
  const attachFolder = useAppStore((s) => s.attachFolder);
  const refreshAllArtifacts = useAppStore((s) => s.refreshAllArtifacts);
  const allArtifacts = useAppStore((s) => s.allArtifacts);
  const conversations = useAppStore((s) => s.conversations);
  const activeId = useAppStore((s) => s.activeConversationId);
  const folderPath = useAppStore(
    (s) => s.conversations.find((c) => c.id === s.activeConversationId)?.folderPath ?? null
  );

  const library = useMemo(
    () => [...allArtifacts].sort((a, b) => b.created_at - a.created_at).slice(0, LIST_LIMIT),
    [allArtifacts]
  );
  const chats = useMemo(
    () =>
      conversations
        .filter((c) => c.id !== activeId && !c.parentConversationId && c.messages.length > 0)
        .slice(0, LIST_LIMIT),
    [conversations, activeId]
  );

  function close() {
    setOpen(false);
    setPanel(null);
  }

  const folderName = folderPath?.split(/[\\/]/).filter(Boolean).pop();

  return (
    <div className="composer-menu-wrap">
      <button
        className={`icon-btn plus-btn ${open ? "on" : ""} ${pending ? "lit" : ""}`}
        aria-label="Add to this message"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Add files, a folder, or something I made"
        onClick={() => {
          setPanel(null);
          setOpen((v) => !v);
        }}
      >
        +
      </button>
      {open && (
        <>
          <div className="composer-menu-backdrop" onClick={close} />
          <div className="composer-menu" role="menu">
            {panel === null && (
              <>
                <button
                  className="composer-menu-item"
                  role="menuitem"
                  onClick={() => {
                    close();
                    onAttachFiles();
                  }}
                >
                  <span className="mi-icon" aria-hidden="true"><PaperclipIcon size={15} /></span>
                  <span className="mi-body">
                    Files and images
                    <span className="mi-hint">images and PDFs, or just paste or drop one</span>
                  </span>
                  <span className="mi-check" />
                </button>
                <button
                  className="composer-menu-item"
                  role="menuitem"
                  disabled={!!folderPath}
                  onClick={() => {
                    close();
                    void attachFolder();
                  }}
                >
                  <span className="mi-icon" aria-hidden="true"><FolderIcon size={15} /></span>
                  <span className="mi-body">
                    A folder
                    <span className="mi-hint">
                      {folderPath
                        ? `already working in ${folderName ?? "a folder"}`
                        : "I'll work in it and read it when it helps"}
                    </span>
                  </span>
                  <span className="mi-check" />
                </button>
                <button
                  className="composer-menu-item"
                  role="menuitem"
                  aria-haspopup="menu"
                  onClick={() => {
                    void refreshAllArtifacts();
                    setPanel("library");
                  }}
                >
                  <span className="mi-icon" aria-hidden="true"><SectionIcon view="library" size={15} /></span>
                  <span className="mi-body">
                    From my library
                    <span className="mi-hint">a picture, page or file I made earlier</span>
                  </span>
                  <span className="mi-more" aria-hidden="true">›</span>
                </button>
                <button
                  className="composer-menu-item"
                  role="menuitem"
                  aria-haspopup="menu"
                  onClick={() => setPanel("chats")}
                >
                  <span className="mi-icon" aria-hidden="true"><MessageIcon size={15} /></span>
                  <span className="mi-body">
                    An earlier conversation
                    <span className="mi-hint">I'll read it before answering</span>
                  </span>
                  <span className="mi-more" aria-hidden="true">›</span>
                </button>
              </>
            )}

            {panel !== null && (
              <>
                <button
                  className="composer-menu-back"
                  onClick={() => setPanel(null)}
                  aria-label="Back to the main menu"
                >
                  <span aria-hidden="true">‹</span>
                  {panel === "library" ? "From my library" : "An earlier conversation"}
                </button>
                <div className="composer-submenu">
                  {panel === "library" &&
                    (library.length === 0 ? (
                      <p className="composer-menu-empty">I haven't made anything yet.</p>
                    ) : (
                      library.map((a) => (
                        <button
                          className="composer-menu-item"
                          role="menuitem"
                          key={a.id}
                          onClick={() => {
                            close();
                            if (a.kind === "image") {
                              onAddAttachment({
                                id: `lib-${a.id}`,
                                kind: "image",
                                name: a.title,
                                path: a.content,
                              });
                            } else {
                              onAddRef({ kind: "artifact", id: a.id, label: a.title });
                            }
                          }}
                        >
                          <span className="mi-icon" aria-hidden="true"><SectionIcon view="library" size={15} /></span>
                          <span className="mi-body">
                            {a.title}
                            <span className="mi-hint">{a.kind}</span>
                          </span>
                          <span className="mi-check" />
                        </button>
                      ))
                    ))}
                  {panel === "chats" &&
                    (chats.length === 0 ? (
                      <p className="composer-menu-empty">There are no other conversations yet.</p>
                    ) : (
                      chats.map((c) => (
                        <button
                          className="composer-menu-item"
                          role="menuitem"
                          key={c.id}
                          onClick={() => {
                            close();
                            onAddRef({ kind: "conversation", id: c.id, label: c.title });
                          }}
                        >
                          <span className="mi-icon" aria-hidden="true"><MessageIcon size={15} /></span>
                          <span className="mi-body">{c.title}</span>
                          <span className="mi-check" />
                        </button>
                      ))
                    ))}
                </div>
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
}
