import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  inTauri,
  verifyProviderKey,
  clearProviderKey,
  mediaSpend as mediaSpendApi,
  usageSummary,
  type ProviderInfo,
} from "../lib/api";
import { useAppStore } from "../lib/store";
import ConfirmDialog from "../components/Confirm/ConfirmDialog";
import "./Surface.css";
import "./Settings.css";
import "./Providers.css";

/** "chat · images · video", or "312 chat models · images" once models are known. */
function unlocksLine(p: ProviderInfo, chatCount: number): string {
  const words = p.unlocks.map((u) => {
    if (u === "chat") return chatCount > 0 ? `${chatCount} chat ${chatCount === 1 ? "model" : "models"}` : "Chat";
    return u === "image" ? "images" : u === "video" ? "video" : u;
  });
  const line = words.join(" · ");
  return line.charAt(0).toUpperCase() + line.slice(1);
}

/** What the success line says a new connection unlocked. */
function unlockedSentence(p: ProviderInfo, chatCount: number): string {
  const parts: string[] = [];
  if (p.unlocks.includes("chat")) {
    parts.push(chatCount > 0 ? `${chatCount} chat ${chatCount === 1 ? "model" : "models"}` : "chat models");
  }
  const media = p.unlocks.filter((u) => u !== "chat").map((u) => (u === "image" ? "images" : u));
  if (media.length) parts.push(media.join(" and "));
  if (parts.length === 0) return "Connected.";
  return `Connected — ${parts.join(", ")} ${parts.length === 1 && chatCount === 1 ? "is" : "are"} now available.`;
}

/** The first of the month, as a day count `usageSummary` understands. */
function daysThisMonth(): number {
  return new Date().getDate();
}

/** `PRV-1`: cloud accounts, one card each, built from the backend's list so a
 * new provider appears without a UI change (`PRV-2`). Own servers live under
 * Runtime → Your servers (`RTM-10`). */
export default function Providers() {
  const providers = useAppStore((s) => s.providers);
  const refreshCloud = useAppStore((s) => s.refreshCloud);
  const setView = useAppStore((s) => s.setView);
  const openRuntime = useAppStore((s) => s.openRuntime);
  const [spend, setSpend] = useState<number | null>(null);

  useEffect(() => {
    if (!inTauri()) return;
    refreshCloud();
    // `PRV-5`: this month's spend across media and priced chat usage. Hidden
    // at $0, as before. Per-provider figures need the provider on each usage
    // row, which isn't recorded yet, so this is the header total only.
    Promise.all([
      mediaSpendApi().catch(() => null),
      usageSummary(daysThisMonth()).catch(() => null),
    ]).then(([media, usage]) => {
      const chat = (usage?.by_model ?? [])
        .filter((b) => b.provenance === "cloud")
        .reduce((sum, b) => sum + (b.cost_usd ?? 0), 0);
      setSpend((media?.month.usd ?? 0) + chat);
    });
  }, [refreshCloud]);

  if (!inTauri()) {
    return (
      <div className="surface">
        <div className="surface-inner">
          <h1>Providers</h1>
          <p className="lede">Cloud accounts are connected in the desktop app.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Providers</h1>
        <p className="lede">
          Connect your accounts to use hosted models. Keys stay in Windows Credential Manager,
          never in a file or your chats.
        </p>

        <div className="providers-head">
          <h2 className="section-title">Cloud accounts</h2>
          {spend !== null && spend > 0 && (
            <button className="link-button" onClick={() => setView("usage")}>
              Spent this month: ${spend.toFixed(2)} →
            </button>
          )}
        </div>

        <div className="provider-cards">
          {providers.map((p) => (
            <ProviderCard key={p.id} provider={p} />
          ))}
        </div>

        <p className="setting-help providers-servers-hint">
          Running Ollama or LM Studio?{" "}
          <button className="link-button inline" onClick={() => openRuntime("servers")}>
            Add it under Runtime → Your servers
          </button>
        </p>
      </div>
    </div>
  );
}

function ProviderCard({ provider: p }: { provider: ProviderInfo }) {
  const models = useAppStore((s) => s.models);
  const refreshCloud = useAppStore((s) => s.refreshCloud);
  const refreshMediaModels = useAppStore((s) => s.refreshMediaModels);
  const openModelsFiltered = useAppStore((s) => s.openModelsFiltered);
  const providerFocus = useAppStore((s) => s.providerFocus);
  const defaultChat = useAppStore((s) => s.modelPrefs.defaults.chat);
  const labels = useAppStore((s) => s.modelPrefs.labels);

  const [formOpen, setFormOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  const chatCount = models.filter((m) => m.provenance === "cloud" && !m.modality && m.provider === p.id).length;
  const state = !p.key_set ? "off" : p.last_error ? "attention" : "on";
  const seeModels = () => openModelsFiltered({ source: `cloud:${p.id}`, label: `Via ${p.name}` });

  // Arriving from Models' "Connect an account" door (`MOD-7`) lands on this
  // card with its form open.
  useEffect(() => {
    if (providerFocus !== p.id) return;
    ref.current?.scrollIntoView({ block: "center" });
    if (!p.key_set) setFormOpen(true);
    useAppStore.setState({ providerFocus: null });
  }, [providerFocus, p.id, p.key_set]);

  async function connect() {
    const key = draft.trim();
    if (!key) return;
    setBusy(true);
    setError(null);
    setSuccess(null);
    try {
      await verifyProviderKey(p.id, key);
      await Promise.all([refreshCloud(), refreshMediaModels()]);
      const count = useAppStore
        .getState()
        .models.filter((m) => m.provenance === "cloud" && !m.modality && m.provider === p.id).length;
      setDraft("");
      setFormOpen(false);
      setSuccess(unlockedSentence(p, count));
    } catch (e) {
      // The field keeps its value so a typo can be fixed in place.
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  }

  async function disconnect() {
    setConfirming(false);
    setBusy(true);
    try {
      await clearProviderKey(p.id);
      await Promise.all([refreshCloud(), refreshMediaModels()]);
      setSuccess(null);
    } finally {
      setBusy(false);
    }
  }

  const defaultHere = defaultChat?.startsWith(`cloud:${p.id}:`) ?? false;
  const disconnectBody =
    `${p.name}'s models stop working in Poiesis. Chats that use them fall back to your default.` +
    (defaultHere
      ? ` ${labels[defaultChat!] ?? "Your default model"} is one of them, so your next favorite takes over.`
      : "");

  return (
    <div className={`provider-card ${state}`} ref={ref}>
      <div className="provider-card-head">
        <span className={`provider-dot ${state}`} aria-hidden="true" />
        <span className="provider-card-name">{p.name}</span>
        <span className={`provider-card-state ${state}`}>
          {state === "off" ? "Not connected" : state === "attention" ? "Needs attention" : "Connected"}
        </span>
      </div>
      <p className="provider-card-unlocks">{unlocksLine(p, p.key_set ? chatCount : 0)}</p>
      {state === "attention" && (
        <p className="provider-card-problem">
          {p.last_error}{" "}
          {p.console_url && (
            <button className="link-button inline" onClick={() => openUrl(p.console_url)}>
              Open {p.name} →
            </button>
          )}
        </p>
      )}
      {success && (
        <p className="provider-card-ok">
          {success}{" "}
          {p.unlocks.includes("chat") && (
            <button className="link-button inline" onClick={seeModels}>
              See models →
            </button>
          )}
        </p>
      )}

      {formOpen && (
        <div className="provider-card-form">
          <input
            className="field-input"
            type="password"
            autoFocus
            aria-label={`${p.name} key`}
            placeholder={p.key_hint}
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => e.key === "Enter" && connect()}
          />
          {error && (
            <p className="provider-card-error" role="alert">
              {error}
            </p>
          )}
          <div className="provider-card-actions">
            <button className="btn-primary" onClick={connect} disabled={busy || !draft.trim()}>
              {busy ? "Checking…" : p.key_set ? "Replace key" : "Connect"}
            </button>
            <button
              className="btn-text"
              onClick={() => {
                setFormOpen(false);
                setError(null);
                setDraft("");
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {!formOpen && (
        <div className="provider-card-actions">
          {!p.key_set ? (
            <button className="btn-secondary" onClick={() => setFormOpen(true)} disabled={busy}>
              Connect
            </button>
          ) : (
            p.unlocks.includes("chat") && (
              <button className="btn-secondary" onClick={seeModels}>
                See models
              </button>
            )
          )}
          {p.key_set ? (
            <div className="provider-manage">
              <button
                className="btn-text"
                aria-haspopup="menu"
                aria-expanded={menuOpen}
                onClick={() => setMenuOpen((o) => !o)}
                disabled={busy}
              >
                Manage ▾
              </button>
              {menuOpen && (
                <div className="provider-menu" role="menu" onMouseLeave={() => setMenuOpen(false)}>
                  <button
                    role="menuitem"
                    onClick={() => {
                      setMenuOpen(false);
                      setFormOpen(true);
                      setSuccess(null);
                    }}
                  >
                    Replace key
                  </button>
                  {p.console_url && (
                    <button
                      role="menuitem"
                      onClick={() => {
                        setMenuOpen(false);
                        openUrl(p.console_url);
                      }}
                    >
                      Open {p.name} console
                    </button>
                  )}
                  <button
                    role="menuitem"
                    className="danger"
                    onClick={() => {
                      setMenuOpen(false);
                      setConfirming(true);
                    }}
                  >
                    Disconnect
                  </button>
                </div>
              )}
            </div>
          ) : (
            p.console_url && (
              <button className="link-button" onClick={() => openUrl(p.console_url)}>
                Get a key →
              </button>
            )
          )}
        </div>
      )}

      {confirming && (
        <ConfirmDialog
          title={`Disconnect ${p.name}?`}
          body={disconnectBody}
          confirmLabel="Disconnect"
          onConfirm={disconnect}
          onCancel={() => setConfirming(false)}
        />
      )}
    </div>
  );
}
