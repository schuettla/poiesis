import { useState } from "react";
import { useAppStore } from "../../lib/store";
import type { DuplicateGroup } from "../../lib/api";
import "./Workbench.css";

function shortName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Never auto-deletes (`PHS-UI-1`): "Keep this one" is the only affordance,
 * and it always names what's being kept. */
function Group({ group }: { group: DuplicateGroup }) {
  const keepDuplicate = useAppStore((s) => s.keepDuplicate);
  const [keeping, setKeeping] = useState<string | null>(null);

  const keep = async (path: string) => {
    setKeeping(path);
    try {
      await keepDuplicate(group, path);
    } finally {
      setKeeping(null);
    }
  };

  return (
    <li className="wb-dup-group">
      <p className="wb-dup-relation">
        {group.kind === "image" ? group.relation : "reads the same"} · {group.files.length} files
      </p>
      <ul className="wb-dup-files">
        {group.files.map((f) => (
          <li className="wb-dup-file" key={f}>
            <span className="wb-dup-path" title={f}>
              {shortName(f)}
            </span>
            <button className="wb-link" disabled={!!keeping} onClick={() => keep(f)}>
              {keeping === f ? "Keeping…" : "Keep this one"}
            </button>
          </li>
        ))}
      </ul>
    </li>
  );
}

/** Results of `Tree.tsx`'s "Find duplicates" (`PHS-UI-1`). Renders nothing
 * until a scan has actually been asked for. */
export default function Duplicates() {
  const groups = useAppStore((s) => s.duplicateGroups);
  const loading = useAppStore((s) => s.duplicatesLoading);
  const error = useAppStore((s) => s.duplicatesError);
  const scanPath = useAppStore((s) => s.duplicateScanPath);
  const dismiss = useAppStore((s) => s.dismissDuplicates);

  if (groups === null && !loading && !error) return null;

  return (
    <section className="wb-section-block wb-duplicates">
      <div className="wb-section">
        Duplicates{scanPath ? ` in ${shortName(scanPath)}` : ""}
        <button
          className="wb-icon wb-section-action"
          title="Close"
          aria-label="Close duplicates"
          onClick={dismiss}
        >
          <svg width="13" height="13" viewBox="0 0 20 20" fill="none" aria-hidden="true">
            <path
              d="M5 5l10 10M15 5 5 15"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
            />
          </svg>
        </button>
      </div>
      {loading && <p className="wb-hint">Comparing files…</p>}
      {error && <p className="wb-error">{error}</p>}
      {!loading && !error && groups && groups.length === 0 && (
        <p className="wb-hint">Nothing here looks like a duplicate.</p>
      )}
      {!loading && groups && groups.length > 0 && (
        <ul className="wb-dup-list">
          {groups.map((g, i) => (
            <Group group={g} key={i} />
          ))}
        </ul>
      )}
    </section>
  );
}
