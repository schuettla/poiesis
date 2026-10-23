/**
 * The reasoning-effort levels, in one place: the composer's pickers, the
 * `/effort` command menu and its validation all read this list, so the terms
 * cannot drift apart again.
 *
 * Matches `cloud::Effort`. `provider` sends no parameter at all, which is the
 * only honest option for a server we know nothing about — so it is last, and
 * its meter reads as unknown rather than as a level.
 */
export const EFFORTS = [
  { value: "off", label: "Off", bars: 0, hint: "Answer straight away. Fastest and cheapest." },
  { value: "low", label: "Low", bars: 1, hint: "A moment's thought first." },
  { value: "medium", label: "Medium", bars: 2, hint: "For questions with a few moving parts." },
  { value: "high", label: "High", bars: 3, hint: "Slow and costly. Difficult work only." },
  { value: "provider", label: "Default", bars: -1, hint: "Send nothing and take the provider's own setting." },
] as const;

export type Effort = (typeof EFFORTS)[number];
