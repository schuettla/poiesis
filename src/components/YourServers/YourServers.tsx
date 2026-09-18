import { useEffect, useState } from "react";
import {
  addEndpoint,
  updateEndpoint,
  setEndpointEnabled,
  deleteEndpoint,
  testEndpoint,
  type EndpointInfo,
  type EndpointProbe,
} from "../../lib/api";
import { useAppStore } from "../../lib/store";
import "../../routes/Settings.css";
import "../../routes/Apps.css";

/** Quick-fill presets for the two servers almost everyone means. */
const ENDPOINT_PRESETS = [
  { label: "Ollama", baseUrl: "http://localhost:11434" },
  { label: "LM Studio", baseUrl: "http://localhost:1234" },
];

/** A user's own OpenAI-compatible model server (Ollama, LM Studio, or a
 * remote box) — a third model source alongside the integrated runtime and
 * cloud providers. `RTM-10`: it lives under Runtime → Your servers, because a
 * server you run yourself is a runtime you already have. Mirrors the MCP
 * connector's test/add flow (`Apps.tsx`). */
export default function YourServers() {
  const endpoints = useAppStore((s) => s.endpoints);
  const openModelsFiltered = useAppStore((s) => s.openModelsFiltered);
  // The picker's live model list, so each row can say how many models its
  // server is actually serving rather than only that it's switched on.
  const endpointModels = useAppStore((s) => s.endpointModels);
  const refreshEndpoints = useAppStore((s) => s.refreshEndpoints);

  const [statuses, setStatuses] = useState<Record<string, EndpointProbe>>({});
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [formOpen, setFormOpen] = useState(false);
  /** Set while editing an existing endpoint; null means the form adds a new one. */
  const [editingId, setEditingId] = useState<string | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [label, setLabel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [ctxSize, setCtxSize] = useState("8192");
  const [apiKey, setApiKey] = useState("");
  const [draftStatus, setDraftStatus] = useState<EndpointProbe | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    refreshEndpoints();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function modelCountFor(id: string) {
    return endpointModels.filter((m) => m.endpoint_id === id).length;
  }

  function applyPreset(preset: (typeof ENDPOINT_PRESETS)[number]) {
    setLabel(preset.label);
    setBaseUrl(preset.baseUrl);
    setDraftStatus(null);
    setError(null);
  }

  function openAddForm() {
    setFormOpen(true);
    setEditingId(null);
    setLabel("");
    setBaseUrl("");
    setCtxSize("8192");
    setApiKey("");
    setDraftStatus(null);
    setError(null);
  }

  function openEditForm(ep: EndpointInfo) {
    setFormOpen(true);
    setEditingId(ep.id);
    setLabel(ep.label);
    setBaseUrl(ep.base_url);
    setCtxSize(String(ep.ctx_size));
    // Never prefill a stored key — it isn't readable, and a blank field here
    // means "leave it alone", not "clear it".
    setApiKey("");
    setShowAdvanced(false);
    setDraftStatus(null);
    setError(null);
  }

  function closeForm() {
    setFormOpen(false);
    setEditingId(null);
    setShowAdvanced(false);
    setLabel("");
    setBaseUrl("");
    setCtxSize("8192");
    setApiKey("");
    setDraftStatus(null);
  }

  async function testDraft() {
    setError(null);
    setBusyId("__draft");
    try {
      // While editing, fall back to the endpoint's stored key so a keyed
      // server doesn't report 401 just because the field is (correctly) blank.
      const status = await testEndpoint(baseUrl, apiKey.trim() || undefined, editingId ?? undefined);
      setDraftStatus(status);
      // The Windows `localhost`→`::1` gotcha: the probe found the server at
      // 127.0.0.1 instead of what was typed, so keep the address that worked.
      if (status.resolved_base_url) setBaseUrl(status.resolved_base_url);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function saveForm() {
    setError(null);
    setSaving(true);
    try {
      const ctx = Number(ctxSize) || 8192;
      const key = apiKey.trim() || undefined;
      if (editingId) {
        await updateEndpoint(editingId, label.trim(), baseUrl.trim(), ctx, key);
      } else {
        await addEndpoint(label.trim(), baseUrl.trim(), key, ctx);
      }
      closeForm();
      await refreshEndpoints();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function testExisting(ep: EndpointInfo) {
    setError(null);
    setBusyId(ep.id);
    try {
      const status = await testEndpoint(ep.base_url, undefined, ep.id);
      setStatuses((s) => ({ ...s, [ep.id]: status }));
      // A rewritten address (the `localhost` gotcha) is worth saving so the
      // next test — or the agent's own turn — doesn't hit the same wall.
      if (status.resolved_base_url && status.resolved_base_url !== ep.base_url) {
        await updateEndpoint(ep.id, ep.label, status.resolved_base_url, ep.ctx_size);
      }
      // Models may have appeared or gone since the last look, so the picker
      // and this row's count both need re-reading either way.
      await refreshEndpoints();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function toggleEnabled(ep: EndpointInfo) {
    setBusyId(ep.id);
    try {
      await setEndpointEnabled(ep.id, !ep.enabled);
      await refreshEndpoints();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function remove(ep: EndpointInfo) {
    setBusyId(ep.id);
    try {
      await deleteEndpoint(ep.id);
      setStatuses((s) => {
        const next = { ...s };
        delete next[ep.id];
        return next;
      });
      await refreshEndpoints();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <section className="setting-block">
      <h2 className="setting-title">Your own servers</h2>
      <p className="setting-help">
        Already running a model server such as Ollama or LM Studio? Point Poiesis at it and its
        models appear under Models. Nothing is downloaded twice, and nothing leaves your machine.
      </p>

      {error && <p className="hw-note error">{error}</p>}

      {endpoints.map((ep) => {
        const status = statuses[ep.id];
        const count = modelCountFor(ep.id);
        return (
          <div className="provider-row" key={ep.id}>
            <div className="provider-head">
              <span className="provider-name">{ep.label}</span>
              <span className={`provider-status ${ep.enabled && count > 0 ? "set" : ""}`}>
                {!ep.enabled
                  ? "Off"
                  : count > 0
                    ? `${count} ${count === 1 ? "model" : "models"}`
                    : "Not reachable"}
              </span>
              {ep.key_set && <span className="provider-status">Key saved</span>}
            </div>
            <div className="connector-url">{ep.base_url}</div>
            {status && (
              <div className={`connector-status ${status.ok ? "ok" : "err"}`}>
                {!status.ok
                  ? `Couldn’t connect: ${status.error}`
                  : status.model_count === 0
                    ? "Reachable, but it isn’t serving any models yet — load one on the server first."
                    : `Connected — ${status.model_count} ${status.model_count === 1 ? "model" : "models"} available`}
              </div>
            )}
            <div className="provider-controls">
              <button className="btn-secondary" onClick={() => testExisting(ep)} disabled={busyId === ep.id}>
                {busyId === ep.id ? "Checking…" : "Test"}
              </button>
              <button
                className="btn-secondary"
                onClick={() => openEditForm(ep)}
                disabled={busyId === ep.id}
              >
                Edit
              </button>
              <label className="toggle-line">
                <input
                  type="checkbox"
                  checked={ep.enabled}
                  disabled={busyId === ep.id}
                  onChange={() => toggleEnabled(ep)}
                />
                <span>{ep.enabled ? "On" : "Off"}</span>
              </label>
              <button
                className="btn-text danger"
                onClick={() => remove(ep)}
                disabled={busyId === ep.id}
              >
                Remove
              </button>
            </div>
            {count > 0 && (
              <button
                className="link-button"
                onClick={() => openModelsFiltered({ source: `endpoint:${ep.id}`, label: ep.label })}
              >
                See its models →
              </button>
            )}
          </div>
        );
      })}

      {!formOpen ? (
        <button className="link-button" onClick={openAddForm}>
          + Add a server
        </button>
      ) : (
        <div className="connect-card">
          {!editingId && (
            <div className="connect-actions">
              {ENDPOINT_PRESETS.map((preset) => (
                <button key={preset.label} className="btn-secondary" onClick={() => applyPreset(preset)}>
                  {preset.label} · {preset.baseUrl.replace("http://", "")}
                </button>
              ))}
            </div>
          )}
          <div className="connect-fields">
            <label className="field">
              <span className="field-label">Name</span>
              <input
                className="field-input"
                placeholder="My Ollama"
                value={label}
                onChange={(e) => setLabel(e.target.value)}
              />
            </label>
            <label className="field">
              <span className="field-label">
                Address <span className="field-hint">(Poiesis adds “/v1” itself)</span>
              </span>
              <input
                className="field-input"
                placeholder="http://localhost:11434"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && testDraft()}
              />
            </label>
            <label className="field">
              <span className="field-label">
                Context window{" "}
                <span className="field-hint">— match what your server is configured for</span>
              </span>
              <input
                className="field-input"
                type="number"
                min={512}
                step={512}
                value={ctxSize}
                onChange={(e) => setCtxSize(e.target.value)}
              />
            </label>
          </div>

          <button className="link-button" onClick={() => setShowAdvanced((v) => !v)}>
            {showAdvanced ? "Hide advanced" : "Advanced"}
          </button>
          {showAdvanced && (
            <label className="field">
              <span className="field-label">
                API key{" "}
                <span className="field-hint">
                  (optional — Ollama and LM Studio usually don’t need one
                  {editingId ? "; leave blank to keep the saved one" : ""})
                </span>
              </span>
              <input
                className="field-input"
                type="password"
                placeholder="Only if your server requires one"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
              />
            </label>
          )}

          {draftStatus && (
            <div className={`connector-status ${draftStatus.ok ? "ok" : "err"}`}>
              {!draftStatus.ok
                ? `Couldn’t connect: ${draftStatus.error}`
                : draftStatus.model_count === 0
                  ? "Reachable, but it isn’t serving any models yet — load one on the server first."
                  : `Connected — ${draftStatus.model_count} ${draftStatus.model_count === 1 ? "model" : "models"} available`}
            </div>
          )}

          <div className="connect-actions">
            <button
              className="btn-secondary"
              onClick={testDraft}
              disabled={busyId === "__draft" || !baseUrl.trim()}
            >
              {busyId === "__draft" ? "Checking…" : "Test connection"}
            </button>
            <button
              className="btn-primary"
              onClick={saveForm}
              disabled={saving || !label.trim() || !baseUrl.trim()}
            >
              {saving ? "Saving…" : editingId ? "Save" : "Add"}
            </button>
            <button className="btn-text" onClick={closeForm}>
              Cancel
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
