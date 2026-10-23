import { useCallback, useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  accountBroken,
  accountExpired,
  accountWaiting,
  composioAccounts,
  composioAppTools,
  composioCancel,
  composioDisconnect,
  composioReconnect,
  composioRemoveKey,
  composioRename,
  composioSaveKey,
  type AppAccount,
  type ComposioStatus,
  type ToolView,
} from "../../lib/api";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import AppLogo from "./AppLogo";
import BrowseApps from "./BrowseApps";
import { ToolChips, WhatIMayDo } from "./ConnectorGate";
import "./ComposioApps.css";

/** Where a Connect (`ck_`) key is found, and where the sign-ins are managed. */
const COMPOSIO_URL = "https://dashboard.composio.dev";
/** The developer platform, for a project (`ak_`) key. */
const COMPOSIO_PLATFORM_URL = "https://platform.composio.dev";

const clean = (e: unknown) => String(e).replace(/^Error:\s*/, "");

/** `CA-UI-3`: a status in words, never only a colour. */
function statusWords(a: AppAccount): string {
  if (a.status === "ACTIVE") return "Connected";
  if (accountWaiting(a)) return "Waiting for sign-in";
  if (accountExpired(a)) return "Sign-in expired";
  if (a.status === "FAILED") return "Sign-in didn't finish";
  return "Not connected";
}

function dotClass(a: AppAccount): string {
  if (a.status === "ACTIVE") return "on";
  if (accountWaiting(a)) return "wait";
  return "bad";
}

/** `CAP-C6`: with a Connect key, an app with two accounts or more has a row for
 * each (`connect:{app}:{id}`); Disconnect on one removes only that account. */
const oneOfSeveral = (a: AppAccount) => a.id.startsWith(`connect:${a.app}:`) && !accountWaiting(a);

/**
 * `MCG-7` for a Composio app: the tools of it I've used, each marked with what
 * it does, so a wrong guess is corrected here. Composio shows no tool list per
 * app, so these (and any given a kind from an approval) are the list.
 */
function UsedTools({ connectorId, app, onError }: { connectorId: string; app: string; onError: (m: string) => void }) {
  const [tools, setTools] = useState<ToolView[] | null>(null);
  const load = useCallback(
    () =>
      composioAppTools(app)
        .then(setTools)
        .catch(() => setTools([])),
    [app]
  );
  useEffect(() => {
    void load();
  }, [load]);
  if (!tools || tools.length === 0) return null;
  return (
    <div className="connector-tools">
      <ToolChips connector={{ id: connectorId, tools }} app={app} lead="Tools I've used:" onChanged={load} onError={onError} />
    </div>
  );
}

/** Naming one account in place. */
function NameField({
  initial,
  busy,
  onSave,
  onCancel,
}: {
  initial: string;
  busy: boolean;
  onSave: (name: string) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initial);
  return (
    <span className="composio-name-field">
      <input
        className="field-input"
        aria-label="Name for this account"
        placeholder="work"
        value={name}
        maxLength={64}
        autoFocus
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && name.trim()) onSave(name.trim());
          if (e.key === "Escape") onCancel();
        }}
      />
      <button className="btn-secondary" disabled={busy || !name.trim()} onClick={() => onSave(name.trim())}>
        Save
      </button>
      <button className="btn-text" onClick={onCancel}>
        Cancel
      </button>
    </span>
  );
}

/**
 * `CA-UI-1`/`CAP-C-UI-1`: the Composio key card and the apps signed in through
 * it. The key is pasted here, kept in the credential store, and never shown
 * again. A `ck_` key (Composio Connect, the person's own) is the default; a
 * developer `ak_` key still works and shows the fuller account list.
 */
export default function ComposioSection({
  status,
  reload,
  onError,
  checkError,
}: {
  status: ComposioStatus | null;
  reload: () => void | Promise<void>;
  onError: (message: string) => void;
  /** Why the last look at an open sign-in failed, if it did. */
  checkError?: string | null;
}) {
  const [draft, setDraft] = useState("");
  const [changing, setChanging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [browsing, setBrowsing] = useState<{ focus?: string } | null>(null);
  const [removingKey, setRemovingKey] = useState(false);
  const [leaving, setLeaving] = useState<AppAccount | null>(null);
  const [naming, setNaming] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  const configured = status?.configured ?? false;
  const isConnect = status?.mode === "connect";
  const live = (status?.accounts ?? []).filter((a) => a.status !== "REVOKED");
  const connectedApps = new Set(live.filter((a) => a.status === "ACTIVE").map((a) => a.app));
  const connectedAccounts = live.filter((a) => a.status === "ACTIVE").length;
  const needsAttention = live.some(accountExpired) || !!status?.last_error;
  const state = !configured ? "off" : needsAttention ? "attention" : "on";
  const grouped = useMemo(() => {
    const by = new Map<string, AppAccount[]>();
    for (const a of live) by.set(a.app, [...(by.get(a.app) ?? []), a]);
    return [...by.entries()];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  async function save() {
    const key = draft.trim();
    if (!key) return;
    setBusy(true);
    setProblem(null);
    setAccepted(false);
    try {
      await composioSaveKey(key);
      setDraft("");
      setChanging(false);
      setAccepted(true);
      await reload();
    } catch (e) {
      // The field keeps its value so a typo can be fixed in place.
      setProblem(clean(e));
    } finally {
      setBusy(false);
    }
  }

  async function removeKey() {
    setRemovingKey(false);
    setBusy(true);
    try {
      await composioRemoveKey();
      setAccepted(false);
      await reload();
    } catch (e) {
      onError(clean(e));
    } finally {
      setBusy(false);
    }
  }

  async function reconnect(a: AppAccount) {
    setBusy(true);
    try {
      await composioReconnect(a.id);
      await reload();
    } catch (e) {
      onError(clean(e));
    } finally {
      setBusy(false);
    }
  }

  async function disconnect(a: AppAccount) {
    setLeaving(null);
    setBusy(true);
    try {
      await composioDisconnect(a.id);
      await reload();
    } catch (e) {
      onError(clean(e));
    } finally {
      setBusy(false);
    }
  }

  async function rename(a: AppAccount, name: string) {
    setBusy(true);
    try {
      await composioRename(a.id, name);
      setNaming(null);
      await reload();
    } catch (e) {
      onError(clean(e));
    } finally {
      setBusy(false);
    }
  }

  async function cancelSignIn(a: AppAccount) {
    setBusy(true);
    try {
      await composioCancel(a.id);
      await reload();
    } catch (e) {
      onError(clean(e));
    } finally {
      setBusy(false);
    }
  }

  /** Connect has no account list: ask about the apps I know of. */
  async function checkAgain() {
    setBusy(true);
    try {
      await composioAccounts(true);
      await reload();
    } catch (e) {
      onError(clean(e));
    } finally {
      setBusy(false);
    }
  }

  const keyField = (
    <div className="composio-key">
      <input
        className="field-input"
        type="password"
        autoFocus={changing}
        aria-label="Composio key"
        placeholder="Paste your Composio key"
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          setProblem(null);
        }}
        onKeyDown={(e) => e.key === "Enter" && void save()}
      />
      <button className="btn-primary" disabled={busy || !draft.trim()} onClick={() => void save()}>
        {busy ? "Checking…" : configured ? "Replace key" : "Save"}
      </button>
      {changing && (
        <button
          className="btn-text"
          onClick={() => {
            setChanging(false);
            setDraft("");
            setProblem(null);
          }}
        >
          Cancel
        </button>
      )}
    </div>
  );

  const stateWord = state === "off" ? "Not connected" : state === "attention" ? "Needs attention" : "Connected";

  return (
    <>
      <section className="connect-card composio-card">
        <div className="provider-card-head composio-head">
          <span className={`provider-dot ${state}`} aria-hidden="true" />
          <h2 className="section-title composio-title">Hundreds of apps</h2>
          <span className={`provider-card-state ${state}`}>{stateWord}</span>
        </div>

        {!configured ? (
          <>
            <p className="composio-disclosure">
              I can reach Gmail, Slack, Notion, Linear and hundreds more through Composio. It is a separate company: when I
              use one of these apps, my request and the app's answer pass through Composio's servers, and Composio holds
              the sign-in. It runs on your own Composio account and key, and I only act when you've allowed it. Mail you set
              up under Mail never goes through them.
            </p>
            <p className="composio-disclosure">
              Composio is free to start. In Composio, open For You, then Settings, then Sessions &amp; API Key, and copy
              your key (it starts with ck_). Paste it here.{" "}
              <button className="link-button inline" onClick={() => void openUrl(COMPOSIO_URL)}>
                Open Composio →
              </button>
            </p>
            {keyField}
            <p className="composio-quiet">
              Building on Composio's developer platform? A project key (ak_) works too.{" "}
              <button className="link-button inline" onClick={() => void openUrl(COMPOSIO_PLATFORM_URL)}>
                Open the platform →
              </button>
            </p>
          </>
        ) : (
          <>
            <p className="composio-line">
              {isConnect
                ? `Connected through Composio Connect · ${connectedApps.size} ${connectedApps.size === 1 ? "app" : "apps"}`
                : `Connected through Composio · ${connectedApps.size} ${connectedApps.size === 1 ? "app" : "apps"} · ${connectedAccounts} ${connectedAccounts === 1 ? "account" : "accounts"}`}
            </p>
            {changing && keyField}
            {!changing && (
              <div className="composio-actions">
                <button className="btn-primary" onClick={() => setBrowsing({})}>
                  Browse apps
                </button>
                <div className="provider-manage composio-manage">
                  <button
                    className="btn-text"
                    aria-haspopup="menu"
                    aria-expanded={menuOpen}
                    disabled={busy}
                    onClick={() => setMenuOpen((o) => !o)}
                  >
                    Manage ▾
                  </button>
                  {menuOpen && (
                    <div className="provider-menu" role="menu" onMouseLeave={() => setMenuOpen(false)}>
                      <button
                        role="menuitem"
                        onClick={() => {
                          setMenuOpen(false);
                          setChanging(true);
                          setAccepted(false);
                        }}
                      >
                        Replace key
                      </button>
                      <button
                        role="menuitem"
                        onClick={() => {
                          setMenuOpen(false);
                          void openUrl(isConnect ? COMPOSIO_URL : COMPOSIO_PLATFORM_URL);
                        }}
                      >
                        Open Composio
                      </button>
                      <button
                        role="menuitem"
                        className="danger"
                        onClick={() => {
                          setMenuOpen(false);
                          setRemovingKey(true);
                        }}
                      >
                        Remove key
                      </button>
                    </div>
                  )}
                </div>
              </div>
            )}
          </>
        )}

        {accepted && configured && <p className="provider-card-ok">Composio accepted it.</p>}
        {problem && (
          <p className="composio-problem" role="alert">
            {problem}
          </p>
        )}
        {configured && status?.last_error && !problem && <p className="composio-problem">{status.last_error}</p>}
        {!configured && (
          <p className="composio-quiet">
            <button className="link-button inline" onClick={() => setBrowsing({})}>
              See which apps →
            </button>
          </p>
        )}
      </section>

      {grouped.length > 0 && (
        <section className="model-section">
          <h2 className="section-title">Connected through Composio</h2>
          {isConnect && (
            <p className="composio-quiet composio-hint">
              Apps you connected in Composio for other AI clients show up here once I've used them.{" "}
              <button className="link-button inline" disabled={busy} onClick={() => void checkAgain()}>
                Check again
              </button>
            </p>
          )}
          {checkError && (
            <p className="composio-problem" role="alert">
              I couldn't check your sign-in: {checkError}
            </p>
          )}
          <div className="connector-list">
            {grouped.map(([app, accounts]) => {
              const policy = status?.policies.find((p) => p.app === app);
              const anyWorking = accounts.some((a) => a.status === "ACTIVE");
              // With a Connect key a single account goes unnamed unless it was named.
              const nameOf = (a: AppAccount) =>
                !isConnect ? (a.label ?? "default") : (a.label ?? (accounts.length > 1 ? "unnamed" : null));
              return (
                <div className="connector-card" key={app}>
                  <div className="connector-head">
                    <div className="connector-title">
                      <AppLogo slug={app} name={accounts[0].app_name} />
                      <span className="connector-name">{accounts[0].app_name}</span>
                      <span className="composio-via">via Composio</span>
                    </div>
                    {(!isConnect || anyWorking) && (
                      <button className="btn-text" disabled={!configured} onClick={() => setBrowsing({ focus: app })}>
                        Add account
                      </button>
                    )}
                  </div>
                  <ul className="composio-accounts">
                    {accounts.map((a) => (
                      <li className="composio-account" key={a.id}>
                        <span className={`status-dot ${dotClass(a)}`} aria-hidden="true" />
                        {naming === a.id ? (
                          <NameField
                            initial={a.label ?? ""}
                            busy={busy}
                            onSave={(name) => void rename(a, name)}
                            onCancel={() => setNaming(null)}
                          />
                        ) : (
                          <>
                            {nameOf(a) && <span className="composio-account-label">{nameOf(a)}</span>}
                            <span className="composio-account-state">{statusWords(a)}</span>
                            <span className="composio-account-actions">
                              {accountBroken(a) && (
                                <button className="btn-secondary" disabled={busy} onClick={() => void reconnect(a)}>
                                  Reconnect
                                </button>
                              )}
                              {isConnect && !accountWaiting(a) && (
                                <button className="btn-text" disabled={busy} onClick={() => setNaming(a.id)}>
                                  {a.label ? "Rename" : "Name"}
                                </button>
                              )}
                              {isConnect && accountWaiting(a) ? (
                                <button className="btn-text" disabled={busy} onClick={() => void cancelSignIn(a)}>
                                  Cancel
                                </button>
                              ) : (
                                <button className="btn-text danger" disabled={busy} onClick={() => setLeaving(a)}>
                                  Disconnect
                                </button>
                              )}
                            </span>
                          </>
                        )}
                      </li>
                    ))}
                  </ul>
                  {status?.connector_id && <UsedTools connectorId={status.connector_id} app={app} onError={onError} />}
                  {status?.connector_id && policy && (
                    <WhatIMayDo
                      connector={{ id: status.connector_id, policy: policy.policy }}
                      app={app}
                      onChanged={reload}
                      onError={onError}
                    />
                  )}
                </div>
              );
            })}
          </div>
        </section>
      )}

      {browsing && (
        <BrowseApps
          status={status}
          reload={reload}
          focusApp={browsing.focus}
          checkError={checkError}
          onClose={() => setBrowsing(null)}
        />
      )}
      {removingKey && (
        <ConfirmDialog
          title="Remove the Composio key?"
          body="Your sign-ins stay at Composio. Paste the key again and they're back. Until then I can't use these apps."
          confirmLabel="Remove key"
          onConfirm={() => void removeKey()}
          onCancel={() => setRemovingKey(false)}
        />
      )}
      {leaving && isConnect && oneOfSeveral(leaving) && (
        <ConfirmDialog
          title={`Disconnect ${leaving.app_name} (${leaving.label ?? "unnamed"})?`}
          body={`I'll ask Composio to remove this ${leaving.app_name} account. Your other ${leaving.app_name} accounts stay. Composio keeps one set of sign-ins for all your AI clients, so other clients lose this account too.`}
          confirmLabel="Disconnect"
          onConfirm={() => void disconnect(leaving)}
          onCancel={() => setLeaving(null)}
        />
      )}
      {leaving && isConnect && !oneOfSeveral(leaving) && (
        <ConfirmDialog
          title={`Disconnect ${leaving.app_name}?`}
          body={`I'll ask Composio to remove ${leaving.app_name}. Composio keeps one set of sign-ins for all your AI clients, so any other client you connected to ${leaving.app_name} through Composio loses it too.`}
          confirmLabel="Disconnect"
          onConfirm={() => void disconnect(leaving)}
          onCancel={() => setLeaving(null)}
        />
      )}
      {leaving && !isConnect && (
        <ConfirmDialog
          title={`Disconnect ${leaving.app_name} (${leaving.label ?? "default"})?`}
          body={`I'll ask ${leaving.app_name} to revoke my access too.`}
          confirmLabel="Disconnect"
          onConfirm={() => void disconnect(leaving)}
          onCancel={() => setLeaving(null)}
        />
      )}
    </>
  );
}
