import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getAppVersion,
  inTauri,
  listPermissions,
  addPermission,
  revokePermission,
  listCapabilityGrants,
  revokeCapabilityGrant,
  setProviderKey,
  clearProviderKey,
  listToolsets,
  setToolsetEnabled,
  getToolStats,
  listIndexRoots,
  embedEngineStatus,
  rerankEngineStatus,
  installRerankEngine,
  setRerankEnabled,
  listMailAccounts,
  type MailSecurity,
  addMailAccount,
  testMailAccount,
  setMailAccountEnabled,
  deleteMailAccount,
  type Grant,
  type CapabilityGrant,
  type ToolsetInfo,
  type ToolsetReliability,
  type IndexRootView,
  type RerankSetupStatus,
  type DownloadProgress,
  type MailAccount,
  type MailTestResult,
  mediaSpend as mediaSpendApi,
  type MediaSpendReport,
} from "../lib/api";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppStore, READING_SCALES } from "../lib/store";
import PersonaEditor from "../components/Personas/PersonaEditor";
import "./Surface.css";
import "./Settings.css";

/** `MAIL-1` provider presets: the #1 setup failure is an app-password the
 * user didn't know to create, so each preset carries its own instructions
 * rather than a bare link. */
const MAIL_PRESETS = {
  gmail: {
    label: "Gmail",
    imapHost: "imap.gmail.com",
    imapPort: 993,
    smtpHost: "smtp.gmail.com",
    smtpPort: 465,
    security: "tls" as MailSecurity,
    hint: "Use an app password, not your normal Google password: myaccount.google.com → Security → 2-Step Verification → App passwords.",
  },
  icloud: {
    label: "iCloud",
    imapHost: "imap.mail.me.com",
    imapPort: 993,
    smtpHost: "smtp.mail.me.com",
    smtpPort: 587,
    // iCloud submission is 587/STARTTLS — pinning implicit TLS here is what
    // made this preset unable to connect at all.
    security: "starttls" as MailSecurity,
    hint: "Use an app-specific password from appleid.apple.com → Sign-In and Security → App-Specific Passwords.",
  },
  fastmail: {
    label: "Fastmail",
    imapHost: "imap.fastmail.com",
    imapPort: 993,
    smtpHost: "smtp.fastmail.com",
    smtpPort: 465,
    security: "tls" as MailSecurity,
    hint: "Create an app password in Settings → Password & Security → App Passwords.",
  },
  protonbridge: {
    label: "Proton Bridge",
    imapHost: "127.0.0.1",
    imapPort: 1143,
    smtpHost: "127.0.0.1",
    smtpPort: 1025,
    // The Bridge listens in the clear and upgrades, with a certificate it
    // signed itself — accepted only because the host is loopback.
    security: "starttls" as MailSecurity,
    hint: "Proton Mail needs the Bridge app running locally first — use the host/port and password it shows you, not your Proton password.",
  },
  generic: {
    label: "Generic",
    imapHost: "",
    imapPort: 993,
    smtpHost: "",
    smtpPort: 465,
    security: "tls" as MailSecurity,
    hint: "Ask your provider for its IMAP/SMTP host and port.",
  },
} as const;

const ATTRIBUTIONS = [
  { name: "llama.cpp", license: "MIT", what: "Local model engine (llama-server)" },
  { name: "Tauri", license: "MIT / Apache-2.0", what: "Desktop application shell" },
  { name: "React", license: "MIT", what: "User interface" },
  { name: "Newsreader, Inter, JetBrains Mono", license: "OFL / MIT", what: "Typefaces" },
  { name: "rusqlite / SQLite", license: "MIT / Public Domain", what: "Local storage + search" },
  { name: "Model weights", license: "Per-model (shown on each model)", what: "e.g. Llama Community, Apache-2.0" },
];

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

export default function Settings() {
  const systemPrompt = useAppStore((s) => s.systemPrompt);
  const setView = useAppStore((s) => s.setView);
  const setSystemPrompt = useAppStore((s) => s.setSystemPrompt);
  const [draft, setDraft] = useState(systemPrompt);
  const [saved, setSaved] = useState(false);
  const [version, setVersion] = useState("");
  const [grants, setGrants] = useState<Grant[]>([]);
  const [capabilityGrants, setCapabilityGrants] = useState<CapabilityGrant[]>([]);
  const [toolsets, setToolsets] = useState<ToolsetInfo[]>([]);
  const [reliability, setReliability] = useState<ToolsetReliability[]>([]);
  const [indexRoots, setIndexRoots] = useState<IndexRootView[]>([]);
  const [mailAccounts, setMailAccounts] = useState<MailAccount[]>([]);
  const [mailBusyId, setMailBusyId] = useState<string | null>(null);
  const [mailTestResults, setMailTestResults] = useState<Record<string, MailTestResult>>({});
  const [mailFormOpen, setMailFormOpen] = useState(false);
  const [mailPreset, setMailPreset] = useState<keyof typeof MAIL_PRESETS>("gmail");
  const [mailLabel, setMailLabel] = useState("");
  const [mailEmail, setMailEmail] = useState("");
  const [mailPassword, setMailPassword] = useState("");
  const [mailImapHost, setMailImapHost] = useState<string>(MAIL_PRESETS.gmail.imapHost);
  const [mailImapPort, setMailImapPort] = useState<number>(MAIL_PRESETS.gmail.imapPort);
  const [mailSmtpHost, setMailSmtpHost] = useState<string>(MAIL_PRESETS.gmail.smtpHost);
  const [mailSmtpPort, setMailSmtpPort] = useState<number>(MAIL_PRESETS.gmail.smtpPort);
  const [mailSecurity, setMailSecurity] = useState<MailSecurity>(MAIL_PRESETS.gmail.security);
  const [mailSaving, setMailSaving] = useState(false);
  const [mailError, setMailError] = useState<string | null>(null);
  const forgetFolderIndex = useAppStore((s) => s.forgetFolderIndex);
  const providers = useAppStore((s) => s.providers);
  const refreshCloud = useAppStore((s) => s.refreshCloud);
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
  const telemetryEnabled = useAppStore((s) => s.telemetryEnabled);
  const setTelemetryEnabled = useAppStore((s) => s.setTelemetryEnabled);
  const contextBudget = useAppStore((s) => s.contextBudget);
  const autoCompact = useAppStore((s) => s.autoCompact);
  const setAutoCompact = useAppStore((s) => s.setAutoCompact);
  const expert = useAppStore((s) => s.expert);
  const setExpert = useAppStore((s) => s.setExpert);
  const resetFirstTimeExplanations = useAppStore((s) => s.resetFirstTimeExplanations);

  useEffect(() => setDraft(systemPrompt), [systemPrompt]);
  useEffect(() => {
    if (!inTauri()) return;
    getAppVersion().then(setVersion).catch(() => {});
    refreshPermissions();
    listCapabilityGrants().then(setCapabilityGrants).catch(() => {});
    refreshCloud();
    listToolsets().then(setToolsets).catch(() => {});
    getToolStats().then(setReliability).catch(() => {});
    listIndexRoots().then(setIndexRoots).catch(() => {});
    refreshMailAccounts();
    mediaSpendApi().then(setMediaSpend).catch(() => {});
  }, [refreshCloud]);

  function formatDiskSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    const kb = bytes / 1024;
    if (kb < 1024) return `${kb.toFixed(1)} KB`;
    return `${(kb / 1024).toFixed(1)} MB`;
  }

  async function forgetIndexRoot(path: string) {
    await forgetFolderIndex(path);
    setIndexRoots((list) => list.filter((r) => r.path !== path));
  }

  async function toggleToolset(id: string, enabled: boolean) {
    // Optimistic; revert on failure.
    setToolsets((list) => list.map((s) => (s.id === id ? { ...s, enabled } : s)));
    try {
      await setToolsetEnabled(id, enabled);
    } catch {
      setToolsets((list) => list.map((s) => (s.id === id ? { ...s, enabled: !enabled } : s)));
    }
  }

  function refreshMailAccounts() {
    if (!inTauri()) return;
    listMailAccounts().then(setMailAccounts).catch(() => {});
  }

  function applyMailPreset(key: keyof typeof MAIL_PRESETS) {
    setMailPreset(key);
    const p = MAIL_PRESETS[key];
    setMailImapHost(p.imapHost);
    setMailImapPort(p.imapPort);
    setMailSmtpHost(p.smtpHost);
    setMailSmtpPort(p.smtpPort);
    setMailSecurity(p.security);
  }

  async function addAccount() {
    setMailSaving(true);
    setMailError(null);
    try {
      await addMailAccount({
        label: mailLabel.trim() || MAIL_PRESETS[mailPreset].label,
        email: mailEmail.trim(),
        imapHost: mailImapHost.trim(),
        imapPort: mailImapPort,
        smtpHost: mailSmtpHost.trim(),
        smtpPort: mailSmtpPort,
        username: mailEmail.trim(),
        password: mailPassword,
        security: mailSecurity,
      });
      setMailLabel("");
      setMailEmail("");
      setMailPassword("");
      setMailFormOpen(false);
      refreshMailAccounts();
    } catch (e) {
      setMailError(String(e));
    } finally {
      setMailSaving(false);
    }
  }

  async function testAccount(id: string) {
    setMailBusyId(id);
    try {
      const result = await testMailAccount(id);
      setMailTestResults((r) => ({ ...r, [id]: result }));
    } catch (e) {
      setMailTestResults((r) => ({ ...r, [id]: { ok: false, message_count: null, error: String(e) } }));
    } finally {
      setMailBusyId(null);
    }
  }

  async function toggleMailAccount(a: MailAccount) {
    setMailBusyId(a.id);
    setMailAccounts((list) => list.map((x) => (x.id === a.id ? { ...x, enabled: !a.enabled } : x)));
    try {
      await setMailAccountEnabled(a.id, !a.enabled);
    } catch {
      setMailAccounts((list) => list.map((x) => (x.id === a.id ? { ...x, enabled: a.enabled } : x)));
    } finally {
      setMailBusyId(null);
    }
  }

  async function removeMailAccount(id: string) {
    setMailBusyId(id);
    try {
      await deleteMailAccount(id);
      refreshMailAccounts();
    } finally {
      setMailBusyId(null);
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

        {inTauri() && toolsets.length > 0 && (
          <section className="setting-block">
            <h2 className="setting-title">Tools</h2>
            <p className="setting-help">
              What Poiesis Agent can do beyond chatting, when tools are turned on in a chat. Each one is
              opt-in; those that leave your device or run code are marked.
            </p>
            {toolsets.map((s) => {
              const rel = reliability.find((r) => r.skill_id === s.id);
              return (
                <div key={s.id} className="toolset-item">
                  <label className="toggle-line toolset-line">
                    <input
                      type="checkbox"
                      checked={s.enabled}
                      onChange={(e) => toggleToolset(s.id, e.target.checked)}
                    />
                    <span className="toolset-text">
                      <span className="toolset-label">
                        {s.label}
                        {s.sensitive && <span className="toolset-flag">leaves device / runs code</span>}
                      </span>
                      <span className="toolset-desc">{s.description}</span>
                      {rel && (
                        <span className="toolset-reliability">
                          {rel.ok_percent}% ok over {rel.calls} call{rel.calls === 1 ? "" : "s"} this
                          week
                        </span>
                      )}
                    </span>
                  </label>
                  {/* IDX-UI-4: the indexed folders this tool has built, wherever
                      they were attached from — with the one undo that matters. */}
                  {s.id === "indexing" && indexRoots.length > 0 && (
                    <ul className="toolset-subitems">
                      {indexRoots.map((r) => (
                        <li key={r.path} className="toolset-subitem">
                          <span className="toolset-subitem-path" title={r.path}>
                            {r.path}
                          </span>
                          <span className="toolset-subitem-meta">{formatDiskSize(r.size_bytes)}</span>
                          <button className="link-button" onClick={() => forgetIndexRoot(r.path)}>
                            Forget this folder
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              );
            })}
          </section>
        )}

        {inTauri() && (
          <section className="setting-block">
            <h2 className="setting-title">Mail</h2>
            <p className="setting-help">
              Connect an email account so Poiesis Agent can read and, with your approval, send mail
              for you — direct IMAP/SMTP, credentials in Windows Credential Manager. Nothing goes
              through a Poiesis server.
            </p>

            {mailAccounts.map((a) => {
              const result = mailTestResults[a.id];
              return (
                <div key={a.id} className={`toolset-item ${a.enabled ? "" : "disabled"}`}>
                  <label className="toggle-line toolset-line">
                    <input
                      type="checkbox"
                      checked={a.enabled}
                      disabled={mailBusyId === a.id}
                      onChange={() => toggleMailAccount(a)}
                    />
                    <span className="toolset-text">
                      <span className="toolset-label">{a.label}</span>
                      <span className="toolset-desc">{a.email}</span>
                      {result && (
                        <span className={`toolset-reliability ${result.ok ? "" : "error"}`}>
                          {result.ok
                            ? `I reached your inbox (${result.message_count ?? 0} messages) and the send server accepted me.`
                            : `Couldn't connect: ${result.error}`}
                        </span>
                      )}
                    </span>
                  </label>
                  <div className="connect-actions">
                    <button className="btn-secondary" onClick={() => testAccount(a.id)} disabled={mailBusyId === a.id}>
                      {mailBusyId === a.id ? "Checking…" : "Test"}
                    </button>
                    <button className="btn-text danger" onClick={() => removeMailAccount(a.id)} disabled={mailBusyId === a.id}>
                      Remove
                    </button>
                  </div>
                </div>
              );
            })}

            {!mailFormOpen ? (
              <button className="btn-secondary" onClick={() => setMailFormOpen(true)}>
                Add account
              </button>
            ) : (
              <div className="connect-card">
                <div className="transport-toggle" role="group" aria-label="Mail provider">
                  {(Object.keys(MAIL_PRESETS) as (keyof typeof MAIL_PRESETS)[]).map((key) => (
                    <button
                      key={key}
                      className={`seg ${mailPreset === key ? "on" : ""}`}
                      aria-pressed={mailPreset === key}
                      onClick={() => applyMailPreset(key)}
                    >
                      {MAIL_PRESETS[key].label}
                    </button>
                  ))}
                </div>
                <p className="field-hint">{MAIL_PRESETS[mailPreset].hint}</p>
                {mailPreset === "generic" && (
                  <div className="connect-fields">
                    <label className="field">
                      <span className="field-label">IMAP host</span>
                      <input className="field-input" value={mailImapHost} onChange={(e) => setMailImapHost(e.target.value)} />
                    </label>
                    <label className="field">
                      <span className="field-label">SMTP host</span>
                      <input className="field-input" value={mailSmtpHost} onChange={(e) => setMailSmtpHost(e.target.value)} />
                    </label>
                    <label className="field">
                      <span className="field-label">IMAP port</span>
                      <input
                        className="field-input"
                        type="number"
                        value={mailImapPort}
                        onChange={(e) => setMailImapPort(Number(e.target.value))}
                      />
                    </label>
                    <label className="field">
                      <span className="field-label">SMTP port</span>
                      <input
                        className="field-input"
                        type="number"
                        value={mailSmtpPort}
                        onChange={(e) => setMailSmtpPort(Number(e.target.value))}
                      />
                    </label>
                    <label className="field">
                      <span className="field-label">Connection</span>
                      <select
                        className="field-input"
                        value={mailSecurity}
                        onChange={(e) => setMailSecurity(e.target.value as MailSecurity)}
                      >
                        <option value="tls">TLS (usually ports 993 and 465)</option>
                        <option value="starttls">STARTTLS (usually ports 143 and 587)</option>
                      </select>
                    </label>
                  </div>
                )}
                <div className="connect-fields">
                  <label className="field">
                    <span className="field-label">Label</span>
                    <input
                      className="field-input"
                      placeholder="Personal"
                      value={mailLabel}
                      onChange={(e) => setMailLabel(e.target.value)}
                    />
                  </label>
                  <label className="field">
                    <span className="field-label">Email</span>
                    <input
                      className="field-input"
                      placeholder="you@example.com"
                      value={mailEmail}
                      onChange={(e) => setMailEmail(e.target.value)}
                    />
                  </label>
                  <label className="field">
                    <span className="field-label">App password</span>
                    <input
                      className="field-input"
                      type="password"
                      value={mailPassword}
                      onChange={(e) => setMailPassword(e.target.value)}
                    />
                  </label>
                </div>
                {mailError && <p className="hw-note error">{mailError}</p>}
                <div className="connect-actions">
                  <button
                    className="btn-primary"
                    onClick={addAccount}
                    disabled={mailSaving || !mailEmail.trim() || !mailPassword}
                  >
                    {mailSaving ? "Adding…" : "Add"}
                  </button>
                  <button className="btn-secondary" onClick={() => setMailFormOpen(false)}>
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </section>
        )}

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

        <section className="setting-block">
          <h2 className="setting-title">Privacy</h2>
          <p className="setting-help">
            Poiesis Agent is local-first. Anonymous usage stats are <strong>off</strong> by default and
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
            <span>Help improve Poiesis Agent with anonymous, content-free usage counts</span>
          </label>
        </section>

        <section className="setting-block">
          <h2 className="setting-title">About &amp; licenses</h2>
          <p className="setting-help">
            Poiesis Agent is built on open-source software. Thank you to these projects.
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

        <p className="version-note">{version ? `Poiesis Agent v${version}` : "Browser preview"}</p>
      </div>
    </div>
  );
}
