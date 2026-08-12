import { useEffect, useState } from "react";
import { useAppStore } from "../../lib/store";
import { inTauri, runtimeOverview, imageSetupStatus } from "../../lib/api";
import "./OnboardingGuide.css";

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
            <strong>Install the engine</strong>
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
        <button className="onboarding-action" onClick={() => setView("settings")}>
          {hasKey ? "Manage keys →" : "Add a key →"}
        </button>
      </div>
    </aside>
  );
}
