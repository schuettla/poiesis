import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import Select from "../Select/Select";
import Toggle from "../Toggle/Toggle";
import "./PresetDialog.css";

/**
 * `WRK-17`: the dialog that makes a style of your own, or renames one. The shared
 * confirm shell, so it looks like every other small question, and it never changes
 * height: the rules it starts with sit in a fixed box, and the sentence under the
 * fields is always there, empty until something needs saying.
 *
 * Making one is three choices: a name, what it starts from (and the dialog shows the
 * rules that brings), and whether I may improve it. The rules themselves are changed
 * afterwards in Self → How I work, where the new style is shown at once.
 *
 * It is opened by the store (`openPresetDialog`) from Self → How I work, the `/style`
 * menu and the board's ⋯ ("Keep this way of working as a style", which brings the
 * moves that fired in that run).
 */
export default function PresetDialog() {
  const req = useAppStore((s) => s.presetDialog);
  const close = useAppStore((s) => s.closePresetDialog);
  const presets = useAppStore((s) => s.presets);
  const loadPresets = useAppStore((s) => s.loadPresets);

  const [name, setName] = useState("");
  const [line, setLine] = useState("");
  const [from, setFrom] = useState("");
  const [improve, setImprove] = useState(false);
  const [problem, setProblem] = useState("");

  // Each time it opens it starts from what was asked for, not from the last time.
  useEffect(() => {
    if (!req) return;
    setName(req.name ?? "");
    setLine(req.line ?? "");
    setFrom(req.from ?? "");
    setImprove(false);
    setProblem("");
  }, [req]);

  if (!req) return null;
  const renaming = req.mode === "rename";
  const start = presets.find((p) => p.slug === from);
  const extra = req.add?.length ?? 0;

  async function go() {
    if (!req) return;
    if (!name.trim()) {
      setProblem("Give it a name.");
      return;
    }
    try {
      let slug = req.slug ?? "";
      if (renaming && req.slug) {
        await api.updatePreset(req.slug, { name, line });
      } else {
        slug = await api.createPreset({
          name,
          line,
          from: from || null,
          add: req.add,
          locked: !improve,
        });
      }
      await loadPresets();
      req.onDone?.(slug);
      close();
    } catch (e) {
      setProblem(String(e));
    }
  }

  return (
    <ConfirmDialog
      title={renaming ? "Rename style" : "New style"}
      confirmLabel={renaming ? "Save" : "Create"}
      onConfirm={go}
      onCancel={close}
    >
      <div className="preset-dialog">
        <label className="preset-field">
          <span>Name</span>
          <input
            className="field-input"
            value={name}
            maxLength={40}
            autoFocus
            placeholder="Careful with mail"
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && go()}
          />
        </label>
        <label className="preset-field">
          <span>What it's for, in one line (optional)</span>
          <input
            className="field-input"
            value={line}
            maxLength={120}
            placeholder="I ask before anything leaves the house"
            onChange={(e) => setLine(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && go()}
          />
        </label>
        {!renaming && (
          <>
            <div className="preset-field">
              <span>Starts from</span>
              <Select
                label="Starts from"
                value={from}
                options={[
                  { value: "", label: "Nothing, no rules yet" },
                  ...presets.filter((p) => p.slug !== "ad-hoc").map((p) => ({ value: p.slug, label: p.name })),
                ]}
                onChange={setFrom}
              />
            </div>
            <div className="preset-rules" aria-label="The rules it starts with">
              {start && start.rules.length > 0 ? (
                <ul>
                  {start.rules.map((r) => (
                    <li key={r}>{r}</li>
                  ))}
                </ul>
              ) : (
                <p className="preset-rules-empty">No rules yet. You add them in How I work once it is made.</p>
              )}
              {extra > 0 && (
                <p className="preset-note">
                  {extra === 1 ? "One more move that fired in that run is kept." : `${extra} more moves that fired in that run are kept.`}
                </p>
              )}
            </div>
            <label className="preset-improve">
              <Toggle checked={improve} onChange={setImprove} label="I may improve it" />
              <span>
                I may improve it
                <span className="preset-improve-help">
                  {improve ? "From how chats go, I suggest changes to its rules." : "Locked: it stays exactly as you set it."}
                </span>
              </span>
            </label>
          </>
        )}
        <p className="preset-problem" role="status">
          {problem}
        </p>
      </div>
    </ConfirmDialog>
  );
}
