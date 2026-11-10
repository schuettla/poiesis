import { useEffect, useState } from "react";

/** True for `ms` after `at` (epoch ms), then false again. For a moment the screen
 * should show once and let go: a pulse, a fade. `undefined` is never recent. */
export function useRecent(at: number | undefined, ms: number): boolean {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (at === undefined) return;
    const left = at + ms - Date.now();
    if (left <= 0) return;
    setNow(Date.now());
    const t = setTimeout(() => setNow(Date.now()), left + 20);
    return () => clearTimeout(t);
  }, [at, ms]);
  return at !== undefined && now < at + ms;
}
