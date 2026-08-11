import { useEffect, useMemo, useRef, useState } from "react";
import { inTauri, pickFiles } from "../../lib/api";
import { useAppStore, useExpert, useSelectedModel } from "../../lib/store";
import { detectIntent } from "../../lib/mediaIntent";
import type { Attachment, Model } from "../../lib/types";
import ContextMeter from "./ContextMeter";
import ContextChip from "../Context/ContextChip";
import ModelPicker from "../ModelPicker/ModelPicker";
import ImageByPath from "../Conversation/ImageByPath";
import "./Composer.css";

/** Which nested panel of the `+` menu is showing, if any. */
type Submenu = "skills" | "start" | "persona";

const IMAGE_EXT = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

function kindFor(path: string): Attachment["kind"] | null {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (IMAGE_EXT.includes(ext)) return "image";
  if (ext === "pdf") return "pdf";
  return null;
}

const isMediaModel = (m: Model) => m.modality === "image" || m.modality === "video";

/** Durations to offer for a video model, capped by what it actually supports
 * (`PIK-4`) — an unsupported length is never offered rather than offered and
 * then remapped. */
function durationChoices(max: number): number[] {
  return [2, 4, 5, 6, 8, 10, 15, 20, 30].filter((d) => d <= max);
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
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const setWorkspaceMode = useAppStore((s) => s.setWorkspaceMode);
  // One entry point for everything the composer can do — attachments, modes,
  // skills and personas all hang off the `+`, with nested panels rather than a
  // row of competing buttons.
  const [menuOpen, setMenuOpen] = useState(false);
  const [submenu, setSubmenu] = useState<Submenu | null>(null);
  const startFromSkill = useAppStore((s) => s.startFromSkill);
  // Filter outside the selector, not inside it: zustand v5 compares snapshots
  // by identity, so a selector returning a fresh array re-renders forever.
  const skills = useAppStore((s) => s.skills);
  const enabledSkills = useMemo(() => skills.filter((sk) => sk.enabled), [skills]);
  const personas = useAppStore((s) => s.personas);
  const applyPersona = useAppStore((s) => s.applyPersona);
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const activePersonaId = useAppStore(
    (s) => s.conversations.find((c) => c.id === s.activeConversationId)?.personaId ?? ""
  );
  const inputRef = useRef<HTMLInputElement>(null);

  // ---- media: the declared route (`PIK-2`) ----
  const models = useAppStore((s) => s.models);
  const selected = useSelectedModel();
  const selectModel = useAppStore((s) => s.selectModel);
  const lastChatModelId = useAppStore((s) => s.lastChatModelId);
  const createMedia = useAppStore((s) => s.createMedia);
  const lastMediaArtifact = useAppStore((s) => s.lastMediaArtifact);
  const clearImplicitReference = useAppStore((s) => s.clearImplicitReference);

  /** `null` for an ordinary chat model — the composer everyone already knows.
   * Set the instant a media model is selected in the chooser (Path E). */
  const mediaTarget = isMediaModel(selected) ? (selected.modality as "image" | "video") : null;
  const [aspectRatio, setAspectRatio] = useState<string | undefined>(undefined);

  // `PIK-4`, Everything mode only. Every one of these is optional; the
  // disclosure that shows them is collapsed until asked for.
  const expert = useExpert();
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [resolution, setResolution] = useState<string | undefined>(undefined);
  const [durationSecs, setDurationSecs] = useState<number | undefined>(undefined);
  const [seed, setSeed] = useState<number | undefined>(undefined);
  const [steps, setSteps] = useState<number | undefined>(undefined);
  const [negative, setNegative] = useState("");
  const [reuseSeed, setReuseSeed] = useState(false);
  /** The seed the last picture actually came out with — what "reuse" reuses,
   * which is the whole reason iteration can be reproducible. */
  const lastSeed = useAppStore((s) => s.lastMediaSeed);

  useEffect(() => {
    // A fresh model's own first ratio, not whatever the previous one had —
    // an unsupported combination should never be silently carried over. Same
    // reasoning for every advanced knob: they belong to the model that was
    // selected when they were set.
    setAspectRatio(selected.supportedAspectRatios?.[0]);
    setResolution(selected.supportedResolutions?.[0]);
    setDurationSecs(undefined);
    setSteps(undefined);
    if (!reuseSeed) setSeed(undefined);
  }, [selected.id]);

  // ---- media: the inferred route (`PIK-3`) ----
  const [pinnedIntent, setPinnedIntent] = useState<"image" | "video" | null>(null);
  const [chipDismissed, setChipDismissed] = useState(false);

  // The media block's **Refine** (`STR-2`) reaches the composer through here:
  // it has already set the artifact as the implicit reference, so all that's
  // left is to pin the intent (or the reference chip wouldn't show), undo any
  // earlier dismissal, and take focus so the user can just start typing.
  const composerPin = useAppStore((s) => s.composerPin);
  const pinNonce = composerPin?.nonce;
  useEffect(() => {
    if (!composerPin) return;
    setPinnedIntent(composerPin.intent);
    setChipDismissed(false);
    inputRef.current?.focus();
    // Keyed on the nonce alone: refining the same artifact twice must re-fire.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pinNonce]);
  const detected = useMemo(() => detectIntent(value, attachments), [value, attachments]);
  const intent = pinnedIntent
    ? { intent: pinnedIntent, confidence: "high" as const }
    : detected;
  // A declaration always wins; inference never runs against Path E, and never
  // shows for a message that's actually a question ("chat").
  const chipModality: "image" | "video" | null =
    mediaTarget === null && !chipDismissed && intent.intent !== "chat"
      ? intent.intent === "edit"
        ? "image"
        : intent.intent
      : null;
  useEffect(() => {
    setChipDismissed(false);
  }, [value === "" ? "" : "typing"]);

  const chipCandidates = useMemo(
    () => (chipModality ? models.filter((m) => m.modality === chipModality) : []),
    [models, chipModality]
  );
  const [chipModelId, setChipModelId] = useState("");
  useEffect(() => {
    if (chipModality && chipCandidates.length > 0 && !chipCandidates.some((m) => m.id === chipModelId)) {
      setChipModelId(chipCandidates.find((m) => m.provenance === "local")?.id ?? chipCandidates[0].id);
    }
  }, [chipModality, chipCandidates, chipModelId]);

  // `EDT-2`: offer the previous picture as an implicit reference the instant
  // this message is itself heading for image/video generation — declared or
  // inferred — and something recent exists to refine. Always shown, never
  // silent: the whole point is that "make it warmer" is unambiguous to the
  // user, not just to the model.
  const wantsMedia = mediaTarget === "image" || chipModality === "image";
  const showImplicitRef =
    wantsMedia && !!lastMediaArtifact && lastMediaArtifact.conversationId === activeConversationId;

  function closeMenu() {
    setMenuOpen(false);
    setSubmenu(null);
  }

  // SKL-UI-2, the direct-invocation half of the standard: typing `/` opens the
  // skill list inline. The query is *derived* from the text rather than held in
  // an "is it open" flag — backspacing past the slash or typing a space closes
  // it on its own, so the menu can never be showing for text that isn't there.
  // Only a leading `/` counts: mid-sentence slashes are dates and paths.
  const slashQuery = useMemo(() => {
    const m = /^\/(\S*)$/.exec(value);
    return m ? m[1].toLowerCase() : null;
  }, [value]);

  const slashMatches = useMemo(
    () =>
      slashQuery === null
        ? []
        : enabledSkills.filter((s) => s.name.toLowerCase().includes(slashQuery)),
    [slashQuery, enabledSkills]
  );

  // Escape hides the list without destroying what was typed; typing again
  // (which changes the query) brings it back.
  const [slashDismissed, setSlashDismissed] = useState(false);
  const [slashIndex, setSlashIndex] = useState(0);
  useEffect(() => {
    setSlashIndex(0);
    setSlashDismissed(false);
  }, [slashQuery]);

  const slashOpen = slashQuery !== null && !slashDismissed && slashMatches.length > 0;

  function chooseSkill(skillName: string) {
    setValue(`/${skillName} `);
    setSlashDismissed(true);
    inputRef.current?.focus();
  }

  function backToChat() {
    selectModel(lastChatModelId);
  }

  function submit() {
    const text = value.trim();
    if (busy) return;

    if (mediaTarget !== null) {
      if (!text) return;
      createMedia({
        prompt: text,
        modelId: selected.id,
        aspectRatio,
        // Only in Everything mode: in Simple mode these stay unset, and the
        // backend picks its own defaults exactly as it does today (`PIK-4`).
        ...(expert
          ? {
              resolution,
              durationSecs,
              seed,
              steps,
              negative: negative.trim() || undefined,
            }
          : {}),
        references: showImplicitRef && lastMediaArtifact ? [lastMediaArtifact.path] : undefined,
        parentArtifactId: showImplicitRef && lastMediaArtifact ? lastMediaArtifact.id : undefined,
      });
      setValue("");
      setPinnedIntent(null);
      return;
    }

    if (chipModality !== null && chipModelId) {
      if (!text) return;
      createMedia({
        prompt: text,
        modelId: chipModelId,
        references: showImplicitRef && lastMediaArtifact ? [lastMediaArtifact.path] : undefined,
        parentArtifactId: showImplicitRef && lastMediaArtifact ? lastMediaArtifact.id : undefined,
      });
      setValue("");
      setPinnedIntent(null);
      setChipDismissed(false);
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

  const placeholder = showImplicitRef
    ? "Describe the change…"
    : mediaTarget === "video"
      ? "Describe a video…"
      : mediaTarget === "image"
        ? "Describe an image…"
        : "Message Poiesis Agent  ·  / for a skill  ·  paste or drop an image";

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

        {/* Path E's target bar (`PIK-2`): what sending does now, and one click
            back to a normal chat. */}
        {mediaTarget !== null && (
          <div className="media-target-bar">
            <span className="media-target-glyph" aria-hidden="true">◈</span>
            <span className="media-target-label">
              {mediaTarget === "video" ? "Video" : "Image"} · {selected.name}
            </span>
            {selected.supportedAspectRatios && selected.supportedAspectRatios.length > 1 && (
              <select
                className="media-target-ratio"
                aria-label="Aspect ratio"
                value={aspectRatio ?? selected.supportedAspectRatios[0]}
                onChange={(e) => setAspectRatio(e.target.value)}
              >
                {selected.supportedAspectRatios.map((r) => (
                  <option key={r} value={r}>
                    {r}
                  </option>
                ))}
              </select>
            )}
            {mediaTarget === "video" && selected.maxDurationSecs ? (
              <select
                className="media-target-ratio"
                aria-label="Duration"
                value={durationSecs ?? Math.min(5, selected.maxDurationSecs)}
                onChange={(e) => setDurationSecs(Number(e.target.value))}
              >
                {durationChoices(selected.maxDurationSecs).map((d) => (
                  <option key={d} value={d}>
                    {d}s
                  </option>
                ))}
              </select>
            ) : null}
            {expert && (
              <button
                className="media-target-more"
                aria-expanded={advancedOpen}
                onClick={() => setAdvancedOpen((v) => !v)}
              >
                {advancedOpen ? "Fewer" : "More"}
              </button>
            )}
            <button className="media-target-back" onClick={backToChat}>
              ← Back to chat
            </button>
          </div>
        )}

        {/* `PIK-4`, Everything mode only. Collapsed by default because none of
            it is needed to make a picture — it is here for the second, third
            and fourth attempt. The seed toggle is the one that matters: it is
            what turns "try again" into "same image, one change". */}
        {mediaTarget !== null && expert && advancedOpen && (
          <div className="media-advanced">
            {selected.supportedResolutions && selected.supportedResolutions.length > 0 && (
              <label className="media-adv-field">
                <span>Resolution</span>
                <select
                  value={resolution ?? selected.supportedResolutions[0]}
                  onChange={(e) => setResolution(e.target.value)}
                >
                  {selected.supportedResolutions.map((r) => (
                    <option key={r} value={r}>
                      {r}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="media-adv-field">
              <span>Seed</span>
              <input
                type="number"
                placeholder="random"
                value={seed ?? ""}
                onChange={(e) => setSeed(e.target.value === "" ? undefined : Number(e.target.value))}
              />
            </label>
            <label className="media-adv-check">
              <input
                type="checkbox"
                checked={reuseSeed}
                onChange={(e) => {
                  setReuseSeed(e.target.checked);
                  // Reusing means reusing the seed the last picture actually
                  // came out with, not whatever is typed here.
                  if (e.target.checked && lastSeed != null) setSeed(lastSeed);
                }}
              />
              <span>Reuse last seed</span>
            </label>
            {/* Steps and the negative prompt are local-engine knobs; a hosted
                model would only report them ignored. */}
            {selected.provenance === "local" && (
              <>
                <label className="media-adv-field">
                  <span>Steps</span>
                  <input
                    type="number"
                    min={1}
                    max={150}
                    placeholder="20"
                    value={steps ?? ""}
                    onChange={(e) => setSteps(e.target.value === "" ? undefined : Number(e.target.value))}
                  />
                </label>
                <label className="media-adv-field wide">
                  <span>Avoid</span>
                  <input
                    type="text"
                    placeholder="what to keep out of it"
                    value={negative}
                    onChange={(e) => setNegative(e.target.value)}
                  />
                </label>
              </>
            )}
          </div>
        )}

        {/* The inferred route's suggestion (`PIK-3`) — a suggestion, never a
            silent hijack, so it always names the model and offers a dismiss. */}
        {chipModality !== null && (
          <div className={`media-intent-chip ${intent.confidence === "low" ? "low-confidence" : ""}`}>
            <span className="mic-glyph" aria-hidden="true">{chipModality === "video" ? "🎬" : "🖼"}</span>
            <span className="mic-label">
              {intent.confidence === "low"
                ? `${chipModality === "video" ? "Video" : "Image"}?`
                : chipModality === "video"
                  ? "Video"
                  : "Image"}
            </span>
            {chipCandidates.length > 0 && (
              <select
                className="mic-model"
                aria-label="Model"
                value={chipModelId}
                onChange={(e) => setChipModelId(e.target.value)}
              >
                {chipCandidates.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                    {m.priceLabel ? ` · ${m.priceLabel}` : ""}
                  </option>
                ))}
              </select>
            )}
            <button
              className="mic-dismiss"
              onClick={() => {
                setChipDismissed(true);
                setPinnedIntent(null);
              }}
            >
              {intent.confidence === "low" ? "use anyway" : "Chat instead"}
            </button>
          </div>
        )}

        {/* The implicit reference (`EDT-2`): always shown before send, never
            silent — "make it warmer" is unambiguous to the user, not just the
            model. */}
        {showImplicitRef && lastMediaArtifact && (
          <div className="implicit-ref-chip">
            <ImageByPath path={lastMediaArtifact.path} className="implicit-ref-thumb" alt="Refining this image" />
            <span className="implicit-ref-label">↳ refining</span>
            <button
              className="implicit-ref-remove"
              aria-label="Don't refine from this image"
              onClick={clearImplicitReference}
            >
              ×
            </button>
          </div>
        )}

        <div className="composer">
          <div className="composer-menu-wrap">
            <button
              className={`icon-btn plus-btn ${menuOpen || workspaceMode ? "on" : ""}`}
              aria-label="Attach files, modes and skills"
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              title="Attach, modes & skills"
              onClick={() => {
                setSubmenu(null);
                setMenuOpen((v) => !v);
              }}
            >
              +
            </button>
            {menuOpen && (
              <>
                <div className="composer-menu-backdrop" onClick={closeMenu} />
                <div className="composer-menu" role="menu">
                  {submenu === null && (
                    <>
                      {/* Attaching is a thing you do, not a mode you're in, so
                          it keeps its own entry at the top. */}
                      <button
                        className="composer-menu-item"
                        role="menuitem"
                        onClick={() => {
                          closeMenu();
                          attach();
                        }}
                      >
                        <span className="mi-icon" aria-hidden="true">◱</span>
                        <span className="mi-body">
                          Attach files
                          <span className="mi-hint">images and PDFs — or just paste or drop one</span>
                        </span>
                        <span className="mi-check" />
                      </button>

                      <div className="composer-menu-sep" role="separator" />

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
                          closeMenu();
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
                          closeMenu();
                        }}
                      >
                        <span className="mi-icon" aria-hidden="true">⚒</span>
                        <span className="mi-body">
                          Tools
                          <span className="mi-hint">
                            {workspaceMode
                              ? "required by Workspace mode"
                              : "let the assistant use its enabled tools"}
                          </span>
                        </span>
                        <span className="mi-check">{toolsEnabled ? "✓" : ""}</span>
                      </button>
                      {/* `PIK-3`: explicit overrides, not a mode — pin the
                          intent for the next message only, same as typing
                          "draw…" would infer, but for a prompt that doesn't
                          say so itself ("a fox in a hat" with no verb). */}
                      <button
                        className="composer-menu-item"
                        role="menuitemcheckbox"
                        aria-checked={pinnedIntent === "image"}
                        onClick={() => {
                          setPinnedIntent((p) => (p === "image" ? null : "image"));
                          setChipDismissed(false);
                          closeMenu();
                          inputRef.current?.focus();
                        }}
                      >
                        <span className="mi-icon" aria-hidden="true">◲</span>
                        <span className="mi-body">
                          Create image
                          <span className="mi-hint">your next message generates a picture</span>
                        </span>
                        <span className="mi-check">{pinnedIntent === "image" ? "✓" : ""}</span>
                      </button>
                      <button
                        className="composer-menu-item"
                        role="menuitemcheckbox"
                        aria-checked={pinnedIntent === "video"}
                        onClick={() => {
                          setPinnedIntent((p) => (p === "video" ? null : "video"));
                          setChipDismissed(false);
                          closeMenu();
                          inputRef.current?.focus();
                        }}
                      >
                        <span className="mi-icon" aria-hidden="true">▶</span>
                        <span className="mi-body">
                          Create video
                          <span className="mi-hint">your next message generates a clip</span>
                        </span>
                        <span className="mi-check">{pinnedIntent === "video" ? "✓" : ""}</span>
                      </button>

                      {(enabledSkills.length > 0 || personas.length > 0) && (
                        <div className="composer-menu-sep" role="separator" />
                      )}

                      {/* SKL-UI-2: direct invocation half of the Agent Skills
                          standard — pick one, its name lands in the message. */}
                      {enabledSkills.length > 0 && (
                        <button
                          className="composer-menu-item"
                          role="menuitem"
                          aria-haspopup="menu"
                          onClick={() => setSubmenu("skills")}
                        >
                          <span className="mi-icon" aria-hidden="true">▦</span>
                          <span className="mi-body">
                            Skills
                            <span className="mi-hint">name one directly instead of waiting for it to fire</span>
                          </span>
                          <span className="mi-more" aria-hidden="true">›</span>
                        </button>
                      )}
                      {/* SKL-5, carrying RCP-UI-2 forward: a skill is also a way
                          to *start*, so it belongs where the other ways to start
                          a turn live. */}
                      {enabledSkills.length > 0 && (
                        <button
                          className="composer-menu-item"
                          role="menuitem"
                          aria-haspopup="menu"
                          onClick={() => setSubmenu("start")}
                        >
                          <span className="mi-icon" aria-hidden="true">◈</span>
                          <span className="mi-body">
                            Start from a skill
                            <span className="mi-hint">start a new chat and run one of my skills</span>
                          </span>
                          <span className="mi-more" aria-hidden="true">›</span>
                        </button>
                      )}
                      {personas.length > 0 && (
                        <button
                          className="composer-menu-item"
                          role="menuitem"
                          aria-haspopup="menu"
                          onClick={() => setSubmenu("persona")}
                        >
                          <span className="mi-icon" aria-hidden="true">◐</span>
                          <span className="mi-body">
                            Persona
                            <span className="mi-hint">
                              {personas.find((p) => p.id === activePersonaId)?.name ??
                                "who I am in this chat"}
                            </span>
                          </span>
                          <span className="mi-more" aria-hidden="true">›</span>
                        </button>
                      )}
                    </>
                  )}

                  {submenu !== null && (
                    <>
                      <button
                        className="composer-menu-back"
                        onClick={() => setSubmenu(null)}
                        aria-label="Back to the main menu"
                      >
                        <span aria-hidden="true">‹</span>
                        {submenu === "skills"
                          ? "Skills"
                          : submenu === "start"
                            ? "Start from a skill"
                            : "Persona"}
                      </button>
                      <div className="composer-submenu">
                        {submenu === "skills" &&
                          enabledSkills.map((s) => (
                            <button
                              className="composer-menu-item"
                              role="menuitem"
                              key={s.name}
                              onClick={() => {
                                closeMenu();
                                setValue((v) =>
                                  v.trim() ? `${v.trim()} /${s.name} ` : `/${s.name} `
                                );
                                inputRef.current?.focus();
                              }}
                            >
                              <span className="mi-icon" aria-hidden="true">▦</span>
                              <span className="mi-body">
                                {s.name}
                                <span className="mi-hint">{s.description}</span>
                              </span>
                              <span className="mi-check" />
                            </button>
                          ))}
                        {submenu === "start" &&
                          enabledSkills.map((s) => (
                            <button
                              className="composer-menu-item"
                              role="menuitem"
                              key={s.name}
                              onClick={() => {
                                closeMenu();
                                startFromSkill(s);
                              }}
                            >
                              <span className="mi-icon" aria-hidden="true">◈</span>
                              <span className="mi-body">
                                {s.name}
                                <span className="mi-hint">
                                  {s.when_to_use ? `when: ${s.when_to_use}` : s.description}
                                </span>
                              </span>
                              <span className="mi-check" />
                            </button>
                          ))}
                        {submenu === "persona" && (
                          <>
                            <button
                              className="composer-menu-item"
                              role="menuitemcheckbox"
                              aria-checked={!activePersonaId}
                              onClick={() => {
                                if (activeConversationId) applyPersona(activeConversationId, null);
                                closeMenu();
                              }}
                            >
                              <span className="mi-icon" aria-hidden="true">○</span>
                              <span className="mi-body">No persona</span>
                              <span className="mi-check">{!activePersonaId ? "✓" : ""}</span>
                            </button>
                            {personas.map((p) => (
                              <button
                                key={p.id}
                                className="composer-menu-item"
                                role="menuitemcheckbox"
                                aria-checked={activePersonaId === p.id}
                                onClick={() => {
                                  if (activeConversationId) applyPersona(activeConversationId, p.id);
                                  closeMenu();
                                }}
                              >
                                <span className="mi-icon" aria-hidden="true">◐</span>
                                <span className="mi-body">{p.name}</span>
                                <span className="mi-check">
                                  {activePersonaId === p.id ? "✓" : ""}
                                </span>
                              </button>
                            ))}
                          </>
                        )}
                      </div>
                    </>
                  )}
                </div>
              </>
            )}
          </div>
          <div className="composer-input-wrap">
            {slashOpen && (
              <div className="composer-menu composer-slash-menu" role="listbox" aria-label="Skills">
                {slashMatches.map((s, i) => (
                  <button
                    className={`composer-menu-item ${i === slashIndex ? "active" : ""}`}
                    role="option"
                    aria-selected={i === slashIndex}
                    key={s.name}
                    // The input's blur would fire before a click lands and
                    // close the menu out from under the pointer.
                    onMouseDown={(e) => e.preventDefault()}
                    onMouseEnter={() => setSlashIndex(i)}
                    onClick={() => chooseSkill(s.name)}
                  >
                    <span className="mi-icon" aria-hidden="true">▦</span>
                    <span className="mi-body">
                      {s.name}
                      <span className="mi-hint">
                        {s.when_to_use ? `when: ${s.when_to_use}` : s.description}
                      </span>
                    </span>
                    <span className="mi-check" />
                  </button>
                ))}
              </div>
            )}
            <input
              ref={inputRef}
              type="text"
              placeholder={placeholder}
              aria-label={mediaTarget === "video" ? "Describe a video to create" : mediaTarget === "image" ? "Describe an image to create" : "Message Poiesis Agent"}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onPaste={onPaste}
              autoComplete="off"
              role="combobox"
              aria-expanded={slashOpen}
              aria-controls="composer-slash-menu"
              onKeyDown={(e) => {
                // The skill list owns the arrows and Enter while it's open —
                // otherwise Enter would send "/we" as a message.
                if (slashOpen) {
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    setSlashIndex((i) => (i + 1) % slashMatches.length);
                    return;
                  }
                  if (e.key === "ArrowUp") {
                    e.preventDefault();
                    setSlashIndex((i) => (i - 1 + slashMatches.length) % slashMatches.length);
                    return;
                  }
                  if (e.key === "Enter" || e.key === "Tab") {
                    e.preventDefault();
                    chooseSkill(slashMatches[slashIndex].name);
                    return;
                  }
                  if (e.key === "Escape") {
                    e.preventDefault();
                    setSlashDismissed(true);
                    return;
                  }
                }
                if (e.key === "Escape" && !value) {
                  if (mediaTarget !== null) {
                    e.preventDefault();
                    backToChat();
                    return;
                  }
                  if (chipModality !== null) {
                    e.preventDefault();
                    setChipDismissed(true);
                    setPinnedIntent(null);
                    return;
                  }
                }
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  submit();
                }
              }}
            />
          </div>
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
        {/* Under the box: what I'm working from on the left, which model will
            answer on the right — both about the message, not the window.
            Everything here stays mounted in media mode too (`PIK-2`): making
            a picture is not leaving the conversation. */}
        <div className="composer-footer">
          <div className="cf-left">
            <ContextChip />
          </div>
          <div className="cf-right">
            <ContextMeter draft={value} />
            <ModelPicker compact dropUp />
          </div>
        </div>
      </div>
    </div>
  );
}
