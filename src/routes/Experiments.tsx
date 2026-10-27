import { useEffect, useState } from "react";
import {
  calibrateDecide,
  decideCalibration,
  decidePolicy,
  experimentReport,
  inTauri,
  resetExperiment,
  type CalibrateProgress,
  type ExperimentInfo,
  type ExperimentReport,
} from "../lib/api";
import { useAppStore } from "../lib/store";
import type { View } from "../lib/types";
import ConfirmDialog from "../components/Confirm/ConfirmDialog";
import Toggle from "../components/Toggle/Toggle";
import "./Surface.css";
import "./Experiments.css";

/** The tag on a card: how sure we are about it. */
const STAGE_LABEL: Record<ExperimentInfo["stage"], string> = { early: "Early", beta: "Beta" };

/** `EXP-UI-1`: the one line under a switch that cannot be turned on yet. */
export function needsLine(e: Pick<ExperimentInfo, "requires">): string | null {
  const missing = e.requires.filter((r) => !r.met);
  return missing.length ? `Needs ${missing.map((r) => r.label.toLowerCase()).join(" and ")}` : null;
}

/** An experiment that makes quick decisions: it needs a decision model. */
const decides = (e: Pick<ExperimentInfo, "requires">) => e.requires.some((r) => r.kind === "decision-model");

/** How many of this experiment's questions have no good calibration yet, or
 * `null` while that is being read. Until they do, its quick decisions can only
 * make me more careful (`CTL-4`). */
function useUncalibrated(e: ExperimentInfo): [number | null, () => void] {
  const [count, setCount] = useState<number | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!inTauri() || !e.on || !decides(e)) return;
    let live = true;
    Promise.all([decidePolicy(), decideCalibration()])
      .then(([policy, reports]) => {
        const mine = policy.filter((p) => p.experiment === e.id).map((p) => p.purpose);
        if (live) setCount(reports.filter((r) => mine.includes(r.purpose) && !r.calibrated).length);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [e, tick]);
  return [e.on ? count : null, () => setTick((n) => n + 1)];
}

/** `EXP-UI-1`: said once an experiment that decides is on and not measured yet. */
function CalibrationNote({ e, refresh }: { e: ExperimentInfo; refresh: () => void }) {
  const [progress, setProgress] = useState<CalibrateProgress | "starting" | null>(null);
  const [later, setLater] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (later && progress === null) return null;

  async function run() {
    setError(null);
    setProgress("starting");
    try {
      await calibrateDecide(setProgress);
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setProgress(null);
    }
  }

  return (
    <div className="exp-note" role="status" aria-label={`${e.title}: calibration`}>
      <p>
        I'll be careful until I've measured how sure my quick decisions are on your setup. That
        takes a few minutes.
      </p>
      <div className="exp-note-actions">
        <button className="btn-secondary" onClick={run} disabled={progress !== null}>
          {progress === null
            ? "Calibrate now"
            : progress === "starting"
              ? "Starting…"
              : `Measuring ${progress.done} of ${progress.total}…`}
        </button>
        {progress === null && (
          <button className="link-button inline" onClick={() => setLater(true)}>
            Later
          </button>
        )}
      </div>
      {error && <p className="exp-error">{error}</p>}
    </div>
  );
}

/** `EXP-4`: the last 30 days in one line. Facts only; the data decides what there is to say. */
export function reportLine(r: Pick<ExperimentReport, "lines" | "days">): string {
  return r.lines.length ? r.lines.join(" · ") : `Nothing recorded in the last ${r.days} days.`;
}

/** `EXP-4`: what a reset says it will forget, before it does. */
export function resetBody(e: Pick<ExperimentInfo, "records" | "requires">, records: number): string {
  const kept = decides(e) ? " What I measured about how sure my quick decisions are is kept." : "";
  return `${records === 1 ? "1 record" : `${records} records`} will be forgotten. ${e.records} Your chats and settings are not touched, and the switch stays as it is.${kept}`;
}

function useReport(e: ExperimentInfo): [ExperimentReport | null, (r: ExperimentReport) => void] {
  const [report, setReport] = useState<ExperimentReport | null>(null);
  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    experimentReport(e.id)
      .then((r) => live && setReport(r))
      .catch(() => {});
    return () => {
      live = false;
    };
    // The switch changes what is recorded from here on, so the line is read again.
  }, [e.id, e.on]);
  return [report, setReport];
}

function Card({ e }: { e: ExperimentInfo }) {
  const setExperiment = useAppStore((s) => s.setExperiment);
  const setView = useAppStore((s) => s.setView);
  const openModelsTab = useAppStore((s) => s.openModelsTab);
  const setActivityFilter = useAppStore((s) => s.setActivityFilter);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [uncalibrated, refresh] = useUncalibrated(e);
  const [report, setReport] = useReport(e);
  const [confirming, setConfirming] = useState(false);

  async function reset() {
    setConfirming(false);
    setBusy(true);
    setError(null);
    try {
      setReport(await resetExperiment(e.id));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function flip(on: boolean) {
    setBusy(true);
    setError(null);
    try {
      await setExperiment(e.id, on);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  // The switch cannot be turned on yet: say why, in the status line and on hover.
  const blocked = !e.on && needsLine(e) ? `${needsLine(e)}.` : null;
  const status = e.on ? "On" : (blocked ?? "Off. Ready to turn on.");

  return (
    <section className={`exp-card ${e.on ? "on" : "off"}`} aria-labelledby={`exp-${e.id}`}>
      <div className="exp-head">
        <span className="exp-glyph" aria-hidden="true" />
        <div className="exp-ident">
          <h2 id={`exp-${e.id}`} className="exp-title">
            {e.title}
          </h2>
          <span className={`exp-status ${blocked ? "warn" : ""}`}>{status}</span>
        </div>
        <span className={`exp-stage ${e.stage}`}>{STAGE_LABEL[e.stage]}</span>
        <label className="exp-switch" title={blocked ?? undefined}>
          <span className={`exp-switch-text ${e.on ? "on" : ""}`}>{e.on ? "On" : "Off"}</span>
          <Toggle
            checked={e.on}
            // Turning off always works. Turning on waits for what it needs.
            disabled={busy || (!e.on && !e.can_turn_on)}
            label={e.title}
            onChange={flip}
          />
        </label>
      </div>
      <p className="exp-summary">{e.summary}</p>

      <div className="exp-body">
        <section className="exp-group" aria-label="What changes">
          <h3 className="exp-sub">What changes</h3>
          <ul className="exp-list">
            {e.what_changes.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </section>

        <section className="exp-group" aria-label="Details">
          <h3 className="exp-sub">Details</h3>
          {e.requires.length > 0 && (
            <div className="exp-row">
              <span className="exp-label">Needs</span>
              <span className="exp-value">
                {e.requires.map((r) => (
                  <span key={r.label} className={`exp-need ${r.met ? "met" : "missing"}`}>
                    <span className="exp-need-dot" aria-hidden="true" />
                    {r.label}
                    {!r.met && r.fix_view && (
                      <button
                        className="link-button inline"
                        onClick={() =>
                          // A decision model is chosen on its own tab in Models.
                          r.kind === "decision-model" || r.kind === "vision"
                            ? openModelsTab("decide")
                            : setView(r.fix_view as View)
                        }
                      >
                        Set up
                      </button>
                    )}
                  </span>
                ))}
              </span>
            </div>
          )}
          <div className="exp-row">
            <span className="exp-label">Records</span>
            <span className="exp-value">{e.records}</span>
          </div>
          <div className="exp-row">
            <span className="exp-label">Leaves Experiments when</span>
            <span className="exp-value">{e.graduation}</span>
          </div>
          {report && (
            <div className="exp-row">
              <span className="exp-label">Last {report.days} days</span>
              <span className="exp-value">
                <span>{reportLine(report)}</span>
                {report.records > 0 && (
                  <button className="link-button inline" disabled={busy} onClick={() => setConfirming(true)}>
                    Reset its data
                  </button>
                )}
              </span>
            </div>
          )}
          {decides(e) && (
            <div className="exp-row">
              <span className="exp-label">Decisions</span>
              <span className="exp-value">
                <button
                  className="link-button inline"
                  onClick={() => {
                    setActivityFilter(`experiment:${e.id}`);
                    setView("activity");
                  }}
                >
                  Show them in Activity
                </button>
              </span>
            </div>
          )}
        </section>

        {e.on && uncalibrated !== null && uncalibrated > 0 && <CalibrationNote e={e} refresh={refresh} />}
        {error && <p className="exp-error">{error}</p>}
      </div>
      {confirming && report && (
        <ConfirmDialog
          title={`Reset the data of ${e.title}?`}
          body={resetBody(e, report.records)}
          confirmLabel="Reset its data"
          onConfirm={reset}
          onCancel={() => setConfirming(false)}
        />
      )}
    </section>
  );
}

/** `EXP-UI-1`: Settings -> Experiments. Things I am still learning to do well.
 * The list is the backend's, so this screen cannot show a switch the backend
 * would not honour. */
export default function Experiments() {
  const experiments = useAppStore((s) => s.experiments);
  const markSeen = useAppStore((s) => s.markExperimentsSeen);
  const load = useAppStore((s) => s.loadExperiments);

  // What a switch needs (a decision model, a mail account, a key) is set up on other
  // screens, so the list is read again each time this one opens. Read once at start,
  // a switch would stay dead after its need was met.
  useEffect(() => {
    void load();
  }, [load]);

  // Arriving here is what clears the tab's dot.
  useEffect(() => {
    markSeen();
  }, [markSeen, experiments]);

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Experiments</h1>
        <p className="lede">
          Things I'm still learning to do well. They change how I behave, so they're off until you
          turn them on. Everything they record stays on this computer.
        </p>
        {!inTauri() ? (
          <p className="exp-hint">Experiments are part of the desktop app.</p>
        ) : experiments === null ? (
          <p className="exp-hint">Reading what's available…</p>
        ) : (
          <div className="exp-cards">
            {experiments.map((e) => (
              <Card key={e.id} e={e} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
