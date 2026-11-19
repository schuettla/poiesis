import { useEffect, useRef, useState } from "react";
import type { EngineInfo, Persona, TeamMember } from "../../lib/api";
import { placeNote } from "../../lib/teamNotes";
import { roleFrom, roleProblem } from "../../lib/teamChart";
import type { Model } from "../../lib/types";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import Select from "../Select/Select";
import "./TeamEditor.css";

const NEW_PERSONA = "new-persona";

const paths = (text: string) =>
  text
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean);

/**
 * `TEAM-9`: Add a member (and, with `member`, change one). A small dialog of a fixed size, on
 * the shared confirm shell: who plays it (a persona of yours, a helper, or a new persona), a job
 * line, a role name made from the persona's name and, when the members own parts, which paths. The note about a persona
 * that does not fit the place (`TEAMS_PLAN` I.4) has its own place in the dialog, so the dialog
 * never changes height when it appears.
 */
export default function AddMemberDialog({
  personas,
  models,
  engines,
  owners,
  taken,
  member,
  onSave,
  onCancel,
  onNewPersona,
}: {
  personas: Persona[];
  models: Model[];
  engines: EngineInfo[];
  /** The members own parts of the folder, so `owns` is open. */
  owners: boolean;
  /** Roles the team has already (not this member's own). */
  taken: string[];
  /** Given: this member is being changed, and its role stays. */
  member?: TeamMember;
  onSave: (m: TeamMember) => void;
  onCancel: () => void;
  onNewPersona: () => void;
}) {
  const [persona, setPersona] = useState(member?.persona ?? (personas[0] ? `persona:${personas[0].id}` : "helper"));
  const [role, setRole] = useState(member?.role ?? (personas[0] ? roleFrom(personas[0].name) : ""));
  const [roleTouched, setRoleTouched] = useState(!!member);
  const [job, setJob] = useState(member?.job ?? "");
  const [owns, setOwns] = useState((member?.owns ?? []).join(", "));
  const [tried, setTried] = useState(false);
  const jobRef = useRef<HTMLInputElement>(null);

  // The shared shell focuses its button; the first thing to type is the job (who plays it is
  // already chosen, and the role is made from the name).
  useEffect(() => {
    const t = setTimeout(() => jobRef.current?.focus(), 0);
    return () => clearTimeout(t);
  }, []);

  const problem = roleProblem(role.trim(), taken);
  const note = placeNote(role.trim() || "member", persona, personas, models, engines);

  function pick(value: string) {
    if (value === NEW_PERSONA) return onNewPersona();
    setPersona(value);
    // A role made from the name until the person types one of their own.
    if (!roleTouched) {
      const p = personas.find((x) => `persona:${x.id}` === value);
      setRole(p ? roleFrom(p.name) : "");
    }
  }

  function save() {
    setTried(true);
    if (problem) return;
    onSave({
      role: role.trim(),
      persona,
      job: job.trim(),
      owns: paths(owns),
    });
  }
  const options = [
    ...personas.map((p) => ({ value: `persona:${p.id}`, label: p.name, hint: p.line ?? undefined })),
    { value: "helper", label: "A helper", hint: "no persona, only a job" },
    { value: NEW_PERSONA, label: "New persona…" },
  ];
  // A persona that is gone stays in the list, so changing a member does not silently swap it.
  if (member && persona.startsWith("persona:") && !personas.some((p) => `persona:${p.id}` === persona)) {
    options.unshift({ value: persona, label: "A persona that is gone", hint: undefined });
  }

  return (
    <ConfirmDialog
      title={member ? `Change ${member.role}` : "Add a member"}
      confirmLabel={member ? "Save" : "Add member"}
      onConfirm={save}
      onCancel={onCancel}
    >
      <div
        className="team-dialog"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.target as HTMLElement).tagName === "INPUT") {
            e.preventDefault();
            save();
          }
        }}
      >
        <div className="field-label">Who plays it</div>
        <Select label="Who plays it" value={persona} options={options} onChange={pick} />
        <label className="field-label" htmlFor="member-job">
          Job
        </label>
        <input
          id="member-job"
          ref={jobRef}
          className="field-input"
          value={job}
          placeholder="e.g. Builds the API"
          onChange={(e) => setJob(e.target.value)}
        />
        <label className="field-label" htmlFor="member-role">
          Role name <span className="team-dialog-aside">{member ? "stays as it is" : "made from the name; the chart and the lines use it"}</span>
        </label>
        <input
          id="member-role"
          className="field-input"
          value={role}
          disabled={!!member}
          placeholder="e.g. api, verifier"
          onChange={(e) => {
            setRole(e.target.value);
            setRoleTouched(true);
          }}
        />
        <label className="field-label" htmlFor="member-owns">
          Owns
        </label>
        <input
          id="member-owns"
          className="field-input"
          value={owns}
          disabled={!owners}
          placeholder={owners ? "src/api/, tests/api/" : "Only when members own parts of the folder"}
          onChange={(e) => setOwns(e.target.value)}
        />
        {/* Both notes have a place of their own, so the dialog keeps its size. */}
        <p className="team-dialog-note" role="status">
          {note?.text ?? ""}
        </p>
        <p className="team-dialog-problem" role="alert">
          {tried && problem ? problem : ""}
        </p>
      </div>
    </ConfirmDialog>
  );
}
