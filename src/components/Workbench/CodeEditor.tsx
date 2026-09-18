import { useEffect, useRef, useState } from "react";
import type { editor as MonacoEditor } from "monaco-editor/esm/vs/editor/editor.api";
import { useAppStore } from "../../lib/store";
import { readFileForEdit, writeTextFile } from "../../lib/api";

/**
 * `EDT-1`: a file tab you can type in.
 *
 * Monaco is loaded on first use and never at startup — it is by far the
 * largest thing the frontend can pull in, and a chat that never opens a file
 * must not pay for it. Everything about it that is app-specific (the CSP-safe
 * worker, the theme built from `tokens.css`, language resolution) lives in
 * `lib/monaco.ts`; this file is the React lifecycle around it.
 *
 * The buffer is the Monaco model. Nothing mirrors the text into React state or
 * into the store, because two copies of an editable document is how you get a
 * keystroke race — the store holds only the *fact* that it is dirty, which is
 * what other components need.
 */

/** The active editors' save functions, by path, so the pane header's Save
 * button and the tab strip can drive an editor they do not own. A plain module
 * map rather than store state: these are functions, and the store is
 * serialised to disk. */
const savers = new Map<string, () => Promise<void>>();

/** Save the file open in this path's editor, if one is mounted. */
export function saveFile(path: string): Promise<void> {
  return savers.get(path)?.() ?? Promise.resolve();
}

export default function CodeEditor({ path, line }: { path: string; line?: number }) {
  const host = useRef<HTMLDivElement | null>(null);
  const editorRef = useRef<MonacoEditor.IStandaloneCodeEditor | null>(null);
  const conversationId = useAppStore((s) => s.activeConversationId);
  const setUnsaved = useAppStore((s) => s.setUnsaved);
  const dirty = useAppStore((s) => !!s.unsavedFiles[path]);

  const [error, setError] = useState<string | null>(null);
  const [readOnly, setReadOnly] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [saving, setSaving] = useState(false);
  const [ready, setReady] = useState(false);

  // The mtime the buffer was read at, and the save-point in Monaco's undo
  // stack. Refs, not state: the save closure reads them, and a save must never
  // act on a stale render's copy of either.
  const modified = useRef(0);
  const savedVersion = useRef(0);

  useEffect(() => {
    let cancelled = false;
    let disposers: { dispose: () => void }[] = [];
    setError(null);
    setConflict(false);
    setReady(false);

    (async () => {
      const [{ monaco, defineTheme, languageForPath, eolOf, THEME }, file] = await Promise.all([
        import("../../lib/monaco"),
        readFileForEdit(path, conversationId ?? undefined).catch((e) => {
          if (!cancelled) setError(String(e));
          return null;
        }),
      ]);
      if (cancelled || !file || !host.current) return;

      defineTheme();
      modified.current = file.modified;
      setReadOnly(file.read_only);

      const model = monaco.editor.createModel(file.content, languageForPath(path));
      model.setEOL(eolOf(file.content));
      savedVersion.current = model.getAlternativeVersionId();

      const ed = monaco.editor.create(host.current, {
        model,
        theme: THEME,
        readOnly: !!file.read_only,
        automaticLayout: true,
        fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--font-mono").trim(),
        fontSize: 12.5,
        lineHeight: 20,
        // The pane is narrow and shares the window with a conversation, so
        // every pixel of chrome is one the code does not get.
        minimap: { enabled: false },
        overviewRulerLanes: 0,
        overviewRulerBorder: false,
        scrollBeyondLastLine: false,
        renderLineHighlight: "none",
        lineNumbersMinChars: 3,
        folding: true,
        padding: { top: 10, bottom: 10 },
        smoothScrolling: true,
        scrollbar: { verticalScrollbarSize: 8, horizontalScrollbarSize: 8, useShadows: false },
        // No language service is registered (see `lib/monaco.ts`), so the only
        // completions on offer are other words in the file. Useful while
        // typing an identifier, and never a popup claiming to know the API.
        quickSuggestions: false,
        wordBasedSuggestions: "currentDocument",
      });
      editorRef.current = ed;
      setReady(true);

      // Dirty is "the undo stack is somewhere other than the save point",
      // not "a keystroke happened" — so undoing back to the saved text
      // correctly clears the dot instead of leaving a false one.
      disposers.push(
        model.onDidChangeContent(() => {
          setUnsaved(path, model.getAlternativeVersionId() !== savedVersion.current);
        })
      );

      const save = async () => {
        if (file.read_only || !editorRef.current) return;
        const version = model.getAlternativeVersionId();
        setSaving(true);
        setError(null);
        try {
          const res = await writeTextFile(
            path,
            model.getValue(),
            conversationId ?? undefined,
            modified.current
          );
          modified.current = res.modified;
          savedVersion.current = version;
          setConflict(false);
          // Typing during the write leaves the buffer ahead of what landed on
          // disk; the version we compare against is the one we *sent*, so the
          // dot correctly stays on.
          setUnsaved(path, model.getAlternativeVersionId() !== version);
        } catch (e) {
          const msg = String(e);
          if (msg.includes("changed on disk")) setConflict(true);
          else setError(msg);
        } finally {
          setSaving(false);
        }
      };
      savers.set(path, save);

      ed.addAction({
        id: "poiesis.save",
        label: "Save",
        keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
        run: () => void save(),
      });
      disposers.push(ed);
      disposers.push(model);

      // The palette is CSS; Monaco's copy of it is not, so the theme has to be
      // rebuilt when the mode flips or the editor would keep the old one.
      const obs = new MutationObserver(() => defineTheme());
      obs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-mode"] });
      disposers.push({ dispose: () => obs.disconnect() });

      if (line) {
        ed.revealLineInCenter(line);
        ed.setPosition({ lineNumber: line, column: 1 });
      }
    })();

    return () => {
      cancelled = true;
      savers.delete(path);
      editorRef.current = null;
      // Reverse order: the editor lets go of the model before the model goes.
      for (const d of disposers.reverse()) d.dispose();
      disposers = [];
    };
    // `line` deliberately absent: reopening the same file at another line must
    // move the cursor, not tear the buffer down and lose the edits in it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, conversationId, setUnsaved]);

  // The agent writes to the same files these tabs hold open, and there is no
  // watcher — so a clean buffer would sit there showing yesterday's text with
  // nothing to hint that the file moved under it. Re-reading whenever the
  // window comes back catches that at the one moment the user is about to look
  // at it again. A *dirty* buffer is never touched: their edits win until they
  // say otherwise, and a save will raise the conflict bar if it comes to that.
  useEffect(() => {
    if (!ready || dirty) return;
    const sync = async () => {
      const ed = editorRef.current;
      const model = ed?.getModel();
      if (!model || useAppStore.getState().unsavedFiles[path]) return;
      try {
        const file = await readFileForEdit(path, conversationId ?? undefined);
        if (file.modified === modified.current || file.content === model.getValue()) return;
        // Position and folds survive an edit operation; `setValue` would reset
        // the view to the top of a file the user was reading halfway down.
        model.pushEditOperations(
          [],
          [{ range: model.getFullModelRange(), text: file.content }],
          () => null
        );
        modified.current = file.modified;
        savedVersion.current = model.getAlternativeVersionId();
        setUnsaved(path, false);
      } catch {
        // Gone or unreadable: the tab's own `gone` check in `ItemView` closes
        // it. Nothing useful to say from in here.
      }
    };
    window.addEventListener("focus", sync);
    return () => window.removeEventListener("focus", sync);
  }, [ready, dirty, path, conversationId, setUnsaved]);

  // A second open of an already-open file (a click in the tree, a search hit)
  // just moves the cursor.
  useEffect(() => {
    const ed = editorRef.current;
    if (!ready || !ed || !line) return;
    ed.revealLineInCenter(line);
    ed.setPosition({ lineNumber: line, column: 1 });
  }, [line, ready]);

  /** Take what is on disk now, discarding the buffer. */
  const reload = async () => {
    const ed = editorRef.current;
    if (!ed) return;
    try {
      const file = await readFileForEdit(path, conversationId ?? undefined);
      const model = ed.getModel();
      if (!model) return;
      // `pushEditOperations` rather than `setValue`, so the reload is one undo
      // step away rather than wiping the history along with the text.
      model.pushEditOperations(
        [],
        [{ range: model.getFullModelRange(), text: file.content }],
        () => null
      );
      modified.current = file.modified;
      savedVersion.current = model.getAlternativeVersionId();
      setUnsaved(path, false);
      setConflict(false);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  /** Keep the buffer and let it win, stamp and all. */
  const overwrite = async () => {
    const ed = editorRef.current;
    const model = ed?.getModel();
    if (!model) return;
    const version = model.getAlternativeVersionId();
    setSaving(true);
    try {
      const res = await writeTextFile(path, model.getValue(), conversationId ?? undefined);
      modified.current = res.modified;
      savedVersion.current = version;
      setConflict(false);
      setUnsaved(path, model.getAlternativeVersionId() !== version);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (error && !ready) return <div className="canvas-loading">{error}</div>;

  return (
    <div className="editor-wrap">
      {conflict && (
        <div className="editor-bar conflict" role="alert">
          <span>
            This file changed on disk while you were editing — most likely the agent wrote to it.
          </span>
          <button className="wb-link" onClick={() => void reload()} disabled={saving}>
            Use the disk version
          </button>
          <button className="wb-link" onClick={() => void overwrite()} disabled={saving}>
            Keep mine
          </button>
        </div>
      )}
      {readOnly && !conflict && (
        <div className="editor-bar note">
          <span>Read-only — {readOnly}</span>
        </div>
      )}
      {error && ready && !conflict && (
        <div className="editor-bar error" role="alert">
          <span>{error}</span>
        </div>
      )}
      <div className="editor-host" ref={host} />
      {/* A status line rather than a toolbar: the tab already carries the dot,
          and the pane header carries the Save button. This says what the file
          is doing right now. */}
      <div className="editor-status">
        {saving ? "Saving…" : dirty ? "Unsaved — ⌘S / Ctrl+S to save" : ready ? "Saved" : "Loading…"}
      </div>
    </div>
  );
}
