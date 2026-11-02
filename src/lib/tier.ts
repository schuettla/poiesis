import type { Model, ModelTier } from "./types";

/** `TIER-UI-1`: what a model can carry, in the words every screen uses. The
 * rule that puts a model on a tier lives in Rust (`agent::tier`); this only
 * reads the answer and words it. */

export const TIER_LABEL: Record<ModelTier, string> = {
  full: "Full",
  basic: "Basic",
  chat: "Chat only",
};

/** The tier a turn on this model runs at. A model that arrives without one (the
 * browser preview, a mock) keeps what `tools: false` always meant. */
export function tierOf(m: Model): ModelTier {
  return m.tier ?? (m.tools === false ? "chat" : "full");
}

/** Why the tier is not Full, in one line. Null at Full. */
export function tierReason(m: Model): string | null {
  const tier = tierOf(m);
  if (tier === "full") return null;
  if (m.tierSet) {
    return tier === "chat" ? "You set this model to chat only." : "You set this model to basic tools.";
  }
  if (tier === "basic") return "Small memory, so I use only files, web search and memory.";
  if (m.tools === false) {
    return "This model can't use tools. It can chat, but it can't search, read files, or run skills.";
  }
  return "Small memory, so tools are off and answers fit.";
}

/** The tag beside a model's name in the picker, and the chip on its Models row. */
export function tierTag(m: Model): { label: string; chip: string; title: string } | null {
  const tier = tierOf(m);
  const title = tierReason(m);
  if (tier === "full" || !title) return null;
  return tier === "chat"
    ? { label: "chat only", chip: "Chat only", title }
    : { label: "basic tools", chip: "Basic tools", title };
}

/** 4096 as "4K", 131072 as "128K". */
export function tokensLabel(n: number): string {
  return n >= 1024 ? `${Math.round(n / 1024)}K` : String(n);
}

/** The steps the Memory window select offers, up to what the model was trained for. */
export const WINDOW_STEPS = [4096, 8192, 16384, 32768, 65536, 131072];

export function windowSteps(m: Model): number[] {
  // A file we could not read has no trained size; stop at the cap Auto never passes.
  const top = m.trainedCtx ?? 32768;
  const steps = WINDOW_STEPS.filter((n) => n <= top);
  const set = m.windowSet;
  if (set && !steps.includes(set)) steps.push(set);
  return steps.sort((a, b) => a - b);
}
