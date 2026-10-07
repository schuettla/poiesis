import { useCallback, useEffect, useRef, useState } from "react";
import { inTauri, voiceCatalog, voiceDownload, voiceSettings, voiceStatus, voiceTranscribe } from "../../lib/api";
import { VoiceAudio, MicError } from "../../lib/voice/audio";
import { Dictation } from "../../lib/voice/dictation";
import { MicIcon } from "../Icons/Icons";
import "./MicButton.css";

/** A press longer than this is push-to-talk: it sends when you let go. */
const HOLD_MS = 300;

type Phase = "idle" | "starting" | "listening" | "hearing";
type Setup = { kind: "needed"; size: string } | { kind: "downloading"; percent: number | null } | null;

/**
 * The mic in the composer (VOC-UI-1). Tap to dictate, tap again to put the
 * words in the box. Hold to talk: the words are sent when you let go. The mic
 * is open only while the ring shows (VXP-3): leaving the window, pressing
 * Escape, or unmounting closes it.
 */
export default function MicButton({
  onText,
  onSendText,
  canSend,
}: {
  /** Put dictated words in the message box. */
  onText: (text: string) => void;
  /** Send dictated words as the message (hold to talk). */
  onSendText: (text: string) => void;
  /** False while a reply is being written: held speech goes to the box instead. */
  canSend: boolean;
}) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [setup, setSetup] = useState<Setup>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const dictation = useRef<Dictation | null>(null);
  const pressedAt = useRef(0);
  const holding = useRef(false);

  const cancel = useCallback(() => {
    dictation.current?.cancel();
    dictation.current = null;
    holding.current = false;
    setPhase("idle");
    buttonRef.current?.style.setProperty("--mic-level", "0");
  }, []);

  // Leaving the window or the page closes the mic (VXP-3).
  useEffect(() => {
    if (phase !== "listening" && phase !== "starting") return;
    const onHidden = () => document.hidden && cancel();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && cancel();
    window.addEventListener("blur", cancel);
    document.addEventListener("visibilitychange", onHidden);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("blur", cancel);
      document.removeEventListener("visibilitychange", onHidden);
      window.removeEventListener("keydown", onKey);
    };
  }, [phase, cancel]);
  useEffect(() => cancel, [cancel]);

  async function begin(): Promise<boolean> {
    setNotice(null);
    if (!inTauri()) {
      setNotice("Voice works in the desktop app.");
      return false;
    }
    const status = await voiceStatus();
    if (!status.hearing) {
      const [catalog, settings] = await Promise.all([voiceCatalog(), voiceSettings()]);
      setSetup({ kind: "needed", size: catalog.hearing.find((h) => h.id === settings.hearing_model)?.size_label ?? "" });
      return false;
    }
    setSetup(null);
    const d = new Dictation(
      (events) =>
        new VoiceAudio({
          onFrame: events.onFrame,
          onLevel: (input) =>
            buttonRef.current?.style.setProperty("--mic-level", String(Math.min(1, input * 6))),
        }),
      voiceTranscribe,
      { onLimit: () => void finish(false) },
    );
    dictation.current = d;
    setPhase("starting");
    try {
      await d.start();
    } catch (e) {
      dictation.current = null;
      setPhase("idle");
      setNotice(e instanceof MicError ? e.message : "The microphone could not be started.");
      return false;
    }
    // Let go (or cancelled) while the mic was still opening.
    if (dictation.current !== d) {
      d.cancel();
      return false;
    }
    setPhase("listening");
    return true;
  }

  async function finish(send: boolean) {
    const d = dictation.current;
    if (!d) return;
    dictation.current = null;
    holding.current = false;
    setPhase("hearing");
    buttonRef.current?.style.setProperty("--mic-level", "0");
    try {
      const text = await d.finish();
      if (!text) setNotice("I did not hear anything.");
      else if (send && canSend) onSendText(text);
      else onText(text);
    } catch (e) {
      setNotice(typeof e === "string" ? e : e instanceof Error ? e.message : "That did not work.");
    } finally {
      setPhase("idle");
    }
  }

  async function onPointerDown(e: React.PointerEvent) {
    if (e.button !== 0 || phase === "hearing" || phase === "starting") return;
    if (phase === "listening") {
      await finish(false); // second tap: put the words in the box
      return;
    }
    pressedAt.current = Date.now();
    holding.current = true;
    await begin();
  }

  function onPointerUp() {
    if (!holding.current) return;
    holding.current = false;
    // A long press is push-to-talk; a short one leaves the mic open until the next tap.
    if (Date.now() - pressedAt.current > HOLD_MS && dictation.current) void finish(true);
  }

  async function download() {
    const settings = await voiceSettings();
    setSetup({ kind: "downloading", percent: null });
    try {
      await voiceDownload("hearing", settings.hearing_model, (p) =>
        setSetup({ kind: "downloading", percent: p.total ? Math.round((p.received / p.total) * 100) : null }),
      );
      setSetup(null);
      setNotice("Ready. Tap the mic and talk.");
    } catch (e) {
      setSetup(null);
      setNotice(typeof e === "string" ? e : "The download did not finish. Try again.");
    }
  }

  const listening = phase === "listening";
  const label = listening ? "Stop listening" : phase === "hearing" ? "Writing out what you said" : "Talk instead of typing";
  return (
    <div className="mic-wrap">
      {(setup || notice) && (
        <div className="mic-pop" role="status">
          {setup?.kind === "needed" && (
            <>
              <span>
                To use your voice, Poiesis needs to download its hearing ({setup.size}). It stays on this computer.
              </span>
              <button className="mic-pop-btn" onClick={download}>
                Download
              </button>
            </>
          )}
          {setup?.kind === "downloading" && (
            <span>Downloading{setup.percent !== null ? ` ${setup.percent}%` : "…"}</span>
          )}
          {!setup && notice && <span>{notice}</span>}
          {setup?.kind !== "downloading" && (
            <button
              className="mic-pop-close"
              aria-label="Dismiss"
              onClick={() => {
                setSetup(null);
                setNotice(null);
              }}
            >
              ×
            </button>
          )}
        </div>
      )}
      <button
        ref={buttonRef}
        className={`icon-btn mic-btn${listening ? " on" : ""}${phase === "hearing" ? " busy" : ""}`}
        aria-label={label}
        aria-pressed={listening}
        title={listening ? "Listening. Tap to stop." : "Talk (tap, or hold to talk and send)"}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
        onKeyDown={(e) => {
          // Keyboard: Space or Enter toggles dictation (no hold gesture).
          if (e.key !== "Enter" && e.key !== " ") return;
          e.preventDefault();
          if (phase === "listening") void finish(false);
          else if (phase === "idle") void begin();
        }}
      >
        <MicIcon size={16} />
        {listening && <span className="mic-dot" aria-hidden="true" />}
      </button>
    </div>
  );
}
