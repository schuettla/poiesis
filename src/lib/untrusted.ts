import type { AgentStep } from "./types";

type Sources = NonNullable<AgentStep["untrusted"]>;

/**
 * `USE-8`: the quick check says the text of source `label` reads like orders to
 * the assistant. It arrives after the text did, so it marks the source it is
 * about. A report with no `untrusted` event of its own (an agent app's) gets a
 * source with no text to show.
 */
export function markReadsAsInstructions(sources: Sources | undefined, label: string, p: number): Sources {
  const next = [...(sources ?? [])];
  const at = next.findIndex((u) => u.label === label);
  if (at >= 0) next[at] = { ...next[at], readsAsInstructions: p };
  else next.push({ label, risk: 0, flags: [], text: "", readsAsInstructions: p });
  return next;
}
