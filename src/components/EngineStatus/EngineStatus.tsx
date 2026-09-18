import { useAppStore } from "../../lib/store";
import { inTauri } from "../../lib/api";
import "./EngineStatus.css";

/**
 * Makes the local runtime visible: a model isn't usable until llama-server is
 * actually running it. Shows starting / ready / idle so it's never ambiguous
 * that an engine must be in place to chat.
 *
 * It sits beside the Settings row at the foot of the Rail, which is both where
 * there is room for it and where the user goes to do anything about it — the
 * Runtime section is one click away in the same column. It used to hold a
 * reserved segment of the header (`SHL-2`), which cost the tab strip width
 * permanently for something that is idle-and-silent nearly all the time.
 *
 * The word "Runtime" is dropped from the label. Standing next to the cog it is
 * the only thing there that has a state, and the full sentence is still in the
 * `title` and `aria-label` for anyone who needs it spelled out.
 */
export default function EngineStatus({ dotOnly = false }: { dotOnly?: boolean }) {
  const engineReady = useAppStore((s) => s.engineReady);
  const loadingModel = useAppStore((s) => s.loadingModel);
  if (!inTauri()) return null;

  let state = "idle";
  let label = "idle";
  let full = "Runtime idle";
  if (loadingModel) {
    state = "starting";
    label = loadingModel.label || "Starting…";
    full = loadingModel.label || "Starting runtime…";
  } else if (engineReady) {
    state = "ready";
    label = "ready";
    full = "Runtime ready";
  }

  return (
    <div
      className={`engine-status ${state} ${dotOnly ? "dot-only" : ""}`}
      title="The local model runtime (llama-server) runs on your PC to power chats. It starts automatically when you use a model."
      aria-label={`Local runtime: ${full}`}
    >
      <span className="engine-dot" aria-hidden="true" />
      {/* Dropped, not merely hidden, when there is no room for words — the
          collapsed rail. Nothing is lost: `title` and `aria-label` above
          already carry the whole sentence, so the dot keeps saying everything
          the words did. */}
      {!dotOnly && <span className="engine-label">{label}</span>}
    </div>
  );
}
