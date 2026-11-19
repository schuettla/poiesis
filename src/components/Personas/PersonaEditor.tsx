import { availableFavorites } from "../../lib/modelPrefs";
import { useEffect, useRef, useState } from "react";
import { useAppStore, useExpert, personaTemperature } from "../../lib/store";
import * as api from "../../lib/api";
import type { Persona, ToolsetInfo } from "../../lib/api";
import { PERSONA_PRESETS, type PersonaPreset } from "../../lib/personaPresets";
import { PERSONA_HUES, lookOf } from "../../lib/personaLook";
import { leaderOf } from "../../lib/leaders";
import { deleteWords, placesOf } from "../../lib/teamNotes";
import PersonaApps from "../Apps/PersonaApps";
import { forgetPersonaApps } from "../Apps/personaAppsLine";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import Fold, { useFolds } from "./Fold";
import PickCards from "./PickCards";
import PersonaGlyph, { PoiesisGlyph } from "./PersonaGlyph";
import PersonaKnows from "./PersonaKnows";
import PoiesisEntry from "./PoiesisEntry";
import "../Self/Self.css";
import "./PersonaEditor.css";

interface Draft {
  id: string | null;
  name: string;
  /** `TEAM-4`: the line under the name. */
  line: string;
  /** `TEAM-4`: a glyph the person chose (JSON), or `""` to have it made from the name. */
  look: string;
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
  /** `WRK-17`: the preset (a slug) chats with this persona start with, or `""` for none. */
  style: string;
  /** What was stored when the form opened, so Save only writes a change. */
  styleWas: string;
}

const EMPTY: Draft = {
  id: null,
  name: "",
  line: "",
  look: "",
  systemPrompt: "",
  modelId: "",
  temperature: "",
  checkedTools: [],
  checkedSkills: [],
  appGrants: null,
  description: "",
  spawnable: false,
  engineId: "",
  style: "",
  styleWas: "",
};

/** Poiesis is not a persona that can be made, so nobody else may take its name. */
const isPoiesis = (name: string) => name.trim().toLowerCase() === "poiesis";

/** What changed, ignoring the bookkeeping that is not the person's doing. */
const key = (d: Draft) => JSON.stringify({ ...d, styleWas: "" });

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

/** The face: a letter in a circle, one of 8 hues. Made from the name until one is chosen. */
function FacePicker({ name, look, onChange }: { name: string; look: string; onChange: (look: string) => void }) {
  const now = lookOf({ name: name || "?", look });
  const set = (letter: string, hue: number) => onChange(JSON.stringify({ letter, hue }));
  return (
    <div className="persona-face">
      <PersonaGlyph name={name || "?"} look={look} size={30} />
      <input
        className="field-input persona-face-letter"
        aria-label="Letter"
        maxLength={2}
        value={now.letter}
        onChange={(e) => {
          const letter = Array.from(e.target.value.trim())[0];
          if (letter) set(letter, now.hue);
        }}
      />
      <div className="persona-face-hues" role="group" aria-label="Colour">
        {Array.from({ length: PERSONA_HUES }, (_, i) => i + 1).map((hue) => (
          <button
            key={hue}
            type="button"
            className="persona-face-hue"
            aria-label={`Colour ${hue}`}
            aria-pressed={now.hue === hue}
            onClick={() => set(now.letter, hue)}
          >
            <span className="persona-glyph" data-hue={hue} style={{ width: 16, height: 16 }} aria-hidden="true" />
          </button>
        ))}
      </div>
      {look && (
        <button type="button" className="btn-text" onClick={() => onChange("")}>
          From the name
        </button>
      )}
    </div>
  );
}

/**
 * `TEAM-4` (was `CHT-4`): the personas, in Settings → Personas & teams. Cards the way How I
 * work shows the styles, Poiesis fixed first; the picked one under them in one card: who it is
 * always open, the rest in sections that fold.
 */
export default function PersonaEditor({ onOpenTeam }: { onOpenTeam?: (slug: string) => void } = {}) {
  const personas = useAppStore((s) => s.personas);
  const conversations = useAppStore((s) => s.conversations);
  const engines = useAppStore((s) => s.engines);
  const openEngines = useAppStore((s) => s.openEngines);
  const openSelf = useAppStore((s) => s.openSelf);
  const showChatsLedBy = useAppStore((s) => s.showChatsLedBy);
  const models = useAppStore((s) => s.models);
  const modelPrefs = useAppStore((s) => s.modelPrefs);
  const favoriteChat = availableFavorites(models, modelPrefs, "chat");
  const favoriteIds = new Set(favoriteChat.map((m) => m.id));
  const createPersona = useAppStore((s) => s.createPersona);
  const updatePersona = useAppStore((s) => s.updatePersona);
  const deletePersona = useAppStore((s) => s.deletePersona);
  const setDefaultPersona = useAppStore((s) => s.setDefaultPersona);
  const presets = useAppStore((s) => s.presets);
  const loadPresets = useAppStore((s) => s.loadPresets);
  const focus = useAppStore((s) => s.personasFocus);

  /** `poiesis`, a persona's id, or `new`. */
  const [selected, setSelected] = useState<string>("poiesis");
  const folds = useFolds();
  const [draft, setDraft] = useState<Draft | null>(null);
  /** The draft as it was loaded: what "changed" is measured against. */
  const [base, setBase] = useState<Draft | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [addingPreset, setAddingPreset] = useState<string | null>(null);
  const [toolsets, setToolsets] = useState<ToolsetInfo[]>([]);
  const skills = useAppStore((s) => s.skills);
  const expert = useExpert();

  // `CLI-UI-4`: the agent apps a persona can run on: the ones that take jobs and
  // are on, and the one this persona already has even if it has since been
  // turned off (so the choice is not silently lost).
  const runsOnChoices = engines.filter((e) => e.found && e.delegate_ready && (e.enabled || e.id === draft?.engineId));
  const onAgentApp = (draft?.spawnable && draft.engineId && engines.find((e) => e.id === draft.engineId)) || null;

  useEffect(() => {
    if (!api.inTauri()) return;
    api.listToolsets().then(setToolsets).catch(() => {});
  }, []);

  const persona = personas.find((p) => p.id === selected) ?? null;

  // `TEAM-5b`: whether new chats start with this persona is what the Everywhere row of
  // Where chats start says, not a flag on the persona.
  const [globalLeader, setGlobalLeader] = useState<string | null>(null);
  function readGlobalLeader() {
    if (!api.inTauri()) return;
    api
      .waysView()
      .then((v) => setGlobalLeader(v.starts?.find((x) => x.scope === "global")?.leader ?? null))
      .catch(() => {});
  }
  useEffect(readGlobalLeader, [selected]);
  const startsHere = !!persona && globalLeader === `persona:${persona.id}`;

  // `TEAM-9`: the teams this persona sits in, for "In teams" and for what deleting does to them.
  const [teams, setTeams] = useState<api.Team[]>([]);
  useEffect(() => {
    if (!api.inTauri()) return;
    api
      .teamsList()
      .then((list) => setTeams(list.map((r) => r.team)))
      .catch(() => {});
  }, [selected]);
  const places = persona ? placesOf(persona.id, teams) : [];

  const loadedFor = useRef<string | null>(null);

  function draftOf(p: Persona): Draft {
    const temp = personaTemperature(p);
    const allow = p.tools_json ? (JSON.parse(p.tools_json) as string[]) : null;
    const allowSkills = p.skills_json ? (JSON.parse(p.skills_json) as string[]) : null;
    return {
      id: p.id,
      name: p.name,
      line: p.line ?? "",
      look: p.look ?? "",
      systemPrompt: p.system_prompt,
      modelId: p.model_id ?? "",
      temperature: typeof temp === "number" ? String(temp) : "",
      checkedTools: allow ?? toolsets.map((s) => s.id),
      checkedSkills: allowSkills ?? skills.map((s) => s.name),
      description: p.description ?? "",
      spawnable: p.spawnable,
      engineId: p.engine_id ?? "",
      appGrants: null,
      style: "",
      styleWas: "",
    };
  }

  function show(id: string) {
    setSelected(id);
    setConfirmDelete(false);
    setSaveError(null);
    folds.reset();
  }

  // Load the form for whoever is picked. A change to the list underneath (a save, a
  // delete) does not reload what the person is typing; only picking someone else does.
  useEffect(() => {
    if (selected === "poiesis") {
      loadedFor.current = selected;
      setDraft(null);
      setBase(null);
      return;
    }
    if (loadedFor.current === selected) return;
    if (selected === "new") {
      loadedFor.current = selected;
      const fresh = { ...EMPTY, checkedTools: toolsets.map((s) => s.id), checkedSkills: skills.map((s) => s.name) };
      setDraft(fresh);
      setBase(fresh);
      return;
    }
    const p = personas.find((x) => x.id === selected);
    if (!p) {
      // Gone (deleted elsewhere): back to Poiesis.
      setSelected("poiesis");
      return;
    }
    loadedFor.current = selected;
    const d = draftOf(p);
    setDraft(d);
    setBase(d);
    // `WRK-17`: how this persona works is read from where the defaults live.
    if (api.inTauri()) {
      api
        .waysView()
        .then((v) => {
          const at = v.defaults.find((x) => x.scope === `persona:${p.id}`)?.preset ?? "";
          const patch = (x: Draft | null) => (x && x.id === p.id ? { ...x, style: at, styleWas: at } : x);
          setDraft(patch);
          setBase(patch);
        })
        .catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, personas.length]);

  // Arriving from a link lands on that persona.
  useEffect(() => {
    // A team (`team:<slug>`) is the Teams tab's to show.
    if (!focus || focus.startsWith("team:")) return;
    loadedFor.current = null;
    show(focus);
    useAppStore.setState({ personasFocus: null });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus]);

  // A preset drops off the list once a persona of that name exists, so the row
  // shrinks to nothing as they're adopted rather than offering duplicates.
  const existingNames = new Set(personas.map((p) => p.name));
  const availablePresets = PERSONA_PRESETS.filter((preset) => !existingNames.has(preset.name));

  async function addPreset(preset: PersonaPreset) {
    setAddingPreset(preset.name);
    try {
      const created = await createPersona({
        name: preset.name,
        systemPrompt: preset.systemPrompt,
        temperature: preset.temperature,
        ...(preset.tools ? { toolsJson: JSON.stringify(preset.tools) } : {}),
        ...(preset.description ? { description: preset.description } : {}),
      });
      if (created) {
        loadedFor.current = null;
        show(created.id);
      }
    } finally {
      setAddingPreset(null);
    }
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

  const nameTaken = !!draft && isPoiesis(draft.name);
  const dirty = !!draft && !!base && key(draft) !== key(base);
  const canSave = !!draft && !!draft.name.trim() && !!draft.systemPrompt.trim() && !nameTaken && (dirty || !draft.id);

  async function save() {
    if (!draft || !canSave) return;
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
            line: draft.line.trim() || null,
            look: draft.look || null,
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
          look: draft.look || null,
          line: draft.line.trim() || null,
        });
        savedId = created?.id ?? null;
      }
      // `WRK-17`: how chats with this persona start. Only a change is written.
      if (savedId && draft.style !== draft.styleWas) {
        try {
          await api.setStyleDefault(`persona:${savedId}`, draft.style || null);
          await loadPresets();
        } catch (e) {
          setSaveError(`The persona is saved, but how it works wasn't: ${String(e).replace(/^Error:\s*/, "")}`);
          return;
        }
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
      if (!draft.id && savedId) {
        // A new one is made: show it as the saved one, not as an empty form.
        loadedFor.current = null;
        show(savedId);
      } else {
        const saved = { ...draft, name: draft.name.trim(), appGrants: null, styleWas: draft.style };
        setDraft(saved);
        setBase(saved);
      }
    } finally {
      setBusy(false);
    }
  }

  /** The chats this persona leads, which go back to Poiesis if it is deleted. */
  const ledList = persona ? conversations.filter((c) => !c.parentConversationId && leaderOf(c) === `persona:${persona.id}`) : [];
  const ledChats = ledList.length;
  const lastLed = ledList.reduce((t, c) => Math.max(t, c.updatedAt), 0);

  async function remove() {
    if (!persona) return;
    setBusy(true);
    try {
      await deletePersona(persona.id);
      loadedFor.current = null;
      show("poiesis");
    } finally {
      setBusy(false);
    }
  }

  const who = draft?.name.trim() || "this persona";
  const poiesisLeads = !globalLeader || globalLeader === "poiesis";
  const defaultBadge = <span className="ways-badge">Default</span>;

  const cards = [
    {
      id: "poiesis",
      glyph: <PoiesisGlyph size={20} />,
      name: "Poiesis",
      line: "Leads every chat, unless you hand it to someone else",
      mark: poiesisLeads ? defaultBadge : undefined,
    },
    ...personas.map((p) => ({
      id: p.id,
      glyph: <PersonaGlyph name={p.name} look={p.look} size={20} />,
      name: p.name,
      line: p.line ?? p.description ?? "",
      mark: globalLeader === `persona:${p.id}` ? defaultBadge : undefined,
    })),
  ];

  return (
    <div className="personas-tab">
      <PickCards
        label="Personas"
        cards={cards}
        selected={selected}
        onPick={show}
        newCard={{ name: "New persona", line: "From scratch or from a preset", onClick: () => show("new"), on: selected === "new" }}
      />

      {selected === "poiesis" && <PoiesisEntry />}

      {selected !== "poiesis" && draft && (
        <section className="self-card ways-detail persona-card" aria-label={draft.id ? (persona?.name ?? draft.name) : "New persona"}>
          <div className="ways-detail-head">
            <PersonaGlyph name={draft.name || "?"} look={draft.look} size={30} />
            <div className="ways-detail-title">
              <h3 className="persona-head-name">{draft.id ? (persona?.name ?? draft.name) : "New persona"}</h3>
              {draft.id && persona?.line && <p className="persona-head-line">{persona.line}</p>}
            </div>
            {persona && (
              <div className="persona-actions">
                {startsHere ? (
                  <>
                    <span className="persona-default" title="Chats start with this persona unless a place says otherwise">
                      New chats start here
                    </span>
                    <button className="btn-text" onClick={() => openSelf("ways", "starts")}>
                      Change
                    </button>
                  </>
                ) : (
                  <button
                    className="btn-text"
                    onClick={async () => {
                      await setDefaultPersona(persona.id);
                      readGlobalLeader();
                    }}
                  >
                    Start new chats with {persona.name}
                  </button>
                )}
                <button className="btn-secondary" onClick={() => setConfirmDelete(true)}>
                  Delete
                </button>
              </div>
            )}
          </div>

          {persona && (
            <p className="ways-version persona-head-led">
              {ledChats > 0 ? (
                <>
                  Led {ledChats} {ledChats === 1 ? "chat" : "chats"}, last on{" "}
                  {new Date(lastLed).toLocaleDateString(undefined, { month: "short", day: "numeric" })}
                  {" · "}
                  <button className="why-link" onClick={() => showChatsLedBy(`persona:${persona.id}`)}>
                    {persona.name}'s chats
                  </button>
                </>
              ) : (
                `Has not led a chat yet.`
              )}
              {/* `TEAM-9`: where this persona works with others. */}
              {places.length > 0 && (
                <span className="persona-teams" aria-label={`Teams ${persona.name} is in`}>
                  {" · In "}
                  {places.map((p, i) => (
                    <span key={`${p.slug}:${p.role}`}>
                      {i > 0 && ", "}
                      <button className="why-link" onClick={() => onOpenTeam?.(p.slug)}>
                        {p.team}
                      </button>{" "}
                      as {p.role}
                    </span>
                  ))}
                </span>
              )}
            </p>
          )}

          {!draft.id && availablePresets.length > 0 && (
            <div className="persona-presets">
              <span className="persona-presets-label">Start from a preset</span>
              <div className="persona-presets-row">
                {availablePresets.map((preset) => (
                  <button
                    key={preset.name}
                    className="team-chip"
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

          {/* Who it is: always open, it is what a persona is. */}
          <div className="persona-who">
            <div className="persona-form-row">
              <div className="persona-field">
                <label className="field-label" htmlFor="persona-name">
                  Name
                </label>
                <input
                  id="persona-name"
                  className="field-input"
                  value={draft.name}
                  placeholder="e.g. Terse code reviewer"
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                />
              </div>
              <div className="persona-field">
                <label className="field-label" htmlFor="persona-line">
                  One line
                </label>
                <input
                  id="persona-line"
                  className="field-input"
                  value={draft.line}
                  placeholder="e.g. Writes and edits, warm but direct"
                  onChange={(e) => setDraft({ ...draft, line: e.target.value })}
                />
              </div>
            </div>
            {nameTaken && (
              <p className="persona-tools-note" role="status">
                Poiesis is taken: it is the one that leads when nobody else does.
              </p>
            )}

            <label className="field-label">Face</label>
            <FacePicker name={draft.name} look={draft.look} onChange={(look) => setDraft({ ...draft, look })} />

            <label className="field-label" htmlFor="persona-prompt">
              Instructions
            </label>
            <textarea
              id="persona-prompt"
              className="system-prompt"
              rows={5}
              spellCheck={false}
              value={draft.systemPrompt}
              placeholder="How this persona should behave…"
              onChange={(e) => setDraft({ ...draft, systemPrompt: e.target.value })}
            />
          </div>

          <div className="persona-form">
            <Fold title="Model and style" open={folds.is("model")} onToggle={() => folds.toggle("model")}>
              {onAgentApp ? (
                <p className="field-hint">{who} runs on {onAgentApp.name}, which brings its own model. Change that under Handing {who} work.</p>
              ) : (
                <div className="persona-form-row">
                  <div className="persona-field">
                    <label className="field-label">Model (optional)</label>
                    <select className="field-input" value={draft.modelId} onChange={(e) => setDraft({ ...draft, modelId: e.target.value })}>
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

              {/* `WRK-17`: the way of working chats with this persona start in. */}
              <label className="field-label" htmlFor="persona-style">
                Chats with {who} start in
              </label>
              <select id="persona-style" className="field-input" value={draft.style} onChange={(e) => setDraft({ ...draft, style: e.target.value })}>
                <option value="">The default</option>
                {presets.map((p) => (
                  <option key={p.slug} value={p.slug}>
                    {p.name}
                  </option>
                ))}
              </select>
              <p className="field-hint">
                <button className="link-btn" onClick={() => openSelf("ways", draft.style || undefined)}>
                  Styles and rules are in How I work
                </button>
                {" · "}
                <button className="link-btn" onClick={() => openSelf("ways", "starts")}>
                  Where chats start
                </button>
              </p>
            </Fold>

            {/* `SUB-UI-5`: personas are agent types. These fields are what turn one into
                something the agent can hand a job to, and (`CLI-UI-4`) which agent app does it. */}
            <Fold title={`Handing ${who} work`} open={folds.is("hand")} onToggle={() => folds.toggle("hand")}>
              <label className="persona-spawnable">
                <input type="checkbox" checked={draft.spawnable} onChange={(e) => setDraft({ ...draft, spawnable: e.target.checked })} />I
                can hand {who} work
              </label>
              <label className="field-label" htmlFor="persona-description">
                When should I hand {who} work?
              </label>
              <input
                id="persona-description"
                className="field-input"
                value={draft.description}
                placeholder="e.g. Reads sources carefully and reports back with citations"
                onChange={(e) => setDraft({ ...draft, description: e.target.value })}
              />
              <p className="field-hint">I read this when deciding who to hand a job to, so say what {who} is good at.</p>

              {draft.spawnable && runsOnChoices.length > 0 && (
                <div className="persona-runs-on">
                  <label className="field-label" htmlFor="persona-runs-on">
                    Runs on
                  </label>
                  <select id="persona-runs-on" className="field-input" value={draft.engineId} onChange={(e) => setDraft({ ...draft, engineId: e.target.value })}>
                    <option value="">Poiesis (I choose a model)</option>
                    {runsOnChoices.map((e) => (
                      <option key={e.id} value={e.id}>
                        {e.enabled ? e.name : `${e.name} (off, turn it on in External agents)`}
                      </option>
                    ))}
                  </select>
                  {onAgentApp && <p className="field-hint">It brings its own model and tools. I give it this persona's instructions with the task, and the folder.</p>}
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
            </Fold>

            {expert && !onAgentApp && (
              <Fold title={`What ${who} may use`} open={folds.is("uses")} onToggle={() => folds.toggle("uses")}>
                <PersonaToolsEditor toolsets={toolsets} checked={draft.checkedTools} onToggle={toggleTool} />
                <PersonaSkillsEditor skills={skills} checked={draft.checkedSkills} onToggle={toggleSkill} />
                <PersonaApps personaId={draft.id} value={draft.appGrants} onChange={(appGrants) => setDraft({ ...draft, appGrants })} />
              </Fold>
            )}

            {/* `TEAM-5`: only a persona that exists has anything to know. */}
            {persona && draft.id && (
              <Fold title={`What ${who} knows`} open={folds.is("knows")} onToggle={() => folds.toggle("knows")}>
                <PersonaKnows persona={persona} />
              </Fold>
            )}
          </div>

          {saveError && (
            <p className="persona-tools-note" role="alert">
              {saveError}
            </p>
          )}
          {(dirty || !draft.id) && (
            <div className="setting-actions">
              <button className="btn-primary" onClick={save} disabled={busy || !canSave}>
                {draft.id ? "Save changes" : "Create persona"}
              </button>
              {draft.id && base && (
                <button className="btn-secondary" onClick={() => setDraft(base)} disabled={busy}>
                  Revert
                </button>
              )}
            </div>
          )}
        </section>
      )}

      {persona && confirmDelete && (
        <ConfirmDialog
          title={`Delete ${persona.name}?`}
          body={`${deleteWords(persona.name, ledChats, places)} This cannot be undone, but what only ${persona.name} knew goes to the memory trash, where it can be brought back.`}
          confirmLabel={`Delete ${persona.name}`}
          onConfirm={() => {
            setConfirmDelete(false);
            void remove();
          }}
          onCancel={() => setConfirmDelete(false)}
        />
      )}
    </div>
  );
}
