import { useEffect, useState } from "react";
import {
  getSetting,
  inTauri,
  listToolsets,
  setSetting,
  setToolsetEnabled,
  getToolStats,
  listIndexRoots,
  formatDiskSize,
  type ToolsetInfo,
  type ToolsetReliability,
  type IndexRootView,
  type ExecPolicy,
} from "../lib/api";
import { useAppStore, useExpert } from "../lib/store";
import DelegationCaps from "../components/Personas/DelegationCaps";
import "./Surface.css";
import "./Settings.css";

/** `RPC-1`: the one thing under Code execution that widens what a snippet can
 * reach. Off by default, and it lives beside the switch that made the sandbox
 * possible rather than in an advanced panel — this is the only place in the app
 * where code the agent wrote reaches the tool layer with no turn in between. */
const SCRIPT_RPC_KEY = "tools.script_rpc";

function ScriptToolsSwitch() {
  const [on, setOn] = useState(false);

  useEffect(() => {
    getSetting(SCRIPT_RPC_KEY)
      .then((v) => setOn(v === "true" || v === "1"))
      .catch(() => {});
  }, []);

  return (
    <div className="toolset-subswitch">
      <label>
        <input
          type="checkbox"
          checked={on}
          onChange={(e) => {
            setOn(e.target.checked);
            setSetting(SCRIPT_RPC_KEY, e.target.checked ? "true" : "false").catch(() => {});
          }}
        />
        Let a script I write call my own tools
      </label>
      <p>
        It still asks you for anything that needs asking, and every call it makes shows up in the
        timeline under the step that ran it. Useful when a job is the same thing over many items: one
        script instead of one turn each. It cannot start another script.
      </p>
    </div>
  );
}

/** `COD-UI-6`: how project tasks run when a project has not chosen for itself,
 * and, in expert mode only, whether free-form commands exist at all. The
 * description says what the confinement does and what it does not. */
const TASK_POLICY_KEY = "code_run.default_policy";
const RUN_COMMAND_KEY = "code_run.run_command";

const TASK_POLICIES: { id: ExecPolicy; label: string; blurb: string }[] = [
  { id: "ask", label: "Ask each time", blurb: "Every run asks, unless you always allowed that task in its project." },
  { id: "allow", label: "Run declared tasks", blurb: "Tasks the project declares run without asking. Scheduled runs still need the project's own allow." },
  { id: "off", label: "Off", blurb: "No task runs unless a project turns it on for itself." },
];

function TaskPolicy() {
  const expert = useExpert();
  const [policy, setPolicy] = useState<ExecPolicy>("ask");
  const [commands, setCommands] = useState(false);

  useEffect(() => {
    getSetting(TASK_POLICY_KEY)
      .then((v) => {
        if (v === "off" || v === "ask" || v === "allow") setPolicy(v);
      })
      .catch(() => {});
    getSetting(RUN_COMMAND_KEY)
      .then((v) => setCommands(v === "true" || v === "1"))
      .catch(() => {});
  }, []);

  const choose = (id: ExecPolicy) => {
    setPolicy(id);
    setSetting(TASK_POLICY_KEY, id).catch(() => {});
  };

  return (
    <div className="toolset-subswitch" role="radiogroup" aria-label="How project tasks run">
      {TASK_POLICIES.map((p) => (
        <label key={p.id} title={p.blurb}>
          <input type="radio" name="task-policy" checked={policy === p.id} onChange={() => choose(p.id)} />
          {p.label}
        </label>
      ))}
      <p>
        {TASK_POLICIES.find((p) => p.id === policy)?.blurb} A task runs in the project folder with a
        time limit, a memory cap and a cap on how many processes it starts, and Stop ends all of them.
        Secret-looking environment variables are left out. It is not a full sandbox: a task can still
        reach the network and write anywhere your account can. A read-only folder never runs one.
      </p>
      {expert && (
        <>
          <label>
            <input
              type="checkbox"
              checked={commands}
              onChange={(e) => {
                setCommands(e.target.checked);
                setSetting(RUN_COMMAND_KEY, e.target.checked ? "true" : "false").catch(() => {});
              }}
            />
            Run any command
          </label>
          <p>
            Lets me ask to run a command the project does not declare, in projects that opt in from their
            header. Every one asks, shows the exact command, and never goes through a shell. Only allow
            commands you would type yourself.
          </p>
        </>
      )}
    </div>
  );
}

/** `HRN-5`: the step limit for a turn you started yourself.
 *
 * It was a constant, which made "step 3 of 12" read as a law of the app rather
 * than a number someone chose. Delegated agents already had their own cap in
 * `DelegationCaps`; this is the same control for the run at the top, and it
 * sits above the toolset list because it governs any turn that uses tools, not
 * one of them. */
const MAX_STEPS_KEY = "agent.max_steps";

function StepLimit() {
  const [value, setValue] = useState("");

  useEffect(() => {
    getSetting(MAX_STEPS_KEY)
      .then((v) => setValue(v ?? "12"))
      .catch(() => {});
  }, []);

  const commit = (raw: string) => {
    const n = Number(raw);
    // Same clamp as the backend, so the box can never show a number the run
    // would not actually use.
    const clamped = Number.isFinite(n) && raw.trim() !== "" ? Math.min(50, Math.max(1, Math.round(n))) : 12;
    setValue(String(clamped));
    setSetting(MAX_STEPS_KEY, String(clamped)).catch(() => {});
  };

  return (
    <section className="setting-block">
      <div className="delegation-caps step-limit">
        <label className="delegation-cap">
          <span>How many steps one turn of mine may take</span>
          <input
            type="number"
            min={1}
            max={50}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onBlur={(e) => commit(e.target.value)}
          />
        </label>
        <p className="delegation-note">
          A step is one round of tool calls. When the count runs out I stop and answer with what I
          have, rather than failing. Raise it for long builds; most questions never reach 12.
        </p>
      </div>
    </section>
  );
}

export default function Tools() {
  const [toolsets, setToolsets] = useState<ToolsetInfo[]>([]);
  const [reliability, setReliability] = useState<ToolsetReliability[]>([]);
  const [indexRoots, setIndexRoots] = useState<IndexRootView[]>([]);
  const forgetFolderIndex = useAppStore((s) => s.forgetFolderIndex);

  useEffect(() => {
    if (!inTauri()) return;
    listToolsets().then(setToolsets).catch(() => {});
    getToolStats().then(setReliability).catch(() => {});
    listIndexRoots().then(setIndexRoots).catch(() => {});
  }, []);

  async function toggleToolset(id: string, enabled: boolean) {
    // Optimistic; revert on failure.
    setToolsets((list) => list.map((s) => (s.id === id ? { ...s, enabled } : s)));
    try {
      await setToolsetEnabled(id, enabled);
    } catch {
      setToolsets((list) => list.map((s) => (s.id === id ? { ...s, enabled: !enabled } : s)));
    }
  }

  async function forgetIndexRoot(path: string) {
    await forgetFolderIndex(path);
    setIndexRoots((list) => list.filter((r) => r.path !== path));
  }

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Tools</h1>
        <p className="lede">
          What Poiesis Agent can do beyond chatting, when tools are turned on in a chat. Each one is
          opt-in; those that leave your device or run code are marked.
        </p>

        {inTauri() && <StepLimit />}

        {inTauri() && (
          <section className="setting-block">
            {toolsets.map((s) => {
              const rel = reliability.find((r) => r.skill_id === s.id);
              return (
                <div key={s.id} className="toolset-item">
                  <label className="toggle-line toolset-line">
                    <input
                      type="checkbox"
                      checked={s.enabled}
                      onChange={(e) => toggleToolset(s.id, e.target.checked)}
                    />
                    <span className="toolset-text">
                      <span className="toolset-label">
                        {s.label}
                        {s.sensitive && <span className="toolset-flag">leaves device / runs code</span>}
                      </span>
                      <span className="toolset-desc">{s.description}</span>
                      {rel && (
                        <span className="toolset-reliability">
                          {rel.ok_percent}% ok over {rel.calls} call{rel.calls === 1 ? "" : "s"} this
                          week
                        </span>
                      )}
                    </span>
                  </label>
                  {/* IDX-UI-4: the indexed folders this tool has built, wherever
                      they were attached from — with the one undo that matters. */}
                  {/* `SUB-UI-7`: delegation's caps belong beside its switch —
                      how many agents, how far each may go, and whether one of
                      them may hand work out again. */}
                  {s.id === "subagents" && s.enabled && <DelegationCaps />}
                  {/* `RPC-1`: only under a sandbox that is actually switched on
                      — the ability has nothing to attach to otherwise. */}
                  {s.id === "code_exec" && s.enabled && <ScriptToolsSwitch />}
                  {s.id === "code_run" && s.enabled && <TaskPolicy />}
                  {s.id === "indexing" && indexRoots.length > 0 && (
                    <ul className="toolset-subitems">
                      {indexRoots.map((r) => (
                        <li key={r.path} className="toolset-subitem">
                          <span className="toolset-subitem-path" title={r.path}>
                            {r.path}
                          </span>
                          <span className="toolset-subitem-meta">{formatDiskSize(r.size_bytes)}</span>
                          <button className="link-button" onClick={() => forgetIndexRoot(r.path)}>
                            Forget this folder
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              );
            })}
          </section>
        )}
      </div>
    </div>
  );
}
