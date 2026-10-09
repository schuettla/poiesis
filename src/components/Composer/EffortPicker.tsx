import { useEffect, useRef, useState } from "react";
import { getSetting, setSetting } from "../../lib/api";
import { ChevronIcon } from "../Icons/Icons";
import "./EffortPicker.css";

/**
 * How hard a reasoning model thinks before it answers.
 *
 * Nothing in the app used to send this parameter at all, so every provider
 * applied its own default — which is always its maximum. That buys latency and
 * cost nobody asked for, and on a free tier it is the setting most likely to
 * end in a model that thinks for ten minutes and never answers.
 *
 * It lives beside the model picker rather than in a settings page: it is a
 * property of the answer you are about to ask for, changed in the same breath
 * as choosing which model gives it. That is also why it is built the same way
 * as `ModelPicker` — a trigger and a drop-up list — instead of a native
 * `<select>`, which renders the operating system's own combobox and would sit
 * in the composer looking like it came from a different application.
 */
const EFFORT_KEY = "models.reasoning_effort";

/** Matches `cloud::Effort`. `provider` sends no parameter at all, which is the
 * only honest option for a server we know nothing about — so it is last, and
 * its meter reads as unknown rather than as a level. */
export const EFFORTS = [
  { value: "off", label: "No thinking", bars: 0, hint: "Answer straight away. Fastest and cheapest." },
  { value: "low", label: "Think briefly", bars: 1, hint: "A moment's thought first." },
  { value: "medium", label: "Think", bars: 2, hint: "For questions with a few moving parts." },
  { value: "high", label: "Think hard", bars: 3, hint: "Slow and costly. Difficult work only." },
  { value: "provider", label: "Model's default", bars: -1, hint: "Send nothing and take the provider's own setting." },
] as const;

export type Effort = (typeof EFFORTS)[number];

const DEFAULT = EFFORTS[1];

/** Three rising bars, filled to the level. `-1` is "not our choice" and shows
 * as an outline throughout, so it never reads as a quantity on the scale. */
export function Meter({ bars }: { bars: number }) {
  return (
    <span className="effort-meter" aria-hidden="true">
      {[1, 2, 3].map((n) => (
        <i key={n} className={bars >= n ? "on" : bars < 0 ? "unknown" : ""} />
      ))}
    </span>
  );
}

/** The stored default, a `/effort` chip over it, and the way to change it —
 * shared by this picker and the composer's combined model control. */
export function useEffort(chip?: string, onPick?: () => void) {
  const [value, setValue] = useState<string>(DEFAULT.value);

  useEffect(() => {
    getSetting(EFFORT_KEY)
      .then((v) => setValue(v ?? DEFAULT.value))
      .catch(() => {});
  }, []);

  // `DEF-2`: a chip was made the default somewhere else (`make default`).
  useEffect(() => {
    function onDefault(e: Event) {
      const v = (e as CustomEvent<string>).detail;
      if (typeof v === "string") setValue(v);
    }
    window.addEventListener("poiesis:effort-default", onDefault);
    return () => window.removeEventListener("poiesis:effort-default", onDefault);
  }, []);

  const standing: Effort = EFFORTS.find((e) => e.value === value) ?? DEFAULT;
  const chipped = chip ? EFFORTS.find((e) => e.value === chip) : undefined;
  const current: Effort = chipped ?? standing;

  function choose(e: Effort) {
    setValue(e.value);
    setSetting(EFFORT_KEY, e.value).catch(() => {});
    onPick?.();
  }

  return { standing, chipped, current, choose };
}

export default function EffortPicker({
  chip,
  onPick,
}: {
  /** `DEF-1`: a `/effort` chip for the next message only. While one is set the
   * picker shows *its* value, marked, and the default stays what it was. */
  chip?: string;
  /** Called when a value is picked in the list, so the chip can go: the two
   * must never disagree on screen. */
  onPick?: () => void;
} = {}) {
  const effort = useEffort(chip, onPick);
  const { standing, chipped, current } = effort;
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // Same dismissal contract as the model picker beside it: a click anywhere
  // else, or Escape. Two neighbouring controls that close differently is the
  // kind of small wrongness you feel without being able to name it.
  useEffect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  function choose(e: Effort) {
    setOpen(false);
    effort.choose(e);
  }

  return (
    <div className="effort-picker" ref={ref}>
      <button
        className="effort-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`How hard to think: ${current.label}${chipped ? " (just this message)" : ""}`}
        title={
          chipped
            ? `Just this message. My default is ${standing.label.toLowerCase()}.`
            : `${current.hint} Only models that can think are affected; the rest ignore it.`
        }
        onClick={() => setOpen((o) => !o)}
      >
        <Meter bars={current.bars} />
        <span className="effort-name">{current.label}</span>
        {chipped && (
          <span className="effort-once" aria-hidden="true">
            ·
          </span>
        )}
        <span className="caret" aria-hidden="true">
          <ChevronIcon dir="up" size={10} strokeWidth={1.8} />
        </span>
      </button>

      {open && (
        <div className="effort-dropdown" role="listbox" aria-label="How hard to think">
          {EFFORTS.map((e) => (
            <div
              key={e.value}
              className={`effort-option ${e.value === current.value ? "selected" : ""}`}
              role="option"
              aria-selected={e.value === current.value}
              tabIndex={0}
              onClick={() => choose(e)}
              onKeyDown={(ev) => {
                if (ev.key === "Enter" || ev.key === " ") {
                  ev.preventDefault();
                  choose(e);
                }
              }}
            >
              <Meter bars={e.bars} />
              <span className="name">{e.label}</span>
              <span className="hint">{e.hint}</span>
            </div>
          ))}
          <p className="effort-note">
            A provider that will not accept this is asked again without it, so it can never cost you
            an answer.
          </p>
        </div>
      )}
    </div>
  );
}
