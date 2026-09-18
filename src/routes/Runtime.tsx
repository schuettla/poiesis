import { useCallback, useEffect, useState } from "react";
import {
  runtimeOverview,
  setBackendOverride,
  checkRuntimeUpdate,
  inTauri,
  type RuntimeOverview,
  type UpdateInfo,
} from "../lib/api";
import { useAppStore } from "../lib/store";
import ImageRuntime from "../components/ImageModels/ImageRuntime";
import RecallRuntime from "../components/RecallRuntime/RecallRuntime";
import YourServers from "../components/YourServers/YourServers";
import type { RuntimeTab } from "../lib/types";
import "./Surface.css";
import "./Models.css";
import "./Runtime.css";

function formatVram(mb: number | null): string {
  if (!mb) return "";
  return mb >= 1024 ? `${(mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1)} GB` : `${mb} MB`;
}
function basename(path: string | null): string {
  if (!path) return "";
  return path.split(/[\\/]/).pop() ?? path;
}

export default function Runtime() {
  // `RTM-8`: Chat · Images · Your servers · Recall. `runtimeTab` is the deep
  // link (the picker and Models' "Your server" groups open `servers`).
  const runtimeTab = useAppStore((s) => s.runtimeTab);
  const [tab, setTab] = useState<RuntimeTab>(runtimeTab ?? "chat");
  const [ov, setOv] = useState<RuntimeOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  // SMP-1c: the recall engine's internals are expert-only. Simple mode still
  // gets recall itself (SMP-2 asks inline, the first time it's needed) — this
  // only hides the install/model-picker card, not the capability.
  const expert = useAppStore((s) => s.expert);

  const libraryModels = useAppStore((s) => s.libraryModels);
  const loadModelById = useAppStore((s) => s.loadModelById);
  const stopEngine = useAppStore((s) => s.stopEngine);
  const loadingModel = useAppStore((s) => s.loadingModel);
  const engineReady = useAppStore((s) => s.engineReady);
  const loadedModelId = useAppStore((s) => s.loadedModelId);
  const setView = useAppStore((s) => s.setView);

  const refresh = useCallback(async () => {
    if (!inTauri()) return;
    try {
      setOv(await runtimeOverview());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh, engineReady, loadingModel]);

  useEffect(() => {
    if (!runtimeTab) return;
    setTab(runtimeTab);
    useAppStore.setState({ runtimeTab: null });
  }, [runtimeTab]);

  useEffect(() => {
    if (tab === "recall" && !expert) setTab("chat");
  }, [tab, expert]);

  const defaultModelId = () =>
    libraryModels.find((m) => m.is_default)?.id ?? libraryModels[0]?.id;

  async function start() {
    const id = loadedModelId ?? defaultModelId();
    if (!id) {
      setError("No model on this PC yet. Download one from Models first.");
      return;
    }
    setError(null);
    await loadModelById(id).catch((e) => setError(String(e)));
    await refresh();
  }

  async function stop() {
    setBusyAction("stop");
    await stopEngine();
    await refresh();
    setBusyAction(null);
  }

  async function chooseBackend(backend: string, recommended: boolean) {
    if (!ov) return;
    setError(null);
    setBusyAction(backend);
    try {
      // Clearing the override (null) returns to the recommended backend.
      await setBackendOverride(recommended ? null : backend);
      await refresh();
      // If an engine is running, restart it on the newly selected backend
      // (this downloads that backend the first time).
      if (loadedModelId) await loadModelById(loadedModelId);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyAction(null);
    }
  }

  async function doCheckUpdate() {
    setBusyAction("update");
    setError(null);
    try {
      setUpdate(await checkRuntimeUpdate());
    } catch (e) {
      setError(`Couldn't check for updates: ${e}`);
    } finally {
      setBusyAction(null);
    }
  }

  if (!inTauri()) {
    return (
      <div className="surface">
        <div className="surface-inner">
          <h1>Runtime</h1>
          <p className="lede">The local runtime runs in the desktop app.</p>
        </div>
      </div>
    );
  }

  const running = ov?.engine.running ?? false;
  const starting = !!loadingModel;

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Runtime</h1>
        <p className="lede">
          The local runtime runs open models on your PC. It downloads by itself and is matched to
          your hardware — <strong>llama.cpp</strong> for chat and recall,{" "}
          <strong>stable-diffusion.cpp</strong> for images.
        </p>

        <div className="model-tabs" role="tablist" aria-label="Runtime type">
          {(
            [
              ["chat", "Chat"],
              ["images", "Images"],
              ["servers", "Your servers"],
              ...(expert ? [["recall", "Recall"]] : []),
            ] as [RuntimeTab, string][]
          ).map(([id, label]) => (
            <button
              key={id}
              className={`model-tab ${tab === id ? "on" : ""}`}
              role="tab"
              aria-selected={tab === id}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </div>

        {tab === "images" && <ImageRuntime />}
        {tab === "servers" && <YourServers />}
        {tab === "recall" && expert && <RecallRuntime />}

        {tab === "chat" && (
          <>
        {error && <p className="hw-note error">{error}</p>}

        {/* Runtime status + lifecycle */}
        <section className="runtime-card">
          <div className="runtime-card-head">
            <h2 className="section-title">Status</h2>
            <span
              className={`runtime-state-badge ${starting ? "starting" : running ? "running" : "idle"}`}
            >
              <span className="dot" aria-hidden="true" />
              {starting ? loadingModel?.label : running ? "Running" : "Stopped"}
            </span>
          </div>
          <div className="hw-grid">
            <div className="hw-row">
              <span className="hw-label">Loaded model</span>
              <span className="hw-value">
                {running ? basename(ov?.engine.model_path ?? null) || "—" : "—"}
              </span>
            </div>
            <div className="hw-row">
              <span className="hw-label">Address</span>
              <span className="hw-value">
                {running && ov?.engine.port
                  ? `127.0.0.1:${ov.engine.port} · this PC only`
                  : "Not listening"}
              </span>
            </div>
            <div className="hw-row">
              <span
                className="hw-label"
                title="Whether the runtime forces tool calls into valid JSON. When it can't, I check each call and retry."
              >
                Structured tool output
              </span>
              <span className="hw-value">
                {running
                  ? ov?.engine.structured_tool_output
                    ? "enforced ✓"
                    : "validate + retry"
                  : "validate + retry"}
              </span>
            </div>
            {/* HEAL-1: self-repair is only worth mentioning once it happened. */}
            {!!ov?.engine.restarts_session && (
              <div className="hw-row">
                <span className="hw-label">Self-healing</span>
                <span className="hw-value">
                  {ov.engine.self_heal_gave_up
                    ? `I couldn't keep my runtime alive after ${ov.engine.restarts_session} tries — I've stopped trying.`
                    : `Self-healed ${ov.engine.restarts_session}× this session`}
                </span>
              </div>
            )}
          </div>
          <div className="runtime-actions">
            <button className="link-button" onClick={() => setView("models")}>
              Choose which model → Models
            </button>
            {running ? (
              <button className="btn-secondary" onClick={stop} disabled={busyAction === "stop"}>
                {busyAction === "stop" ? "Stopping…" : "Stop runtime"}
              </button>
            ) : (
              <button className="btn-primary" onClick={start} disabled={starting}>
                {starting ? loadingModel?.label : "Start runtime"}
              </button>
            )}
          </div>
        </section>

        {/* Acceleration backend (manual override) */}
        <section className="runtime-card">
          <h2 className="section-title">Acceleration</h2>
          <p className="runtime-sub">{ov?.recommended.rationale}</p>
          <div className="backend-list">
            {ov?.options.map((opt) => {
              const active = opt.backend === ov.active_backend;
              return (
                <button
                  key={opt.backend}
                  className={`backend-row ${active ? "active" : ""}`}
                  onClick={() => chooseBackend(opt.backend, opt.recommended)}
                  disabled={busyAction === opt.backend}
                  aria-pressed={active}
                >
                  <span className="backend-radio" aria-hidden="true" />
                  <span className="backend-name">{opt.label}</span>
                  {opt.recommended && <span className="tag tag-rec">Recommended</span>}
                  {opt.installed ? (
                    <span className="tag tag-installed">Installed</span>
                  ) : (
                    <span className="tag tag-download">Downloads on use</span>
                  )}
                  {busyAction === opt.backend && <span className="tag">Applying…</span>}
                </button>
              );
            })}
          </div>
          <p className="runtime-hint">
            Most people should keep the recommended option. Switching downloads that build the
            first time and restarts it if it’s running.
          </p>
        </section>

        {/* Installed runtime build + updates */}
        <section className="runtime-card">
          <h2 className="section-title">Runtime build</h2>
          <div className="hw-grid">
            <div className="hw-row">
              <span className="hw-label">Version</span>
              <span className="hw-value">
                llama.cpp <code>{ov?.recommended.build_tag}</code> ·{" "}
                {ov?.installed ? "installed" : "not yet downloaded"}
              </span>
            </div>
            {ov?.install_path && (
              <div className="hw-row">
                <span className="hw-label">Location</span>
                <span className="hw-value path">{ov.install_path}</span>
              </div>
            )}
          </div>
          <div className="runtime-actions">
            <button
              className="btn-secondary"
              onClick={doCheckUpdate}
              disabled={busyAction === "update"}
            >
              {busyAction === "update" ? "Checking…" : "Check for updates"}
            </button>
            {update && (
              <span className="update-note">
                {update.update_available
                  ? `A newer build (${update.latest}) is available. Poiesis Agent pins a tested build (${update.current}) for stability.`
                  : `You’re on the latest tested build (${update.current}).`}
              </span>
            )}
          </div>
        </section>

        {/* Hardware */}
        <section className="runtime-card">
          <h2 className="section-title">Your hardware</h2>
          {ov && (
            <div className="hw-grid">
              <div className="hw-row">
                <span className="hw-label">Processor</span>
                <span className="hw-value">
                  {ov.hardware.cpu.brand} · {ov.hardware.cpu.physical_cores} cores
                  {ov.hardware.cpu.avx512 ? " · AVX-512" : ov.hardware.cpu.avx2 ? " · AVX2" : ""}
                </span>
              </div>
              <div className="hw-row">
                <span className="hw-label">Memory</span>
                <span className="hw-value">{(ov.hardware.ram_mb / 1024).toFixed(1)} GB RAM</span>
              </div>
              {ov.hardware.gpus.map((g, i) => (
                <div className="hw-row" key={i}>
                  <span className="hw-label">Graphics</span>
                  <span className="hw-value">
                    {g.name}
                    {g.vram_mb ? ` · ${formatVram(g.vram_mb)}` : ""}
                  </span>
                </div>
              ))}
            </div>
          )}
        </section>
          </>
        )}
      </div>
    </div>
  );
}
