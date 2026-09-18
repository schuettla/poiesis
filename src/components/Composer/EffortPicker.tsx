import { useEffect, useRef, useState } from "react";
import { getSetting, setSetting } from "../../lib/api";
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
const EFFORTS = [
  { value: "off", label: "No thinking", bars: 0, hint: "Answer straight away. Fastest and cheapest." },
  { value: "low", label: "Think briefly", bars: 1, hint: "A moment's thought first." },
  { value: "medium", label: "Think", bars: 2, hint: "For questions with a few moving parts." },
  { value: "high", label: "Think hard", bars: 3, hint: "Slow and costly. Difficult work only." },
  { value: "provider", label: "Model's default", bars: -1, hint: "Send nothing and take the provider's own setting." },
] as const;

type Effort = (typeof EFFORTS)[number];

const DEFAULT = EFFORTS[1];

/** Three rising bars, filled to the level. `-1` is "not our choice" and shows
 * as an outline throughout, so it never reads as a quantity on the scale. */
function Meter({ bars }: { bars: number }) {
  return (
    <span className="effort-meter" aria-hidden="true">
      {[1, 2, 3].map((n) => (
        <i key={n} className={bars >= n ? "on" : bars < 0 ? "unknown" : ""} />
      ))}
    </span>
  );
}

export default function EffortPicker() {
  const [value, setValue] = useState<string>(DEFAULT.value);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    getSetting(EFFORT_KEY)
      .then((v) => setValue(v ?? DEFAULT.value))
      .catch(() => {});
  }, []);

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

  const current: Effort = EFFORTS.find((e) => e.value === value) ?? DEFAULT;

  function choose(e: Effort) {
    setValue(e.value);
    setOpen(false);
    setSetting(EFFORT_KEY, e.value).catch(() => {});
  }

  return (
    <div className="effort-picker" ref={ref}>
      <button
        className="effort-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`How hard to think: ${current.label}`}
        title={`${current.hint} Only models that can think are affected; the rest ignore it.`}
        onClick={() => setOpen((o) => !o)}
      >
        <Meter bars={current.bars} />
        <span className="effort-name">{current.label}</span>
        <span className="caret" aria-hidden="true">
          ▴
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
