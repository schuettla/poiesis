import { useState } from "react";
import type { Diagnostic } from "../../lib/api";
import "./Diagnostics.css";

/** How many files show before the rest fold away (`COD-UI-3`). */
const FILES_SHOWN = 5;

/** A diagnostic's file as an absolute path, when it can be placed: relative
 * paths hang off the folder the task ran in. `null` for a failure that names
 * no file. */
export function locateDiagnostic(file: string, root: string | null, cwd = ""): string | null {
  if (!file) return null;
  if (/^[a-zA-Z]:[\\/]/.test(file) || file.startsWith("/") || file.startsWith("\\\\")) return file;
  if (!root) return null;
  const sep = root.includes("\\") ? "\\" : "/";
  const parts = [root.replace(/[\\/]+$/, ""), cwd, file.replace(/^\.[\\/]/, "")].filter(Boolean);
  return parts.join(sep).replace(/[\\/]/g, sep);
}

function severityLabel(d: Diagnostic): string {
  return d.severity === "failure" ? "failed" : d.severity;
}

/**
 * `COD-UI-3`: one error grammar across the app. A build, a test run, a
 * `diagnostics` block and a code artifact's traceback all render through this:
 * severity dot, `path:line`, message, code, grouped by file.
 *
 * `onOpen` makes every location a button (`COD-UI-2`). Without it — a block
 * with nowhere to resolve a path against — locations are plain text.
 */
export default function DiagnosticsList({
  items,
  onOpen,
}: {
  items: Diagnostic[];
  onOpen?: (d: Diagnostic) => void;
}) {
  const [showAll, setShowAll] = useState(false);
  if (items.length === 0) return null;

  const groups: { file: string; items: Diagnostic[] }[] = [];
  for (const d of items) {
    const g = groups.find((x) => x.file === d.file);
    if (g) g.items.push(d);
    else groups.push({ file: d.file, items: [d] });
  }
  const shown = showAll ? groups : groups.slice(0, FILES_SHOWN);

  return (
    <div className="diag-list">
      {shown.map((g) => (
        <div key={g.file || "(no file)"} className="diag-group">
          {g.file && <div className="diag-file">{g.file}</div>}
          <ul className="diag-rows">
            {g.items.map((d, i) => {
              const where = d.line ? `${d.line}${d.col ? `:${d.col}` : ""}` : "";
              return (
                <li key={`${d.line}-${d.col}-${i}`} className={`diag-row diag-${d.severity}`}>
                  <span className="diag-dot" aria-label={severityLabel(d)} />
                  {where &&
                    (onOpen && g.file ? (
                      <button
                        className="diag-where"
                        onClick={() => onOpen(d)}
                        title={`Open ${g.file} at line ${d.line}`}
                      >
                        {where}
                      </button>
                    ) : (
                      <span className="diag-where plain">{where}</span>
                    ))}
                  <span className="diag-message">{d.message}</span>
                  {d.code && <span className="diag-code">{d.code}</span>}
                </li>
              );
            })}
          </ul>
        </div>
      ))}
      {groups.length > FILES_SHOWN && (
        <button className="diag-more" onClick={() => setShowAll((v) => !v)}>
          {showAll ? "Show fewer files" : `${groups.length - FILES_SHOWN} more files`}
        </button>
      )}
    </div>
  );
}
