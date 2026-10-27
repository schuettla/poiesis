import { useAppStore } from "../../lib/store";

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * The two things a new conversation can say about the organism.
 *
 * PRES-6 — the first-run introduction, once ever: what Poiesis is, in its own
 * voice, before it has done anything.
 * PRES-5 — afterwards, at most one muted line about something learned this
 * week. Ambient, not promotional: no card, no border, absent when there's
 * nothing recent, and absent while the composer offers "Last time" (`EPI-UI-1`).
 * It reads lessons and facts alike, a corrected fact included (`G11`).
 */
export default function Introduction() {
  const introduced = useAppStore((s) => s.selfIntroduced);
  const dismiss = useAppStore((s) => s.dismissIntroduction);
  const setView = useAppStore((s) => s.setView);
  // `EPI-UI-1`: while the composer says "Last time ...", this stays quiet. One line.
  const lastTimeShown = useAppStore((s) => s.lastTimeOffer !== null);
  const recent = useAppStore((s) => {
    const cutoff = Date.now() - WEEK_MS;
    // `G11`: a lesson, a fact I learned, or one I replaced or corrected. A disputed
    // fact is not something I learned yet, so it stays out.
    const dated = [...s.lessons, ...s.facts.filter((f) => f.status !== "contested")]
      .map((e) => ({ entry: e, on: e.valid_from || e.created }))
      .filter(({ on }) => {
        const at = Date.parse(on);
        return !Number.isNaN(at) && at >= cutoff;
      });
    // Newest wins; the dates are YYYY-MM-DD, so string order is date order.
    dated.sort((a, b) => b.on.localeCompare(a.on));
    return dated[0]?.entry;
  });

  if (!introduced) {
    return (
      <div className="intro-card">
        <p className="intro-body">
          <strong>I'm Poiesis Agent.</strong> I work for you locally, and I maintain myself:
          I remember what matters to you, learn from my own mistakes, and keep procedures we
          develop together. Everything I know lives in plain files on this device — you can
          read, edit, or delete any of it.
        </p>
        <div className="setting-actions">
          <button
            className="btn-secondary"
            onClick={() => {
              dismiss();
              setView("self");
            }}
          >
            See my Self panel
          </button>
          <button className="btn-text" onClick={() => dismiss()}>
            Got it
          </button>
        </div>
      </div>
    );
  }

  if (!recent || lastTimeShown) return null;

  return (
    <button className="ambient-line" onClick={() => setView("self")}>
      ◆ Recently learned: {recent.description || recent.name}
    </button>
  );
}
