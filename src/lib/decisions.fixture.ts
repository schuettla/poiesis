import type { DecisionEntry } from "./api";

/** A goal-check decision for tests; override what the case is about. */
export const decision = (over: Partial<DecisionEntry> = {}): DecisionEntry => ({
  id: "d1",
  conversation_id: "c1",
  run_id: null,
  purpose: "goal-met",
  label: "Is the goal reached?",
  background: false,
  experiment: null,
  backend: "local:Clef-flash",
  model: "Clef-Flash-Q4_K_M",
  ms: 41,
  calibrated: false,
  answers: { met: { p: 0.93 } },
  input_tokens: 300,
  state_preview: "Goal: make the tests pass",
  action: "acted",
  at: 1_700_000_000_000,
  outcomes: [],
  labels: [],
  ...over,
});
