import { useEffect, useRef, useState } from "react";
import {
  accountBroken,
  composioAccounts,
  composioCancel,
  composioConnect,
  composioStatus,
  inTauri,
  type AppAccount,
} from "../../lib/api";
import { useAppStore } from "../../lib/store";
import type { BlockView } from "../../lib/types";
import AppLogo from "../Apps/AppLogo";
import Frame from "./Frame";
import "../Apps/ComposioApps.css";

/** How often to ask Composio whether the sign-in finished, and when to stop. */
export const POLL_MS = 3000;
export const GIVE_UP_MS = 10 * 60 * 1000;

type Phase = "waiting" | "signing" | "connected" | "failed" | "cancelled";

/** What is kept on the block, so a sign-in survives a restart. The address to
 * sign in at is never kept: it is made when the button is pressed. */
interface ConnectState {
  phase?: Phase;
  accountId?: string;
  label?: string | null;
  startedAt?: number;
  reason?: string;
}

const clean = (e: unknown) => String(e).replace(/^Error:\s*/, "");

/**
 * `CA-UI-4`: the card the model asks for when an app is not connected yet. The
 * person signs in once, in their own browser, and the conversation carries on.
 */
export default function ConnectBlock({ block }: { block: BlockView }) {
  const data = (block.data && typeof block.data === "object" ? block.data : {}) as Record<string, unknown>;
  const app = String(data.app ?? "");
  const name = String(data.name ?? app);
  const state = (block.state && typeof block.state === "object" ? block.state : {}) as ConnectState;
  const phase: Phase = state.phase ?? "waiting";

  const setBlockState = useAppStore((s) => s.setBlockState);
  const sendBlockAction = useAppStore((s) => s.sendBlockAction);
  const setView = useAppStore((s) => s.setView);

  const [label, setLabel] = useState(typeof data.label === "string" ? data.label : "");
  // A second account for an app needs a name of its own.
  const [needsLabel, setNeedsLabel] = useState(typeof data.label === "string" && data.label !== "");
  const [keyed, setKeyed] = useState<boolean | null>(null);
  // A Connect key has no labels: nothing is asked for and nothing is shown.
  const [isConnect, setIsConnect] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const done = useRef(false);

  useEffect(() => {
    if (!inTauri()) return;
    composioStatus()
      .then((s) => {
        setKeyed(s.configured);
        setIsConnect(s.mode === "connect");
        if (phase !== "waiting") return;
        const mine = s.accounts.filter((a) => a.app === app && a.status !== "REVOKED");
        if (s.mode !== "connect" && mine.length > 0) {
          setNeedsLabel(true);
          // The backend's suggestion if it made one; else whichever of these two
          // names is not taken yet, and nothing when both are (`CAP-C-F8`).
          const taken = mine.map((a) => (a.label ?? "").toLowerCase());
          setLabel((l) => l || ["work", "personal"].find((n) => !taken.includes(n)) || "");
        }
      })
      .catch(() => setKeyed(true));
  }, [phase, app]);

  function finish(next: Phase, patch: Partial<ConnectState> = {}) {
    if (done.current) return;
    done.current = true;
    setBlockState(block.id, { ...state, ...patch, phase: next });
  }

  function connected(account: AppAccount) {
    // Connect has no labels: "Connected.", not "Connected as default."
    const shown = isConnect ? null : (account.label ?? "default");
    finish("connected", { label: shown });
    void sendBlockAction(block.id, shown ? `${name} is connected (${shown}).` : `${name} is connected.`, {
      a: "block_action",
      block: block.id,
      kind: "connect",
      action: "connected",
      app,
      ...(shown ? { label: shown } : {}),
    });
  }

  async function signIn() {
    setWorking(true);
    setError(null);
    try {
      const account = await composioConnect(app, needsLabel && !isConnect ? label.trim() || undefined : undefined);
      done.current = false;
      // Already signed in: nothing opened, nothing to wait for.
      if (account.status === "ACTIVE") {
        connected(account);
        return;
      }
      setBlockState(block.id, {
        phase: "signing",
        accountId: account.id,
        label: account.label,
        startedAt: Date.now(),
      });
    } catch (e) {
      setError(clean(e));
    } finally {
      setWorking(false);
    }
  }

  function cancel() {
    // The half-made sign-in is dropped, so it does not linger and turn red later.
    if (state.accountId && inTauri()) void composioCancel(state.accountId).catch(() => {});
    finish("cancelled");
    void sendBlockAction(block.id, `I didn't connect ${name}.`, {
      a: "block_action",
      block: block.id,
      kind: "connect",
      action: "cancelled",
      app,
    });
  }

  // While the person is in the browser: ask every few seconds, and again the
  // moment this window is looked at, until it is done, fails, or ten minutes pass.
  useEffect(() => {
    if (phase !== "signing" || !state.accountId || !inTauri()) return;
    const accountId = state.accountId;
    const startedAt = state.startedAt ?? 0;
    let stopped = false;
    const check = async () => {
      if (stopped || done.current) return;
      if (Date.now() - startedAt > GIVE_UP_MS) {
        finish("failed", { reason: "Nothing happened for 10 minutes, so I stopped waiting." });
        return;
      }
      try {
        const list = await composioAccounts(true, [app]);
        // The row watched can be folded into one row per account once the app has
        // two (`CAP-C6`); then any working account of the app means it is done.
        const mine =
          list.find((a) => a.id === accountId) ?? list.find((a) => a.app === app && a.status === "ACTIVE");
        if (stopped) return;
        setError(null);
        if (!mine) return;
        if (mine.status === "ACTIVE") connected(mine);
        else if (accountBroken(mine)) finish("failed", { reason: `The sign-in to ${name} didn't finish.` });
      } catch (e) {
        // A hiccup is not a failure and the next look tries again, but a sign-in
        // that never finishes should say why.
        if (!stopped) setError(clean(e));
      }
    };
    const timer = setInterval(() => void check(), POLL_MS);
    const onFocus = () => void check();
    window.addEventListener("focus", onFocus);
    void check();
    return () => {
      stopped = true;
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
    // The block is re-read from the store on every state change; the sign-in it
    // watches is identified by its account.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase, state.accountId, state.startedAt]);

  return (
    <Frame title={block.title || `Sign in to ${name}`} kind="sign-in">
      <div className="connect-block">
        <AppLogo slug={app} name={name} large />
        <div className="connect-main">
          {phase === "waiting" && keyed === false && (
            <>
              <p className="connect-line">I can connect apps once you've added a Composio key.</p>
              <div className="signin-actions">
                <button className="btn-secondary" onClick={() => setView("apps")}>
                  Open Apps settings
                </button>
              </div>
            </>
          )}
          {phase === "waiting" && keyed !== false && (
            <>
              <p className="connect-line">
                I need your {name} for this. Sign in once and I'll carry on.
              </p>
              {needsLabel && !isConnect && (
                <label className="connect-label">
                  <span className="field-label">Call this account</span>
                  <input
                    className="field-input"
                    value={label}
                    maxLength={64}
                    placeholder="work"
                    onChange={(e) => setLabel(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && !working && void signIn()}
                  />
                </label>
              )}
              {error && (
                <p className="connect-error" role="alert">
                  {error}
                </p>
              )}
              <div className="signin-actions">
                <button className="btn-primary" disabled={working || (needsLabel && !isConnect && !label.trim())} onClick={() => void signIn()}>
                  {working ? "Opening…" : `Sign in to ${name}`}
                </button>
              </div>
              <p className="connect-quiet">
                Opens {name}'s own sign-in page in your browser. Runs through Composio.
              </p>
            </>
          )}
          {phase === "signing" && (
            <>
              <p className="connect-line">Waiting for you in the browser…</p>
              {error && (
                <p className="connect-error" role="alert">
                  {error}
                </p>
              )}
              <div className="signin-actions">
                <button
                  className="btn-text"
                  disabled={working}
                  onClick={async () => {
                    // The sign-in that is open is opened again, not made a second time.
                    setWorking(true);
                    setError(null);
                    try {
                      const account = await composioConnect(app, undefined);
                      done.current = false;
                      if (account.status === "ACTIVE") {
                        connected(account);
                        return;
                      }
                      setBlockState(block.id, {
                        phase: "signing",
                        accountId: account.id,
                        label: account.label,
                        startedAt: Date.now(),
                      });
                    } catch (e) {
                      setError(clean(e));
                    } finally {
                      setWorking(false);
                    }
                  }}
                >
                  Open again
                </button>
                <button className="btn-text" onClick={cancel}>
                  Cancel
                </button>
              </div>
            </>
          )}
          {phase === "connected" && (
            <p className="connect-line connect-ok">
              {state.label ? `Connected as ${state.label}.` : "Connected."}
            </p>
          )}
          {phase === "cancelled" && <p className="connect-line">I didn't connect {name}.</p>}
          {phase === "failed" && (
            <>
              <p className="connect-line">{state.reason ?? `The sign-in to ${name} didn't finish.`}</p>
              <div className="signin-actions">
                <button
                  className="btn-secondary"
                  onClick={() => {
                    done.current = false;
                    setBlockState(block.id, { phase: "waiting" });
                  }}
                >
                  Try again
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </Frame>
  );
}
