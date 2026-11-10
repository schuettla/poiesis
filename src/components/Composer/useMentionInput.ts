import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";
import type { ContextRef } from "../../lib/types";
import {
  dropMention,
  insertMention,
  mentionAt,
  mentionsStillIn,
  rankFiles,
  rankLibrary,
} from "../../lib/mentions";

/**
 * The composer's `@` menu: files from the working folder and things from the
 * library. Like the `/` menu it is *derived* from the text and the caret rather
 * than held in an "is it open" flag, so it can never show for a word that is not
 * there.
 *
 * A file goes into the text as `@path` and is remembered here; only the ones
 * still in the text when the message goes out are sent along (as `file` refs, so
 * the model is told to read them). A library item has no useful text form, so it
 * becomes a chip, the same way the `+` menu adds it, and the `@word` is removed.
 */

const FILE_LIMIT = 8;
const LIBRARY_LIMIT = 4;
const LIST_ID = "composer-mention-menu";

export type MentionItem =
  | { kind: "file"; key: string; path: string }
  | { kind: "library"; key: string; artifact: api.Artifact };

export interface MentionMenuModel {
  open: boolean;
  items: MentionItem[];
  index: number;
  listId: string;
  query: string;
  /** Whether this chat has a folder at all, for the empty message. */
  hasFolder: boolean;
}

export function useMentionInput({
  value,
  setValue,
  inputRef,
  onLibrary,
}: {
  value: string;
  setValue: (v: string) => void;
  inputRef: React.RefObject<HTMLInputElement>;
  /** A library item was chosen: the composer adds it as a chip. */
  onLibrary: (a: api.Artifact) => void;
}) {
  const convId = useAppStore((s) => s.activeConversationId);
  const hasFolder = useAppStore(
    (s) => !!s.conversations.find((c) => c.id === s.activeConversationId)?.folderPath
  );
  const allArtifacts = useAppStore((s) => s.allArtifacts);
  const refreshAllArtifacts = useAppStore((s) => s.refreshAllArtifacts);

  const [caret, setCaret] = useState(0);
  const [index, setIndex] = useState(0);
  // The `@` Escape was pressed on. Typing past it does not bring the menu back.
  const [dismissed, setDismissed] = useState<number | null>(null);
  const [files, setFiles] = useState<{ convId: string | null; list: string[] }>({ convId: null, list: [] });
  const [mentions, setMentions] = useState<ContextRef[]>([]);
  const pendingCaret = useRef<number | null>(null);

  const token = useMemo(() => mentionAt(value, Math.min(caret, value.length)), [value, caret]);
  const open = token !== null && dismissed !== token.start;
  const query = token?.query ?? "";

  useEffect(() => {
    if (token === null) setDismissed(null);
  }, [token]);
  useEffect(() => setIndex(0), [query, open]);

  // Read the folder each time the menu opens, so a file made a minute ago is there.
  useEffect(() => {
    if (!open || !convId || !api.inTauri()) return;
    let live = true;
    api
      .listFolderFiles(convId)
      .then((list) => live && setFiles({ convId, list }))
      .catch(() => {});
    void refreshAllArtifacts();
    return () => {
      live = false;
    };
  }, [open, convId, refreshAllArtifacts]);

  // Put the caret after what was inserted, once the new text is in the box.
  useEffect(() => {
    const at = pendingCaret.current;
    if (at === null) return;
    pendingCaret.current = null;
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(at, at);
    setCaret(at);
  }, [value, inputRef]);

  const items = useMemo<MentionItem[]>(() => {
    if (!open) return [];
    const list = files.convId === convId ? files.list : [];
    return [
      ...rankFiles(list, query, FILE_LIMIT).map((path): MentionItem => ({ kind: "file", key: `f:${path}`, path })),
      ...rankLibrary(allArtifacts, query, LIBRARY_LIMIT).map(
        (artifact): MentionItem => ({ kind: "library", key: `l:${artifact.id}`, artifact })
      ),
    ];
  }, [open, files, convId, query, allArtifacts]);

  function choose(i: number) {
    const item = items[i];
    if (!item || !token) return;
    if (item.kind === "file") {
      const next = insertMention(value, token, `@${item.path}`);
      pendingCaret.current = next.caret;
      setValue(next.text);
      setMentions((list) =>
        list.some((m) => m.id === item.path) ? list : [...list, { kind: "file", id: item.path, label: item.path }]
      );
    } else {
      const next = dropMention(value, token);
      pendingCaret.current = next.caret;
      setValue(next.text);
      onLibrary(item.artifact);
    }
    setDismissed(null);
  }

  /** Returns true when the key was the menu's. With nothing to choose, only
   * Escape is: Enter still sends the message. */
  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>): boolean {
    if (!open || !token) return false;
    if (e.key === "Escape") {
      e.preventDefault();
      setDismissed(token.start);
      return true;
    }
    if (!items.length) return false;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setIndex((i) => (i + step + items.length) % items.length);
      return true;
    }
    if ((e.key === "Enter" || e.key === "Tab") && !e.shiftKey) {
      e.preventDefault();
      choose(index);
      return true;
    }
    return false;
  }

  const model: MentionMenuModel = { open, items, index, listId: LIST_ID, query, hasFolder };

  return {
    model,
    /** Call from `onChange` and `onSelect`, so the caret is always current. */
    track: (el: HTMLInputElement) => setCaret(el.selectionStart ?? el.value.length),
    onKeyDown,
    choose,
    setIndex,
    /** The files to send with `text`: the ones still mentioned in it. */
    refsFor: (text: string) => mentionsStillIn(text, mentions),
    clear: () => setMentions([]),
    activeId: open && items.length ? `${LIST_ID}-opt-${index}` : undefined,
  };
}
