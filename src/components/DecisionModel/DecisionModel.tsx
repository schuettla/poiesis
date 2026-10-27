import { useCallback, useEffect, useState } from "react";
import {
  calibrateDecide,
  decideCalibration,
  decideModels,
  decidePolicy,
  decideStatus,
  deleteDecideModel,
  downloadDecideModel,
  inTauri,
  setDecideBackend,
  setDecidePlacement,
  setDecidePurposeCloud,
  setDefaultDecideModel,
  type DecideModelRow,
  type CalibrateProgress,
  type CalibrationReport,
  type DecideModels,
  type DecidePlacement,
  type DecidePolicyRow,
  type CloudDecisionStatus,
  type DecideStatus,
  type DownloadProgress,
} from "../../lib/api";
import { useAppStore } from "../../lib/store";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import { CloudModels, cloudLine, useCloudStatus } from "./CloudDecisions";
import "./DecisionModel.css";

/** Where a decision model would run, in the words the screen uses (`DEC-UI-1`). */
export function fitText(row: Pick<DecideModelRow, "fit">, chatModel: string | null): string {
  if (row.fit === "gpu") {
    return chatModel ? `Fits on your graphics card next to ${chatModel}` : "Fits on your graphics card";
  }
  if (row.fit === "cpu") return "Runs on the processor, in the background only";
  return "Won't fit in this PC's memory";
}

/** The status line of `DEC-UI-2`: a dot and what it means. */
export function statusLine(s: DecideStatus | null): { tone: "idle" | "starting" | "running" | "failed"; text: string } {
  if (!s || !s.model_installed) return { tone: "idle", text: "Not set up" };
  if (s.state === "ready") {
    const where = s.gpu === null ? "" : s.gpu ? " · graphics card" : " · processor";
    const ms = s.median_ms !== null ? ` · ${s.median_ms} ms` : "";
    return { tone: "running", text: `Ready${where}${ms}` };
  }
  if (s.state === "starting") return { tone: "starting", text: "Starting…" };
  if (s.state === "failed") return { tone: "failed", text: `Couldn't start: ${s.error ?? "unknown reason"}` };
  return { tone: "idle", text: "Starts when needed" };
}

/** `CTL-4`: what one purpose's calibration says, in the words the screen uses. */
export function calibrationText(r: CalibrationReport): string {
  if (!r.fit) return "Not measured yet";
  const ece = r.fit.ece.toFixed(2);
  const was = r.ece_before !== null ? `, was ${r.ece_before.toFixed(2)}` : "";
  if (r.calibrated) return `Calibrated · error ${ece}${was} · ${r.fit.n} examples`;
  if (r.fit.n < 40) return `Not enough examples yet (${r.fit.n} of 40)`;
  return `Measured, still off · error ${ece}${was} · ${r.fit.n} examples`;
}

/** The line under "Calibration": how many questions are calibrated, and what that means. */
export function calibrationSummary(reports: CalibrationReport[] | null): string {
  if (!reports || reports.length === 0 || reports.every((r) => !r.fit)) {
    return "Not measured yet. Until it is, a quick decision can only make me more careful, never less.";
  }
  const done = reports.filter((r) => r.calibrated).length;
  if (done === reports.length) return "Measured for every question. I trust its numbers as they are.";
  return `Measured for ${done} of ${reports.length} questions. For the rest a quick decision can only make me more careful.`;
}

/** The bar a decision has to reach, as the Advanced table shows it. */
export function barText(r: Pick<DecidePolicyRow, "act_at_calibrated" | "act_at_uncalibrated">): string {
  const cal = r.act_at_calibrated.toFixed(2);
  const raw = r.act_at_uncalibrated.toFixed(2);
  return cal === raw ? cal : `${cal} (${raw} until measured)`;
}

/** Everything both screens do with the decision model, in one place. */
function useDecide() {
  const [data, setData] = useState<DecideModels | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [prog, setProg] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!inTauri()) return;
    try {
      setData(await decideModels());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function run(label: string, job: () => Promise<unknown>) {
    setBusy(label);
    setError(null);
    try {
      await job();
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(null);
      setProg(null);
    }
  }

  return {
    data,
    busy,
    prog,
    error,
    refresh,
    download: (name: string) => run(name, () => downloadDecideModel(name, setProg)),
    makeDefault: (id: string) =>
      run(id, async () => {
        await setDefaultDecideModel(id);
        // Choosing a model on this computer is choosing this computer.
        await setDecideBackend("local");
      }),
    remove: (id: string) => run(id, () => deleteDecideModel(id)),
    place: (p: DecidePlacement) => run("placement", () => setDecidePlacement(p)),
  };
}

function Progress({ prog }: { prog: DownloadProgress }) {
  const pct = prog.total ? Math.round((prog.received / prog.total) * 100) : null;
  return (
    <div className="dl-progress wide" style={{ marginTop: 12 }}>
      <div className="dl-bar" style={{ width: pct !== null ? `${pct}%` : "40%" }} />
      <span className="dl-pct">{prog.label}</span>
    </div>
  );
}

/** The badge at the top of the Decision model tab: where quick decisions run now. */
export function panelState(
  cloud: Pick<CloudDecisionStatus, "backend"> | null,
  local: Pick<DecideModelRow, "name"> | undefined
): { tone: "idle" | "running"; text: string } {
  if (cloud?.backend === "cloud") return { tone: "running", text: "Running in the cloud" };
  if (local) return { tone: "running", text: "Running on this computer" };
  return { tone: "idle", text: "Not set up" };
}

/** `DEC-UI-1`: the Decision model tab in Models. It is laid out like the Chat and
 * Images tabs: a first section, then one group per place the model can run, in the
 * same rows. "Use this one" on a local or a cloud model is the whole choice, with no
 * second switch in Runtime. `filter` is the page's All / On this PC / Cloud. */
export function DecisionModelGroup({ filter = "all" }: { filter?: "all" | "local" | "cloud" }) {
  const d = useDecide();
  const cloud = useCloudStatus();
  const [removing, setRemoving] = useState<DecideModelRow | null>(null);
  if (!inTauri() || !d.data) return null;
  const local = d.data.models.find((m) => m.is_default);
  const state = panelState(cloud.status, local);
  const inCloud = cloud.status?.backend === "cloud";

  async function useLocal(row: DecideModelRow) {
    await d.makeDefault(row.id!);
    await cloud.refresh();
  }

  return (
    <>
      <section className="model-section">
        <div className="section-head">
          <h2 className="section-title">Decision model</h2>
          <span className="section-note">{state.text}</span>
        </div>
        <p className="add-help">
          A small model I ask quick yes/no and pick-one questions, like "do these results answer
          the question?". It is not the model you chat with. Without one I ask your chat model, as
          before. Experiments such as Fast &amp; slow thinking need it. Pick one below.
        </p>
      </section>

      {filter !== "cloud" && (
        <section className="model-section">
          <div className="section-head">
            <h2 className="section-title">
              <span className="provenance-dot local" aria-hidden="true" /> On this PC
            </h2>
            <span className="disk-total">Free · private · offline</span>
          </div>
          <div className="model-rows">
            {d.data.models.map((row) => {
              const inUse = row.is_default && !inCloud;
              return (
                <div className="model-row-wrap" key={row.name}>
                  <div className={`model-row ${inUse ? "in-use" : ""}`}>
                    <span className="provenance-dot local" aria-hidden="true" />
                    <span className="model-row-name" title={row.note}>
                      {row.name}
                    </span>
                    <span className="model-row-chips">
                      <span className="cap-chip">{row.size_label}</span>
                    </span>
                    <span className="model-row-cost">{fitText(row, d.data!.chat_model)}</span>
                    <span className="model-row-actions">
                      {row.installed ? (
                        <>
                          {inUse ? (
                            <span className="default-tag">In use</span>
                          ) : (
                            <button className="btn-text" disabled={!!d.busy} onClick={() => void useLocal(row)}>
                              Use this one
                            </button>
                          )}
                          <button className="btn-text danger" disabled={!!d.busy} onClick={() => setRemoving(row)}>
                            Delete
                          </button>
                        </>
                      ) : (
                        <button
                          className="btn-use"
                          disabled={!!d.busy || row.fit === "wont-fit"}
                          onClick={() => d.download(row.name)}
                        >
                          {d.busy === row.name ? "Downloading…" : "Download"}
                        </button>
                      )}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
          {d.busy && d.prog && <Progress prog={d.prog} />}
          {d.busy && !d.prog && d.data.models.some((m) => m.name === d.busy) && (
            <p className="add-help">Checking that it can make decisions…</p>
          )}
          {d.error && <p className="hw-note error">{d.error}</p>}
        </section>
      )}

      {filter !== "local" && (
        <section className="model-section decide-cloud-section">
          <CloudModels />
        </section>
      )}
      {removing && (
        <ConfirmDialog
          title={`Delete ${removing.name}?`}
          body="Its files are removed from this PC. You can download it again later."
          onCancel={() => setRemoving(null)}
          onConfirm={() => {
            const row = removing;
            setRemoving(null);
            if (row.id) d.remove(row.id);
          }}
        />
      )}
    </>
  );
}

/** The Advanced table's last column: a decision that runs on its own stays on this
 * computer unless the person allows the cloud for it by name (decision 12). */
export function cloudCell(
  r: Pick<DecidePolicyRow, "purpose" | "background" | "backend_override">,
  keySet: boolean,
  toggle: (purpose: string, allow: boolean) => void
) {
  if (!r.background) return "follows the choice above";
  return (
    <label className={keySet ? undefined : "disabled"} title={keySet ? undefined : "Add an OpenRouter key in Providers first"}>
      <input
        type="checkbox"
        checked={r.backend_override === "cloud"}
        disabled={!keySet && r.backend_override !== "cloud"}
        onChange={(e) => toggle(r.purpose, e.target.checked)}
      />{" "}
      Allow the cloud
    </label>
  );
}

const PLACEMENTS: [DecidePlacement, string][] = [
  ["auto", "Automatic"],
  ["gpu", "Graphics card"],
  ["cpu", "Processor"],
];

/** `DEC-UI-2`: the "Quick decisions" card in Runtime. */
export function QuickDecisionsCard() {
  const d = useDecide();
  const openModelsTab = useAppStore((s) => s.openModelsTab);
  const [status, setStatus] = useState<DecideStatus | null>(null);
  const [policy, setPolicy] = useState<DecidePolicyRow[] | null>(null);
  const [policyError, setPolicyError] = useState<string | null>(null);
  const cloud = useCloudStatus();
  const [advanced, setAdvanced] = useState(false);
  const [calibration, setCalibration] = useState<CalibrationReport[] | null>(null);
  const [calibrating, setCalibrating] = useState<CalibrateProgress | "starting" | null>(null);
  const [calibrationError, setCalibrationError] = useState<string | null>(null);

  useEffect(() => {
    if (inTauri()) decideCalibration().then(setCalibration).catch(() => {});
  }, []);

  async function calibrateNow() {
    setCalibrationError(null);
    setCalibrating("starting");
    try {
      setCalibration(await calibrateDecide(setCalibrating));
    } catch (e) {
      setCalibrationError(String(e));
    } finally {
      setCalibrating(null);
    }
  }

  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    const tick = () =>
      decideStatus()
        .then((s) => live && setStatus(s))
        .catch(() => {});
    tick();
    const id = window.setInterval(tick, 4000);
    return () => {
      live = false;
      window.clearInterval(id);
    };
  }, []);

  useEffect(() => {
    if (advanced && !policy) decidePolicy().then(setPolicy).catch(() => {});
  }, [advanced, policy]);

  async function toggleCloud(purpose: string, allow: boolean) {
    setPolicyError(null);
    try {
      await setDecidePurposeCloud(purpose, allow);
      setPolicy(await decidePolicy());
    } catch (e) {
      setPolicyError(String(e));
    }
  }

  if (!inTauri()) return null;
  const line = statusLine(status);
  const model = d.data?.models.find((m) => m.is_default);

  return (
    <section className="runtime-card">
      <div className="runtime-card-head">
        <h2 className="section-title">Quick decisions</h2>
        <span className={`runtime-state-badge decide-${line.tone} ${line.tone === "running" ? "running" : line.tone === "starting" ? "starting" : "idle"}`}>
          <span className="dot" aria-hidden="true" />
          {line.text}
        </span>
      </div>
      <p className="runtime-sub">
        Small questions I ask myself that have a yes/no or pick-one answer, like "do these results
        answer the question?" or "which tools does this need?".
      </p>

      <div className="hw-grid">
        <div className="hw-row">
          <span className="hw-label">Model</span>
          <span className="hw-value">
            {cloud.status?.backend === "cloud"
              ? "A cloud model via OpenRouter."
              : model
                ? model.name
                : "None yet. I ask the chat model for these, as before."}{" "}
            <button className="link-button inline" onClick={() => openModelsTab("decide")}>
              {model || cloud.status?.backend === "cloud" ? "Change in Models" : "Get one in Models"}
            </button>
          </span>
        </div>
      </div>

      <h3 className="decide-sub">Runs on</h3>
      <div className="backend-list">
        {PLACEMENTS.map(([id, label]) => {
          const active = (d.data?.placement ?? "auto") === id;
          return (
            <button
              key={id}
              className={`backend-row ${active ? "active" : ""}`}
              aria-pressed={active}
              disabled={d.busy === "placement"}
              onClick={() => d.place(id)}
            >
              <span className="backend-radio" aria-hidden="true" />
              <span className="backend-name">{label}</span>
            </button>
          );
        })}
      </div>
      <p className="runtime-hint">
        On the processor I only use it in the background. Automatic picks the graphics card when
        the model fits next to your chat model.
      </p>

      {cloud.status?.backend === "cloud" && (
        <>
          <h3 className="decide-sub">Where</h3>
          <p className="runtime-hint">
            In the cloud, via OpenRouter. You chose that in Models, and it is changed there too.
          </p>
          {cloudLine(cloud.status) && (
            <p className={`decide-cloud-line ${cloudLine(cloud.status)!.tone}`}>{cloudLine(cloud.status)!.text}</p>
          )}
          <p className="runtime-hint">
            Hints while you type come from the cloud too, so what you type goes there before you send it.
          </p>
        </>
      )}

      <h3 className="decide-sub">Calibration</h3>
      <p className="runtime-hint">{calibrationSummary(calibration)}</p>
      {cloud.status?.backend === "cloud" && (
        <p className="runtime-hint">
          Answers from the cloud are not measured yet. Like any unmeasured answer, they can only make me
          more careful.
        </p>
      )}
      {calibration && calibration.some((r) => r.fit) && (
        <ul className="decide-calibration">
          {calibration.map((r) => (
            <li key={r.purpose}>
              <span>{r.label}</span>
              <span>{calibrationText(r)}</span>
            </li>
          ))}
        </ul>
      )}
      <button
        className="btn-secondary"
        disabled={!model || calibrating !== null}
        onClick={calibrateNow}
        title={model ? undefined : "Get a decision model first"}
      >
        {calibrating === null
          ? "Calibrate now"
          : calibrating === "starting"
            ? "Starting…"
            : `Measuring ${calibrating.done} of ${calibrating.total}…`}
      </button>
      <p className="runtime-hint">
        Runs 40 examples for each question through the model on this computer. A few seconds on a
        graphics card, a few minutes on the processor. Nothing leaves this computer.
      </p>
      {calibrationError && <p className="hw-note error">{calibrationError}</p>}

      {d.error && <p className="hw-note error">{d.error}</p>}

      <button className="link-button" aria-expanded={advanced} onClick={() => setAdvanced((a) => !a)}>
        Advanced {advanced ? "▴" : "▸"}
      </button>
      {advanced && (
        <table className="decide-policy">
          <thead>
            <tr>
              <th>Question</th>
              <th>Acts at</th>
              <th>Below that</th>
              <th>Runs in the background</th>
              <th>If not ready</th>
              <th>Gives up after</th>
              <th>Cloud</th>
            </tr>
          </thead>
          <tbody>
            {(policy ?? []).map((r) => (
              <tr key={r.purpose}>
                <td>{r.label}</td>
                <td>{barText(r)}</td>
                <td>{r.below}</td>
                <td>{r.background ? "yes" : "no"}</td>
                <td>{r.skipped_if_not_ready ? "skipped" : "waits"}</td>
                <td>{r.deadline_s} s</td>
                <td>{cloudCell(r, cloud.status?.key_set ?? false, toggleCloud)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {advanced && policyError && <p className="hw-note error">{policyError}</p>}
    </section>
  );
}
