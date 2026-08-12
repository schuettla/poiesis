import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  inTauri,
  listPermissions,
  addPermission,
  revokePermission,
  listCapabilityGrants,
  revokeCapabilityGrant,
  setProviderKey,
  clearProviderKey,
  embedEngineStatus,
  rerankEngineStatus,
  installRerankEngine,
  setRerankEnabled,
  addEndpoint,
  updateEndpoint,
  setEndpointEnabled,
  deleteEndpoint,
  testEndpoint,
  type Grant,
  type CapabilityGrant,
  type RerankSetupStatus,
  type DownloadProgress,
  type EndpointInfo,
  type EndpointProbe,
  mediaSpend as mediaSpendApi,
  type MediaSpendReport,
} from "../lib/api";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppStore, READING_SCALES } from "../lib/store";
import PersonaEditor from "../components/Personas/PersonaEditor";
import "./Surface.css";
import "./Settings.css";

/** `SMP-3`: Simple mode's single Recall control. The embedder and the
 * reranker are two engines and two downloads underneath, but to a Simple-mode
 * user they're one thing with a quality choice — `Good` (the default) or
 * `Sharper`, which installs and enables the reranker through the same flow.
 * Expert mode has no use for this: the full Engine → Recall tab already
 * covers both cards individually (`RRK-UI-1`/`2`/`3`), so this only renders
 * for `!expert` in `Settings`, below. Hidden entirely until recall itself is
 * installed (`SMP-2`) — there's nothing here to sharpen yet. */
function RecallModeControl() {
  const [embedReady, setEmbedReady] = useState(false);
  const [rerank, setRerank] = useState<RerankSetupStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [prog, setProg] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      const [e, r] = await Promise.all([embedEngineStatus(), rerankEngineStatus()]);
      setEmbedReady(!!e.model_installed && !!e.engine_installed);
      setRerank(r);
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function chooseGood() {
    setBusy(true);
    setError(null);
    try {
      await setRerankEnabled(false);
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(false);
    }
  }

  async function chooseSharper() {
    setBusy(true);
    setError(null);
    try {
      if (!rerank?.model_installed) {
        await installRerankEngine((p) => setProg(p));
      }
      await setRerankEnabled(true);
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(false);
      setProg(null);
    }
  }

  if (!embedReady) return null;

  const sharper = !!rerank?.enabled;
  const pct = prog?.total ? Math.round((prog.received / prog.total) * 100) : null;

  return (
    <section className="setting-block">
      <h2 className="setting-title">Recall</h2>
      <p className="setting-help">How carefully I re-read what I find before answering from it.</p>
      {error && <p className="hw-note error">{error}</p>}
      {busy && prog ? (
        <div className="dl-progress wide">
          <div className="dl-bar" style={{ width: pct !== null ? `${pct}%` : "40%" }} />
          <span className="dl-pct">{prog.label}</span>
        </div>
      ) : (
        <div className="setting-actions" role="group" aria-label="Recall quality">
          <button
            className={`btn-secondary ${!sharper ? "selected" : ""}`}
            aria-pressed={!sharper}
            onClick={chooseGood}
            disabled={busy}
          >
            Good
          </button>
          <button
            className={`btn-secondary ${sharper ? "selected" : ""}`}
            aria-pressed={sharper}
            onClick={chooseSharper}
            disabled={busy}
          >
            Sharper
          </button>
        </div>
      )}
      <p className="engine-hint">
        {sharper
          ? "Re-reads the closest matches before answering. A little slower, and another 540 MB."
          : "Matches by meaning. Fast, and usually enough."}
      </p>
    </section>
  );
}

/** Quick-fill presets for the two servers almost everyone means. */
const ENDPOINT_PRESETS = [
  { label: "Ollama", baseUrl: "http://localhost:11434" },
  { label: "LM Studio", baseUrl: "http://localhost:1234" },
];

/** A user's own OpenAI-compatible model server (Ollama, LM Studio, or a
 * remote box) — a third model source alongside the integrated runtime and
 * BYOK cloud providers above. Mirrors the BYOK block's shape and the MCP
 * connector's test/add flow (`Apps.tsx`). */
function LocalEndpointsSettings() {
  const endpoints = useAppStore((s) => s.endpoints);
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
      <h2 className="setting-title">Your own model servers</h2>
      <p className="setting-help">
        Already running Ollama or LM Studio? Point Poiesis Agent at it and its models join your
        picker. Nothing is downloaded twice, and nothing leaves your machine.
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

export default function Settings() {
  const systemPrompt = useAppStore((s) => s.systemPrompt);
  const setView = useAppStore((s) => s.setView);
  const setSystemPrompt = useAppStore((s) => s.setSystemPrompt);
  const [draft, setDraft] = useState(systemPrompt);
  const [saved, setSaved] = useState(false);
  const [grants, setGrants] = useState<Grant[]>([]);
  const [capabilityGrants, setCapabilityGrants] = useState<CapabilityGrant[]>([]);
  const providers = useAppStore((s) => s.providers);
  const refreshCloud = useAppStore((s) => s.refreshCloud);
  // A provider key gates that provider's *image/video* models as well as its
  // chat models, so both lists have to be re-read whenever a key changes.
  const refreshMediaModels = useAppStore((s) => s.refreshMediaModels);
  const [keyDrafts, setKeyDrafts] = useState<Record<string, string>>({});
  const [keyBusy, setKeyBusy] = useState<string | null>(null);
  /** `CST-2`: what generated media has cost, shown under the keys that pay
   * for it. Read once on open — spend moves when a picture is made, not while
   * this screen sits there. */
  const [mediaSpend, setMediaSpend] = useState<MediaSpendReport | null>(null);
  const mode = useAppStore((s) => s.mode);
  const setMode = useAppStore((s) => s.setMode);
  const readingScale = useAppStore((s) => s.readingScale);
  const setReadingScale = useAppStore((s) => s.setReadingScale);
  const contextBudget = useAppStore((s) => s.contextBudget);
  const autoCompact = useAppStore((s) => s.autoCompact);
  const setAutoCompact = useAppStore((s) => s.setAutoCompact);
  const expert = useAppStore((s) => s.expert);
  const setExpert = useAppStore((s) => s.setExpert);
  const resetFirstTimeExplanations = useAppStore((s) => s.resetFirstTimeExplanations);

  useEffect(() => setDraft(systemPrompt), [systemPrompt]);
  useEffect(() => {
    if (!inTauri()) return;
    refreshPermissions();
    listCapabilityGrants().then(setCapabilityGrants).catch(() => {});
    refreshCloud();
    mediaSpendApi().then(setMediaSpend).catch(() => {});
  }, [refreshCloud]);

  async function saveKey(id: string) {
    const key = (keyDrafts[id] ?? "").trim();
    if (!key) return;
    setKeyBusy(id);
    try {
      await setProviderKey(id, key);
      setKeyDrafts((d) => ({ ...d, [id]: "" }));
      await refreshCloud();
      await refreshMediaModels();
    } catch {
      /* surfaced inline below would need state; keep simple */
    } finally {
      setKeyBusy(null);
    }
  }

  async function removeKey(id: string) {
    setKeyBusy(id);
    try {
      await clearProviderKey(id);
      await refreshCloud();
      await refreshMediaModels();
    } finally {
      setKeyBusy(null);
    }
  }

  function refreshPermissions() {
    listPermissions().then(setGrants).catch(() => {});
  }

  async function save() {
    await setSystemPrompt(draft);
    setSaved(true);
    setTimeout(() => setSaved(false), 1600);
  }

  async function addFolder(mode: "read" | "read-write") {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") {
      await addPermission(dir, mode);
      refreshPermissions();
    }
  }

  async function revoke(id: string) {
    await revokePermission(id);
    refreshPermissions();
  }

  async function revokeCapability(id: string) {
    await revokeCapabilityGrant(id);
    setCapabilityGrants((gs) => gs.filter((g) => g.id !== id));
  }

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>General</h1>
        <p className="lede">Your system prompt, file access, and a log of what Poiesis Agent has done.</p>

        <section className="setting-block">
          <h2 className="setting-title">System prompt</h2>
          <p className="setting-help">
            Sets how Poiesis Agent behaves across every chat. One global prompt for now; saved profiles
            come later.
          </p>
          <textarea
            className="system-prompt"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            rows={5}
            spellCheck={false}
          />
          <div className="setting-actions">
            <button className="btn-primary" onClick={save} disabled={draft === systemPrompt}>
              Save
            </button>
            {saved && <span className="saved-note">Saved</span>}
          </div>
        </section>

        {inTauri() && (
          <section className="setting-block" id="settings-personas">
            <h2 className="setting-title">Personas</h2>
            <p className="setting-help">
              Saved profiles that bundle a system prompt (and optionally a model and temperature).
              Pick one per chat from the composer; the global prompt above is the fallback.
            </p>
            <PersonaEditor />
          </section>
        )}

        <section className="setting-block">
          <h2 className="setting-title">Interface</h2>
          <p className="setting-help">
            Poiesis Agent arrives with everything switched on but most of the machinery out of sight.
          </p>
          <label className="toggle-line">
            <input
              type="checkbox"
              checked={expert}
              onChange={(e) => setExpert(e.target.checked)}
            />
            <span>
              Show me everything — every engine, every control, every setting I usually keep out
              of your way
            </span>
          </label>
          {expert && (
            <div className="setting-actions">
              <button className="btn-text" onClick={() => resetFirstTimeExplanations()}>
                Explain things to me again
              </button>
            </div>
          )}
        </section>

        {!expert && <RecallModeControl />}

        <section className="setting-block">
          <h2 className="setting-title">Theme</h2>
          <p className="setting-help">
            Switch between light and dark appearance.
          </p>
          <div className="setting-actions" role="group" aria-label="Color theme">
            <button
              className={`btn-secondary ${mode === "light" ? "selected" : ""}`}
              aria-pressed={mode === "light"}
              onClick={() => setMode("light")}
            >
              Daylight
            </button>
            <button
              className={`btn-secondary ${mode === "dark" ? "selected" : ""}`}
              aria-pressed={mode === "dark"}
              onClick={() => setMode("dark")}
            >
              Backlit
            </button>
          </div>
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Reading size</h2>
          <p className="setting-help">
            Adjusts the assistant's reading column. Text reflows — nothing is cut off.
          </p>
          <div className="setting-actions" role="group" aria-label="Reading size">
            {READING_SCALES.map((opt) => (
              <button
                key={opt.value}
                className={`btn-secondary ${readingScale === opt.value ? "selected" : ""}`}
                aria-pressed={readingScale === opt.value}
                onClick={() => setReadingScale(opt.value)}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </section>

        {inTauri() && (
          <section className="setting-block">
            <h2 className="setting-title">Cloud models — your keys</h2>
            <p className="setting-help">
              Optionally use hosted models with your own API key. Keys are stored in Windows
              Credential Manager — never in a file or in your chats. Poiesis Agent stays local-first; this
              is entirely opt-in.
            </p>
            {providers.map((p) => (
              <div className="provider-row" key={p.id}>
                <div className="provider-head">
                  <span className="provider-name">{p.name}</span>
                  <span className={`provider-status ${p.key_set ? "set" : ""}`}>
                    {p.key_set ? "Key saved" : "Not set"}
                  </span>
                </div>
                <div className="provider-controls">
                  <input
                    className="field-input"
                    type="password"
                    placeholder={p.key_hint}
                    value={keyDrafts[p.id] ?? ""}
                    onChange={(e) => setKeyDrafts((d) => ({ ...d, [p.id]: e.target.value }))}
                    onKeyDown={(e) => e.key === "Enter" && saveKey(p.id)}
                  />
                  <button
                    className="btn-secondary"
                    onClick={() => saveKey(p.id)}
                    disabled={keyBusy === p.id || !(keyDrafts[p.id] ?? "").trim()}
                  >
                    {p.key_set ? "Replace" : "Save"}
                  </button>
                  {p.key_set && (
                    <button
                      className="btn-text danger"
                      onClick={() => removeKey(p.id)}
                      disabled={keyBusy === p.id}
                    >
                      Remove
                    </button>
                  )}
                </div>
                <button className="link-button" onClick={() => openUrl(p.console_url)}>
                  Get a {p.name} key →
                </button>
              </div>
            ))}
            {/* `CST-2`: what the keys above have actually cost, in the place
                the user is already thinking about spending. Shown only once
                there is something to show — a local-only install never sees a
                money line at all. */}
            {mediaSpend && mediaSpend.all_time.usd > 0 && (
              <p className="setting-help media-spend-line">
                Media this month: <strong>${mediaSpend.month.usd.toFixed(2)}</strong>
                {mediaSpend.month.images > 0 || mediaSpend.month.videos > 0 ? (
                  <>
                    {" · "}
                    {mediaSpend.month.images} image{mediaSpend.month.images === 1 ? "" : "s"},{" "}
                    {mediaSpend.month.videos} clip{mediaSpend.month.videos === 1 ? "" : "s"}
                  </>
                ) : null}
                {" · "}${mediaSpend.all_time.usd.toFixed(2)} all time
              </p>
            )}
          </section>
        )}

        {inTauri() && <LocalEndpointsSettings />}

        <section className="setting-block">
          <h2 className="setting-title">Memory &amp; context</h2>
          <p className="setting-help">
            A model can only hold so much of a conversation at once. When a chat outgrows that,
            Poiesis Agent summarizes the older turns instead of letting them fall off the front.
            Your messages are never deleted — this changes only what the model is shown.
          </p>
          <p className="setting-readout">
            Model context window: {contextBudget.toLocaleString()} tokens
          </p>
          <label className="toggle-line">
            <input
              type="checkbox"
              checked={autoCompact}
              onChange={(e) => setAutoCompact(e.target.checked)}
            />
            <span>Summarize older turns automatically</span>
          </label>
          {/* PRES-3: the self is a place of its own; Settings only points to it. */}
          <button className="settings-self-link" onClick={() => setView("self")}>
            Memory, lessons and autonomy live in my Self panel →
          </button>
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Always-allowed folders</h2>
          <p className="setting-help">
            Folders Poiesis Agent may reach in every chat. Most work happens in a single working
            folder you attach to a conversation from the Workbench panel — these are the standing
            exceptions on top of it.
          </p>
          {grants.length === 0 && <p className="empty-hint">No folders allowed yet.</p>}
          {grants.map((g) => (
            <div className="grant-row" key={g.id}>
              <span className="grant-path">{g.path}</span>
              <span className="grant-mode">{g.mode === "read-write" ? "read & write" : "read"}</span>
              <button className="grant-revoke" onClick={() => revoke(g.id)}>
                Remove
              </button>
            </div>
          ))}
          {inTauri() && (
            <div className="setting-actions">
              <button className="btn-secondary" onClick={() => addFolder("read")}>
                Add a folder (read)
              </button>
              <button className="btn-secondary" onClick={() => addFolder("read-write")}>
                Add a folder (read &amp; write)
              </button>
            </div>
          )}
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Always-allowed sites &amp; apps</h2>
          <p className="setting-help">
            Domains and applications you told Poiesis Agent it never needs to ask about again —
            from the Browser and Screen &amp; apps tools.
          </p>
          {capabilityGrants.length === 0 && <p className="empty-hint">Nothing standing yet.</p>}
          {capabilityGrants.map((g) => (
            <div className="grant-row" key={g.id}>
              <span className="grant-path">{g.value}</span>
              <span className="grant-mode">{g.kind === "domain" ? "site" : "app"}</span>
              <button className="grant-revoke" onClick={() => revokeCapability(g.id)}>
                Remove
              </button>
            </div>
          ))}
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Activity</h2>
          <p className="setting-help">
            The log of everything Poiesis Agent did on your computer now has its own section.
          </p>
          <button className="btn-secondary" onClick={() => setView("activity")}>
            Open Activity
          </button>
        </section>
      </div>
    </div>
  );
}
