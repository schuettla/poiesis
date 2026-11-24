import { useCallback, useEffect, useRef, useState } from "react";
import {
  inTauri,
  listenAppEvent,
  remoteCancelCode,
  remoteNewCode,
  remoteRemoveSender,
  remoteSetEnabled,
  remoteStatus,
  remoteTelegramCode,
  remoteTelegramConnect,
  remoteTelegramDisconnect,
  remoteTelegramStatus,
  REMOTE_EVENT,
  type RemotePairingCode,
  type RemoteSender,
  type RemoteStatus,
  type RemoteTelegramCode,
  type RemoteTelegramStatus,
} from "../lib/api";
import {
  codeGroups,
  countdown,
  lastSeenWords,
  phonesOf,
  remoteStateLine,
  telegramLine,
  telegramOf,
} from "../lib/remote";
import ConfirmDialog from "../components/Confirm/ConfirmDialog";
import Toggle from "../components/Toggle/Toggle";
import "./Surface.css";
import "./Remote.css";

/**
 * Remote: let a phone or a Telegram account reach this computer. Three blocks: the switch, the phone, and
 * Telegram. Off by default, and while it is off nothing outside can reach me.
 */
export default function Remote() {
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [telegram, setTelegram] = useState<RemoteTelegramStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [telegramError, setTelegramError] = useState<string | null>(null);
  const [token, setToken] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [pairing, setPairing] = useState(false);
  const [pairingTelegram, setPairingTelegram] = useState(false);
  const [removing, setRemoving] = useState<RemoteSender | null>(null);

  const refresh = useCallback(async () => {
    if (!inTauri()) return;
    try {
      setStatus(await remoteStatus());
    } catch (e) {
      setError(String(e));
    }
    // The bot's own state is a second question: a failure there must not hide the switch.
    remoteTelegramStatus().then(setTelegram, () => undefined);
  }, []);

  useEffect(() => {
    void refresh();
    // The gateway says when the switch or a link changes, from the tray too.
    return listenAppEvent(REMOTE_EVENT, () => void refresh());
  }, [refresh]);

  if (!inTauri()) {
    return (
      <div className="surface">
        <div className="surface-inner">
          <h1>Remote</h1>
          <p className="lede">Reaching me from a phone works in the desktop app.</p>
        </div>
      </div>
    );
  }

  const enabled = status?.enabled ?? false;
  const phones = phonesOf(status?.senders ?? []);
  const accounts = telegramOf(status?.senders ?? []);

  async function connect() {
    setTelegramError(null);
    setConnecting(true);
    try {
      setTelegram(await remoteTelegramConnect(token));
      setToken("");
    } catch (e) {
      setTelegramError(String(e));
    } finally {
      setConnecting(false);
    }
  }

  async function disconnect() {
    setTelegramError(null);
    try {
      setTelegram(await remoteTelegramDisconnect());
    } catch (e) {
      setTelegramError(String(e));
    }
  }

  async function flip(on: boolean) {
    setError(null);
    try {
      setStatus(await remoteSetEnabled(on));
    } catch (e) {
      setError(String(e));
    }
  }

  async function remove(sender: RemoteSender) {
    setRemoving(null);
    try {
      await remoteRemoveSender(sender.transport, sender.id);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="surface">
      <div className="surface-inner remote">
        <h1>Remote</h1>
        <p className="lede">
          Use me from your phone while I keep running here. Your phone asks me, I do the work on this computer, and I
          ask you before I change or send anything.
        </p>

        {error && <p className="hw-note error">{error}</p>}

        <section className="remote-block" aria-label="Switch">
          <label className="remote-switch">
            <Toggle checked={enabled} onChange={(on) => void flip(on)} disabled={!status} />
            <span className="remote-switch-label">Let me be reached from outside</span>
          </label>
          <p className="remote-state">{remoteStateLine(enabled, status?.senders.length ?? 0)}</p>
        </section>

        <section className="remote-block" aria-label="Phone">
          <div className="remote-block-head">
            <h2>Phone</h2>
            <button className="btn-secondary" disabled={!enabled} onClick={() => setPairing(true)}>
              Pair a phone
            </button>
          </div>
          {!enabled && <p className="remote-hint">Turn the switch on first.</p>}
          {phones.length === 0 ? (
            <p className="remote-hint">No phone is paired.</p>
          ) : (
            <ul className="remote-phones">
              {phones.map((p) => (
                <li key={`${p.transport}:${p.id}`} className="remote-phone">
                  <span className="remote-phone-name">{p.label || "Phone"}</span>
                  <span className="remote-phone-seen">{lastSeenWords(p.last_seen, Date.now())}</span>
                  <button className="link-button inline" onClick={() => setRemoving(p)}>
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
          <p className="remote-hint">Your computer has to be on, with Poiesis running, for your phone to reach it.</p>
          <p className="remote-hint">
            Your phone finds this computer through iroh's free relay service. It sees your IP address, never your
            messages.
          </p>
        </section>

        <section className="remote-block" aria-label="Telegram">
          <div className="remote-block-head">
            <h2>Telegram</h2>
            <button
              className="btn-secondary"
              disabled={!enabled || !telegram?.polling}
              onClick={() => setPairingTelegram(true)}
            >
              Pair an account
            </button>
          </div>
          <p className="remote-state">{telegramLine(telegram, enabled)}</p>
          {telegramError && <p className="hw-note error">{telegramError}</p>}
          {telegram?.token_hint ? (
            <p className="remote-hint">
              Token {telegram.token_hint}.{" "}
              <button className="link-button inline" onClick={() => void disconnect()}>
                Disconnect
              </button>
            </p>
          ) : (
            <form
              className="remote-token"
              onSubmit={(e) => {
                e.preventDefault();
                void connect();
              }}
            >
              <input
                className="field-input"
                type="password"
                autoComplete="off"
                aria-label="Telegram bot token"
                placeholder="Paste the token from BotFather"
                value={token}
                onChange={(e) => setToken(e.target.value)}
              />
              <button className="btn-secondary" type="submit" disabled={!token.trim() || connecting}>
                {connecting ? "Checking ..." : "Connect"}
              </button>
            </form>
          )}
          {accounts.length === 0 ? (
            <p className="remote-hint">No account is paired.</p>
          ) : (
            <ul className="remote-phones">
              {accounts.map((a) => (
                <li key={`${a.transport}:${a.id}`} className="remote-phone">
                  <span className="remote-phone-name">{a.label || "Telegram"}</span>
                  <span className="remote-phone-seen">{lastSeenWords(a.last_seen, Date.now())}</span>
                  <button className="link-button inline" onClick={() => setRemoving(a)}>
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
          <p className="remote-hint">Messages to the Telegram bot pass through Telegram.</p>
        </section>
      </div>

      {pairing && (
        <PairDialog phones={phones.length} onClose={() => setPairing(false)} onPaired={() => void refresh()} />
      )}
      {pairingTelegram && (
        <TelegramPairDialog
          bot={telegram?.bot ?? null}
          accounts={accounts.length}
          onClose={() => setPairingTelegram(false)}
          onPaired={() => void refresh()}
        />
      )}
      {removing && (
        <ConfirmDialog
          title={`Remove ${removing.label || (removing.transport === "telegram" ? "this account" : "this phone")}?`}
          body="It has to pair again with a new code before it can reach me."
          confirmLabel="Remove"
          onConfirm={() => void remove(removing)}
          onCancel={() => setRemoving(null)}
        />
      )}
    </div>
  );
}

/**
 * The pairing dialog: a QR code to scan, the code to type, and the five minutes it works. Fixed size,
 * so it never jumps. When a phone pairs while it is open, it says so.
 */
function PairDialog({ phones, onClose, onPaired }: { phones: number; onClose: () => void; onPaired: () => void }) {
  const [code, setCode] = useState<RemotePairingCode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [copied, setCopied] = useState(false);
  const before = useRef(phones);
  const paired = phones > before.current;

  const next = useCallback(async () => {
    setError(null);
    setCopied(false);
    try {
      setCode(await remoteNewCode());
      setNow(Date.now());
    } catch (e) {
      setCode(null);
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void next();
    // Off the screen, the code stops working: nobody may knock with it any more.
    return () => void remoteCancelCode().catch(() => undefined);
  }, [next]);

  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => {
      clearInterval(tick);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  // A phone that pairs while the dialog is open shows up in the list behind it.
  useEffect(() => listenAppEvent(REMOTE_EVENT, onPaired), [onPaired]);

  const left = code ? countdown(code.expires_at, now) : "";
  const qr = code?.svg ? `data:image/svg+xml;utf8,${encodeURIComponent(code.svg)}` : null;

  async function copy() {
    if (!code) return;
    try {
      await navigator.clipboard.writeText(code.text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div className="confirm-backdrop" onClick={onClose}>
      <div
        className="confirm-card remote-pair"
        role="dialog"
        aria-modal="true"
        aria-label="Pair a phone"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="confirm-title">Pair a phone</h2>
        <p className="confirm-body">
          {paired ? "Your phone is paired." : "Open Poiesis on your phone and scan this code, or type it."}
        </p>
        <div className="remote-qr" aria-hidden={!qr}>
          {paired ? (
            <span className="remote-qr-done">Done</span>
          ) : qr ? (
            <img src={qr} alt="Pairing code for your phone" width={188} height={188} />
          ) : null}
        </div>
        <div className="remote-code" aria-live="polite">
          {paired ? "" : error ? <span className="remote-code-error">{error}</span> : code ? codeGroups(code.code) : "…"}
        </div>
        <p className="remote-countdown">
          {paired || error ? "" : left ? `Works for ${left} more, once.` : code ? "This code has run out." : ""}
        </p>
        <div className="remote-pair-alt">
          {!paired && code && (
            <button className="link-button inline" onClick={() => void copy()}>
              {copied ? "Copied" : "Can't scan? Copy the pairing text"}
            </button>
          )}
        </div>
        <div className="confirm-actions">
          <button className="btn-secondary" onClick={() => void next()} disabled={paired}>
            New code
          </button>
          <button className="btn-primary" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * The Telegram pairing dialog: a code to send to the bot, and the five minutes it works. Fixed size, like the
 * phone's. When an account pairs while it is open, it says so.
 */
function TelegramPairDialog({
  bot,
  accounts,
  onClose,
  onPaired,
}: {
  bot: string | null;
  accounts: number;
  onClose: () => void;
  onPaired: () => void;
}) {
  const [code, setCode] = useState<RemoteTelegramCode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const before = useRef(accounts);
  const paired = accounts > before.current;

  const next = useCallback(async () => {
    setError(null);
    try {
      setCode(await remoteTelegramCode());
      setNow(Date.now());
    } catch (e) {
      setCode(null);
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void next();
    // Off the screen, the code stops working: nobody may use it any more.
    return () => void remoteCancelCode().catch(() => undefined);
  }, [next]);

  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => {
      clearInterval(tick);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  useEffect(() => listenAppEvent(REMOTE_EVENT, onPaired), [onPaired]);

  const left = code ? countdown(code.expires_at, now) : "";

  return (
    <div className="confirm-backdrop" onClick={onClose}>
      <div
        className="confirm-card remote-pair remote-pair-telegram"
        role="dialog"
        aria-modal="true"
        aria-label="Pair a Telegram account"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="confirm-title">Pair a Telegram account</h2>
        <p className="confirm-body">
          {paired ? "Your account is paired." : `Send this code to ${bot ?? "your bot"} in Telegram.`}
        </p>
        <div className="remote-code" aria-live="polite">
          {paired ? "" : error ? <span className="remote-code-error">{error}</span> : code ? codeGroups(code.code) : "…"}
        </div>
        <p className="remote-countdown">
          {paired || error ? "" : left ? `Works for ${left} more, once.` : code ? "This code has run out." : ""}
        </p>
        <div className="confirm-actions">
          <button className="btn-secondary" onClick={() => void next()} disabled={paired}>
            New code
          </button>
          <button className="btn-primary" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
