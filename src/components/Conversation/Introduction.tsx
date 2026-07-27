import { useAppStore } from "../../lib/store";

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * The two things a new conversation can say about the organism.
 *
 * PRES-6 — the first-run introduction, once ever: what Poiesis is, in its own
 * voice, before it has done anything.
 * PRES-5 — afterwards, at most one muted line about something learned this
 * week. Ambient, not promotional: no card, no border, absent when there's
 * nothing recent.
 */
export default function Introduction() {
  const introduced = useAppStore((s) => s.selfIntroduced);
  const dismiss = useAppStore((s) => s.dismissIntroduction);
  const setView = useAppStore((s) => s.setView);
  const recent = useAppStore((s) => {
    const cutoff = Date.now() - WEEK_MS;
    const dated = [...s.lessons].filter((l) => {
      const at = Date.parse(l.created);
      return !Number.isNaN(at) && at >= cutoff;
    });
    // Newest wins; `created` is YYYY-MM-DD, so string order is date order.
    dated.sort((a, b) => b.created.localeCompare(a.created));
    return dated[0];
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

  if (!recent) return null;

  return (
    <button className="ambient-line" onClick={() => setView("self")}>
      ◆ Recently learned: {recent.description || recent.name}
    </button>
  );
}
