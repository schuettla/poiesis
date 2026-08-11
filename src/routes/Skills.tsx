import { useEffect, useState } from "react";
import * as api from "../lib/api";
import { useAppStore } from "../lib/store";
import "./Surface.css";
import "./Apps.css";
import "./Skills.css";

const SOURCE_LABEL: Record<string, string> = {
  personal: "yours",
  project: "this folder",
  app: "mine",
};

export default function Skills() {
  const skills = useAppStore((s) => s.skills);
  const refresh = useAppStore((s) => s.refreshSkills);
  const setEnabled = useAppStore((s) => s.setSkillEnabled);
  const forget = useAppStore((s) => s.forgetSkill);
  const revealInSystem = useAppStore((s) => s.revealInSystem);
  const conversations = useAppStore((s) => s.conversations);
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const folderPath = conversations.find((c) => c.id === activeConversationId)?.folderPath ?? null;

  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [writing, setWriting] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [whenToUse, setWhenToUse] = useState("");
  const [body, setBody] = useState("");
  const [saving, setSaving] = useState(false);

  // `View`: any skill's body, read-only. `Edit`: an App-sourced skill's body,
  // writable via `update_skill_cmd` — Personal/Project skills aren't ours to
  // rewrite, so they only ever get `View`.
  const [viewing, setViewing] = useState<string | null>(null);
  const [viewBody, setViewBody] = useState("");
  const [viewLoading, setViewLoading] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [editDescription, setEditDescription] = useState("");
  const [editWhenToUse, setEditWhenToUse] = useState("");
  const [editBody, setEditBody] = useState("");
  const [editSaving, setEditSaving] = useState(false);

  // Resolved (and created) on open, so the path shown is always real.
  const [personalDir, setPersonalDir] = useState("");

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    if (!api.inTauri()) return;
    api
      .personalSkillsDir()
      .then(setPersonalDir)
      .catch(() => {});
  }, []);

  function openSkillsFolder() {
    if (personalDir) revealInSystem(personalDir);
  }

  // `SKL-4` import: what other agents on this machine have lying around.
  // Scanned only when the user opens the panel — Poiesis doesn't survey the
  // disk for other products' folders in the background.
  const [importOpen, setImportOpen] = useState(false);
  const [found, setFound] = useState<api.ImportableSkill[]>([]);
  const [scanning, setScanning] = useState(false);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [importing, setImporting] = useState(false);
  const [importNote, setImportNote] = useState<string | null>(null);

  async function scanForImports(extraRoot?: string) {
    setScanning(true);
    setImportNote(null);
    try {
      const rows = await api.discoverableSkillImports(extraRoot ? [extraRoot] : undefined);
      setFound(rows);
      setChosen(new Set(rows.filter((r) => !r.already_have).map((r) => r.dir)));
      if (rows.length === 0) {
        setImportNote("I didn't find any skills from other agents on this machine.");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }

  function openImport() {
    setImportOpen(true);
    if (found.length === 0) scanForImports();
  }

  async function browseForImports() {
    const dir = await api.pickFolder().catch(() => null);
    if (dir) await scanForImports(dir);
  }

  async function runImport() {
    setImporting(true);
    setImportNote(null);
    try {
      const failed = await api.importSkills([...chosen]);
      const done = chosen.size - failed.length;
      setImportNote(
        failed.length === 0
          ? `Copied ${done} skill${done === 1 ? "" : "s"} in. They're mine now — the originals are untouched.`
          : `Copied ${done} in. I couldn't take: ${failed.join("; ")}.`
      );
      await refresh();
      await scanForImports();
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  }

  async function toggle(skill: api.SkillView) {
    setBusy(skill.name);
    try {
      await setEnabled(skill.source, skill.name, !skill.enabled);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function remove(skill: api.SkillView) {
    setBusy(skill.name);
    try {
      await forget(skill.name);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function addFromFolder() {
    setError(null);
    try {
      const dir = await api.pickFolder();
      if (!dir) return;
      await api.installSkill(dir);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function addFromZip() {
    setError(null);
    try {
      const archive = await api.pickZipFile();
      if (!archive) return;
      await api.installSkillZip(archive);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function view(skill: api.SkillView) {
    if (viewing === skill.name) {
      setViewing(null);
      return;
    }
    setEditing(null);
    setViewing(skill.name);
    setViewLoading(true);
    setError(null);
    try {
      setViewBody(await api.skillBody(skill.name, folderPath));
    } catch (e) {
      setError(String(e));
      setViewing(null);
    } finally {
      setViewLoading(false);
    }
  }

  async function startEdit(skill: api.SkillView) {
    setViewing(null);
    setError(null);
    try {
      const current = await api.skillBody(skill.name, folderPath);
      setEditDescription(skill.description);
      setEditWhenToUse(skill.when_to_use ?? "");
      setEditBody(current);
      setEditing(skill.name);
    } catch (e) {
      setError(String(e));
    }
  }

  async function saveEdit() {
    if (!editing) return;
    setEditSaving(true);
    setError(null);
    try {
      await api.updateSkill(editing, editDescription.trim(), editWhenToUse.trim(), editBody);
      setEditing(null);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setEditSaving(false);
    }
  }

  async function write() {
    setSaving(true);
    setError(null);
    try {
      await api.createSkill(name.trim(), description.trim(), whenToUse.trim(), body);
      setName("");
      setDescription("");
      setWhenToUse("");
      setBody("");
      setWriting(false);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  if (!api.inTauri()) {
    return (
      <div className="surface">
        <div className="surface-inner">
          <h1>Skills</h1>
          <p className="lede">Skills run in the desktop app.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Skills</h1>
        <p className="lede">
          Step-by-step procedures I can read before doing a task — mine or yours. Reading a skill is
          free; a skill I write for myself still asks first.
        </p>
        {/* The folder has to be nameable, because "drop a skill in" is the
            main way one arrives and it's a path the user has never seen. The
            button creates it if it isn't there yet, so the advice is never
            about a place that doesn't exist. */}
        <p className="skills-folder-line">
          Your skills live in{" "}
          <button className="link-btn" onClick={openSkillsFolder} title="Open this folder">
            {personalDir || "your Poiesis folder"}
          </button>
          . Anything you drop there shows up here. I use the open{" "}
          <span className="skills-nowrap">SKILL.md</span> format, so a folder written for another
          agent works as-is once you copy it in — I don't read other agents' folders on my own.
        </p>

        {error && <p className="hw-note error">{error}</p>}

        <section className="connect-card">
          <div className="skills-header-actions">
            <button className="btn-primary" onClick={() => setWriting((v) => !v)}>
              {writing ? "Cancel" : "Write a skill"}
            </button>
            <button className="btn-secondary" onClick={addFromFolder}>
              Add from folder…
            </button>
            <button className="btn-secondary" onClick={addFromZip}>
              Add from zip…
            </button>
            <button className="btn-secondary" onClick={openImport}>
              Import from another agent…
            </button>
          </div>

          {importOpen && (
            <div className="skills-import">
              <p className="skills-import-lede">
                Skills other agents on this machine already have. I don't read these folders on my
                own — copying one in is what makes it mine.
              </p>
              <div className="skills-header-actions">
                <button className="btn-secondary" onClick={() => scanForImports()} disabled={scanning}>
                  {scanning ? "Looking…" : "Look again"}
                </button>
                <button className="btn-secondary" onClick={browseForImports} disabled={scanning}>
                  Pick a folder…
                </button>
                <button className="btn-text" onClick={() => setImportOpen(false)}>
                  Close
                </button>
              </div>

              {importNote && <p className="hw-note">{importNote}</p>}

              {found.length > 0 && (
                <>
                  <div className="skills-import-list">
                    {found.map((f) => (
                      <label className="skills-import-row" key={f.dir}>
                        <input
                          type="checkbox"
                          checked={chosen.has(f.dir)}
                          onChange={(e) => {
                            const next = new Set(chosen);
                            if (e.target.checked) next.add(f.dir);
                            else next.delete(f.dir);
                            setChosen(next);
                          }}
                        />
                        <span className="skills-import-body">
                          <span className="skills-import-name">
                            {f.name}
                            <span className="skill-source-badge">{f.agent}</span>
                            {f.already_have && (
                              <span className="skill-partial-chip" title="Importing replaces mine">
                                ◇ I already have one of these
                              </span>
                            )}
                            {f.risk > 0 && (
                              <span
                                className="skill-partial-chip"
                                title={`I found: ${f.risk_flags.join(", ")}`}
                              >
                                ◇ reads like instructions to me
                              </span>
                            )}
                          </span>
                          <span className="skills-import-desc">{f.description}</span>
                        </span>
                      </label>
                    ))}
                  </div>
                  <button
                    className="btn-primary"
                    onClick={runImport}
                    disabled={importing || chosen.size === 0}
                  >
                    {importing
                      ? "Copying…"
                      : `Copy ${chosen.size} skill${chosen.size === 1 ? "" : "s"} in`}
                  </button>
                </>
              )}
            </div>
          )}

          {writing && (
            <div className="skills-write-form">
              <label className="field">
                <span className="field-label">Name</span>
                <input
                  className="field-input"
                  placeholder="e.g. weekly-report"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                />
              </label>
              <label className="field">
                <span className="field-label">Description</span>
                <input
                  className="field-input"
                  placeholder="One line — what it does"
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                />
              </label>
              <label className="field">
                <span className="field-label">When to use it</span>
                <input
                  className="field-input"
                  placeholder="One line — when I should reach for this"
                  value={whenToUse}
                  onChange={(e) => setWhenToUse(e.target.value)}
                />
              </label>
              <label className="field">
                <span className="field-label">Steps</span>
                <textarea
                  className="bundle-text"
                  rows={8}
                  placeholder="Numbered steps, imperative."
                  value={body}
                  onChange={(e) => setBody(e.target.value)}
                />
              </label>
              <div className="connect-actions">
                <button
                  className="btn-primary"
                  onClick={write}
                  disabled={saving || !name.trim() || !description.trim() || !whenToUse.trim() || !body.trim()}
                >
                  {saving ? "Saving…" : "Save skill"}
                </button>
              </div>
            </div>
          )}
        </section>

        <section className="model-section">
          <h2 className="section-title">My skills</h2>
          {skills.length === 0 ? (
            <p className="placeholder-note">
              I don't have any skills yet. Write me one, drop a SKILL.md folder into my skills
              folder above, or finish a task and I'll ask whether to keep the procedure.
            </p>
          ) : (
            <div className="connector-list">
              {skills.map((s) => (
                <div className={`connector-card ${s.enabled ? "" : "disabled"}`} key={`${s.source}-${s.name}`}>
                  <div className="connector-head">
                    <div className="connector-title">
                      <span className={`status-dot ${s.enabled ? "on" : "off"}`} aria-hidden="true" />
                      <span className="connector-name">{s.name}</span>
                      <span className="skill-source-badge">{SOURCE_LABEL[s.source] ?? s.source}</span>
                      {s.unsupported.length > 0 && (
                        <span className="skill-partial-chip" title={`I ignore: ${s.unsupported.join(", ")}`}>
                          ◇ partial
                        </span>
                      )}
                      {/* SKL-4: what this skill contains, before the decision
                          to enable it. Provenance, not an alarm — same `◇`
                          family and ink tone as the outside-content chip, no
                          warning colour, and it never blocks anything. */}
                      {s.risk > 0 && (
                        <span
                          className="skill-partial-chip"
                          title={`I found: ${s.risk_flags.join(", ")}`}
                        >
                          ◇ reads like instructions to me
                        </span>
                      )}
                    </div>
                    <label className="switch" title={s.enabled ? "Enabled" : "Disabled"}>
                      <input
                        type="checkbox"
                        checked={s.enabled}
                        disabled={busy === s.name}
                        onChange={() => toggle(s)}
                      />
                      <span className="switch-label">{s.enabled ? "On" : "Off"}</span>
                    </label>
                  </div>
                  <div className="connector-url">{s.description}</div>
                  {s.when_to_use && <div className="skill-when">Use when: {s.when_to_use}</div>}
                  {s.used > 0 && (
                    <div className="skill-usage">
                      {s.rough > 0 ? `used ${s.used}× · ${s.rough} rough` : `used ${s.used}×`}
                    </div>
                  )}
                  <div className="connector-actions">
                    <button className="btn-text" onClick={() => view(s)}>
                      {viewing === s.name ? "Hide" : "View"}
                    </button>
                    {s.source === "app" && (
                      <button className="btn-text" onClick={() => startEdit(s)}>
                        Edit
                      </button>
                    )}
                    <button className="btn-text" onClick={() => api.revealPath(s.dir)}>
                      Reveal in Explorer
                    </button>
                    {s.source === "app" && (
                      <button className="btn-text danger" onClick={() => remove(s)} disabled={busy === s.name}>
                        Forget
                      </button>
                    )}
                  </div>

                  {viewing === s.name && (
                    <div className="skills-view-body">
                      {viewLoading ? (
                        <p className="placeholder-note">Reading…</p>
                      ) : (
                        <pre className="bundle-text skills-view-pre">{viewBody}</pre>
                      )}
                    </div>
                  )}

                  {editing === s.name && (
                    <div className="skills-write-form">
                      <label className="field">
                        <span className="field-label">Description</span>
                        <input
                          className="field-input"
                          value={editDescription}
                          onChange={(e) => setEditDescription(e.target.value)}
                        />
                      </label>
                      <label className="field">
                        <span className="field-label">When to use it</span>
                        <input
                          className="field-input"
                          value={editWhenToUse}
                          onChange={(e) => setEditWhenToUse(e.target.value)}
                        />
                      </label>
                      <label className="field">
                        <span className="field-label">Steps</span>
                        <textarea
                          className="bundle-text"
                          rows={8}
                          value={editBody}
                          onChange={(e) => setEditBody(e.target.value)}
                        />
                      </label>
                      <div className="connect-actions">
                        <button
                          className="btn-primary"
                          onClick={saveEdit}
                          disabled={editSaving || !editDescription.trim() || !editWhenToUse.trim() || !editBody.trim()}
                        >
                          {editSaving ? "Saving…" : "Save changes"}
                        </button>
                        <button className="btn-text" onClick={() => setEditing(null)} disabled={editSaving}>
                          Cancel
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
