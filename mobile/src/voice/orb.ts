// The two small things the voice code needs from the desktop's orb module, kept here so the phone does not
// copy the whole orb.

/** Who has the floor in a live conversation, as the desktop's voice session reports it. */
export type VoiceFloorName = "listening" | "user_speaking" | "thinking" | "speaking";

export type StepKind = "searching" | "solving" | "composing" | "working";

/** What kind of work a step's verb is. The verbs are the ones the desktop writes. */
export function orbForStep(verb: string): StepKind {
  const v = verb.toLowerCase();
  if (/^(remembered|recalled memory)/.test(v)) return "working";
  if (/^(searched|searching|looked up|recalled|visited)/.test(v)) return "searching";
  if (/^(worked out|ran|checked)/.test(v)) return "solving";
  if (/^(wrote|edited|generated)/.test(v)) return "composing";
  return "working";
}
