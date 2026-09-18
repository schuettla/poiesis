import { useEffect, useState } from "react";
import { isInsideFolder, itemKey, useActiveConversation, useAppStore, useLiveItems } from "../../lib/store";
import { CloseIcon, DownloadIcon, FolderIcon } from "../Icons/Icons";
import { RunView } from "./AgentsPanel";
import DiffView, { changeLabel } from "./DiffView";
import { saveFile } from "./CodeEditor";
import { ArtifactView, FileView, canPreview } from "./Viewer";
import { downloadArtifact } from "./artifactFiles";
import "./Workbench.css";

/**
 * `SHL-27`: the focused item tab, filling the conversation's own column.
 *
 * Two narrower homes were tried first and both failed the same way. Inside the
 * sidebar a file was stuck at 340px and covered the tree it came from; in a
 * column of its own (`SHL-26`) a document and the conversation split one
 * window and neither had room. A file, a patch or an agent transcript wants
 * the widest surface the shell has, and the reason it can take it — unlike the
 * first attempt at this (`SHL-22`) — is the session tab: the conversation is
 * not gone, it is one tab to the left, still scrolled where you left it.
 */
export default function ItemView() {
  const conversation = useActiveConversation();
  const { items, activeKey } = useLiveItems();
  const closeItem = useAppStore((s) => s.closeItem);
  const revealInSystem = useAppStore((s) => s.revealInSystem);
  const convId = conversation?.id ?? null;
  const folder = conversation?.folderPath ?? null;

  // `useLiveItems` has already narrowed to the live chat and resolved which
  // tab is showing, so the strip, this view and the shell's `item-open` class
  // agree about it without any of them deriving it a second time.
  const item = items.find((t) => itemKey(t) === activeKey);
  const artifact = useAppStore((s) =>
    item?.kind === "artifact" && convId ? (s.artifacts[convId] ?? []).find((a) => a.id === item.id) : undefined
  );
  // Not yet loaded is not gone: a restored artifact tab must survive the
  // moment before this chat's artifacts arrive.
  const artifactsLoaded = useAppStore((s) => !!convId && s.artifacts[convId] !== undefined);
  const run = useAppStore((s) => (item?.kind === "run" ? s.subRuns[item.id] : undefined));
  // `PRJ-UI-3`: a patch resolves from its own chat's change set. Not loaded
  // yet is not gone, for the same reason as an artifact above.
  const changeSet = useAppStore((s) => (item?.kind === "diff" && convId ? s.changeSets[convId] : undefined));
  const change = changeSet?.files.find((f) => item?.kind === "diff" && f.path === item.id);
  const undoChanges = useAppStore((s) => s.undoChanges);
  // `EDT-1`: this tab's editor holds edits that aren't on disk.
  const unsaved = useAppStore((s) => item?.kind === "file" && !!s.unsavedFiles[item.id]);
  // `EDT-1`: for a file that renders as well as reads, which of the two this
  // tab is showing. It lives up here because the control that sets it belongs
  // in the header row with Save and Close, not floating over the content —
  // and it resets per file, so a Markdown file opened after an HTML one still
  // opens the way its own kind opens.
  const [editing, setEditing] = useState(false);
  const previewable = item?.kind === "file" && canPreview(item.id);
  const filePath = item?.kind === "file" ? item.id : null;
  useEffect(() => setEditing(false), [filePath]);

  // An item that no longer resolves must not leave a ghost tab behind: an
  // artifact that vanished, a run that is gone, or a file whose folder was
  // detached or swapped underneath it. Closing lands on a neighbour, or on
  // the conversation when there is none.
  const gone =
    !!item &&
    ((item.kind === "artifact" && artifactsLoaded && !artifact) ||
      (item.kind === "run" && !run) ||
      // The change set no longer holds this file: undone, or kept.
      (item.kind === "diff" && !!changeSet && !change) ||
      (item.kind === "file" && (!folder || !isInsideFolder(item.id, folder))));
  useEffect(() => {
    if (gone && activeKey) closeItem(activeKey);
  }, [gone, activeKey, closeItem]);

  if (!item || gone) return null;

  const title =
    item.kind === "file" || item.kind === "diff"
      ? item.id.split(/[\\/]/).pop()
      : item.kind === "artifact"
        ? artifact?.title
        : run?.agent;

  return (
    <section className="item-view wb-viewer" aria-label={title}>
      <div className="wb-viewer-head">
        <span className="wb-viewer-title" title={item.kind === "file" || item.kind === "diff" ? item.id : title}>
          {title}
        </span>
        {item.kind === "diff" && change && (
          <span className="chg-tab-head">
            <span className="chg-added">+{change.added}</span>
            <span className="chg-removed">−{change.removed}</span>
            {changeLabel(change) && <span>{changeLabel(change)}</span>}
          </span>
        )}
        <div className="wb-viewer-actions">
          {/* `EDT-1`: leftmost, because it says what you are looking at —
              the two icons to its right act on it. */}
          {previewable && (
            <div className="editor-modes" role="group" aria-label="How to show this file">
              <button
                className={`editor-mode ${!editing ? "active" : ""}`}
                aria-pressed={!editing}
                onClick={() => setEditing(false)}
              >
                Preview
              </button>
              <button
                className={`editor-mode ${editing ? "active" : ""}`}
                aria-pressed={editing}
                onClick={() => setEditing(true)}
              >
                Source
                {unsaved && !editing && <span className="editor-mode-dot" aria-label="unsaved" />}
              </button>
            </div>
          )}
          {item.kind === "artifact" && artifact && (
            <button
              className="wb-icon"
              title="Save a copy…"
              aria-label={`Save a copy of ${artifact.title}`}
              onClick={() => downloadArtifact(artifact)}
            >
              <DownloadIcon size={15} />
            </button>
          )}
          {item.kind === "diff" && change && convId && (
            <button className="wb-link" onClick={() => void undoChanges(convId, change.path)}>
              Undo this file
            </button>
          )}
          {/* `EDT-1`: only drawn once there is something to save, so the
              header stays quiet for a file being read. The shortcut is the
              primary way in; this is the discoverable one. */}
          {item.kind === "file" && unsaved && (
            <button
              className="wb-link item-save"
              title="Save this file (Ctrl+S)"
              onClick={() => void saveFile(item.id)}
            >
              Save
            </button>
          )}
          {item.kind === "file" && (
            <button
              className="wb-icon"
              title="Show in file manager"
              aria-label="Show in file manager"
              onClick={() => revealInSystem(item.id)}
            >
              <FolderIcon size={15} />
            </button>
          )}
          <button className="wb-icon" onClick={() => closeItem(itemKey(item))} aria-label="Close this tab" title="Close">
            <CloseIcon size={14} strokeWidth={1.4} />
          </button>
        </div>
      </div>
      <div className="wb-viewer-body">
        {item.kind === "file" ? (
          <FileView path={item.id} line={item.line} editing={editing || !previewable} />
        ) : item.kind === "diff" ? (
          <div className="chg-tab">
            {change ? <DiffView file={change} /> : <p className="chg-note">Loading the patch…</p>}
          </div>
        ) : item.kind === "artifact" && artifact ? (
          <ArtifactView kind={artifact.kind} content={artifact.content} artifactId={artifact.id} title={artifact.title} />
        ) : run ? (
          <RunView run={run} />
        ) : null}
      </div>
    </section>
  );
}
