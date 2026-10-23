import { availableFavorites } from "../../lib/modelPrefs";
import { useEffect, useState } from "react";
import { useAppStore, useExpert, personaTemperature } from "../../lib/store";
import * as api from "../../lib/api";
import type { Persona, ToolsetInfo } from "../../lib/api";
import { PERSONA_PRESETS, type PersonaPreset } from "../../lib/personaPresets";
import PersonaApps from "../Apps/PersonaApps";
import { forgetPersonaApps } from "../Apps/personaAppsLine";
import "./PersonaEditor.css";

/**
 * Standing instructions and the proposals to change them (SOUL-UI-1).
 *
 * The agent can only ever *propose* an edit here; accepting is a user action
 * outside the agent loop. The diff is two plain blocks on purpose — seeing the
 * whole before and after is more honest than a coloured line-level guess.
 */
function SoulEditor() {
  const soul = useAppStore((s) => s.memoryContext.soul);
  const refreshMemoryContext = useAppStore((s) => s.refreshMemoryContext);
  const proposals = useAppStore((s) => s.changeProposals);
  const resolveProposal = useAppStore((s) => s.resolveChangeProposal);

  const [text, setText] = useState(soul);
  const [saved, setSaved] = useState(false);

  // Follow the stored soul when it changes underneath us — accepting a
  // proposal rewrites it, and the textarea should show the result.
  useEffect(() => {
    setText(soul);
  }, [soul]);

  if (!api.inTauri()) return null;

  const pending = proposals.filter((p) => p.target === "soul");
  const dirty = text !== soul;

  async function save() {
    await api.setSoul(text);
    await refreshMemoryContext();
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  }

  return (
    <div className="soul-section">
      <div className="soul-head">
        <span className="soul-title">Soul</span>
        <span className="soul-hint">
          Standing instructions, sent with every conversation.
        </span>
      </div>

      <textarea
        className="system-prompt"
        rows={4}
        spellCheck={false}
        value={text}
        placeholder="e.g. Answer in German unless I write in English."
        onChange={(e) => setText(e.target.value)}
      />
      {(dirty || saved) && (
        <div className="setting-actions">
          <button className="btn-primary" onClick={save} disabled={!dirty}>
            {saved && !dirty ? "Saved" : "Save"}
          </button>
          {dirty && (
            <button className="btn-text" onClick={() => setText(soul)}>
              Revert
            </button>
          )}
        </div>
      )}

      {pending.map((p) => (
        <div className="soul-proposal" key={p.id}>
          <p className="soul-proposal-why">{p.rationale}</p>
          <div className="soul-diff">
            <div className="soul-diff-side">
              <span className="soul-diff-label">now</span>
              <pre>{soul || "(empty)"}</pre>
            </div>
            <div className="soul-diff-side">
              <span className="soul-diff-label">proposed</span>
              <pre>{p.proposed_text}</pre>
            </div>
          </div>
          <div className="setting-actions">
            <button className="btn-primary" onClick={() => resolveProposal(p.id, true)}>
              Accept
            </button>
            <button className="btn-text" onClick={() => resolveProposal(p.id, false)}>
              Dismiss
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}

interface Draft {
  id: string | null;
  name: string;
  systemPrompt: string;
  modelId: string;
  temperature: string;
  /** `PER-1`: toolset ids currently checked. Populated from every known toolset id
   * (persona-side "everything") whenever the persona has no allowlist yet. */
  checkedTools: string[];
  /** `SKL-6`: skill names currently checked, the same shape as `checkedTools`. */
  checkedSkills: string[];
  /** `SUB-3`: one line the lead reads when choosing who to hand a job to. */
  description: string;
  /** `SUB-3`: may the agent delegate work to this persona. */
  spawnable: boolean;
  /** `CLI-UI-4`: the agent app it runs on, or `""` for Poiesis. */
  engineId: string;
  /** `CA-UI-6`: the connected apps this persona may use. `null` is untouched, so
   * Save leaves what is stored alone. */
  appGrants: api.PersonaAppGrant[] | null;
}

const EMPTY: Draft = {
  id: null,
  name: "",
  systemPrompt: "",
  modelId: "",
  temperature: "",
  checkedTools: [],
  checkedSkills: [],
  appGrants: null,
  description: "",
  spawnable: false,
  engineId: "",
};

/** `PER-UI-1`: what this persona may do — checked-but-disabled + a note is how
 * `PER-2`'s intersection with the global toggle stays visible instead of
 * silently withdrawing a tool the persona itself never restricted. */
function PersonaToolsEditor({
  toolsets,
  checked,
  onToggle,
}: {
  toolsets: ToolsetInfo[];
  checked: string[];
  onToggle: (id: string) => void;
}) {
  if (toolsets.length === 0) return null;
  return (
    <div className="persona-tools">
      <label className="field-label">What this persona of mine may do</label>
      <div className="persona-tools-list">
        {toolsets.map((s) => {
          const isChecked = !s.enabled || checked.includes(s.id);
          return (
            <label
              key={s.id}
              className={`persona-tools-row${!s.enabled ? " disabled" : ""}`}
            >
              <input
                type="checkbox"
                checked={isChecked}
                disabled={!s.enabled}
                onChange={() => onToggle(s.id)}
              />
              <span className="persona-tools-name">{s.label}</span>
              {!s.enabled && (
                <span className="persona-tools-note">turned off in Settings</span>
              )}
            </label>
          );
        })}
      </div>
    </div>
  );
}

/** `SKL-6`: which of this persona's skills it may reach for — the same
 * checked-but-disabled + note pattern as `PersonaToolsEditor` for a skill
 * turned off globally. */
function PersonaSkillsEditor({
  skills,
  checked,
  onToggle,
}: {
  skills: api.SkillView[];
  checked: string[];
  onToggle: (name: string) => void;
}) {
  if (skills.length === 0) return null;
  return (
    <div className="persona-tools">
      <label className="field-label">Which of my skills this persona may use</label>
      <div className="persona-tools-list">
        {skills.map((s) => {
          const isChecked = !s.enabled || checked.includes(s.name);
          return (
            <label key={s.name} className={`persona-tools-row${!s.enabled ? " disabled" : ""}`}>
              <input
                type="checkbox"
                checked={isChecked}
                disabled={!s.enabled}
                onChange={() => onToggle(s.name)}
              />
              <span className="persona-tools-name">{s.name}</span>
              {!s.enabled && <span className="persona-tools-note">turned off in my Skills</span>}
            </label>
          );
        })}
      </div>
    </div>
  );
}

/** CRUD editor for saved personas (CHT-4), shown in Settings. */
export default function PersonaEditor() {
  const personas = useAppStore((s) => s.personas);
  const engines = useAppStore((s) => s.engines);
  const openEngines = useAppStore((s) => s.openEngines);
  const models = useAppStore((s) => s.models);
  const modelPrefs = useAppStore((s) => s.modelPrefs);
  const favoriteChat = availableFavorites(models, modelPrefs, "chat");
  const favoriteIds = new Set(favoriteChat.map((m) => m.id));
  const createPersona = useAppStore((s) => s.createPersona);
  const updatePersona = useAppStore((s) => s.updatePersona);
  const deletePersona = useAppStore((s) => s.deletePersona);
  const setDefaultPersona = useAppStore((s) => s.setDefaultPersona);

  const [draft, setDraft] = useState<Draft | null>(null);
  // `CLI-UI-4`: the agent apps a persona can run on: the ones that take jobs and
  // are on, and the one this persona already has even if it has since been
  // turned off (so the choice is not silently lost).
  const runsOnChoices = engines.filter((e) => e.found && e.delegate_ready && (e.enabled || e.id === draft?.engineId));
  const onAgentApp = (draft?.spawnable && draft.engineId && engines.find((e) => e.id === draft.engineId)) || null;
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [addingPreset, setAddingPreset] = useState<string | null>(null);
  const [toolsets, setToolsets] = useState<ToolsetInfo[]>([]);
  const skills = useAppStore((s) => s.skills);
  const expert = useExpert();

  useEffect(() => {
    if (!api.inTauri()) return;
    api.listToolsets().then(setToolsets).catch(() => {});
  }, []);

  // A preset drops off the list once a persona of that name exists, so the row
  // shrinks to nothing as they're adopted rather than offering duplicates.
  const existingNames = new Set(personas.map((p) => p.name));
  const availablePresets = PERSONA_PRESETS.filter((preset) => !existingNames.has(preset.name));

  async function addPreset(preset: PersonaPreset) {
    setAddingPreset(preset.name);
    try {
      await createPersona({
        name: preset.name,
        systemPrompt: preset.systemPrompt,
        temperature: preset.temperature,
        ...(preset.tools ? { toolsJson: JSON.stringify(preset.tools) } : {}),
        ...(preset.description ? { description: preset.description } : {}),
      });
    } finally {
      setAddingPreset(null);
    }
  }

  function startNew() {
    setDraft({ ...EMPTY, checkedTools: toolsets.map((s) => s.id), checkedSkills: skills.map((s) => s.name) });
  }

  function startEdit(p: Persona) {
    const temp = personaTemperature(p);
    const allow = p.tools_json ? (JSON.parse(p.tools_json) as string[]) : null;
    const allowSkills = p.skills_json ? (JSON.parse(p.skills_json) as string[]) : null;
    setDraft({
      id: p.id,
      name: p.name,
      systemPrompt: p.system_prompt,
      modelId: p.model_id ?? "",
      temperature: typeof temp === "number" ? String(temp) : "",
      checkedTools: allow ?? toolsets.map((s) => s.id),
      checkedSkills: allowSkills ?? skills.map((s) => s.name),
      description: p.description ?? "",
      spawnable: p.spawnable,
      engineId: p.engine_id ?? "",
      appGrants: null,
    });
  }

  function toggleTool(id: string) {
    setDraft((d) => {
      if (!d) return d;
      const has = d.checkedTools.includes(id);
      return {
        ...d,
        checkedTools: has ? d.checkedTools.filter((x) => x !== id) : [...d.checkedTools, id],
      };
    });
  }

  function toggleSkill(name: string) {
    setDraft((d) => {
      if (!d) return d;
      const has = d.checkedSkills.includes(name);
      return {
        ...d,
        checkedSkills: has ? d.checkedSkills.filter((x) => x !== name) : [...d.checkedSkills, name],
      };
    });
  }

  // `PER-1`: NULL means "every enabled toolset" — only persist a restriction once
  // at least one currently-enabled toolset has actually been unchecked.
  function toolsJsonForSave(checkedTools: string[]): string | null {
    if (toolsets.length === 0) return null;
    const fullChecked = toolsets.filter((s) => !s.enabled || checkedTools.includes(s.id));
    return fullChecked.length === toolsets.length ? null : JSON.stringify(fullChecked.map((s) => s.id));
  }

  // `SKL-6`: mirrors `toolsJsonForSave` exactly.
  function skillsJsonForSave(checkedSkills: string[]): string | null {
    if (skills.length === 0) return null;
    const fullChecked = skills.filter((s) => !s.enabled || checkedSkills.includes(s.name));
    return fullChecked.length === skills.length ? null : JSON.stringify(fullChecked.map((s) => s.name));
  }

  async function save() {
    if (!draft || !draft.name.trim() || !draft.systemPrompt.trim()) return;
    setBusy(true);
    setSaveError(null);
    try {
      let savedId = draft.id;
      const temperature = draft.temperature.trim() === "" ? undefined : Number(draft.temperature);
      const modelId = draft.modelId || null;
      const toolsJson = toolsJsonForSave(draft.checkedTools);
      const skillsJson = skillsJsonForSave(draft.checkedSkills);
      if (draft.id) {
        const existing = personas.find((p) => p.id === draft.id);
        if (existing) {
          await updatePersona({
            ...existing,
            name: draft.name.trim(),
            system_prompt: draft.systemPrompt.trim(),
            model_id: modelId,
            params_json:
              typeof temperature === "number" && !Number.isNaN(temperature)
                ? JSON.stringify({ temperature })
                : null,
            tools_json: toolsJson,
            skills_json: skillsJson,
            description: draft.description.trim() || null,
            spawnable: draft.spawnable,
            engine_id: draft.spawnable && draft.engineId ? draft.engineId : null,
          });
        }
      } else {
        const created = await createPersona({
          name: draft.name.trim(),
          systemPrompt: draft.systemPrompt.trim(),
          modelId,
          temperature:
            typeof temperature === "number" && !Number.isNaN(temperature) ? temperature : undefined,
          toolsJson,
          skillsJson,
          description: draft.description.trim() || null,
          spawnable: draft.spawnable,
          engineId: draft.spawnable && draft.engineId ? draft.engineId : null,
        });
        savedId = created?.id ?? null;
      }
      // Which apps it may use is saved with it, like its tools and skills.
      if (draft.appGrants && savedId) {
        try {
          await api.setPersonaAppGrants(savedId, draft.appGrants);
          forgetPersonaApps(savedId);
        } catch (e) {
          setSaveError(`The persona is saved, but its apps weren't: ${String(e).replace(/^Error:\s*/, "")}`);
          return;
        }
      }
      setDraft(null);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="persona-editor">
      <SoulEditor />

      {personas.length === 0 && !draft && (
        <p className="empty-hint">
          No personas yet. Create one to switch prompt + settings per chat, or start from a preset below.
        </p>
      )}

      {!draft && availablePresets.length > 0 && (
        <div className="persona-presets">
          <span className="persona-presets-label">Start from a preset</span>
          <div className="persona-presets-row">
            {availablePresets.map((preset) => (
              <button
                key={preset.name}
                className="persona-preset-chip"
                onClick={() => addPreset(preset)}
                disabled={addingPreset === preset.name}
                title={preset.systemPrompt}
              >
                {addingPreset === preset.name ? "Adding…" : preset.name}
              </button>
            ))}
          </div>
        </div>
      )}

      {!draft &&
        personas.map((p) => (
          <div className="persona-row" key={p.id}>
            <div className="persona-info">
              <span className="persona-name">
                {p.name}
                {p.is_default && <span className="persona-default">default</span>}
              </span>
              <span className="persona-preview">{p.system_prompt}</span>
            </div>
            <div className="persona-actions">
              {!p.is_default && (
                <button className="btn-text" onClick={() => setDefaultPersona(p.id)}>
                  Set default
                </button>
              )}
              <button className="btn-text" onClick={() => startEdit(p)}>
                Edit
              </button>
              <button className="btn-text danger" onClick={() => deletePersona(p.id)}>
                Delete
              </button>
            </div>
          </div>
        ))}

      {draft && (
        <div className="persona-form">
          <label className="field-label">Name</label>
          <input
            className="field-input"
            value={draft.name}
            placeholder="e.g. Terse code reviewer"
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          />

          <label className="field-label">System prompt</label>
          <textarea
            className="system-prompt"
            rows={4}
            spellCheck={false}
            value={draft.systemPrompt}
            placeholder="How this persona should behave…"
            onChange={(e) => setDraft({ ...draft, systemPrompt: e.target.value })}
          />

          {/* `SUB-UI-5`: personas are agent types. These two fields are what
              turn one into something the agent can hand a job to. */}
          <label className="field-label">When should I use this agent?</label>
          <input
            className="field-input"
            value={draft.description}
            placeholder="e.g. Reads sources carefully and reports back with citations"
            onChange={(e) => setDraft({ ...draft, description: e.target.value })}
          />
          <p className="field-hint">
            I read this when deciding who to hand a job to, so say what it is good at.
          </p>

          <label className="persona-spawnable">
            <input
              type="checkbox"
              checked={draft.spawnable}
              onChange={(e) => setDraft({ ...draft, spawnable: e.target.checked })}
            />
            I can hand work to this agent
          </label>

          {/* `CLI-UI-4`: which agent app does the work, if not me. */}
          {draft.spawnable && runsOnChoices.length > 0 && (
            <div className="persona-runs-on">
              <label className="field-label" htmlFor="persona-runs-on">
                Runs on
              </label>
              <select
                id="persona-runs-on"
                className="field-input"
                value={draft.engineId}
                onChange={(e) => setDraft({ ...draft, engineId: e.target.value })}
              >
                <option value="">Poiesis (I choose a model)</option>
                {runsOnChoices.map((e) => (
                  <option key={e.id} value={e.id}>
                    {e.enabled ? e.name : `${e.name} (off, turn it on in External agents)`}
                  </option>
                ))}
              </select>
              {onAgentApp && (
                <p className="field-hint">
                  It brings its own model and tools. I give it this persona's instructions with the task, and
                  the folder.
                </p>
              )}
              {onAgentApp && !onAgentApp.enabled && (
                <p className="persona-tools-note" role="status">
                  {onAgentApp.name} is off, so I won't hand this agent any work.{" "}
                  <button className="link-btn" onClick={() => openEngines(onAgentApp.id)}>
                    Turn it on in External agents
                  </button>
                </p>
              )}
            </div>
          )}

          {!onAgentApp && (
          <div className="persona-form-row">
            <div className="persona-field">
              <label className="field-label">Model (optional)</label>
              <select
                className="field-input"
                value={draft.modelId}
                onChange={(e) => setDraft({ ...draft, modelId: e.target.value })}
              >
                <option value="">Use the chat's current model</option>
                {/* `MOD-4`: favorites first, the full list behind them. */}
                {favoriteChat.length > 0 && (
                  <optgroup label="Favorites">
                    {favoriteChat.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.name}
                      </option>
                    ))}
                  </optgroup>
                )}
                <optgroup label={favoriteChat.length > 0 ? "More models…" : "Models"}>
                  {models
                    .filter((m) => !favoriteIds.has(m.id))
                    .map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.name}
                      </option>
                    ))}
                </optgroup>
              </select>
            </div>
            <div className="persona-field narrow">
              <label className="field-label">Temperature (optional)</label>
              <input
                className="field-input"
                type="number"
                min={0}
                max={2}
                step={0.1}
                value={draft.temperature}
                placeholder="0.7"
                onChange={(e) => setDraft({ ...draft, temperature: e.target.value })}
              />
            </div>
          </div>
          )}

          {expert && !onAgentApp && (
            <>
              <PersonaToolsEditor toolsets={toolsets} checked={draft.checkedTools} onToggle={toggleTool} />
              <PersonaSkillsEditor skills={skills} checked={draft.checkedSkills} onToggle={toggleSkill} />
              <PersonaApps
                personaId={draft.id}
                value={draft.appGrants}
                onChange={(appGrants) => setDraft({ ...draft, appGrants })}
              />
            </>
          )}

          {saveError && (
            <p className="persona-tools-note" role="alert">
              {saveError}
            </p>
          )}
          <div className="setting-actions">
            <button
              className="btn-primary"
              onClick={save}
              disabled={busy || !draft.name.trim() || !draft.systemPrompt.trim()}
            >
              {draft.id ? "Save changes" : "Create persona"}
            </button>
            <button className="btn-secondary" onClick={() => setDraft(null)} disabled={busy}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {!draft && (
        <div className="setting-actions">
          <button className="btn-secondary" onClick={startNew}>
            New persona
          </button>
        </div>
      )}
    </div>
  );
}
