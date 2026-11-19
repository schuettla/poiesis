import { useEffect, useState, type ReactNode } from "react";
import * as api from "../../lib/api";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import StyleGlyph from "../Mark/StyleGlyph";
import { ordinal, shortDate } from "./waysFormat";
import "./PresetHistory.css";

/** What the dialog reads of an entry: a style's has its `rules`, a team's (`TEAM-9`) only what
 * it added and took away. */
type Entry = Omit<api.PresetHistoryEntry, "rules"> & { rules?: string[] };

/** Where the versions come from and how to go back: a style's by default, a team's when given. */
export interface HistorySource {
  list: () => Promise<Entry[]>;
  back: (seq: number) => Promise<unknown>;
}

/** What an entry is called in the list. */
function title(e: Entry): string {
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

const carriesRules = (e: Entry) => e.what === "original" || e.what === "version" || e.what === "back";

/**
 * `WRK-20`: one preset's history in the shared confirm shell, at a fixed height. Every
 * change is a version; going back writes an old one as a new one, so going back can
 * itself be gone back from. Locking is an entry of its own. `TEAM-9`: a team's history is the
 * same dialog (`source`, and `mark` for its icon).
 */
export default function PresetHistory({
  slug,
  name,
  glyph,
  mark,
  source,
  onClose,
  onChanged,
}: {
  slug: string;
  name: string;
  glyph?: string;
  mark?: ReactNode;
  source?: HistorySource;
  onClose: () => void;
  onChanged: () => void;
}) {
  const list = source?.list ?? (() => api.presetHistory(slug));
  const back = source?.back ?? ((seq: number) => api.presetBack(slug, seq));
  const [entries, setEntries] = useState<Entry[] | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  const [compare, setCompare] = useState(false);
  const [problem, setProblem] = useState("");

  useEffect(() => {
    let live = true;
    list()
      .then((all) => {
        if (!live) return;
        setEntries(all);
        // Start on the newest version that is not the one in use.
        setPicked(all.find((e) => carriesRules(e) && !e.current)?.seq ?? null);
      })
      .catch((e) => live && setProblem(String(e)));
    return () => {
      live = false;
    };
  }, [slug]);

  const now = entries?.find((e) => e.current);
  const chosen = entries?.find((e) => e.seq === picked);
  const canCompare = !!chosen?.rules && !!now?.rules;
  const bringBack = chosen?.rules && now?.rules ? chosen.rules.filter((r) => !now.rules!.includes(r)) : [];
  const takeAway = chosen?.rules && now?.rules ? now.rules.filter((r) => !chosen.rules!.includes(r)) : [];

  async function goBack() {
    if (!chosen || chosen.current) {
      setProblem(chosen ? "It already works that way." : "Pick a version first.");
      return;
    }
    try {
      await back(chosen.seq);
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
                  {mark ?? <StyleGlyph glyph={glyph ?? "ad-hoc"} size={13} />}
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
          {canCompare && (
            <button className="why-link" onClick={() => setCompare((c) => !c)} disabled={!chosen || chosen.current}>
              {compare ? "Hide the comparison" : "Compare with now"}
            </button>
          )}
          {canCompare && compare && chosen && !chosen.current && (
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
