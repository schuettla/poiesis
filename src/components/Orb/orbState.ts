import type { OrbState } from "thinking-orbs";
import type { SubRun } from "../../lib/types";

/**
 * Which orb animation says what the agent is doing. Every mapping reads a fact
 * the app already holds (a step's verb, a presence, a child's status), so the
 * animation is a second way to read the status line and never a decoration
 * that could disagree with it.
 *
 * Verbs are the ones the backend writes (`src-tauri/src/agent/*.rs`). A verb
 * that is not listed here is plain work, which is what `working` means.
 */
export function orbForStep(verb: string): OrbState {
  const v = verb.toLowerCase();
  if (/^(planned|updated the plan)/.test(v)) return "shaping";
  // `CPX-2`: the agent acting on its own session shapes; a question to the user
  // listens. `asked you` is `ask_user`'s verb; `asked to switch/schedule` are not.
  if (/^(made room|cleared)/.test(v)) return "shaping";
  if (/^asked you/.test(v)) return "listening";
  // Before the search rule: "recalled memory" is the memory tool, while a bare
  // "recalled" is a search of past conversations.
  if (/^(remembered|recalled memory)/.test(v)) return "weaving";
  if (/^(searched|searching|looked up|recalled|visited)/.test(v)) return "searching";
  if (/^(worked out|ran|checked)/.test(v)) return "solving";
  if (/^(wrote|edited|generated)/.test(v)) return "composing";
  return "working";
}

/** The mark's presence, as an orb: at rest it breathes, and each busy kind of
 * presence takes the animation closest to what it is. */
export function orbForPresence(
  presence: "idle" | "active" | "reflecting" | "healing" | "tending" | "listening"
): OrbState {
  switch (presence) {
    case "active":
      return "working";
    case "reflecting":
      return "weaving";
    case "healing":
    case "tending":
      return "shaping";
    case "listening":
      return "listening";
    default:
      return "breathing";
  }
}

/** One delegated child. A queued child has done nothing yet, so it breathes
 * rather than claiming work; a child that has taken no step is still joining
 * the lead; after that it shows whatever step it is on. */
export function orbForSubRun(run: Pick<SubRun, "status" | "steps">): OrbState {
  if (run.status === "queued") return "breathing";
  if (run.steps.length === 0) return "connecting";
  const running = run.steps.find((s) => s.status === "running");
  return running ? orbForStep(running.verb) : "working";
}

/** Who has the floor in a voice conversation (`VOC-UI-4`). Same names the
 * session reports. */
export type VoiceFloorName = "listening" | "user_speaking" | "thinking" | "speaking";

/** After this long with nobody talking, the orb goes back to breathing. */
export const VOICE_QUIET_MS = 2000;

/**
 * The orb of a voice conversation (`VXP-1`). `quietMs` is how long it has been
 * since anyone spoke; `stepVerb` is the verb of a tool step that is running.
 * A running step wins while Poiesis is working, so the orb says what it is
 * doing; while it speaks, it stays the speaking orb.
 */
export function orbForFloor(floor: VoiceFloorName, quietMs = 0, stepVerb?: string | null): OrbState {
  switch (floor) {
    case "user_speaking":
      return "listening";
    case "thinking":
      return stepVerb ? orbForStep(stepVerb) : "connecting";
    case "speaking":
      return "composing";
    default:
      return quietMs >= VOICE_QUIET_MS ? "breathing" : "listening";
  }
}
