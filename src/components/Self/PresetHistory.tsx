import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import StyleGlyph from "../Mark/StyleGlyph";
import { ordinal, shortDate } from "./waysFormat";
import "./PresetHistory.css";

/** What an entry is called in the list. */
function title(e: api.PresetHistoryEntry): string {
  switch (e.what) {
    case "original":
      return "Original";
    case "locked":
      return "Locked";
    case "unlocked":
      return "Unlocked";
    case "back":
      return `${ordinal(e.version)} version, went back`;
    default:
      return `${ordinal(e.version)} version`;
  }
}

const carriesRules = (e: api.PresetHistoryEntry) => e.what === "original" || e.what === "version" || e.what === "back";

/**
 * `WRK-20`: one preset's history in the shared confirm shell, at a fixed height. Every
 * change is a version; going back writes an old one as a new one, so going back can
 * itself be gone back from. Locking is an entry of its own.
 */
export default function PresetHistory({
  slug,
  name,
  glyph,
  onClose,
  onChanged,
}: {
  slug: string;
  name: string;
  glyph: string;
  onClose: () => void;
  onChanged: () => void;
}) {
  const [entries, setEntries] = useState<api.PresetHistoryEntry[] | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  const [compare, setCompare] = useState(false);
  const [problem, setProblem] = useState("");

  useEffect(() => {
    let live = true;
    api
      .presetHistory(slug)
      .then((list) => {
        if (!live) return;
        setEntries(list);
        // Start on the newest version that is not the one in use.
        setPicked(list.find((e) => carriesRules(e) && !e.current)?.seq ?? null);
      })
      .catch((e) => live && setProblem(String(e)));
    return () => {
      live = false;
    };
  }, [slug]);

  const now = entries?.find((e) => e.current);
  const chosen = entries?.find((e) => e.seq === picked);
  const bringBack = chosen && now ? chosen.rules.filter((r) => !now.rules.includes(r)) : [];
  const takeAway = chosen && now ? now.rules.filter((r) => !chosen.rules.includes(r)) : [];

  async function goBack() {
    if (!chosen || chosen.current) {
      setProblem(chosen ? "It already works that way." : "Pick a version first.");
      return;
    }
    try {
      await api.presetBack(slug, chosen.seq);
      onChanged();
      onClose();
    } catch (e) {
      setProblem(String(e));
    }
  }

  return (
    <ConfirmDialog
      title={`${name} · history`}
      confirmLabel="Go back to this version"
      cancelLabel="Close"
      onConfirm={() => void goBack()}
      onCancel={onClose}
    >
      <div className="preset-history">
        <div className="preset-history-list" role="listbox" aria-label={`Versions of ${name}`}>
          {!entries && !problem && <p className="ways-quiet">Loading…</p>}
          {entries?.length === 0 && <p className="ways-quiet">Nothing has changed yet.</p>}
          {entries?.map((e) => {
            const pickable = carriesRules(e);
            return (
              <div
                key={e.seq}
                role="option"
                aria-selected={e.seq === picked}
                className={`preset-history-row${e.seq === picked ? " picked" : ""}${pickable ? "" : " event"}`}
                onClick={() => pickable && setPicked(e.seq)}
              >
                <span className="preset-history-head">
                  <StyleGlyph glyph={glyph} size={13} />
                  <strong>{title(e)}</strong>
                  <span className="preset-history-date">{shortDate(e.at)}</span>
                  {e.current && <span className="ways-tag">now</span>}
                  <span className="preset-history-by">{e.by === "you" ? "you" : e.by === "evolution" ? "me" : ""}</span>
                </span>
                {e.added.map((r) => (
                  <span className="preset-history-line add" key={`+${r}`}>
                    + {r}
                  </span>
                ))}
                {e.removed.map((r) => (
                  <span className="preset-history-line del" key={`-${r}`}>
                    − {r}
                  </span>
                ))}
                {e.why && <span className="preset-history-why">{e.why}</span>}
              </div>
            );
          })}
        </div>
        <div className="preset-history-compare">
          <button className="why-link" onClick={() => setCompare((c) => !c)} disabled={!chosen || chosen.current}>
            {compare ? "Hide the comparison" : "Compare with now"}
          </button>
          {compare && chosen && !chosen.current && (
            <p className="preset-history-diff">
              {bringBack.length === 0 && takeAway.length === 0
                ? "Both switch on the same moves."
                : [
                    bringBack.length ? `Going back switches on: ${bringBack.join(" ")}` : "",
                    takeAway.length ? `and switches off: ${takeAway.join(" ")}` : "",
                  ]
                    .filter(Boolean)
                    .join(" ")}
            </p>
          )}
        </div>
        <p className="preset-problem" role="status">
          {problem}
        </p>
      </div>
    </ConfirmDialog>
  );
}
