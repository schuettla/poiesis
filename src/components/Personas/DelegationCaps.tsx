import { useEffect, useState } from "react";
import { getSetting, inTauri, setSetting } from "../../lib/api";

/**
 * `SUB-UI-7`: delegation's caps, inline under its toggle in Settings → Tools.
 *
 * Parallel agents multiply what a turn costs, and nothing else in the app
 * brakes that. These four numbers are the brake, so they belong where the
 * switch is rather than behind an "advanced" disclosure somewhere else.
 */

const FIELDS = [
  {
    key: "subagents.max_parallel",
    label: "How many agents I may run at once",
    fallback: 3,
    min: 1,
    max: 5,
  },
  {
    key: "subagents.max_steps",
    label: "How many steps each of them may take",
    fallback: 8,
    min: 1,
    max: 24,
  },
  {
    key: "subagents.timeout_secs",
    label: "How long each of them may run, in seconds",
    fallback: 300,
    min: 30,
    max: 3600,
  },
] as const;

const NESTED_KEY = "subagents.allow_nested";

export default function DelegationCaps() {
  const [values, setValues] = useState<Record<string, string>>({});
  const [nested, setNested] = useState(false);

  useEffect(() => {
    if (!inTauri()) return;
    for (const f of FIELDS) {
      getSetting(f.key)
        .then((v) => setValues((old) => ({ ...old, [f.key]: v ?? String(f.fallback) })))
        .catch(() => {});
    }
    getSetting(NESTED_KEY)
      .then((v) => setNested(v === "true"))
      .catch(() => {});
  }, []);

  const commit = (key: string, raw: string, min: number, max: number, fallback: number) => {
    const n = Number(raw);
    const clamped = Number.isFinite(n) ? Math.min(max, Math.max(min, Math.round(n))) : fallback;
    setValues((old) => ({ ...old, [key]: String(clamped) }));
    setSetting(key, String(clamped)).catch(() => {});
  };

  return (
    <div className="delegation-caps">
      {FIELDS.map((f) => (
        <label key={f.key} className="delegation-cap">
          <span>{f.label}</span>
          <input
            type="number"
            min={f.min}
            max={f.max}
            value={values[f.key] ?? ""}
            onChange={(e) => setValues((old) => ({ ...old, [f.key]: e.target.value }))}
            onBlur={(e) => commit(f.key, e.target.value, f.min, f.max, f.fallback)}
          />
        </label>
      ))}
      <label className="delegation-nested">
        <input
          type="checkbox"
          checked={nested}
          onChange={(e) => {
            setNested(e.target.checked);
            setSetting(NESTED_KEY, e.target.checked ? "true" : "false").catch(() => {});
          }}
        />
        Let an agent I started hand work out again (off is safer)
      </label>
      <p className="delegation-note">
        Child conversations do not appear in the Rail. You will find one in Library if you go
        looking, and inside the turn that started it.
      </p>
    </div>
  );
}
