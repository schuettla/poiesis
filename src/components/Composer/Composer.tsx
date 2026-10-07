import { useEffect, useMemo, useRef, useState } from "react";
import { inTauri, pickFiles, stillWorking } from "../../lib/api";
import { useAppStore, useExpert, useSelectedModel } from "../../lib/store";
import { detectIntent } from "../../lib/mediaIntent";
import type { Attachment, ContextRef, Model, SendOptions } from "../../lib/types";
import ContextMeter from "./ContextMeter";
import EffortPicker from "./EffortPicker";
import ContextChip from "../Context/ContextChip";
import ModelPicker from "../ModelPicker/ModelPicker";
import ImageByPath from "../Conversation/ImageByPath";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import CommandMenu from "./CommandMenu";
import PlusMenu from "./PlusMenu";
import ModifierChips from "./ModifierChips";
import ModeChips from "./ModeChips";
import RunBar from "./RunBar";
import SuggestionChip from "./SuggestionChip";
import BtwCard from "./BtwCard";
import { useCommandInput } from "./useCommandInput";
import MicButton from "../Voice/MicButton";
import { joinSpoken } from "../../lib/voice/dictation";
import "./Composer.css";

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
  onSend: (text: string, attachments?: Attachment[], opts?: SendOptions) => void;
  busy?: boolean;
  onStop?: () => void;
}) {
  const [value, setValue] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const modelNotice = useAppStore((s) => s.modelNotice);
  const dismissModelNotice = useAppStore((s) => s.dismissModelNotice);
  // `CMP-3`: things the `+` pointed at (an earlier chat, a library page) that
  // go out with the next message.
  const [refs, setRefs] = useState<ContextRef[]>([]);
  // `HRN-UI-1`: a live agent run is something you can talk to. Media jobs also
  // set `busy` but register no run, so this is what tells the two apart.
  const steerActiveRun = useAppStore((s) => s.steerActiveRun);
  const canSteer = useAppStore((s) => s.activeRun !== null);
  // `AGC-3`: while the run waits on a question, what you type answers it.
  const asking = useAppStore((s) => s.pendingQuestion !== null);
  // `SUB-UI-3`: how many agents the running turn has out right now. Stop takes
  // all of them, and the user has to know that before pressing it.
  const convId = useAppStore((s) => s.activeConversationId);
  const activeConversationId = convId;
  const subRunMap = useAppStore((s) => s.subRuns);
  const turnEffort = useAppStore((s) => s.turnModifiers.effort);
  const clearTurnModifier = useAppStore((s) => s.clearTurnModifier);
  const setTurnModifier = useAppStore((s) => s.setTurnModifier);
  const planFirstOn = useAppStore((s) => !!s.turnModifiers.planFirst);
  // `PLF-4`: "Change something" asks what to change, in the box.
  const planRevising = useAppStore((s) => s.planRevising);
  const composerRequest = useAppStore((s) => s.composerRequest);
  const agentsWorking = Object.values(subRunMap).filter(
    (r) => stillWorking(r.status) && r.parentConversationId === convId
  ).length;
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

  function sendWithExtras(text: string, opts?: SendOptions) {
    const extras: SendOptions = { ...opts, refs: refs.length ? refs : undefined };
    onSend(text, attachments, extras.skill || extras.refs ? extras : undefined);
    setValue("");
    setAttachments([]);
    setRefs([]);
  }

  // `CMP-1`: the `/` menu, its query, its keys and running a command live in one
  // hook. `submit` below asks it first whether the text is a command at all.
  const cmd = useCommandInput({
    value,
    setValue,
    focus: () => inputRef.current?.focus(),
    sendSkill: (text, skill, skillArgs) => sendWithExtras(text, { skill, skillArgs }),
    sendText: (text) => sendWithExtras(text),
    busy: !!busy,
  });

  // The `/` button, `Ctrl /`, and a mode chip's label all reach the composer as
  // a request for some text and focus. The nonce makes a repeat register.
  const requestNonce = composerRequest?.nonce;
  useEffect(() => {
    if (!composerRequest) return;
    setValue(composerRequest.text);
    inputRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestNonce]);

  // Accepting a suggestion (`AGC-2`) runs its command as if it had been typed.
  const commandRequest = useAppStore((s) => s.commandRequest);
  const commandNonce = commandRequest?.nonce;
  useEffect(() => {
    if (!commandRequest) return;
    cmd.tryRun(commandRequest.text);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [commandNonce]);

  function backToChat() {
    selectModel(lastChatModelId);
  }

  function submit() {
    const text = value.trim();
    // A command is not a message: run it and send nothing (`CMP-9`).
    if (text.startsWith("/") && cmd.tryRun(text)) return;
    if (busy) {
      // `HRN-UI-1`: while a run is working, Enter talks to it. The run reads
      // this at the top of its next iteration, so it lands between tool calls
      // rather than waiting for a turn that may be minutes away.
      if (canSteer && text) {
        setValue("");
        steerActiveRun(text).then((delivered) => {
          // The run ended in the gap. Nothing was queued, so send it as an
          // ordinary message instead of dropping what the user typed.
          if (!delivered) onSend(text);
        });
      }
      return;
    }

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

    if (!text && attachments.length === 0 && refs.length === 0) return;
    sendWithExtras(text);
  }

  function chooseFromPopover(i: number) {
    const line = cmd.model.lines.filter((l) => l.kind === "command")[i];
    if (line && line.kind === "command") cmd.chooseFromPopover(line.row.view);
  }

  function addAttachment(a: Attachment) {
    setAttachments((list) => [...list, a]);
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

  const placeholder = asking
    ? "Answer my question, in your own words or from the choices above"
    : planRevising
    ? "What should change in the plan?"
    : canSteer
    ? "Tell me something while I work"
    : showImplicitRef
    ? "Describe the change…"
    : mediaTarget === "video"
      ? "Describe a video…"
      : mediaTarget === "image"
        ? "Describe an image…"
        : "Message Poiesis Agent  ·  / for commands  ·  + to add files";

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
        {/* `RUN`: what the run is doing and did, above everything else here. */}
        <RunBar />
        {/* `AGC-2`: at most one, and only for what could run right now. */}
        <SuggestionChip />
        {/* `BTW-UI-1`: a side answer, outside the conversation. */}
        <BtwCard />
        {(attachments.length > 0 || refs.length > 0) && (
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
            {refs.map((r) => (
              <span className="attachment-chip" key={`${r.kind}:${r.id}`}>
                <span className="attachment-kind">{r.kind === "conversation" ? "↳" : "▤"}</span>
                {r.label}
                <button
                  className="attachment-remove"
                  aria-label={`Remove ${r.label}`}
                  onClick={() => setRefs((list) => list.filter((x) => x !== r))}
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

        {/* `CMP-6`: what the next message alone has been told to do. */}
        <ModifierChips />

        {/* `CMP-9`: a command that couldn't run says why, in the same quiet
            voice as the model notice. */}
        {cmd.error && (
          <div className="composer-model-notice" role="status">
            <span>{cmd.error}</span>
            <button className="implicit-ref-remove" aria-label="Dismiss" onClick={cmd.clearError}>
              ×
            </button>
          </div>
        )}

        {/* `MOD-3`: said once when the default model couldn't be used. */}
        {modelNotice && (
          <div className="composer-model-notice" role="status">
            <span>{modelNotice}</span>
            <button className="implicit-ref-remove" aria-label="Dismiss" onClick={dismissModelNotice}>
              ×
            </button>
          </div>
        )}

        <div className="composer">
          <PlusMenu
            onAttachFiles={attach}
            onAddAttachment={addAttachment}
            onAddRef={(r) => setRefs((list) => (list.some((x) => x.id === r.id) ? list : [...list, r]))}
            pending={attachments.length > 0 || refs.length > 0}
          />
          {/* `CMP-4`: `+` adds to the message, `/` does something. */}
          <button
            className={`icon-btn slash-btn ${cmd.model.open ? "on" : ""}`}
            aria-label="Commands"
            aria-haspopup="listbox"
            aria-expanded={cmd.model.open}
            title="Commands  ( / )"
            onClick={cmd.openFromButton}
          >
            /
          </button>
          <div className="composer-input-wrap">
            {cmd.model.popover && <div className="composer-menu-backdrop" onClick={cmd.closePopover} />}
            <CommandMenu
              model={cmd.model}
              onHover={cmd.setIndex}
              onChoose={(i) => (cmd.model.popover ? chooseFromPopover(i) : cmd.activateAt(i))}
              onFilter={cmd.model.popover ? cmd.setFilter : undefined}
            />
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
              aria-expanded={cmd.model.open}
              aria-controls={cmd.model.listId}
              aria-activedescendant={cmd.activeId}
              onKeyDown={(e) => {
                // The command list owns the arrows, Tab, Escape and Enter while
                // it is open — otherwise Enter would send "/we" as a message.
                if (cmd.onKeyDown(e)) return;
                // `PLF-5`: Shift+Tab is the one mode-cycling key. It flips the
                // Plan first chip for this message and does nothing else.
                if (e.key === "Tab" && e.shiftKey) {
                  e.preventDefault();
                  if (planFirstOn) clearTurnModifier("planFirst");
                  else setTurnModifier({ planFirst: true });
                  return;
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
          <MicButton
            onText={(t) => setValue((v) => joinSpoken(v, t))}
            onSendText={(t) => sendWithExtras(joinSpoken(value, t))}
            canSend={!busy}
          />
          {busy && canSteer && value.trim() ? (
            // Typing during a run means you have something to say to it, not
            // that you want it stopped — so the same key sends, and Stop is
            // one keystroke away again the moment the box is empty.
            <button
              className="icon-btn send"
              aria-label="Send this to the run"
              title="Send to the running agent"
              onClick={submit}
            >
              ↑
            </button>
          ) : busy ? (
            <button
              className="icon-btn send"
              aria-label="Stop generating"
              title={
                agentsWorking
                  ? `Stop me and the ${agentsWorking} agent${agentsWorking === 1 ? "" : "s"} I started`
                  : "Stop"
              }
              onClick={onStop}
            >
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
            <ModeChips />
          </div>
          <div className="cf-right">
            <ContextMeter draft={value} />
            {/* Beside the model, because it is a property of the answer that
                model is about to give. Hidden for an image or video model,
                where there is nothing to think about. */}
            {!mediaTarget && (
              <EffortPicker chip={turnEffort} onPick={() => clearTurnModifier("effort")} />
            )}
            <ModelPicker compact dropUp />
          </div>
        </div>
      </div>
      {cmd.confirm && (
        <ConfirmDialog
          title={cmd.confirm.title}
          body={cmd.confirm.body}
          confirmLabel={cmd.confirm.confirmLabel}
          onCancel={cmd.clearConfirm}
          onConfirm={() => {
            const run = cmd.confirm!.run;
            cmd.clearConfirm();
            void run();
          }}
        />
      )}
    </div>
  );
}
