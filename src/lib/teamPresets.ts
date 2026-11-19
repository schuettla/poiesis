import type { Persona, TeamMember, TeamShape } from "./api";

export interface TeamPresetMember {
  role: string;
  job: string;
  /** The persona preset that plays it when the person has a persona of that name; a helper otherwise. */
  persona?: string;
}

export interface TeamPreset {
  name: string;
  line: string;
  shape: TeamShape;
  members: TeamPresetMember[];
}

/**
 * Team-shaped starting points, the way `PERSONA_PRESETS` are for a persona: a few jobs that work
 * well together, in the shape they work in. A new team offers the ones not made yet.
 * Each member is a helper with a job unless the person has the persona preset it names.
 */
export const TEAM_PRESETS: TeamPreset[] = [
  {
    name: "Build and review",
    line: "Builds a change, tests it, and has it reviewed",
    shape: "review",
    members: [
      { role: "builder", job: "Makes the change in the code", persona: "Builder" },
      { role: "tester", job: "Writes and runs the tests for the change" },
      { role: "reviewer", job: "Reads the change and sends back what is wrong" },
    ],
  },
  {
    name: "Research and write",
    line: "Finds sources, writes from them, and edits",
    shape: "pipeline",
    members: [
      { role: "researcher", job: "Finds sources and notes what each one says" },
      { role: "writer", job: "Writes the piece from the notes" },
      { role: "editor", job: "Cuts the piece and checks it against the notes", persona: "The Editor" },
    ],
  },
  {
    name: "Bug hunt",
    line: "Reproduces a bug, fixes its cause, and proves the fix",
    shape: "hub",
    members: [
      { role: "reproducer", job: "Reproduces the bug and writes down the steps" },
      { role: "fixer", job: "Fixes the cause, not the symptom", persona: "Builder" },
      { role: "prover", job: "Writes a test that fails before the fix and passes after" },
    ],
  },
  {
    name: "Second opinion",
    line: "Talks a decision through before you make it",
    shape: "open",
    members: [
      { role: "proposer", job: "Proposes a way to do it" },
      { role: "skeptic", job: "Looks for what would break", persona: "The Skeptic" },
      { role: "pragmatist", job: "Says what to do first" },
    ],
  },
];

/** The preset's members, each played by the person's persona of the name it gives, if there is one. */
export function presetMembers(preset: TeamPreset, personas: Pick<Persona, "id" | "name">[]): TeamMember[] {
  return preset.members.map((m) => {
    const own = m.persona ? personas.find((p) => p.name === m.persona) : undefined;
    return { role: m.role, persona: own ? `persona:${own.id}` : "helper", job: m.job, owns: [] };
  });
}
