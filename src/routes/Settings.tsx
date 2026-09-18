import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  inTauri,
  listPermissions,
  addPermission,
  revokePermission,
  listCapabilityGrants,
  revokeCapabilityGrant,
  embedEngineStatus,
  rerankEngineStatus,
  installRerankEngine,
  setRerankEnabled,
  type Grant,
  type CapabilityGrant,
  type RerankSetupStatus,
  type DownloadProgress,
} from "../lib/api";
import { useAppStore, READING_SCALES, type PlanMode } from "../lib/store";
import PersonaEditor from "../components/Personas/PersonaEditor";
import "./Surface.css";
import "./Settings.css";

/** `SMP-3`: Simple mode's single Recall control. The embedder and the
 * reranker are two engines and two downloads underneath, but to a Simple-mode
 * user they're one thing with a quality choice — `Good` (the default) or
 * `Sharper`, which installs and enables the reranker through the same flow.
 * Expert mode has no use for this: the full Runtime → Recall tab already
 * covers both cards individually (`RRK-UI-1`/`2`/`3`), so this only renders
 * for `!expert` in `Settings`, below. Hidden entirely until recall itself is
 * installed (`SMP-2`) — there's nothing here to sharpen yet. */
function RecallModeControl() {
  const [embedReady, setEmbedReady] = useState(false);
  const [rerank, setRerank] = useState<RerankSetupStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [prog, setProg] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      const [e, r] = await Promise.all([embedEngineStatus(), rerankEngineStatus()]);
      setEmbedReady(!!e.model_installed && !!e.engine_installed);
      setRerank(r);
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function chooseGood() {
    setBusy(true);
    setError(null);
    try {
      await setRerankEnabled(false);
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(false);
    }
  }

  async function chooseSharper() {
    setBusy(true);
    setError(null);
    try {
      if (!rerank?.model_installed) {
        await installRerankEngine((p) => setProg(p));
      }
      await setRerankEnabled(true);
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(false);
      setProg(null);
    }
  }

  if (!embedReady) return null;

  const sharper = !!rerank?.enabled;
  const pct = prog?.total ? Math.round((prog.received / prog.total) * 100) : null;

  return (
    <section className="setting-block">
      <h2 className="setting-title">Recall</h2>
      <p className="setting-help">How carefully I re-read what I find before answering from it.</p>
      {error && <p className="hw-note error">{error}</p>}
      {busy && prog ? (
        <div className="dl-progress wide">
          <div className="dl-bar" style={{ width: pct !== null ? `${pct}%` : "40%" }} />
          <span className="dl-pct">{prog.label}</span>
        </div>
      ) : (
        <div className="setting-actions" role="group" aria-label="Recall quality">
          <button
            className={`btn-secondary ${!sharper ? "selected" : ""}`}
            aria-pressed={!sharper}
            onClick={chooseGood}
            disabled={busy}
          >
            Good
          </button>
          <button
            className={`btn-secondary ${sharper ? "selected" : ""}`}
            aria-pressed={sharper}
            onClick={chooseSharper}
            disabled={busy}
          >
            Sharper
          </button>
        </div>
      )}
      <p className="runtime-hint">
        {sharper
          ? "Re-reads the closest matches before answering. A little slower, and another 540 MB."
          : "Matches by meaning. Fast, and usually enough."}
      </p>
    </section>
  );
}

/** `PLN-UI-4`: whether I work out a plan before starting.
 *
 * It belongs here rather than under Tools: it is not a capability you grant,
 * it is how I go about the work — the same kind of choice as the persona above
 * it. Nothing gets switched on or off by it; only whether I say where I am
 * going before I set off.
 *
 * A segmented group rather than a dropdown, matching Reading size below: three
 * options that all fit on one line should show all three, so the choice can be
 * made in one click and the alternatives are readable without opening
 * anything.
 *
 * *When it helps* is the default and should stay it. Always is noise on "what
 * is 2+2" and costs a round trip before any work starts; never is the right
 * answer for a small model that writes junk plans.
 */
function PlanFirst() {
  const planMode = useAppStore((s) => s.planMode);
  const setPlanMode = useAppStore((s) => s.setPlanMode);

  const options: { value: PlanMode; label: string }[] = [
    { value: "always", label: "Always" },
    { value: "auto", label: "When it helps" },
    { value: "never", label: "Never" },
  ];

  return (
    <section className="setting-block">
      <h2 className="setting-title">Planning the work</h2>
      <p className="setting-help">
        When a job has several parts I can write down what I mean to do, show you the list, and
        work through it — so you can see where I am and steer me while I am still going. Writing
        one costs a round trip before any work starts, so on <em>when it helps</em> I only do it
        for jobs big enough to be worth it.
      </p>
      <div className="setting-actions" role="group" aria-label="Planning the work">
        {options.map((o) => (
          <button
            key={o.value}
            className={`btn-secondary ${planMode === o.value ? "selected" : ""}`}
            aria-pressed={planMode === o.value}
            onClick={() => void setPlanMode(o.value)}
          >
            {o.label}
          </button>
        ))}
      </div>
    </section>
  );
}

export default function Settings() {
  const systemPrompt = useAppStore((s) => s.systemPrompt);
  const setView = useAppStore((s) => s.setView);
  const setSystemPrompt = useAppStore((s) => s.setSystemPrompt);
  const [draft, setDraft] = useState(systemPrompt);
  const [saved, setSaved] = useState(false);
  const [grants, setGrants] = useState<Grant[]>([]);
  const [capabilityGrants, setCapabilityGrants] = useState<CapabilityGrant[]>([]);
  const openRuntime = useAppStore((s) => s.openRuntime);
  const mode = useAppStore((s) => s.mode);
  const setMode = useAppStore((s) => s.setMode);
  const readingScale = useAppStore((s) => s.readingScale);
  const setReadingScale = useAppStore((s) => s.setReadingScale);
  const contextBudget = useAppStore((s) => s.contextBudget);
  const autoCompact = useAppStore((s) => s.autoCompact);
  const setAutoCompact = useAppStore((s) => s.setAutoCompact);
  const expert = useAppStore((s) => s.expert);
  const setExpert = useAppStore((s) => s.setExpert);
  const resetFirstTimeExplanations = useAppStore((s) => s.resetFirstTimeExplanations);

  useEffect(() => setDraft(systemPrompt), [systemPrompt]);
  useEffect(() => {
    if (!inTauri()) return;
    refreshPermissions();
    listCapabilityGrants().then(setCapabilityGrants).catch(() => {});
  }, []);

  function refreshPermissions() {
    listPermissions().then(setGrants).catch(() => {});
  }

  async function save() {
    await setSystemPrompt(draft);
    setSaved(true);
    setTimeout(() => setSaved(false), 1600);
  }

  async function addFolder(mode: "read" | "read-write") {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") {
      await addPermission(dir, mode);
      refreshPermissions();
    }
  }

  async function revoke(id: string) {
    await revokePermission(id);
    refreshPermissions();
  }

  async function revokeCapability(id: string) {
    await revokeCapabilityGrant(id);
    setCapabilityGrants((gs) => gs.filter((g) => g.id !== id));
  }

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>General</h1>
        <p className="lede">Your system prompt, file access, and a log of what Poiesis Agent has done.</p>

        <section className="setting-block">
          <h2 className="setting-title">System prompt</h2>
          <p className="setting-help">
            Sets how Poiesis Agent behaves across every chat. One global prompt for now; saved profiles
            come later.
          </p>
          <textarea
            className="system-prompt"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            rows={5}
            spellCheck={false}
          />
          <div className="setting-actions">
            <button className="btn-primary" onClick={save} disabled={draft === systemPrompt}>
              Save
            </button>
            {saved && <span className="saved-note">Saved</span>}
          </div>
        </section>

        {inTauri() && (
          <section className="setting-block" id="settings-personas">
            <h2 className="setting-title">Personas</h2>
            <p className="setting-help">
              Saved profiles that bundle a system prompt (and optionally a model and temperature).
              Pick one per chat from the composer; the global prompt above is the fallback.
            </p>
            <PersonaEditor />
          </section>
        )}

        {inTauri() && <PlanFirst />}

        <section className="setting-block">
          <h2 className="setting-title">Interface</h2>
          <p className="setting-help">
            Poiesis Agent arrives with everything switched on but most of the machinery out of sight.
          </p>
          <label className="toggle-line">
            <input
              type="checkbox"
              checked={expert}
              onChange={(e) => setExpert(e.target.checked)}
            />
            <span>
              Show me everything — every engine, every control, every setting I usually keep out
              of your way
            </span>
          </label>
          {expert && (
            <div className="setting-actions">
              <button className="btn-text" onClick={() => resetFirstTimeExplanations()}>
                Explain things to me again
              </button>
            </div>
          )}
        </section>

        {!expert && <RecallModeControl />}

        <section className="setting-block">
          <h2 className="setting-title">Theme</h2>
          <p className="setting-help">
            Switch between light and dark appearance.
          </p>
          <div className="setting-actions" role="group" aria-label="Color theme">
            <button
              className={`btn-secondary ${mode === "light" ? "selected" : ""}`}
              aria-pressed={mode === "light"}
              onClick={() => setMode("light")}
            >
              Daylight
            </button>
            <button
              className={`btn-secondary ${mode === "dark" ? "selected" : ""}`}
              aria-pressed={mode === "dark"}
              onClick={() => setMode("dark")}
            >
              Backlit
            </button>
          </div>
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Reading size</h2>
          <p className="setting-help">
            Scales the conversation — your messages, replies, steps and cards together.
            Text reflows — nothing is cut off.
          </p>
          <div className="setting-actions" role="group" aria-label="Reading size">
            {READING_SCALES.map((opt) => (
              <button
                key={opt.value}
                className={`btn-secondary ${readingScale === opt.value ? "selected" : ""}`}
                aria-pressed={readingScale === opt.value}
                onClick={() => setReadingScale(opt.value)}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </section>

        {/* `PRV-1`: keys and servers moved out. A pointer for one release, so
            nobody who knew where they were is left searching. */}
        {inTauri() && (
          <section className="setting-block">
            <h2 className="setting-title">Cloud keys and model servers</h2>
            <p className="setting-help">
              Cloud keys moved to{" "}
              <button className="link-button inline" onClick={() => setView("providers")}>
                Providers →
              </button>
              , model servers to{" "}
              <button className="link-button inline" onClick={() => openRuntime("servers")}>
                Runtime →
              </button>
            </p>
          </section>
        )}

        <section className="setting-block">
          <h2 className="setting-title">Memory &amp; context</h2>
          <p className="setting-help">
            A model can only hold so much of a conversation at once. When a chat outgrows that,
            Poiesis Agent summarizes the older turns instead of letting them fall off the front.
            Your messages are never deleted — this changes only what the model is shown.
          </p>
          <p className="setting-readout">
            Model context window: {contextBudget.toLocaleString()} tokens
          </p>
          <label className="toggle-line">
            <input
              type="checkbox"
              checked={autoCompact}
              onChange={(e) => setAutoCompact(e.target.checked)}
            />
            <span>Summarize older turns automatically</span>
          </label>
          {/* PRES-3: the self is a place of its own; Settings only points to it. */}
          <button className="settings-self-link" onClick={() => setView("self")}>
            Memory, lessons and autonomy live in my Self panel →
          </button>
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Always-allowed folders</h2>
          <p className="setting-help">
            Folders Poiesis Agent may reach in every chat. Most work happens in a single working
            folder you attach to a conversation from the Workbench panel — these are the standing
            exceptions on top of it.
          </p>
          {grants.length === 0 && <p className="empty-hint">No folders allowed yet.</p>}
          {grants.map((g) => (
            <div className="grant-row" key={g.id}>
              <span className="grant-path">{g.path}</span>
              <span className="grant-mode">{g.mode === "read-write" ? "read & write" : "read"}</span>
              <button className="grant-revoke" onClick={() => revoke(g.id)}>
                Remove
              </button>
            </div>
          ))}
          {inTauri() && (
            <div className="setting-actions">
              <button className="btn-secondary" onClick={() => addFolder("read")}>
                Add a folder (read)
              </button>
              <button className="btn-secondary" onClick={() => addFolder("read-write")}>
                Add a folder (read &amp; write)
              </button>
            </div>
          )}
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Always-allowed sites &amp; apps</h2>
          <p className="setting-help">
            Domains and applications you told Poiesis Agent it never needs to ask about again —
            from the Browser and Screen &amp; apps tools.
          </p>
          {capabilityGrants.length === 0 && <p className="empty-hint">Nothing standing yet.</p>}
          {capabilityGrants.map((g) => (
            <div className="grant-row" key={g.id}>
              <span className="grant-path">{g.value}</span>
              <span className="grant-mode">{g.kind === "domain" ? "site" : "app"}</span>
              <button className="grant-revoke" onClick={() => revokeCapability(g.id)}>
                Remove
              </button>
            </div>
          ))}
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Activity</h2>
          <p className="setting-help">
            The log of everything Poiesis Agent did on your computer now has its own section.
          </p>
          <button className="btn-secondary" onClick={() => setView("activity")}>
            Open Activity
          </button>
        </section>
      </div>
    </div>
  );
}
