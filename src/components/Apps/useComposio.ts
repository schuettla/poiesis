import { useCallback, useEffect, useRef, useState } from "react";
import { accountWaiting, composioAccounts, composioStatus, type ComposioStatus } from "../../lib/api";
import { useAppStore } from "../../lib/store";

/** How often to ask Composio while a sign-in is still open in a browser. */
const POLL_MS = 3000;

/**
 * The Composio connection as the Apps page sees it. It reads what is kept
 * locally at once, asks Composio once on opening so labels and states are
 * current, and keeps asking every few seconds while any sign-in is waiting.
 */
export function useComposio(active: boolean) {
  const [status, setStatus] = useState<ComposioStatus | null>(null);
  // Why the last look at a waiting sign-in failed, so a stuck sign-in says so.
  const [checkError, setCheckError] = useState<string | null>(null);
  const asked = useRef(false);

  const reload = useCallback(async () => {
    try {
      setStatus(await composioStatus());
      // The hub badge and `/connect` read the same local copy.
      void useAppStore.getState().refreshApps();
    } catch {
      // The page still works without it: the card then reads as not configured.
    }
  }, []);

  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  const configured = status?.configured ?? false;
  useEffect(() => {
    if (!active || !configured || asked.current) return;
    asked.current = true;
    composioAccounts(true)
      .then(reload)
      .catch(() => {});
  }, [active, configured, reload]);

  // With a Connect key each ask is a call to Composio about named apps, so only
  // the apps whose sign-in is open are asked about.
  const waitingApps = (status?.accounts ?? []).filter(accountWaiting).map((a) => a.app);
  const waitingKey = waitingApps.join(",");
  useEffect(() => {
    if (!active || !waitingKey) {
      setCheckError(null);
      return;
    }
    const look = () =>
      composioAccounts(true, waitingKey.split(","))
        .then(() => {
          setCheckError(null);
          return reload();
        })
        .catch((e) => setCheckError(String(e).replace(/^Error:\s*/, "")));
    const timer = setInterval(look, POLL_MS);
    // Coming back from the browser: look at once, not after the next tick.
    window.addEventListener("focus", look);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", look);
    };
  }, [active, waitingKey, reload]);

  return { status, reload, setStatus, checkError };
}
