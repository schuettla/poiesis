import { useEffect, useRef, useState } from "react";

/** Hold a value back so it changes at most once per `ms`. A fast stream cannot starve it:
 * the next change is due `ms` after the last one, however many values arrive in between. */
export function useThrottled<T>(value: T, ms: number): T {
  const [held, setHeld] = useState(value);
  const last = useRef(0);
  useEffect(() => {
    const wait = last.current + ms - Date.now();
    if (wait <= 0) {
      last.current = Date.now();
      setHeld(value);
      return;
    }
    const t = setTimeout(() => {
      last.current = Date.now();
      setHeld(value);
    }, wait);
    return () => clearTimeout(t);
  }, [value, ms]);
  return held;
}
