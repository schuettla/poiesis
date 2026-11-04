import { useEffect, useRef, useState } from "react";
import {
  detectHardware,
  recommendedCatalog,
  listModels,
  inTauri,
  type CatalogEntry,
  type HardwareProfile,
} from "../lib/api";
import { useAppStore } from "../lib/store";
import { canChatNow, computerLine, filenameOf, pickFirstModel, sizeLabel } from "../lib/firstSteps";
import VoiceStep from "../components/Onboarding/VoiceStep";
import "./Surface.css";
import "./Models.css";
import "./FirstSteps.css";

/** 0 = not started, 1 = downloading the model, 2 = starting it. */
type Stage = 0 | 1 | 2;

/** The first-steps screen. Says what is needed to chat, and has one button that
 * does all of it: pick a model that fits this PC, download it, start it, open
 * the chat. Opens by itself the first time there is nothing to chat with, and
 * stays reachable from Settings. */
export default function FirstSteps() {
  const libraryModels = useAppStore((s) => s.libraryModels);
  const providers = useAppStore((s) => s.providers);
  const endpointModels = useAppStore((s) => s.endpointModels);
  const progress = useAppStore((s) => s.modelDownloads);
  const loadingModel = useAppStore((s) => s.loadingModel);
  const downloadCatalogModel = useAppStore((s) => s.downloadCatalogModel);
  const selectModel = useAppStore((s) => s.selectModel);
  const loadModelById = useAppStore((s) => s.loadModelById);
  const setView = useAppStore((s) => s.setView);

  const [hw, setHw] = useState<HardwareProfile | null>(null);
  const [catalog, setCatalog] = useState<CatalogEntry[] | null>(null);
  const [stage, setStage] = useState<Stage>(0);
  const [error, setError] = useState<string | null>(null);
  const desktop = inTauri();
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!desktop) return;
    Promise.all([detectHardware(), recommendedCatalog()])
      .then(([h, c]) => {
        setHw(h);
        setCatalog(c);
      })
      .catch(() => setCatalog([]));
  }, [desktop]);

  const busy = stage !== 0;
  // While the button runs, the model lands in the library and `ready` would flip
  // under it. The steps stay on screen until the run ends.
  const ready = canChatNow({ libraryModels, providers, endpointModels }) && !busy;
  const pick = catalog ? pickFirstModel(catalog) : null;
  const percent = pick ? progress[pick.id] : undefined;
  const hasKey = providers.some((p) => p.key_set);

  async function doItAll() {
    if (!pick || busy) return;
    setError(null);
    setStage(1);
    let at: Stage = 1;
    try {
      await downloadCatalogModel(pick);
      const fresh = await listModels();
      const file = filenameOf(pick.url);
      const entry = fresh.find((m) => m.path.split(/[\\/]/).pop() === file);
      if (!entry) {
        throw new Error(
          "I could not find the downloaded model yet. If a download is still running, wait for it, then press the button again."
        );
      }
      at = 2;
      setStage(2);
      // Choosing a local model starts it (and sets up the runtime the first time).
      // Waiting on the same load here keeps the progress and any error on this screen.
      selectModel(entry.id);
      await loadModelById(entry.id);
      if (mounted.current) setView("chat");
    } catch (e) {
      const text = String(e).replace(/^Error:\s*/, "");
      setError(at === 2 ? `The model is downloaded, but it did not start. ${text}` : text);
    } finally {
      if (mounted.current) setStage(0);
    }
  }

  const stepState = (n: 1 | 2 | 3): "done" | "active" | "" => {
    if (n === 1) return hw ? "done" : "";
    if (n === 2) return stage === 2 ? "done" : stage === 1 ? "active" : "";
    return stage === 2 ? "active" : "";
  };

  return (
    <div className="surface first-steps">
      <div className="surface-inner">
        <h1>First steps</h1>
        <p className="fs-lede">
          To talk with you, Poiesis needs a <strong>language model</strong>. Think of it as the brain that writes the
          answers. You can run one on this computer for free, or use an online account you already have.
        </p>

        {error && (
          <p className="fs-error" role="alert">
            {error}
          </p>
        )}

        {ready ? (
          <section className="fs-card fs-ready">
            <h2 className="fs-card-title">You are ready to chat</h2>
            <p className="fs-card-body">
              {libraryModels.some((m) => !m.role || m.role === "chat")
                ? "A model is on this computer. It works offline."
                : hasKey
                  ? "Your online account is connected."
                  : "Your connected server is ready."}
            </p>
            <button className="btn-primary big" onClick={() => setView("chat")}>
              Open chat
            </button>
          </section>
        ) : (
          <>
            <section className="fs-card fs-main">
              <h2 className="fs-card-title">The easy way: let me do it</h2>
              <p className="fs-card-body">
                One button. I pick a free model that fits this computer, download it, start it and open your first
                chat. You do not change any setting.
              </p>

              <h3 className="fs-sub">What you need</h3>
              <ul className="fs-needs">
                <li>An internet connection, only for the download.</li>
                <li>{pick ? `About ${sizeLabel(pick.size_mb)} of free disk space.` : "Some free disk space."}</li>
                <li>A few minutes. It depends on your connection.</li>
                <li>Nothing else. No account, no key, no payment.</li>
              </ul>

              <h3 className="fs-sub">What happens</h3>
              <ol className="fs-steps">
                <li className={stepState(1)}>
                  <span className="fs-mark" aria-hidden="true">
                    {stepState(1) === "done" ? "✓" : "1"}
                  </span>
                  <span className="fs-body">
                    <strong>Look at this computer</strong>
                    <span>{hw ? computerLine(hw) : "Checking…"}</span>
                  </span>
                </li>
                <li className={stepState(2)}>
                  <span className="fs-mark" aria-hidden="true">
                    {stepState(2) === "done" ? "✓" : "2"}
                  </span>
                  <span className="fs-body">
                    <strong>Download a model that fits</strong>
                    <span>
                      {pick
                        ? `${pick.name}, ${sizeLabel(pick.size_mb)}. Free, private, stays on this computer.`
                        : catalog
                          ? "Nothing in the free list fits this computer."
                          : "Looking for the best one…"}
                    </span>
                    {stage === 1 && (
                      <div className="dl-progress wide">
                        <div className="dl-bar" style={{ width: `${typeof percent === "number" ? percent : 0}%` }} />
                        <span className="dl-pct">
                          {typeof percent === "number" ? `Downloading, ${percent}%` : "Starting the download…"}
                        </span>
                      </div>
                    )}
                  </span>
                </li>
                <li className={stepState(3)}>
                  <span className="fs-mark" aria-hidden="true">
                    {stepState(3) === "done" ? "✓" : "3"}
                  </span>
                  <span className="fs-body">
                    <strong>Start it and open your first chat</strong>
                    <span>
                      {stage === 2
                        ? (loadingModel?.label ?? "Starting…")
                        : "The first time, this also sets up the small program that runs the model (about 60 MB)."}
                    </span>
                  </span>
                </li>
              </ol>

              {!desktop ? (
                <button className="btn-primary big" disabled>
                  Available in the desktop app
                </button>
              ) : catalog && !pick ? (
                <p className="fs-card-body">
                  This computer is too small for the free models. Use an online account below instead.
                </p>
              ) : (
                <button className="btn-primary big" onClick={doItAll} disabled={busy || !pick}>
                  {busy ? "Working on it…" : error ? "Try again" : "Do it all for me"}
                </button>
              )}
            </section>

            <section className="fs-card">
              <h2 className="fs-card-title">Or use an account you have</h2>
              <p className="fs-card-body">
                Already have an OpenAI, Anthropic or OpenRouter account? Add its key and chat right away. Nothing is
                downloaded.
              </p>
              <button className="btn-secondary big" onClick={() => setView("providers")} disabled={busy}>
                Add an account →
              </button>
            </section>

            <p className="fs-skip">
              <button className="fs-link" onClick={() => setView("chat")} disabled={busy}>
                Skip for now
              </button>
            </p>
          </>
        )}

        <section className="fs-later">
          <h2 className="fs-card-title">Later, if you want</h2>
          <ul className="fs-steps">
            <VoiceStep />
            <li>
              <span className="fs-mark" aria-hidden="true">
                +
              </span>
              <span className="fs-body">
                <strong>Make pictures</strong>
                <span>Download an image model for pictures next to your chat.</span>
                <button className="fs-link" onClick={() => setView("models")}>
                  Open Models →
                </button>
              </span>
            </li>
          </ul>
        </section>
      </div>
    </div>
  );
}
