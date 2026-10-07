import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import { inTauri, runtimeOverview, imageSetupStatus, voiceDownload } from "../../lib/api";
import { voiceNeeds, type VoiceNeeds } from "../../lib/voice/controller";
import "./OnboardingGuide.css";

/** `VOC-UI-7`: the optional step "Want to talk to me?". One button gets hearing
 * and a voice; it is never needed for anything else. */
function VoiceStep() {
  const [needs, setNeeds] = useState<VoiceNeeds | null | undefined>(undefined);
  const [percent, setPercent] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    voiceNeeds()
      .then(setNeeds)
      .catch(() => setNeeds(undefined));
  }, []);

  async function get() {
    if (!needs) return;
    setBusy(true);
    setFailed(false);
    try {
      const progress = (p: { received: number; total: number | null }) =>
        setPercent(p.total ? Math.round((p.received / p.total) * 100) : null);
      if (needs.hearingId) await voiceDownload("hearing", needs.hearingId, progress);
      if (needs.voiceModelId) await voiceDownload("voice", needs.voiceModelId, progress);
      setNeeds(null);
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  }

  // Unknown (not the desktop app, or the check failed): say nothing.
  if (needs === undefined) return null;
  const done = needs === null;
  const size = needs ? [needs.hearingId && needs.hearingSize, needs.voiceModelId && needs.voiceSize].filter(Boolean).join(" and ") : "";
  return (
    <li className={done ? "done" : "optional"}>
      <span className="onboarding-mark" aria-hidden="true">
        {done ? "✓" : "+"}
      </span>
      <span className="onboarding-body">
        <strong>Want to talk to me?</strong>
        <span>
          {done
            ? "Ready. Press the wave button next to the message box."
            : "Optional. I listen and speak right on your computer. Nothing is sent anywhere."}
        </span>
        {failed && <span>The download did not finish. Try again.</span>}
        {!done && (
          <button className="onboarding-action" onClick={get} disabled={busy}>
            {busy ? `Downloading${percent !== null ? ` ${percent}%` : "…"}` : `Get voice (${size.toLowerCase()}) →`}
          </button>
        )}
      </span>
    </li>
  );
}

/** A little floating checklist, not a blocking modal — shown any time the app
 * opens with nothing set up locally: no language model in the library and no
 * engine installed. A saved API key deliberately does *not* suppress it, since
 * the local path is still unconfigured; the key simply shows as already done.
 * Disappears as soon as a model lands, or when closed for this session. */
export default function OnboardingGuide() {
  // Deliberately not `bootstrapped`: that flips before the model lists load,
  // so gating on it flashed this guide on every launch — including for people
  // who already have a model or a key.
  const modelsLoaded = useAppStore((s) => s.modelsLoaded);
  const libraryModels = useAppStore((s) => s.libraryModels);
  const providers = useAppStore((s) => s.providers);
  const setView = useAppStore((s) => s.setView);

  const [dismissed, setDismissed] = useState(false);
  /** `null` until the probe answers — the guide must never judge on an
   * unknown, or it flashes before the engine status is back. */
  const [engineInstalled, setEngineInstalled] = useState<boolean | null>(null);
  const [imageModelInstalled, setImageModelInstalled] = useState(false);

  const hasChatModel = libraryModels.length > 0;
  const hasKey = providers.some((p) => p.key_set);

  useEffect(() => {
    if (!modelsLoaded || !inTauri()) return;
    // A failed probe counts as "not installed": if the runtime can't even be
    // inspected, guidance is more use than silence.
    runtimeOverview()
      .then((ov) => setEngineInstalled(ov.installed))
      .catch(() => setEngineInstalled(false));
    imageSetupStatus()
      .then((s) => setImageModelInstalled(s.model_installed))
      .catch(() => {});
  }, [modelsLoaded]);

  const needsSetup = modelsLoaded && inTauri() && !hasChatModel && engineInstalled === false;
  if (!needsSetup || dismissed) return null;

  const openModels = () => setView("models");

  return (
    <aside className="onboarding-guide" role="complementary" aria-label="Get Poiesis Agent running">
      <button className="onboarding-close" aria-label="Dismiss for now" onClick={() => setDismissed(true)}>
        ×
      </button>
      <h2 className="onboarding-title">Get Poiesis Agent running</h2>
      <p className="onboarding-lede">Pick one path — you don't need both.</p>

      <ol className="onboarding-steps">
        <li>
          <span className="onboarding-mark" aria-hidden="true">
            1
          </span>
          <span className="onboarding-body">
            <strong>Install the runtime</strong>
            <span>Downloads automatically the first time you get a model below.</span>
          </span>
        </li>
        <li>
          <span className="onboarding-mark" aria-hidden="true">
            2
          </span>
          <span className="onboarding-body">
            <strong>Download a language model</strong>
            <span>For chat, matched to your hardware.</span>
            <button className="onboarding-action" onClick={openModels}>
              Open Models →
            </button>
          </span>
        </li>
        <li className={imageModelInstalled ? "done" : "optional"}>
          <span className="onboarding-mark" aria-hidden="true">
            {imageModelInstalled ? "✓" : "+"}
          </span>
          <span className="onboarding-body">
            <strong>Download an image model</strong>
            <span>Optional — for pictures alongside chat.</span>
            {!imageModelInstalled && (
              <button className="onboarding-action" onClick={openModels}>
                Open Models →
              </button>
            )}
          </span>
        </li>
        <VoiceStep />
      </ol>

      <div className="onboarding-or" role="separator">
        or
      </div>

      <div className="onboarding-alt">
        <strong>{hasKey ? "API key saved ✓" : "Use your own API key"}</strong>
        <span>
          {hasKey
            ? "You can already chat through the cloud. A local model also works offline."
            : "Skip local downloads — chat through a provider you already have a key for."}
        </span>
        <button className="onboarding-action" onClick={() => setView("providers")}>
          {hasKey ? "Manage keys →" : "Add a key →"}
        </button>
      </div>
    </aside>
  );
}
