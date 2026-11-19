import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../../lib/api";
import type { Team, TeamLine, TeamLineKind, TeamMember, TeamRead, TeamShape } from "../../lib/api";
import { useAppStore, useExpert } from "../../lib/store";
import {
  SHAPES,
  isFixedLine,
  lineKindsFor,
  lineKey,
  lineVerb,
  lineWords,
  sameLines,
  teamNamer,
  withMember,
  withNewLines,
  withoutRole,
} from "../../lib/teamChart";
import { whoIs } from "../../lib/teamLive";
import { teamNotes } from "../../lib/teamNotes";
import { teamProposals } from "../../lib/teamProposals";
import { leaderOf } from "../../lib/leaders";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import { TeamIcon } from "../Icons/Icons";
import PresetHistory from "../Self/PresetHistory";
import { ordinal } from "../Self/waysFormat";
import Select from "../Select/Select";
import Toggle from "../Toggle/Toggle";
import OrgDiagram from "../Workbench/OrgDiagram";
import AddMemberDialog from "./AddMemberDialog";
import Fold, { useFolds } from "./Fold";
import MemberGlyph from "./MemberGlyph";
import { TEAM_PRESETS, presetMembers, type TeamPreset } from "../../lib/teamPresets";
import PickCards from "./PickCards";
import TeamProposals from "./TeamProposals";
import "../Self/Self.css";
import "./PersonaEditor.css";
import "./TeamEditor.css";

const say = (e: unknown) => String(e).replace(/^Error:\s*/, "");
const key = (t: Team) => JSON.stringify(t);
const pairKey = (a: string, b: string) => [a, b].sort().join("|");

/** A number that is only taken once it is one the team accepts; the box shows what was typed. */
function NumField({
  id,
  value,
  min,
  max,
  step = 1,
  integer = true,
  onChange,
}: {
  id: string;
  value: number;
  min: number;
  max?: number;
  step?: number;
  integer?: boolean;
  onChange: (n: number) => void;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  return (
    <input
      id={id}
      className="field-input team-num"
      type="number"
      min={min}
      max={max}
      step={step}
      value={text}
      onChange={(e) => {
        setText(e.target.value);
        const n = Number(e.target.value);
        if (e.target.value.trim() !== "" && Number.isFinite(n) && n >= min && (max === undefined || n <= max) && (!integer || Number.isInteger(n))) {
          onChange(n);
        }
      }}
      onBlur={() => setText(String(value))}
    />
  );
}

/**
 * `TEAM-9`: the Teams tab of Settings → Personas & teams. The teams as cards, the way How I work
 * shows the styles; the picked one under them in one card: its name, who is in it, how they work
 * together (the chart, the lines in words and a row that adds a line), and three folded sections.
 * New team opens a blank one in that card, the way New persona does, with the presets to start
 * from. Changes to a team are a draft until Save, because each save is a version.
 */
export default function TeamEditor() {
  const personas = useAppStore((s) => s.personas);
  const models = useAppStore((s) => s.models);
  const engines = useAppStore((s) => s.engines);
  const presets = useAppStore((s) => s.presets);
  const conversations = useAppStore((s) => s.conversations);
  const openSelf = useAppStore((s) => s.openSelf);
  const openPersonas = useAppStore((s) => s.openPersonas);
  const showChatsLedBy = useAppStore((s) => s.showChatsLedBy);
  const focus = useAppStore((s) => s.personasFocus);
  const expert = useExpert();
  // `TEAM-23`: the teams Poiesis would like to change.
  const changeProposals = useAppStore((s) => s.changeProposals);
  const waiting = useMemo(() => new Set(teamProposals(changeProposals).map((p) => p.team)), [changeProposals]);

  const [reads, setReads] = useState<TeamRead[]>([]);
  const [loaded, setLoaded] = useState(false);
  /** A team's slug, `new` for one that is not made yet, or nothing. */
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Team | null>(null);
  const [base, setBase] = useState<Team | null>(null);
  const folds = useFolds();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [history, setHistory] = useState(false);
  /** The preset a team is being made from. */
  const [addingPreset, setAddingPreset] = useState<string | null>(null);
  // The line being added: from, to and what it is. Clicking the chart fills from and to.
  const [picked, setPicked] = useState<string | null>(null);
  const [target, setTarget] = useState<string | null>(null);
  const [kind, setKind] = useState<TeamLineKind | null>(null);
  /** The two places whose lines are shown, after a line on the chart was clicked. */
  const [lineSel, setLineSel] = useState<string | null>(null);
  const [dialog, setDialog] = useState<{ edit?: TeamMember } | null>(null);
  const [shapeAsk, setShapeAsk] = useState<TeamShape | null>(null);
  const shown = useRef<string | null>(null);

  const resetChart = () => {
    setPicked(null);
    setTarget(null);
    setKind(null);
    setLineSel(null);
  };

  /** Show a team in the card under the list; `given` is one that is not made yet. */
  async function show(id: string | null, list: TeamRead[] = reads, given?: Team) {
    shown.current = id;
    setSelected(id);
    setConfirmDelete(false);
    setProblem(null);
    folds.reset();
    resetChart();
    const team = given ?? (id ? list.find((r) => r.team.slug === id)?.team : undefined);
    if (shown.current !== id) return;
    setDraft(team ?? null);
    setBase(team ?? null);
  }

  async function reload(): Promise<TeamRead[]> {
    const list = await api.teamsList();
    setReads(list);
    // The Who leads menu and the sidebar read the teams from the store.
    useAppStore.setState({ teams: list.map((r) => r.team) });
    return list;
  }

  useEffect(() => {
    if (!api.inTauri()) return;
    reload()
      .then((list) => {
        setLoaded(true);
        void show(list[0]?.team.slug ?? null, list);
      })
      .catch((e) => {
        setLoaded(true);
        setProblem(say(e));
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Arriving from a link lands on that team.
  useEffect(() => {
    if (!focus?.startsWith("team:") || !loaded) return;
    // `TEAM-22`: "Keep these helpers as a team" arrives with a team to start from.
    const given = useAppStore.getState().teamDraft;
    if (given) {
      useAppStore.setState({ personasFocus: null, teamDraft: null });
      void reload().then((list) => show("new", list, given));
      return;
    }
    useAppStore.setState({ personasFocus: null });
    void reload().then((list) => show(list.some((r) => r.team.slug === focus.slice(5)) ? focus.slice(5) : (list[0]?.team.slug ?? null), list));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, loaded]);

  const read = reads.find((r) => r.team.slug === selected);
  const dirty = !!draft && !!base && key(draft) !== key(base);

  // A persona deleted on the other tab leaves a helper in its place: read the teams again, unless
  // there is something typed here that a reload would take away.
  const personaCount = useRef(personas.length);
  useEffect(() => {
    if (!loaded || personaCount.current === personas.length) return;
    personaCount.current = personas.length;
    void reload().then((list) => {
      if (!dirty && selected && selected !== "new") void show(selected, list);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [personas.length]);
  const isNew = !!draft && !draft.slug;
  const canSave = !!draft && !!draft.name.trim() && (dirty || isNew);
  const update = (patch: Partial<Team>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const nameOf = draft ? teamNamer(draft, personas) : () => "";
  const notes = draft ? teamNotes(draft, personas, models, engines) : [];

  async function save() {
    if (!draft || !canSave) return;
    setBusy(true);
    setProblem(null);
    try {
      const saved = await api.teamSave(draft);
      const list = await reload();
      await show(saved.slug, list);
    } catch (e) {
      setProblem(say(e));
    } finally {
      setBusy(false);
    }
  }

  /** New team: a blank one in the card, made once it has a name, the way a new persona is. */
  async function startNew() {
    try {
      await show("new", reads, await api.teamBlank());
    } catch (e) {
      setProblem(say(e));
    }
  }

  /** A preset is made at once, with the lines of its shape, and shown, as a persona preset is. */
  async function addPreset(preset: TeamPreset) {
    setAddingPreset(preset.name);
    setProblem(null);
    try {
      const blank = await api.teamBlank();
      const members = presetMembers(preset, personas);
      const lines = await api.teamShapeLines(preset.shape, members);
      const saved = await api.teamSave({ ...blank, name: preset.name, line: preset.line, shape: preset.shape, members, lines });
      const list = await reload();
      await show(saved.slug, list);
    } catch (e) {
      setProblem(say(e));
    } finally {
      setAddingPreset(null);
    }
  }
  // A preset drops off once a team of that name exists, as the persona presets do.
  const availablePresets = TEAM_PRESETS.filter((p) => !reads.some((r) => r.team.name === p.name));

  async function remove() {
    if (!draft?.slug) return;
    setBusy(true);
    try {
      await api.teamDelete(draft.slug);
      // The chats it led went back to Poiesis in the backend.
      const gone = `team:${draft.slug}`;
      useAppStore.setState((s) => ({
        conversations: s.conversations.map((c) => (c.leader === gone ? { ...c, leader: null, personaId: null } : c)),
        leaderFilter: s.leaderFilter === gone ? null : s.leaderFilter,
      }));
      const list = await reload();
      await show(list[0]?.team.slug ?? null, list);
    } catch (e) {
      setProblem(say(e));
    } finally {
      setBusy(false);
    }
  }

  async function backToOriginal() {
    if (!draft?.slug) return;
    try {
      await api.teamBack(draft.slug, null);
      const list = await reload();
      await show(draft.slug, list);
    } catch (e) {
      setProblem(say(e));
    }
  }

  /* ---- lines ---- */

  /** A click on a place: the first is where a line starts, the next where it goes. */
  function onPlace(k: string) {
    setLineSel(null);
    if (target || !picked) {
      setPicked(k);
      setTarget(null);
    } else if (picked === k) setPicked(null);
    else setTarget(k);
  }

  function onLine(k: string) {
    setPicked(null);
    setTarget(null);
    setLineSel((now) => (now === k ? null : k));
  }

  const taken = new Set((draft?.lines ?? []).map(lineKey));
  const kinds = picked && target ? lineKindsFor(picked, target).filter((k) => !taken.has(lineKey({ kind: k, from: picked, to: target }))) : [];
  const kindNow = kind && kinds.includes(kind) ? kind : (kinds[0] ?? null);

  function draw() {
    if (!draft || !picked || !target || !kindNow) return;
    const line: TeamLine = { kind: kindNow, from: picked, to: target };
    update({ lines: [...draft.lines, line] });
    setLineSel(pairKey(picked, target));
    setPicked(null);
    setTarget(null);
    setKind(null);
  }

  const removeLine = (l: TeamLine) => draft && update({ lines: draft.lines.filter((x) => lineKey(x) !== lineKey(l)) });

  /* ---- members ---- */

  /** Members come with the lines their shape gives them; what was drawn by hand stays. */
  async function addMember(m: TeamMember) {
    if (!draft) return;
    setDialog(null);
    try {
      const members = withMember(draft.shape, draft.members, m);
      const before = await api.teamShapeLines(draft.shape, draft.members);
      const after = await api.teamShapeLines(draft.shape, members);
      update({ members, lines: sameLines(draft.lines, before) ? after : withNewLines(draft.lines, before, after) });
      resetChart();
    } catch (e) {
      setProblem(say(e));
    }
  }

  function changeMember(m: TeamMember) {
    if (!draft) return;
    setDialog(null);
    update({ members: draft.members.map((x) => (x.role === m.role ? m : x)) });
  }

  async function removeMember(role: string) {
    if (!draft) return;
    try {
      const members = draft.members.filter((m) => m.role !== role);
      const own = await api.teamShapeLines(draft.shape, draft.members);
      // Lines that are still the shape's own follow it (a pipeline closes its gap).
      update({ members, lines: sameLines(draft.lines, own) ? await api.teamShapeLines(draft.shape, members) : withoutRole(draft.lines, role) });
      resetChart();
    } catch (e) {
      setProblem(say(e));
    }
  }

  async function startFrom(shape: TeamShape) {
    if (!draft || shape === draft.shape) return;
    try {
      const own = await api.teamShapeLines(draft.shape, draft.members);
      if (sameLines(draft.lines, own)) await applyShape(shape);
      else setShapeAsk(shape);
    } catch (e) {
      setProblem(say(e));
    }
  }

  async function applyShape(shape: TeamShape) {
    if (!draft) return;
    setShapeAsk(null);
    try {
      update({ shape, lines: await api.teamShapeLines(shape, draft.members) });
      resetChart();
    } catch (e) {
      setProblem(say(e));
    }
  }

  /* ---- what the card shows ---- */

  const styleOptions = [{ value: "", label: "The usual order decides" }, ...presets.map((p) => ({ value: p.slug, label: p.name }))];
  const coordinatorOptions = [
    { value: "poiesis", label: "Poiesis" },
    ...personas.map((p) => ({ value: `persona:${p.id}`, label: p.name, hint: p.line ?? undefined })),
  ];
  // A coordinator that is gone is still shown, so nothing is silently swapped.
  if (draft && draft.coordinator.startsWith("persona:") && !coordinatorOptions.some((o) => o.value === draft.coordinator)) {
    coordinatorOptions.push({ value: draft.coordinator, label: "A persona that is gone", hint: undefined });
  }
  // The places a line can start from and go to, in the words of the chart.
  const placeOptions = draft
    ? [
        { value: "you", label: "You" },
        { value: "coordinator", label: nameOf("coordinator", true), hint: "coordinates" },
        ...draft.members.map((m) => ({ value: m.role, label: m.role })),
      ]
    : [];
  const choose = { value: "", label: "Choose…" };

  const led = draft?.slug ? conversations.filter((c) => !c.parentConversationId && leaderOf(c) === `team:${draft.slug}`) : [];
  const shapeOf = (s: TeamShape) => SHAPES.find((x) => x.id === s);
  const coordinatorNote = notes.find((n) => n.who === "coordinator");

  const cards = reads.map((r) => ({
    id: r.team.slug,
    glyph: <TeamIcon size={20} />,
    name: r.team.name,
    line: r.team.line || `${shapeOf(r.team.shape)?.name ?? "Team"} · ${r.team.members.length} ${r.team.members.length === 1 ? "member" : "members"}`,
    mark: waiting.has(r.team.slug) ? (
      <span className="team-waiting-dot" role="img" aria-label="A change is waiting for you" title="A change is waiting for you" />
    ) : undefined,
  }));

  return (
    <div className="personas-tab">
      <PickCards
        label="Teams"
        cards={cards}
        selected={selected}
        onPick={(id) => void show(id)}
        newCard={{ name: "New team", line: "From scratch or from a preset", onClick: () => void startNew(), on: selected === "new" }}
      />
      {loaded && reads.length === 0 && !draft && (
        <p className="field-hint">No teams yet. A team is a few personas or helpers and the lines that say who works with whom.</p>
      )}

      {draft && (
        <section className="self-card ways-detail persona-card team-detail" aria-label={isNew ? "New team" : draft.name}>
          <div className="ways-detail-head">
            <span className="team-glyph" aria-hidden="true">
              <TeamIcon size={26} />
            </span>
            <div className="ways-detail-title">
              <h3 className="persona-head-name">{isNew ? "New team" : (base?.name ?? draft.name)}</h3>
              {!isNew && base?.line && <p>{base.line}</p>}
            </div>
            {!isNew && (
              <div className="persona-actions">
                <button className="btn-secondary" onClick={() => setConfirmDelete(true)}>
                  Delete
                </button>
              </div>
            )}
          </div>

          <p className="ways-version persona-head-led">
            {isNew ? (
              "Not made yet. A new team starts locked."
            ) : (
              <>
                {led.length === 0 ? "Has not led a chat yet" : `Led ${led.length} ${led.length === 1 ? "chat" : "chats"}`}
                {led.length > 0 && (
                  <>
                    {" · "}
                    <button className="why-link" onClick={() => showChatsLedBy(`team:${draft.slug}`)}>
                      Show them
                    </button>
                  </>
                )}
                {` · ${ordinal(draft.version)} version · `}
                <button className="why-link" onClick={() => setHistory(true)}>
                  History
                </button>
                {draft.version > 1 && (
                  <>
                    {" · "}
                    <button className="why-link" onClick={() => void backToOriginal()}>
                      Back to original
                    </button>
                  </>
                )}
              </>
            )}
          </p>

          <div className="ways-detail-switches">
            <label className="ways-switch team-lock">
              <Toggle checked={!draft.locked} onChange={(on) => update({ locked: !on })} label="I may improve it" />
              <span>
                I may improve it
                <span className="ways-switch-help">
                  {draft.locked ? "Locked: I never change its members or lines on my own." : "From how its tasks go, I suggest changes to its members and lines."}
                </span>
              </span>
            </label>
          </div>

          {!isNew && (
            <TeamProposals
              slug={draft.slug}
              unsaved={dirty}
              onChanged={async () => {
                // Poiesis changed the team: show it as it is now.
                const list = await reload();
                await show(draft.slug, list);
              }}
            />
          )}

          {read && read.problems.length > 0 && (
            <div className="team-problems" role="status">
              <p>Some of its file could not be read, and I left it out:</p>
              <ul>
                {read.problems.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </div>
          )}

          {isNew && availablePresets.length > 0 && (
            <div className="persona-presets">
              <span className="persona-presets-label">Start from a preset</span>
              <div className="persona-presets-row">
                {availablePresets.map((preset) => (
                  <button
                    key={preset.name}
                    className="team-chip"
                    onClick={() => void addPreset(preset)}
                    disabled={addingPreset !== null}
                    title={`${preset.line}: ${preset.members.map((m) => m.role).join(", ")}`}
                  >
                    {addingPreset === preset.name ? "Adding…" : preset.name}
                  </button>
                ))}
              </div>
            </div>
          )}

          <div className="persona-form-row">
            <div className="persona-field">
              <label className="field-label" htmlFor="team-name">
                Name
              </label>
              <input id="team-name" className="field-input" value={draft.name} placeholder="e.g. Shop team" onChange={(e) => update({ name: e.target.value })} />
            </div>
            <div className="persona-field">
              <label className="field-label" htmlFor="team-line">
                One line
              </label>
              <input
                id="team-line"
                className="field-input"
                value={draft.line}
                placeholder="e.g. Builds and checks web shop features"
                onChange={(e) => update({ line: e.target.value })}
              />
            </div>
          </div>

          {/* Who is in it: who coordinates, then a row for each member. */}
          <div className="team-part">
            <h4 className="ways-subhead">Who is in it</h4>
            <div className="team-coordinator">
              <span className="team-coordinator-label">Coordinates</span>
              <Select label="Who coordinates" value={draft.coordinator} options={coordinatorOptions} onChange={(v) => update({ coordinator: v })} />
            </div>
            {coordinatorNote && (
              <p className="persona-tools-note" role="status">
                {coordinatorNote.text}
              </p>
            )}
            <ul className="team-roster" aria-label="Members">
              {draft.members.map((m) => {
                const who = whoIs(draft, m.role, personas);
                const note = notes.find((n) => n.who === m.role);
                return (
                  <li key={m.role} className={`team-roster-row${picked === m.role || target === m.role ? " on" : ""}`}>
                    <MemberGlyph who={who} size={20} />
                    <span className="team-roster-row-text">
                      <span className="team-roster-row-who">
                        {who.name ?? "A helper"} <span className="team-roster-row-role">as {m.role}</span>
                      </span>
                      {m.job && <span className="team-roster-row-job">{m.job}</span>}
                      {note && <span className="persona-tools-note">{note.text}</span>}
                    </span>
                    <button className="btn-text" onClick={() => setDialog({ edit: m })}>
                      Change
                    </button>
                    <button className="btn-text danger" aria-label={`Remove ${m.role}`} onClick={() => void removeMember(m.role)}>
                      Remove
                    </button>
                  </li>
                );
              })}
            </ul>
            {draft.members.length === 0 && <p className="field-hint">No members yet.</p>}
            <button className="ways-add-btn" onClick={() => setDialog({})}>
              + Add a member
            </button>
          </div>

          {/* How they work together: the shape, the chart, and its lines in words. */}
          <div className="team-part">
            <h4 className="ways-subhead">How they work together</h4>
            <div className="team-startfrom" role="group" aria-label="Shape">
              {SHAPES.map((s) => (
                <button key={s.id} className="team-chip" aria-pressed={draft.shape === s.id} onClick={() => void startFrom(s.id)}>
                  {s.name}
                </button>
              ))}
            </div>
            <p className="field-hint team-shape-hint">{shapeOf(draft.shape)?.hint}</p>

            <OrgDiagram
              team={{
                team: draft,
                personas,
                edit: { picked, selected: lineSel, onPlace, onLine },
                sentence: `${shapeOf(draft.shape)?.name ?? "Team"} · ${draft.members.length} ${draft.members.length === 1 ? "member" : "members"}`,
              }}
            />

            <ul className="team-words" aria-label="Lines in words">
              {draft.lines.map((l) => (
                <li key={lineKey(l)} className={lineSel === pairKey(l.from, l.to) ? "on" : undefined}>
                  <span>{lineWords(l, nameOf)}.</span>
                  {isFixedLine(l) ? (
                    <span className="team-strip-fixed">Always there</span>
                  ) : (
                    <button className="btn-text danger" onClick={() => removeLine(l)}>
                      Remove
                    </button>
                  )}
                </li>
              ))}
            </ul>

            <div className="field-label">Add a line</div>
            <div className="team-addline" role="group" aria-label="Add a line">
              <Select
                label="From"
                value={picked ?? ""}
                options={[choose, ...placeOptions]}
                onChange={(v) => {
                  setLineSel(null);
                  setPicked(v || null);
                  if (v === target) setTarget(null);
                }}
              />
              {picked && target && kinds.length > 0 ? (
                <Select label="What the line is" value={kindNow ?? ""} options={kinds.map((k) => ({ value: k, label: lineVerb(k) }))} onChange={(v) => setKind(v as TeamLineKind)} />
              ) : (
                <span className="team-addline-verb">{picked && target ? "has every line it can with" : "to"}</span>
              )}
              <Select
                label="To"
                value={target ?? ""}
                options={[choose, ...placeOptions.filter((o) => o.value !== picked)]}
                onChange={(v) => {
                  setLineSel(null);
                  setTarget(v || null);
                }}
              />
              <button className="btn-secondary" onClick={draw} disabled={!kindNow}>
                Add line
              </button>
            </div>
            <p className="field-hint">Or click two places on the chart. Click a line to find it in the list.</p>
          </div>

          <div className="persona-form">
            <Fold title="How it runs" open={folds.is("runs")} onToggle={() => folds.toggle("runs")}>
              <div className="field-label">How the coordinator works</div>
              <Select label="How the coordinator works" value={draft.style} options={styleOptions} onChange={(v) => update({ style: v })} />
              <div className="field-label">How the members work</div>
              <Select label="How the members work" value={draft.member_style} options={styleOptions} onChange={(v) => update({ member_style: v })} />
              <p className="field-hint">
                One style for all members.{" "}
                <button className="link-btn" onClick={() => openSelf("ways", draft.style || undefined)}>
                  Styles and rules are in How I work
                </button>
              </p>
              <div className="field-label">Reporting to you</div>
              {(
                [
                  ["each-item", "After each item"],
                  ["end", "Only at the end"],
                  ["problems", "Only on problems"],
                ] as const
              ).map(([v, label]) => (
                <label key={v} className="persona-spawnable">
                  <input type="radio" name="team-report" checked={draft.report === v} onChange={() => update({ report: v })} />
                  {label}
                </label>
              ))}
            </Fold>

            <Fold title="Limits" open={folds.is("limits")} onToggle={() => folds.toggle("limits")}>
              <div className="team-limits">
                <label htmlFor="team-budget">Budget per task ($)</label>
                <NumField
                  id="team-budget"
                  value={draft.limits.budget}
                  min={0}
                  step={0.5}
                  integer={false}
                  onChange={(n) => update({ limits: { ...draft.limits, budget: n } })}
                />
                <label htmlFor="team-minutes">Time per task (minutes)</label>
                <NumField id="team-minutes" value={draft.limits.minutes} min={1} onChange={(n) => update({ limits: { ...draft.limits, minutes: n } })} />
                {expert && (
                  <>
                    <label htmlFor="team-at-once">Members working at once</label>
                    <NumField id="team-at-once" value={draft.limits.at_once} min={1} max={5} onChange={(n) => update({ limits: { ...draft.limits, at_once: n } })} />
                    <label htmlFor="team-rounds">Rounds between two members</label>
                    <NumField id="team-rounds" value={draft.limits.rounds} min={1} onChange={(n) => update({ limits: { ...draft.limits, rounds: n } })} />
                  </>
                )}
              </div>
              <label className="persona-spawnable">
                <input type="checkbox" checked={draft.limits.ask_cost} onChange={(e) => update({ limits: { ...draft.limits, ask_cost: e.target.checked } })} />
                Ask me before a task starts
              </label>
            </Fold>

            <Fold title="The folder" open={folds.is("folder")} onToggle={() => folds.toggle("folder")}>
              <label className="persona-spawnable">
                <input type="radio" name="team-folder" checked={draft.folder === "copies"} onChange={() => update({ folder: "copies" })} />
                Each member works in its own copy
              </label>
              <label className="persona-spawnable">
                <input type="radio" name="team-folder" checked={draft.folder === "owners"} onChange={() => update({ folder: "owners" })} />
                Members own parts
              </label>
              {draft.folder === "owners" && <p className="field-hint">Say which paths each member owns when you add or change a member.</p>}
            </Fold>
          </div>

          {problem && (
            <p className="persona-tools-note" role="alert">
              {problem}
            </p>
          )}
          {(dirty || isNew) && (
            <div className="setting-actions">
              <button className="btn-primary" onClick={() => void save()} disabled={busy || !canSave}>
                {isNew ? "Create team" : "Save changes"}
              </button>
              {!isNew && base && (
                <button
                  className="btn-secondary"
                  onClick={() => {
                    setDraft(base);
                    resetChart();
                  }}
                  disabled={busy}
                >
                  Revert
                </button>
              )}
            </div>
          )}
        </section>
      )}
      {!draft && loaded && problem && (
        <p className="persona-tools-note" role="alert">
          {problem}
        </p>
      )}

      {confirmDelete && draft && !isNew && (
        <ConfirmDialog
          title={`Delete ${draft.name}?`}
          body={`${
            led.length > 0
              ? `${draft.name} led ${led.length} ${led.length === 1 ? "chat" : "chats"}. ${led.length === 1 ? "It goes" : "They go"} back to Poiesis. `
              : `${draft.name} led no chats. `
          }Where chats start with ${draft.name}, they start with Poiesis again.`}
          confirmLabel={`Delete ${draft.name}`}
          onConfirm={() => {
            setConfirmDelete(false);
            void remove();
          }}
          onCancel={() => setConfirmDelete(false)}
        />
      )}

      {dialog && draft && (
        <AddMemberDialog
          personas={personas}
          models={models}
          engines={engines}
          owners={draft.folder === "owners"}
          taken={draft.members.filter((m) => m.role !== dialog.edit?.role).map((m) => m.role)}
          member={dialog.edit}
          onSave={dialog.edit ? changeMember : (m) => void addMember(m)}
          onCancel={() => setDialog(null)}
          onNewPersona={() => {
            setDialog(null);
            openPersonas("new");
          }}
        />
      )}

      {shapeAsk && (
        <ConfirmDialog
          title="Replace the lines you drew?"
          body={`Replace the lines you drew with ${SHAPES.find((s) => s.id === shapeAsk)?.name}'s?`}
          confirmLabel="Replace"
          cancelLabel="Keep mine"
          onConfirm={() => void applyShape(shapeAsk)}
          onCancel={() => setShapeAsk(null)}
        />
      )}

      {history && draft?.slug && (
        <PresetHistory
          slug={draft.slug}
          name={draft.name}
          mark={<TeamIcon size={13} />}
          source={{ list: () => api.teamHistory(draft.slug), back: (seq) => api.teamBack(draft.slug, seq) }}
          onClose={() => setHistory(false)}
          onChanged={() => void reload().then((list) => show(draft.slug, list))}
        />
      )}
    </div>
  );
}
