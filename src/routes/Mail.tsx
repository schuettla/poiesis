import { useEffect, useState } from "react";
import {
  inTauri,
  listMailAccounts,
  type MailSecurity,
  addMailAccount,
  testMailAccount,
  setMailAccountEnabled,
  deleteMailAccount,
  type MailAccount,
  type MailTestResult,
} from "../lib/api";
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

export default function Mail() {
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

  useEffect(() => {
    refreshMailAccounts();
  }, []);

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

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Mail</h1>
        <p className="lede">
          Connect an email account so Poiesis Agent can read and, with your approval, send mail
          for you — direct IMAP/SMTP, credentials in Windows Credential Manager. Nothing goes
          through a Poiesis server.
        </p>

        <section className="setting-block">
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
      </div>
    </div>
  );
}
