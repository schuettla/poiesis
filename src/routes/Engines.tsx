import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  addAcpEngine,
  clearEngineKey,
  getSetting,
  inTauri,
  listEngineModels,
  openEngineLogin,
  probeEngine,
  removeEngine,
  setEngineKey,
  setSetting,
  testEngine,
  type EngineInfo,
  type EngineModel,
  type EnginePatch,
  type EngineTryResult,
  type EngineTrust,
} from "../lib/api";
import { cannotEnable, engineStatus, envToMap, linesToArgs, tryLine } from "../lib/engines";
import { useAppStore } from "../lib/store";
import Select from "../components/Select/Select";
import Toggle from "../components/Toggle/Toggle";
import { ChevronIcon } from "../components/Icons/Icons";
import ConfirmDialog from "../components/Confirm/ConfirmDialog";
import Opt from "../components/Engines/Opt";
import RuleList, { RecentJobs, TryRequest } from "../components/Engines/RuleList";
import "./Surface.css";
import "./Settings.css";
import "./Engines.css";

const TRUST_OPTIONS: { value: EngineTrust; label: string }[] = [
  { value: "read-only", label: "Read only" },
  { value: "confirm", label: "Ask me first" },
  { value: "auto", label: "Edits without asking" },
];

const errText = (e: unknown) => String(e).replace(/^Error:\s*/, "");

/** `CLI-UI-1`: every agent app Poiesis can hand work to, and all of its settings.
 * Finding an app is not consent: a row that was just found starts switched off. */
export default function Engines() {
  const engines = useAppStore((s) => s.engines);
  const detectEngines = useAppStore((s) => s.detectEngines);
  const markEnginesSeen = useAppStore((s) => s.markEnginesSeen);
  const adoptEngines = useAppStore((s) => s.adoptEngines);
  const engineFocus = useAppStore((s) => s.engineFocus);

  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [checking, setChecking] = useState(false);
  const [adding, setAdding] = useState(false);
  // What was new when the section opened. The dot clears at once, the words stay.
  const [newNames] = useState(() => engines.filter((e) => e.is_new).map((e) => e.name));

  useEffect(() => {
    if (!inTauri()) return;
    markEnginesSeen();
    detectEngines(false);
  }, [markEnginesSeen, detectEngines]);

  const toggle = useCallback((id: string, force?: boolean) => {
    setExpanded((old) => {
      const next = new Set(old);
      const on = force ?? !next.has(id);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });
  }, []);

  // A link from elsewhere (a Fleet card's gear, a permission panel) lands here
  // with one agent open.
  useEffect(() => {
    if (!engineFocus) return;
    toggle(engineFocus, true);
    useAppStore.setState({ engineFocus: null });
  }, [engineFocus, toggle]);

  async function checkAgain() {
    setChecking(true);
    try {
      await detectEngines(true);
    } finally {
      setChecking(false);
    }
  }

  if (!inTauri()) {
    return (
      <div className="surface">
        <div className="surface-inner">
          <h1>External agents</h1>
          <p className="lede">Agent apps are found and set up in the desktop app.</p>
        </div>
      </div>
    );
  }

  const installed = engines.filter((e) => e.found);
  const missing = engines.filter((e) => !e.found);
  const names = (list: string[]) =>
    list.length <= 1 ? list[0] : `${list.slice(0, -1).join(", ")} and ${list[list.length - 1]}`;

  return (
    <div className="surface">
      <div className="surface-inner engines">
        <h1>External agents</h1>
        <p className="lede">
          I can hand work to agent apps you've installed. They work in your folder with their own
          model and tools; I keep the conversation and the final word.
        </p>
        {newNames.length > 0 && (
          <p className="engines-found" role="status">
            I found {names(newNames)}. Turn {newNames.length === 1 ? "it" : "them"} on when you want
            me to hand work to {newNames.length === 1 ? "it" : "them"}.
          </p>
        )}

        <div className="engines-head">
          <h2 className="section-title">Installed</h2>
          <button className="link-button" onClick={checkAgain} disabled={checking}>
            {checking ? "Checking…" : "Check again"}
          </button>
        </div>
        {installed.length === 0 ? (
          <p className="engines-empty">
            I didn't find any agent apps on this computer. If you install Claude Code, Codex or
            OpenCode, I'll find it here.
          </p>
        ) : (
          <div className="engines-list">
            {installed.map((e) => (
              <EngineRow key={e.id} engine={e} open={expanded.has(e.id)} onToggle={() => toggle(e.id)} />
            ))}
          </div>
        )}

        {missing.length > 0 && (
          <>
            <h2 className="section-title engines-section">Not found</h2>
            <div className="engines-list">
              {missing.map((e) => (
                <MissingRow key={e.id} engine={e} />
              ))}
            </div>
          </>
        )}

        {adding ? (
          <AddAcp
            onDone={(newId) => {
              setAdding(false);
              if (newId) toggle(newId, true);
            }}
            known={engines.map((e) => e.id)}
            adopt={adoptEngines}
          />
        ) : (
          <button className="btn-secondary engines-add" onClick={() => setAdding(true)}>
            + Add another agent (ACP)…
          </button>
        )}

        <Limits />
        {installed.some((e) => e.enabled && e.delegate_ready) && <TryRequest />}
      </div>
    </div>
  );
}

function Glyph({ on }: { on: boolean }) {
  return <span className={`engines-glyph ${on ? "on" : ""}`} aria-hidden="true" />;
}

/** An agent that was never found, or has gone: the row offers an install link
 * and a way to point at it. Its choices are kept while it is gone. */
function MissingRow({ engine: e }: { engine: EngineInfo }) {
  const updateEngine = useAppStore((s) => s.updateEngine);
  const [error, setError] = useState<string | null>(null);
  const status = engineStatus(e);

  async function setPath() {
    setError(null);
    try {
      const picked = await openDialog({ multiple: false, filters: [{ name: "Program", extensions: ["exe", "cmd", "bat"] }] });
      if (typeof picked === "string") await updateEngine(e.id, { program: picked });
    } catch (err) {
      setError(errText(err));
    }
  }

  return (
    <div className="engines-row missing">
      <div className="engines-row-head">
        <div className="engines-row-main static">
          <Glyph on={false} />
          <span className="engines-name">{e.name}</span>
          <span className="engines-status">{status.text}</span>
        </div>
        <div className="engines-row-actions">
          {e.install_url && (
            <button className="link-button" onClick={() => openUrl(e.install_url!)}>
              How to install
            </button>
          )}
          <button className="btn-secondary" onClick={setPath}>
            Set path…
          </button>
        </div>
      </div>
      {error && (
        <p className="engines-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

function EngineRow({ engine: e, open, onToggle }: { engine: EngineInfo; open: boolean; onToggle: () => void }) {
  const updateEngine = useAppStore((s) => s.updateEngine);
  const adoptEngines = useAppStore((s) => s.adoptEngines);
  const [error, setError] = useState<string | null>(null);
  const [signInWaiting, setSignInWaiting] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const status = engineStatus(e);
  const blocked = cannotEnable(e);

  useEffect(() => {
    if (open) ref.current?.scrollIntoView?.({ block: "nearest" });
  }, [open]);

  const save = useCallback(
    async (patch: EnginePatch) => {
      setError(null);
      try {
        await updateEngine(e.id, patch);
      } catch (err) {
        setError(errText(err));
      }
    },
    [e.id, updateEngine],
  );

  // After **Sign in**, the terminal is somewhere else. When this window comes
  // back to the front, look again.
  useEffect(() => {
    if (!signInWaiting) return;
    const onFocus = () => {
      probeEngine(e.id)
        .then((list) => {
          adoptEngines(list);
          if (list.find((x) => x.id === e.id)?.signed_in) setSignInWaiting(false);
        })
        .catch(() => {});
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [signInWaiting, e.id, adoptEngines]);

  async function signIn() {
    setError(null);
    try {
      await openEngineLogin(e.id);
      setSignInWaiting(true);
    } catch (err) {
      setError(errText(err));
    }
  }

  return (
    <div className={`engines-row ${open ? "open" : ""} ${e.enabled ? "on" : "off"}`} ref={ref} id={`engine-${e.id}`}>
      <div className="engines-row-head">
        <button
          className="engines-row-main"
          onClick={onToggle}
          aria-expanded={open}
          aria-controls={`engine-body-${e.id}`}
        >
          <span className="engines-chevron" aria-hidden="true">
            <ChevronIcon dir={open ? "down" : "right"} size={12} strokeWidth={1.8} />
          </span>
          <Glyph on={e.enabled} />
          <span className="engines-ident">
            <span className="engines-name">{e.name}</span>
            <span className={`engines-status ${status.tone}`}>{status.text}</span>
          </span>
        </button>
        <div className="engines-row-actions">
          {e.signed_in === false && e.builtin && (
            <button className="btn-secondary sm" onClick={signIn}>
              Sign in
            </button>
          )}
          <label className="engines-switch" title={blocked ?? undefined}>
            <span className={`engines-switch-state ${e.enabled ? "on" : ""}`}>{e.enabled ? "On" : "Off"}</span>
            <Toggle
              checked={e.enabled}
              disabled={!e.enabled && blocked !== null}
              onChange={(on) => save({ enabled: on })}
              label={`Let me use ${e.name}`}
            />
          </label>
        </div>
      </div>
      {e.signed_in === false && (
        <p className="engines-note">
          Installed, but not signed in. Sign in opens its own login, and I never see your password or
          tokens.
          {signInWaiting && " I'll look again when you come back."}
        </p>
      )}
      {e.found && e.last_error && e.signed_in !== false && (
        <p className="engines-error" role="alert">
          {e.last_error}
        </p>
      )}
      {error && (
        <p className="engines-error" role="alert">
          {error}
        </p>
      )}
      {open && (
        <div className="engines-body" id={`engine-body-${e.id}`}>
          <RuleList engine={e} />
          <ConnectionGroup engine={e} save={save} />
          <BillingGroup engine={e} save={save} />
          <PermissionsGroup engine={e} save={save} />
          {e.can_pick_model && <ModelGroup engine={e} save={save} />}
          <RecentJobs engine={e} />
          <AdvancedGroup engine={e} save={save} />
        </div>
      )}
    </div>
  );
}

type Save = (patch: EnginePatch) => Promise<void>;

function ConnectionGroup({ engine: e, save }: { engine: EngineInfo; save: Save }) {
  const adoptEngines = useAppStore((s) => s.adoptEngines);
  const [trying, setTrying] = useState(false);
  const [result, setResult] = useState<EngineTryResult | null>(null);
  const [checking, setChecking] = useState(false);

  async function change() {
    const picked = await openDialog({ multiple: false, filters: [{ name: "Program", extensions: ["exe", "cmd", "bat"] }] });
    if (typeof picked === "string") await save({ program: picked });
  }

  async function tryIt() {
    setTrying(true);
    setResult(null);
    try {
      setResult(await testEngine(e.id));
    } catch (err) {
      setResult({ ok: false, reply: "", seconds: 0, paid_by: null, model: null, error: errText(err), needs_sign_in: false });
    } finally {
      setTrying(false);
    }
  }

  async function lookAgain() {
    setChecking(true);
    try {
      adoptEngines(await probeEngine(e.id));
    } catch {
      /* the row keeps what it had */
    } finally {
      setChecking(false);
    }
  }

  return (
    <section className="engines-group" aria-label="Connection">
      <h3 className="engines-group-title">Connection</h3>
      <Opt
        title="Program"
        help={
          <>
            <span className="mono" title={e.found_path ?? undefined}>
              {e.found_path ?? "not found"}
            </span>
            {e.shim && <span className="engines-tag">npm shim</span>}
          </>
        }
      >
        <button className="btn-secondary sm" onClick={change}>
          Change…
        </button>
        {e.program && (
          <button className="btn-text" onClick={() => save({ program: "" })}>
            Reset
          </button>
        )}
      </Opt>
      <Opt
        title="Version"
        help={
          <span className={e.tested === "newer" || e.tested === "older" ? "warn" : undefined}>
            {e.version ?? "unknown"}
            {e.tested === "yes" && " · tested"}
            {e.tested === "newer" && " · newer than I've been tested with. It should still work, and if it doesn't, the error will say why."}
            {e.tested === "older" && " · older than I've been tested with"}
          </span>
        }
      >
        <button className="btn-secondary sm" onClick={lookAgain} disabled={checking}>
          {checking ? "Checking…" : "Check again"}
        </button>
      </Opt>
      <Opt
        title="Try it"
        help={
          result ? (
            <span className={result.ok ? "ok" : "bad"} role="status">
              {tryLine(result)}
            </span>
          ) : (
            "One tiny request, with no tools, to see that it answers."
          )
        }
      >
        <button className="btn-secondary sm" onClick={tryIt} disabled={trying}>
          {trying ? "Trying…" : "Try it"}
        </button>
      </Opt>
      {result && !result.ok && result.needs_sign_in && (
        <p className="engines-note">Sign in with the button on the row, then try again.</p>
      )}
    </section>
  );
}

function BillingGroup({ engine: e, save }: { engine: EngineInfo; save: Save }) {
  const adoptEngines = useAppStore((s) => s.adoptEngines);
  const [wantKey, setWantKey] = useState(e.payment === "api-key" && !e.has_key);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  if (!e.supports_api_key) {
    return (
      <section className="engines-group" aria-label="Billing">
        <h3 className="engines-group-title">Billing</h3>
        <p className="engines-muted">
          {e.paid_by === "own"
            ? "It uses a free model or your own account with the provider you set up inside it."
            : "Its usage counts toward your plan's limits, the same as using it yourself."}
        </p>
      </section>
    );
  }

  async function saveKey() {
    const key = draft.trim();
    if (!key) return;
    setBusy(true);
    setError(null);
    try {
      adoptEngines(await setEngineKey(e.id, key));
      setDraft("");
      setWantKey(false);
      await save({ payment: "api-key" });
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  }

  async function removeKey() {
    setBusy(true);
    try {
      adoptEngines(await clearEngineKey(e.id));
      setWantKey(false);
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  }

  const usingKey = e.payment === "api-key" || wantKey;
  return (
    <section className="engines-group" aria-label="Billing">
      <h3 className="engines-group-title">Billing</h3>
      <div className="engines-choices" role="radiogroup" aria-label="Pay with">
        <label className={`engines-choice ${usingKey ? "" : "on"}`}>
          <input
            type="radio"
            name={`pay-${e.id}`}
            checked={!usingKey}
            onChange={() => {
              setWantKey(false);
              save({ payment: "plan" });
            }}
          />
          <span className="engines-opt-text">
            <span className="engines-opt-title">My plan</span>
            <span className="engines-opt-help">
              The app's own sign-in. Its usage counts toward your plan's limits, the same as using it yourself.
            </span>
          </span>
        </label>
        <label className={`engines-choice ${usingKey ? "on" : ""}`}>
          <input
            type="radio"
            name={`pay-${e.id}`}
            checked={usingKey}
            onChange={() => {
              if (e.has_key) save({ payment: "api-key" });
              else setWantKey(true);
            }}
          />
          <span className="engines-opt-text">
            <span className="engines-opt-title">An API key</span>
            <span className="engines-opt-help">
              {usingKey && e.has_key ? (
                <>
                  Key saved in the vault ·{" "}
                  <button className="link-button inline" onClick={removeKey} disabled={busy}>
                    Remove
                  </button>
                </>
              ) : (
                "Billed by the provider for each use. I keep the key in the vault."
              )}
            </span>
          </span>
        </label>
      </div>
      {usingKey && !e.has_key && (
        <div className="engines-keyform">
          <input
            className="field-input"
            type="password"
            autoComplete="off"
            aria-label={`${e.name} API key`}
            placeholder="Paste a key"
            value={draft}
            onChange={(ev) => {
              setDraft(ev.target.value);
              setError(null);
            }}
            onKeyDown={(ev) => ev.key === "Enter" && saveKey()}
          />
          <button className="btn-primary" onClick={saveKey} disabled={busy || !draft.trim()}>
            Save key
          </button>
          {error && (
            <p className="engines-error" role="alert">
              {error}
            </p>
          )}
        </div>
      )}
    </section>
  );
}

function PermissionsGroup({ engine: e, save }: { engine: EngineInfo; save: Save }) {
  return (
    <section className="engines-group" aria-label="Permissions">
      <h3 className="engines-group-title">Permissions</h3>
      <Opt
        title="Most it may do"
        help="Even in a folder where I may edit freely, it never gets more than this. Commands always ask."
      >
        <Select
          label={`${e.name}: the most it may do`}
          value={e.trust_ceiling}
          options={TRUST_OPTIONS}
          onChange={(v) => save({ trust_ceiling: v as EngineTrust })}
        />
      </Opt>
      <Opt
        as="label"
        title="In scheduled and background jobs"
        help="Nobody is there to approve anything, so it can only do what needs no approval."
      >
        <Toggle checked={e.headless} onChange={(on) => save({ headless: on })} />
      </Opt>
      {/* `MCPS-2`: only for an agent that can be given my tools. */}
      {e.memory_ready && (
        <Opt
          as="label"
          title="Let it use my memory"
          help="It can search your past chats and saved memories, ask you a question, show a table or progress bar on its card, and suggest something for me to keep. What it finds goes to its model. It cannot save a memory by itself: you read each suggestion first."
        >
          <Toggle checked={e.memory_tools} onChange={(on) => save({ memory_tools: on })} />
        </Opt>
      )}
      {/* `CAP-14`: the same kind of agent, and a switch of its own, because reaching my apps is a bigger thing than reading my memory. */}
      {e.memory_ready && (
        <Opt
          as="label"
          title="Let it use my connected apps"
          help="It can look things up in the apps I'm connected to, like my calendar, mail and chat, and ask to change them. I ask you before anything is changed or sent, and a job nobody is watching can only do what you've already said yes to. What it reads goes to its model. It never gets my Composio key or a sign-in, and which apps it may use follows the persona it runs under. This needs a Composio key under Apps."
        >
          <Toggle checked={e.app_tools} onChange={(on) => save({ app_tools: on })} />
        </Opt>
      )}
      <p className="engines-muted">
        It runs outside my sandbox, in the folder of the conversation. Its own settings file may allow
        some steps without asking me, and I'll tell you when that happens.
      </p>
    </section>
  );
}

function ModelGroup({ engine: e, save }: { engine: EngineInfo; save: Save }) {
  const [models, setModels] = useState<EngineModel[] | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let live = true;
    listEngineModels(e.id)
      .then((m) => live && setModels(m))
      .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [e.id]);

  const options = [{ value: "", label: "Its own default" }];
  for (const m of models ?? []) options.push({ value: m.id, label: m.name });
  // A model chosen earlier that the list no longer shows is still the choice.
  if (e.model && !options.some((o) => o.value === e.model)) options.push({ value: e.model, label: e.model });

  return (
    <section className="engines-group" aria-label="Model">
      <h3 className="engines-group-title">Model</h3>
      <Opt title="Model" help="Which model it uses for the work I hand it.">
        {failed ? (
          <input
            className="field-input"
            aria-label={`${e.name} model`}
            placeholder="Its own default"
            defaultValue={e.model ?? ""}
            onBlur={(ev) => ev.target.value.trim() !== (e.model ?? "") && save({ model: ev.target.value })}
          />
        ) : (
          <Select label={`${e.name}: model`} value={e.model ?? ""} options={options} onChange={(v) => save({ model: v })} />
        )}
      </Opt>
    </section>
  );
}

function AdvancedGroup({ engine: e, save }: { engine: EngineInfo; save: Save }) {
  const adopt = useAppStore((s) => s.adoptEngines);
  const [argsText, setArgsText] = useState(e.extra_args.join("\n"));
  const [rows, setRows] = useState(() => Object.entries(e.env).map(([name, value]) => ({ name, value })));
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const custom = !e.builtin;

  const commitEnv = (next: { name: string; value: string }[]) => save({ env: envToMap(next) });

  async function remove() {
    setConfirming(false);
    try {
      adopt(await removeEngine(e.id));
    } catch (err) {
      setError(errText(err));
    }
  }

  return (
    <details className="engines-group engines-advanced">
      <summary className="engines-group-title">Advanced</summary>
      <label className="engines-block">
        <span className="engines-label">Extra arguments (one per line)</span>
        <textarea
          className="field-input"
          rows={3}
          spellCheck={false}
          value={argsText}
          onChange={(ev) => setArgsText(ev.target.value)}
          onBlur={() => {
            const next = linesToArgs(argsText);
            if (JSON.stringify(next) !== JSON.stringify(e.extra_args)) save({ extra_args: next });
          }}
        />
      </label>
      <div className="engines-block">
        <span className="engines-label">Environment</span>
        {rows.map((r, i) => (
          <div className="engines-envrow" key={i}>
            <input
              className="field-input"
              aria-label="Variable name"
              placeholder="NAME"
              value={r.name}
              onChange={(ev) => setRows(rows.map((x, j) => (j === i ? { ...x, name: ev.target.value } : x)))}
              onBlur={() => commitEnv(rows)}
            />
            <input
              className="field-input"
              aria-label="Variable value"
              placeholder="value"
              value={r.value}
              onChange={(ev) => setRows(rows.map((x, j) => (j === i ? { ...x, value: ev.target.value } : x)))}
              onBlur={() => commitEnv(rows)}
            />
            <button
              className="btn-text"
              aria-label="Remove variable"
              onClick={() => {
                const next = rows.filter((_, j) => j !== i);
                setRows(next);
                commitEnv(next);
              }}
            >
              Remove
            </button>
          </div>
        ))}
        <button className="btn-text" onClick={() => setRows([...rows, { name: "", value: "" }])}>
          + Add a variable
        </button>
        <p className="engines-muted">
          For keys, use Billing → An API key, so I keep them in the vault. A name that looks like a secret is
          refused.
        </p>
      </div>
      {custom && (
        <div className="engines-block">
          <span className="engines-label">Name</span>
          <input
            className="field-input"
            aria-label="Name"
            defaultValue={e.name}
            onBlur={(ev) => ev.target.value.trim() !== e.name && save({ display_name: ev.target.value })}
          />
          <span className="engines-label">Start arguments (one per line)</span>
          <textarea
            className="field-input"
            rows={2}
            spellCheck={false}
            aria-label="Start arguments"
            defaultValue={e.args.join("\n")}
            onBlur={(ev) => save({ args: linesToArgs(ev.target.value) })}
          />
          <button className="btn-text danger" onClick={() => setConfirming(true)}>
            Remove this agent
          </button>
        </div>
      )}
      {error && (
        <p className="engines-error" role="alert">
          {error}
        </p>
      )}
      {confirming && (
        <ConfirmDialog
          title={`Remove ${e.name}?`}
          body="I forget it and its saved settings. The app itself stays on your computer."
          confirmLabel="Remove"
          onConfirm={remove}
          onCancel={() => setConfirming(false)}
        />
      )}
    </details>
  );
}

function AddAcp({
  onDone,
  known,
  adopt,
}: {
  onDone: (newId: string | null) => void;
  known: string[];
  adopt: (list: EngineInfo[]) => void;
}) {
  const [name, setName] = useState("");
  const [program, setProgram] = useState("");
  const [args, setArgs] = useState("acp");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function browse() {
    const picked = await openDialog({ multiple: false, filters: [{ name: "Program", extensions: ["exe", "cmd", "bat"] }] });
    if (typeof picked === "string") setProgram(picked);
  }

  async function add() {
    setBusy(true);
    setError(null);
    try {
      const list = await addAcpEngine(name, program, linesToArgs(args.replace(/ +/g, "\n")));
      adopt(list);
      onDone(list.find((x) => !known.includes(x.id))?.id ?? null);
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="engines-addcard">
      <h3 className="engines-group-title">Add another agent</h3>
      <p className="engines-muted">
        Any app that speaks the Agent Client Protocol. I run it the way you set it here, and I can only
        find it again by this path.
      </p>
      <label className="engines-block">
        <span className="engines-label">Name</span>
        <input className="field-input" value={name} onChange={(ev) => setName(ev.target.value)} />
      </label>
      <div className="engines-block">
        <span className="engines-label">Program</span>
        <div className="engines-keyform">
          <input
            className="field-input"
            aria-label="Program"
            value={program}
            placeholder="C:\\path\\to\\agent.exe"
            onChange={(ev) => setProgram(ev.target.value)}
          />
          <button className="btn-secondary" onClick={browse}>
            Browse…
          </button>
        </div>
      </div>
      <label className="engines-block">
        <span className="engines-label">Arguments</span>
        <input className="field-input" value={args} onChange={(ev) => setArgs(ev.target.value)} />
      </label>
      {error && (
        <p className="engines-error" role="alert">
          {error}
        </p>
      )}
      <div className="engines-actions">
        <button className="btn-primary" onClick={add} disabled={busy || !name.trim() || !program.trim()}>
          {busy ? "Checking…" : "Add and check"}
        </button>
        <button className="btn-text" onClick={() => onDone(null)}>
          Cancel
        </button>
      </div>
    </div>
  );
}

const PARALLEL_OPTIONS = [1, 2, 3, 4].map((n) => ({ value: String(n), label: String(n) }));
const TIMEOUT_OPTIONS = [
  { value: "300", label: "5 min" },
  { value: "900", label: "15 min" },
  { value: "1800", label: "30 min" },
  { value: "3600", label: "1 hour" },
  { value: "7200", label: "2 hours" },
];

/** Limits for all external agents (`CLI-12`, the global keys). */
function Limits() {
  const [parallel, setParallel] = useState("2");
  const [timeout, setTimeoutSecs] = useState("1800");
  const [detect, setDetect] = useState(true);

  useEffect(() => {
    if (!inTauri()) return;
    getSetting("engines.max_parallel").then((v) => v && setParallel(v)).catch(() => {});
    getSetting("engines.timeout_secs").then((v) => v && setTimeoutSecs(v)).catch(() => {});
    getSetting("engines.detect_on_start").then((v) => setDetect(v !== "false")).catch(() => {});
  }, []);

  const timeoutOptions = TIMEOUT_OPTIONS.some((o) => o.value === timeout)
    ? TIMEOUT_OPTIONS
    : [...TIMEOUT_OPTIONS, { value: timeout, label: `${Math.round(Number(timeout) / 60)} min` }];

  return (
    <div className="engines-limits">
      <h2 className="section-title">Limits for all external agents</h2>
      <div className="engines-panel">
        <Opt title="At once" help="How many external agents may run at the same time.">
          <Select
            label="How many external agents may run at once"
            value={parallel}
            options={PARALLEL_OPTIONS}
            onChange={(v) => {
              setParallel(v);
              setSetting("engines.max_parallel", v).catch(() => {});
            }}
          />
        </Opt>
        <Opt title="Longest job" help="A job that runs longer than this is stopped.">
          <Select
            label="How long one external agent may run"
            value={timeout}
            options={timeoutOptions}
            onChange={(v) => {
              setTimeoutSecs(v);
              setSetting("engines.timeout_secs", v).catch(() => {});
            }}
          />
        </Opt>
        <Opt as="label" title="Look for new agent apps when I start" help="A new one is only listed. It stays off until you turn it on.">
          <Toggle
            checked={detect}
            onChange={(on) => {
              setDetect(on);
              setSetting("engines.detect_on_start", on ? "true" : "false").catch(() => {});
            }}
          />
        </Opt>
      </div>
    </div>
  );
}
