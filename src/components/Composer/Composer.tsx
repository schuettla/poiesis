import { useEffect, useState } from "react";
import { inTauri, listImageModels, pickFiles, type ImageModel } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import type { Attachment } from "../../lib/types";
import ContextMeter from "./ContextMeter";
import "./Composer.css";

const IMAGE_EXT = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

function kindFor(path: string): Attachment["kind"] | null {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (IMAGE_EXT.includes(ext)) return "image";
  if (ext === "pdf") return "pdf";
  return null;
}

export default function Composer({
  onSend,
  busy,
  onStop,
}: {
  onSend: (text: string, attachments?: Attachment[]) => void;
  busy?: boolean;
  onStop?: () => void;
}) {
  const [value, setValue] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const toolsEnabled = useAppStore((s) => s.toolsEnabled);
  const setToolsEnabled = useAppStore((s) => s.setToolsEnabled);
  const imageMode = useAppStore((s) => s.imageMode);
  const setImageMode = useAppStore((s) => s.setImageMode);
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const setWorkspaceMode = useAppStore((s) => s.setWorkspaceMode);
  const [menuOpen, setMenuOpen] = useState(false);
  // The drop-up becomes a recipe list in place, rather than opening a submenu
  // over a menu — one surface at a time (RCP-UI-2).
  const [recipePicker, setRecipePicker] = useState(false);
  const recipes = useAppStore((s) => s.recipes);
  const startFromRecipe = useAppStore((s) => s.startFromRecipe);
  const createImage = useAppStore((s) => s.createImage);
  const [imageModels, setImageModels] = useState<ImageModel[]>([]);
  const [imageModel, setImageModel] = useState("");
  const personas = useAppStore((s) => s.personas);
  const applyPersona = useAppStore((s) => s.applyPersona);
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const activePersonaId = useAppStore(
    (s) => s.conversations.find((c) => c.id === s.activeConversationId)?.personaId ?? ""
  );

  // Load the available image models when Create-image mode is switched on.
  useEffect(() => {
    if (!imageMode || !inTauri()) return;
    listImageModels()
      .then((ms) => {
        setImageModels(ms);
        setImageModel((prev) => prev || ms.find((m) => m.is_default)?.path || ms[0]?.path || "");
      })
      .catch(() => {});
  }, [imageMode]);

  function submit() {
    const text = value.trim();
    if (busy) return;
    if (imageMode) {
      if (!text) return;
      createImage(text, imageModel || undefined);
      setValue("");
      return;
    }
    if (!text && attachments.length === 0) return;
    onSend(text, attachments);
    setValue("");
    setAttachments([]);
  }

  // Pasted / dropped images carry their bytes inline (no filesystem path).
  function addImageFile(file: File) {
    if (!file.type.startsWith("image/")) return;
    const reader = new FileReader();
    reader.onload = () => {
      const dataUri = reader.result as string;
      setAttachments((a) => [
        ...a,
        {
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          kind: "image",
          name: file.name || "pasted-image.png",
          path: "",
          dataUri,
        },
      ]);
    };
    reader.readAsDataURL(file);
  }

  function onDrop(e: React.DragEvent) {
    e.preventDefault();
    setDragOver(false);
    for (const f of Array.from(e.dataTransfer.files)) addImageFile(f);
  }

  function onPaste(e: React.ClipboardEvent) {
    for (const item of Array.from(e.clipboardData.items)) {
      if (item.type.startsWith("image/")) {
        const f = item.getAsFile();
        if (f) addImageFile(f);
      }
    }
  }

  async function attach() {
    if (!inTauri()) return;
    // Routed through the backend picker so the chosen paths are recorded as
    // consent — reading them back later goes through the same scope check as
    // everything else that touches the user's disk.
    const picked = await pickFiles();
    setAttachments((a) => [
      ...a,
      ...picked.flatMap((path, i) => {
        const kind = kindFor(path);
        if (!kind) return [];
        return [{ id: `${Date.now()}-${i}`, kind, name: path.split(/[\\/]/).pop() ?? path, path }];
      }),
    ]);
  }

  function removeAttachment(id: string) {
    setAttachments((a) => a.filter((x) => x.id !== id));
  }

  return (
    <div
      className={`composer-wrap ${dragOver ? "drag-over" : ""}`}
      onDragOver={(e) => {
        e.preventDefault();
        if (!dragOver) setDragOver(true);
      }}
      onDragLeave={(e) => {
        if (e.currentTarget === e.target) setDragOver(false);
      }}
      onDrop={onDrop}
    >
      <div className="composer-col">
        {attachments.length > 0 && (
          <div className="attachment-row">
            {attachments.map((a) => (
              <span className="attachment-chip" key={a.id}>
                <span className="attachment-kind">{a.kind === "image" ? "▣" : "▤"}</span>
                {a.name}
                <button
                  className="attachment-remove"
                  aria-label={`Remove ${a.name}`}
                  onClick={() => removeAttachment(a.id)}
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="composer">
          <button
            className="icon-btn"
            aria-label="Attach an image or PDF"
            title="Attach an image or PDF"
            onClick={attach}
          >
            +
          </button>
          <div className="composer-menu-wrap">
            <button
              className={`icon-btn tools-toggle ${workspaceMode || imageMode ? "on" : ""}`}
              aria-label="Modes and tools"
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              title="Modes & tools"
              onClick={() => {
                setRecipePicker(false);
                setMenuOpen((v) => !v);
              }}
            >
              {workspaceMode ? "▦" : imageMode ? "◲" : "⌁"}
            </button>
            {menuOpen && (
              <>
                <div
                  className="composer-menu-backdrop"
                  onClick={() => {
                    setMenuOpen(false);
                    setRecipePicker(false);
                  }}
                />
                <div className="composer-menu" role="menu">
                  <button
                    className="composer-menu-item"
                    role="menuitemcheckbox"
                    aria-checked={workspaceMode}
                    onClick={() => {
                      const next = !workspaceMode;
                      setWorkspaceMode(next);
                      // Workspace needs the render_ui tool — turning the mode on
                      // force-enables tools so the surface can actually compose.
                      if (next) setToolsEnabled(true);
                      setMenuOpen(false);
                    }}
                  >
                    <span className="mi-icon" aria-hidden="true">▦</span>
                    <span className="mi-body">
                      Workspace mode
                      <span className="mi-hint">the agent composes a live interface; chat becomes the log</span>
                    </span>
                    <span className="mi-check">{workspaceMode ? "✓" : ""}</span>
                  </button>
                  <button
                    className="composer-menu-item"
                    role="menuitemcheckbox"
                    aria-checked={toolsEnabled}
                    aria-disabled={workspaceMode}
                    disabled={workspaceMode}
                    title={workspaceMode ? "Required by Workspace mode" : undefined}
                    onClick={() => {
                      // Locked on while workspace mode is active — the workspace
                      // can't compose without the render_ui tool.
                      if (workspaceMode) return;
                      setToolsEnabled(!toolsEnabled);
                      setMenuOpen(false);
                    }}
                  >
                    <span className="mi-icon" aria-hidden="true">⚒</span>
                    <span className="mi-body">
                      Tools
                      <span className="mi-hint">
                        {workspaceMode
                          ? "required by Workspace mode"
                          : "let the assistant use its enabled skills"}
                      </span>
                    </span>
                    <span className="mi-check">{toolsEnabled ? "✓" : ""}</span>
                  </button>
                  <button
                    className="composer-menu-item"
                    role="menuitemcheckbox"
                    aria-checked={imageMode}
                    onClick={() => {
                      setImageMode(!imageMode);
                      setMenuOpen(false);
                    }}
                  >
                    <span className="mi-icon" aria-hidden="true">◲</span>
                    <span className="mi-body">
                      Create image
                      <span className="mi-hint">your next message generates a picture</span>
                    </span>
                    <span className="mi-check">{imageMode ? "✓" : ""}</span>
                  </button>
                  {/* RCP-UI-2: a saved procedure is a way to *start*, so it
                      belongs where the other ways to start a turn live. */}
                  {recipes.length > 0 && !recipePicker && (
                    <button
                      className="composer-menu-item"
                      role="menuitem"
                      onClick={() => setRecipePicker(true)}
                    >
                      <span className="mi-icon" aria-hidden="true">◈</span>
                      <span className="mi-body">
                        Start from recipe…
                        <span className="mi-hint">
                          open a new workspace and follow a procedure we kept
                        </span>
                      </span>
                      <span className="mi-check" />
                    </button>
                  )}
                  {recipePicker &&
                    recipes.map((r) => (
                      <button
                        className="composer-menu-item"
                        role="menuitem"
                        key={r.name}
                        onClick={() => {
                          setMenuOpen(false);
                          setRecipePicker(false);
                          startFromRecipe(r);
                        }}
                      >
                        <span className="mi-icon" aria-hidden="true">
                          {r.surface_json ? "▦" : "◈"}
                        </span>
                        <span className="mi-body">
                          {r.name}
                          <span className="mi-hint">when: {r.trigger}</span>
                        </span>
                        <span className="mi-check" />
                      </button>
                    ))}
                </div>
              </>
            )}
          </div>
          {imageMode && imageModels.length > 0 && (
            <select
              className="persona-picker"
              aria-label="Image model"
              title="Image model"
              value={imageModel}
              onChange={(e) => setImageModel(e.target.value)}
            >
              {imageModels.map((m) => (
                <option key={m.path} value={m.path}>
                  {m.name}
                </option>
              ))}
            </select>
          )}
          {!imageMode && personas.length > 0 && (
            <select
              className="persona-picker"
              aria-label="Persona for this chat"
              title="Persona for this chat"
              value={activePersonaId}
              onChange={(e) =>
                activeConversationId &&
                applyPersona(activeConversationId, e.target.value || null)
              }
            >
              <option value="">No persona</option>
              {personas.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          )}
          <input
            type="text"
            placeholder={
              imageMode
                ? imageModels.length === 0
                  ? "Get an image model in Models → Image first"
                  : "Describe an image to create…"
                : "Message Poiesis Agent  ·  paste or drop an image"
            }
            aria-label={imageMode ? "Describe an image to create" : "Message Poiesis Agent"}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onPaste={onPaste}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submit();
              }
            }}
          />
          {!imageMode && <ContextMeter draft={value} />}
          {busy ? (
            <button className="icon-btn send" aria-label="Stop generating" title="Stop" onClick={onStop}>
              ■
            </button>
          ) : (
            <button className="icon-btn send" aria-label="Send message" title="Send" onClick={submit}>
              ↑
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
