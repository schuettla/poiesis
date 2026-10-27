import type { ExperimentInfo } from "./api";

/** A Workspace-shaped experiment for tests; override what the case is about. */
export const experiment = (over: Partial<ExperimentInfo> = {}): ExperimentInfo => ({
  id: "workspace",
  title: "Workspace mode",
  summary: "I build an interface for the task.",
  what_changes: ["A chat can open as a Workspace"],
  records: "Nothing of its own.",
  stage: "beta",
  graduation: "Used in a quarter of active chats for a month.",
  on: false,
  requires: [],
  can_turn_on: true,
  is_new: false,
  ...over,
});
