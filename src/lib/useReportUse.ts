import { useEffect } from "react";
import { inTauri, noteUserInput } from "./api";
import { useAppStore } from "./store";

/** A key or a click is reported at most this often. The backend only needs to know
 * the app is in use, and to end a daily pass that began on its own. */
export const INPUT_THROTTLE_MS = 2000;
/** While a turn streams, the app says so this often: a long run is use, though no
 * one is typing, and the daily pass must not start under it (`SLP-1`). */
export const BUSY_BEAT_MS = 30000;

/**
 * `SLP-1`: tell the backend when the app is in use. Idle start and stop on input
 * both hang on this; the backend has no other way to see a key press.
 */
export function useReportUse() {
  const busy = useAppStore((s) => s.busy);

  useEffect(() => {
    if (!inTauri()) return;
    let last = 0;
    const report = () => {
      const now = Date.now();
      if (now - last < INPUT_THROTTLE_MS) return;
      last = now;
      void noteUserInput(true).catch(() => {});
    };
    window.addEventListener("keydown", report, true);
    window.addEventListener("pointerdown", report, true);
    return () => {
      window.removeEventListener("keydown", report, true);
      window.removeEventListener("pointerdown", report, true);
    };
  }, []);

  useEffect(() => {
    if (!inTauri() || !busy) return;
    void noteUserInput(false).catch(() => {});
    const t = setInterval(() => void noteUserInput(false).catch(() => {}), BUSY_BEAT_MS);
    return () => clearInterval(t);
  }, [busy]);
}
