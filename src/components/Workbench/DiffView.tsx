import type { FileChange } from "../../lib/api";
import { languageForPath, tokenizeDiff, useTokens } from "../../lib/highlight";
import TokenLine from "./TokenLine";
import "./Changes.css";

/** What happened to a file, in words: the status the patch alone cannot say. */
export function changeLabel(file: FileChange): string {
  switch (file.status) {
    case "added":
      return "new file";
    case "deleted":
      return "deleted";
    case "moved":
      return file.from ? `moved from ${file.from}` : "moved";
    default:
      return "";
  }
}

/**
 * `PRJ-UI-3`: one file's patch, as two gutters — the old line number and the
 * new one — beside the marked line. It scrolls sideways inside its own box so
 * a long line never widens the panel around it.
 */
export default function DiffView({ file }: { file: FileChange }) {
  // The line text stays plain until the colours land. The same array identity
  // is what a refreshed change set replaces, so a new patch re-colours.
  const tokens = useTokens(async () => {
    const lang = await languageForPath(file.path);
    return lang ? tokenizeDiff(file, lang) : null;
  }, [file.path, file.hunks]);
  if (file.binary || file.too_large) {
    return (
      <p className="chg-note">
        {file.binary ? "This is a binary file, so there is no patch to show." : "This file is too large to show as a patch."}
      </p>
    );
  }
  if (file.hunks.length === 0) {
    return <p className="chg-note">{file.status === "moved" ? "Moved without changing its contents." : "No lines changed."}</p>;
  }
  return (
    <div className="chg-diff" role="table" aria-label={`Changes to ${file.display}`}>
      {file.hunks.map((h, i) => (
        <div key={i} className="chg-hunk" role="rowgroup">
          <div className="chg-hunk-head" role="row">
            <span role="cell">
              @@ −{h.old_start},{h.old_lines} +{h.new_start},{h.new_lines} @@
            </span>
          </div>
          {h.lines.map((l, j) => (
            <div key={j} className={`chg-line chg-l-${l.kind}`} role="row">
              <span className="chg-gutter" role="cell" aria-label={l.old_no ? `old line ${l.old_no}` : undefined}>
                {l.old_no ?? ""}
              </span>
              <span className="chg-gutter" role="cell" aria-label={l.new_no ? `new line ${l.new_no}` : undefined}>
                {l.new_no ?? ""}
              </span>
              <span className="chg-sign" role="cell" aria-hidden="true">
                {l.kind === "added" ? "+" : l.kind === "removed" ? "−" : " "}
              </span>
              <span className={`chg-text${tokens ? " hl" : ""}`} role="cell">
                {tokens?.[i]?.[j]?.length ? <TokenLine tokens={tokens[i][j]} /> : l.text || " "}
              </span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
