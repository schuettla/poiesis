import { useEffect, useState } from "react";
import { voiceDownload } from "../../lib/api";
import { hideVoiceSurface, leaveVoice, openVoice, voiceNeeds, voiceSession, type VoiceNeeds } from "../../lib/voice/controller";
import { useVoiceStore } from "../../lib/voice/voiceStore";
import { useAppStore } from "../../lib/store";
import Orb from "../Orb/Orb";
import { orbForFloor } from "../Orb/orbState";
import { MicIcon, MicOffIcon } from "../Icons/Icons";
import "./VoiceMode.css";

type Setup = { kind: "needed"; needs: VoiceNeeds } | { kind: "downloading"; percent: number | null } | null;

/** The line under the orb: who has the floor, in plain words (VXP-1, VXP-6). */
function stateLine(floor: string, step: string | null, muted: boolean): string {
  if (muted) return "Mic is muted";
  switch (floor) {
    case "starting":
      return "Getting ready";
    case "user_speaking":
    case "listening":
      return "Listening";
    case "thinking":
      return step ?? "Thinking";
    default:
      return "";
  }
}

/**
 * The voice conversation (`VOC-UI-3`): the orb is the face, the words are
 * captions, and the bar below holds what you may need to do. It covers the
 * conversation pane only, so the rail and top bar still say where you are.
 */
export default function VoiceMode() {
  const floor = useVoiceStore((s) => s.floor);
  const floorSince = useVoiceStore((s) => s.floorSince);
  const muted = useVoiceStore((s) => s.muted);
  const user = useVoiceStore((s) => s.user);
  const reply = useVoiceStore((s) => s.reply);
  const hint = useVoiceStore((s) => s.hint);
  const error = useVoiceStore((s) => s.error);
  const level = useVoiceStore((s) => (s.floor === "speaking" ? s.output : s.input));
  const [setup, setSetup] = useState<Setup>(null);
  const [now, setNow] = useState(() => Date.now());

  // The verb of the tool step that is running, for the line under the orb.
  const step = useAppStore((s) => {
    const conv = s.conversations.find((c) => c.id === s.activeConversationId);
    const last = conv?.messages[conv.messages.length - 1];
    const running = last?.role === "assistant" ? last.steps?.find((x) => x.status === "running") : undefined;
    return running ? `${running.verb}${running.target ? ` ${running.target}` : ""}` : null;
  });
  const artifactId = useAppStore((s) => {
    const conv = s.conversations.find((c) => c.id === s.activeConversationId);
    const last = conv?.messages[conv.messages.length - 1];
    const ids = last?.role === "assistant" ? last.artifactIds : undefined;
    return ids?.length ? ids[ids.length - 1] : null;
  });
  const artifactTitle = useAppStore((s) =>
    artifactId && s.activeConversationId
      ? s.artifacts[s.activeConversationId]?.find((a) => a.id === artifactId)?.title ?? null
      : null
  );
  const openArtifact = useAppStore((s) => s.openArtifact);
  const local = useAppStore((s) => s.models.find((m) => m.id === s.selectedModelId)?.provenance !== "cloud");

  async function start() {
    const result = await openVoice();
    if (result !== "setup") return;
    const needs = await voiceNeeds().catch(() => null);
    if (needs) setSetup({ kind: "needed", needs });
  }

  useEffect(() => {
    void start();
  }, []);

  // Escape stops the speech first; a second Escape leaves (VOC-UI-9).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const f = useVoiceStore.getState().floor;
      if (f === "speaking" || f === "thinking") voiceSession().stopSpeaking();
      else void leaveVoice();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Nobody has spoken for a while: the orb breathes (VXP-1, VXP-7).
  useEffect(() => {
    if (floor !== "listening") return;
    const id = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(id);
  }, [floor]);

  /** Gets what is missing, hearing first, then the voice. */
  async function download(needs: VoiceNeeds) {
    setSetup({ kind: "downloading", percent: null });
    try {
      const progress = (p: { received: number; total: number | null }) =>
        setSetup({ kind: "downloading", percent: p.total ? Math.round((p.received / p.total) * 100) : null });
      if (needs.hearingId) await voiceDownload("hearing", needs.hearingId, progress);
      if (needs.voiceModelId) await voiceDownload("voice", needs.voiceModelId, progress);
      setSetup(null);
      await start();
    } catch (e) {
      setSetup(null);
      useVoiceStore.setState({ error: typeof e === "string" ? e : "The download did not finish. Try again." });
    }
  }

  const quietMs = floor === "listening" ? now - floorSince : 0;
  const orb = floor === "off" || floor === "starting" ? "connecting" : orbForFloor(floor, quietMs, step);
  const shown = Math.min(1, level * 6);
  const speaking = floor === "speaking";
  const line = stateLine(floor, step, muted);
  const micOpen = floor !== "off" && floor !== "starting" && !muted;

  return (
    <div className="voice-mode" role="region" aria-label="Voice conversation">
      <div className="voice-center">
        <div className="voice-orb" style={{ ["--voice-level" as string]: shown }}>
          <Orb state={orb} size={64} />
        </div>
        <p className="voice-state" role="status" aria-live="polite">
          {line}
        </p>

        {setup ? (
          <div className="voice-note">
            {setup.kind === "needed" ? (
              <>
                <span>
                  To talk with Poiesis, it needs to download{" "}
                  {setup.needs.hearingId && setup.needs.voiceModelId
                    ? `its hearing (${setup.needs.hearingSize.toLowerCase()}) and a voice (${setup.needs.voiceSize.toLowerCase()})`
                    : setup.needs.hearingId
                      ? `its hearing (${setup.needs.hearingSize.toLowerCase()})`
                      : `a voice (${setup.needs.voiceSize.toLowerCase()})`}
                  . It stays on this computer.
                </span>
                <button className="voice-btn" onClick={() => void download(setup.needs)}>
                  Download
                </button>
              </>
            ) : (
              <span>Downloading{setup.percent !== null ? ` ${setup.percent}%` : "…"}</span>
            )}
          </div>
        ) : error ? (
          <div className="voice-note voice-note-error" role="alert">
            <span>{error}</span>
            <button className="voice-btn" onClick={() => void start()}>
              Try again
            </button>
          </div>
        ) : (
          <div className="voice-captions" aria-live="off">
            <p className={`voice-user${speaking ? " is-older" : ""}`}>{user}</p>
            <p className="voice-reply">{speaking ? reply : ""}</p>
          </div>
        )}
        {hint && !setup && !error && <p className="voice-hint">{hint}</p>}
      </div>

      {artifactId && artifactTitle && (
        <button
          className="voice-artifact"
          onClick={() => {
            void leaveVoice();
            openArtifact(artifactId);
          }}
        >
          {artifactTitle}
        </button>
      )}

      <div className="voice-bar">
        <button
          className={`icon-btn voice-mute${muted ? " on" : ""}`}
          aria-pressed={muted}
          aria-label={muted ? "Unmute the microphone" : "Mute the microphone"}
          title={muted ? "Unmute" : "Mute"}
          onClick={() => voiceSession().setMuted(!muted)}
        >
          {muted ? <MicOffIcon size={16} /> : <MicIcon size={16} />}
        </button>
        {(speaking || floor === "thinking") && (
          <button className="voice-btn" onClick={() => voiceSession().stopSpeaking()}>
            Stop
          </button>
        )}
        <button className="voice-btn" onClick={hideVoiceSurface}>
          Show chat
        </button>
        <button className="voice-btn" onClick={() => void leaveVoice()}>
          End
        </button>
        <span className="voice-privacy">{local ? "On this device" : "Voice on this device"}</span>
        {micOpen && (
          <span className="voice-live" role="status">
            <span className="voice-live-dot" aria-hidden="true" />
            Mic on
          </span>
        )}
      </div>
    </div>
  );
}
