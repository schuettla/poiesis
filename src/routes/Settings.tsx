import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getAppVersion,
  inTauri,
  listPermissions,
  addPermission,
  revokePermission,
  listActivity,
  setProviderKey,
  clearProviderKey,
  listSkills,
  setSkillEnabled,
  type Grant,
  type ActivityEntry,
  type SkillInfo,
} from "../lib/api";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppStore, READING_SCALES } from "../lib/store";
import PersonaEditor from "../components/Personas/PersonaEditor";
import "./Surface.css";
import "./Settings.css";

const ATTRIBUTIONS = [
  { name: "llama.cpp", license: "MIT", what: "Local model engine (llama-server)" },
  { name: "Tauri", license: "MIT / Apache-2.0", what: "Desktop application shell" },
  { name: "React", license: "MIT", what: "User interface" },
  { name: "Newsreader, Inter, JetBrains Mono", license: "OFL / MIT", what: "Typefaces" },
  { name: "rusqlite / SQLite", license: "MIT / Public Domain", what: "Local storage + search" },
  { name: "Model weights", license: "Per-model (shown on each model)", what: "e.g. Llama Community, Apache-2.0" },
];

export default function Settings() {
  const systemPrompt = useAppStore((s) => s.systemPrompt);
  const setSystemPrompt = useAppStore((s) => s.setSystemPrompt);
  const [draft, setDraft] = useState(systemPrompt);
  const [saved, setSaved] = useState(false);
  const [version, setVersion] = useState("");
  const [grants, setGrants] = useState<Grant[]>([]);
  const [activity, setActivity] = useState<ActivityEntry[]>([]);
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const providers = useAppStore((s) => s.providers);
  const refreshCloud = useAppStore((s) => s.refreshCloud);
  const [keyDrafts, setKeyDrafts] = useState<Record<string, string>>({});
  const [keyBusy, setKeyBusy] = useState<string | null>(null);
  const mode = useAppStore((s) => s.mode);
  const setMode = useAppStore((s) => s.setMode);
  const readingScale = useAppStore((s) => s.readingScale);
  const setReadingScale = useAppStore((s) => s.setReadingScale);
  const telemetryEnabled = useAppStore((s) => s.telemetryEnabled);
  const setTelemetryEnabled = useAppStore((s) => s.setTelemetryEnabled);

  useEffect(() => setDraft(systemPrompt), [systemPrompt]);
  useEffect(() => {
    if (!inTauri()) return;
    getAppVersion().then(setVersion).catch(() => {});
    refreshPermissions();
    refreshCloud();
    listActivity(50).then(setActivity).catch(() => {});
    listSkills().then(setSkills).catch(() => {});
  }, [refreshCloud]);

  async function toggleSkill(id: string, enabled: boolean) {
    // Optimistic; revert on failure.
    setSkills((list) => list.map((s) => (s.id === id ? { ...s, enabled } : s)));
    try {
      await setSkillEnabled(id, enabled);
    } catch {
      setSkills((list) => list.map((s) => (s.id === id ? { ...s, enabled: !enabled } : s)));
    }
  }

  async function saveKey(id: string) {
    const key = (keyDrafts[id] ?? "").trim();
    if (!key) return;
    setKeyBusy(id);
    try {
      await setProviderKey(id, key);
      setKeyDrafts((d) => ({ ...d, [id]: "" }));
      await refreshCloud();
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

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Settings</h1>
        <p className="lede">Your system prompt, file access, and a log of what Nexus has done.</p>

        <section className="setting-block">
          <h2 className="setting-title">System prompt</h2>
          <p className="setting-help">
            Sets how Nexus behaves across every chat. One global prompt for now; saved profiles
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
          <section className="setting-block">
            <h2 className="setting-title">Personas</h2>
            <p className="setting-help">
              Saved profiles that bundle a system prompt (and optionally a model and temperature).
              Pick one per chat from the composer; the global prompt above is the fallback.
            </p>
            <PersonaEditor />
          </section>
        )}

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
              Paper
            </button>
            <button
              className={`btn-secondary ${mode === "dark" ? "selected" : ""}`}
              aria-pressed={mode === "dark"}
              onClick={() => setMode("dark")}
            >
              Slate
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
              Credential Manager — never in a file or in your chats. Nexus stays local-first; this
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
          </section>
        )}

        {inTauri() && skills.length > 0 && (
          <section className="setting-block">
            <h2 className="setting-title">Skills</h2>
            <p className="setting-help">
              What Nexus can do beyond chatting, when tools are turned on in a chat. Each skill is
              opt-in; those that leave your device or run code are marked.
            </p>
            {skills.map((s) => (
              <label className="toggle-line skill-line" key={s.id}>
                <input
                  type="checkbox"
                  checked={s.enabled}
                  onChange={(e) => toggleSkill(s.id, e.target.checked)}
                />
                <span className="skill-text">
                  <span className="skill-label">
                    {s.label}
                    {s.sensitive && <span className="skill-flag">leaves device / runs code</span>}
                  </span>
                  <span className="skill-desc">{s.description}</span>
                </span>
              </label>
            ))}
          </section>
        )}

        <section className="setting-block">
          <h2 className="setting-title">File access</h2>
          <p className="setting-help">
            Folders Nexus is allowed to read or change. Nothing is accessible until you allow it.
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
          <h2 className="setting-title">Activity</h2>
          <p className="setting-help">Everything Nexus did on your computer, most recent first.</p>
          {activity.length === 0 && <p className="empty-hint">No activity yet.</p>}
          <ul className="activity-list">
            {activity.map((a) => (
              <li key={a.id} className="activity-row">
                <span className={`activity-kind kind-${a.kind}`}>{a.kind}</span>
                <span className="activity-detail">{a.detail}</span>
                <span className="activity-time">
                  {new Date(a.created_at).toLocaleString()}
                </span>
              </li>
            ))}
          </ul>
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Privacy</h2>
          <p className="setting-help">
            Nexus is local-first. Anonymous usage stats are <strong>off</strong> by default and
            <strong> content-free</strong> — only counts of actions (like how many chats you start),
            never your messages, files, prompts, or model choices. Nothing is sent anywhere in this
            version; the counts stay on your PC.
          </p>
          <label className="toggle-line">
            <input
              type="checkbox"
              checked={telemetryEnabled}
              onChange={(e) => setTelemetryEnabled(e.target.checked)}
            />
            <span>Help improve Nexus with anonymous, content-free usage counts</span>
          </label>
        </section>

        <section className="setting-block">
          <h2 className="setting-title">About &amp; licenses</h2>
          <p className="setting-help">
            Nexus is built on open-source software. Thank you to these projects.
          </p>
          <ul className="attribution-list">
            {ATTRIBUTIONS.map((a) => (
              <li key={a.name} className="attribution-row">
                <span className="attribution-name">{a.name}</span>
                <span className="attribution-what">{a.what}</span>
                <span className="attribution-license">{a.license}</span>
              </li>
            ))}
          </ul>
        </section>

        <p className="version-note">{version ? `Nexus v${version}` : "Browser preview"}</p>
      </div>
    </div>
  );
}
