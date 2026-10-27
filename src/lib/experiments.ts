/**
 * `EXP-1`: what the frontend asks about experiments. The registry itself is the
 * backend's (`experiments.rs`); the store holds the list `experiments_list_cmd`
 * returns, so there is no second list here to drift.
 *
 * Takes the store state's shape, not the store, so the store can import it.
 */
import type { ExperimentInfo } from "./api";

type HasExperiments = { experiments: ExperimentInfo[] | null };

/** Is this experiment on? Unknown, or not read yet, is off. */
export function experimentOn(s: HasExperiments, id: string): boolean {
  return s.experiments?.find((e) => e.id === id)?.on === true;
}

/** `EXP-3`: may a chat be in Workspace mode?
 *
 * Until the list has been read (a browser tab, the first tick of startup) this is
 * yes: hiding a layout somebody is already in because a read has not finished
 * would be worse than showing it, and the backend gates the agent's side either
 * way. Once it has been read, the experiment decides. */
export function workspaceAvailable(s: HasExperiments): boolean {
  return s.experiments === null || experimentOn(s, "workspace");
}

/** An experiment appeared that the Experiments screen has not shown yet. */
export function hasNewExperiment(s: HasExperiments): boolean {
  return s.experiments?.some((e) => e.is_new) === true;
}
