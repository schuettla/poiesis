import { useEffect, useMemo, useState } from "react";
import {
  addEngineRule,
  deleteEngineRule,
  inTauri,
  prematchCounts,
  previewEngineRoute,
  reorderEngineRules,
  resetEngineRules,
  updateEngineRule,
  type EngineGuards,
  type EngineInfo,
  type EngineRoutePreview,
  type EngineRule,
  type EngineUseMode,
  type PrematchCount,
} from "../../lib/api";
import { answeredBy, matchCounter, MODE_LABELS, modeHelp, previewLine, ruleLooksWrong } from "../../lib/engines";
import { useAppStore } from "../../lib/store";
import ConfirmDialog from "../Confirm/ConfirmDialog";
import ProposalCard, { parseRuleChange } from "../Conversation/ProposalCard";
import Select from "../Select/Select";
import Toggle from "../Toggle/Toggle";
import Opt from "./Opt";
import "./RuleList.css";

/**
 * `RTE-UI-1`: **When I use it**. The user's rules for one agent app, in words,
 * and the three ways to switch the whole thing: only when asked, suggest, on its
 * own. The conditions under them are checked in Rust whatever the model says
 * (`RTE-3`), so they read as facts, not as advice.
 */

/** Long enough for a sentence, short enough to sit in the lead's prompt (`RTE-2`). */
export const RULE_MAX = 140;
const RULES_MAX = 24;

const MODES: EngineUseMode[] = ["on_request", "suggest", "auto"];
const errText = (e: unknown) => String(e).replace(/^Error:\s*/, "");

const KIND_TITLE = { use: "Use it for", avoid: "Don't use it for" } as const;
const KIND_HINT = {
  use: "Describe the work in your own words, for example: a change across many files.",
  avoid: "Describe what I should keep for myself, for example: quick questions.",
} as const;

const PER_CHAT = [1, 2, 3, 5, 10, 20];

export default function RuleList({ engine: e }: { engine: EngineInfo }) {
  const updateEngine = useAppStore((s) => s.updateEngine);
  const adopt = useAppStore((s) => s.adoptEngines);
  const proposals = useAppStore((s) => s.changeProposals);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  // `USE-UI-10`: how each rule fared when a quick decision matched it early.
  const [early, setEarly] = useState<Record<string, PrematchCount>>({});
  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    Promise.resolve()
      .then(() => prematchCounts())
      .then((rows) => live && setEarly(Object.fromEntries(rows.map((r) => [r.rule, r]))))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [e.id]);

  // `RTE-6`: changes I have suggested to this agent's rules, waiting for a yes.
  const suggested = useMemo(
    () => proposals.filter((p) => p.target === "engine-rule" && parseRuleChange(p.proposed_text)?.engine_id === e.id),
    [proposals, e.id]
  );

  const rules = [...e.rules].sort((a, b) => a.position - b.position);

  /** Every change goes through here: the backend answers with the whole list. */
  async function run(work: () => Promise<EngineInfo[]>) {
    setError(null);
    try {
      adopt(await work());
    } catch (err) {
      setError(errText(err));
    }
  }

  async function saveGuards(next: Partial<EngineGuards>) {
    setError(null);
    try {
      await updateEngine(e.id, { guards: { ...e.guards, ...next } });
    } catch (err) {
      setError(errText(err));
    }
  }

  /** Swap a rule with its neighbour of the same kind; the order is one list. */
  function move(rule: EngineRule, by: -1 | 1) {
    const same = rules.filter((r) => r.kind === rule.kind);
    const at = same.findIndex((r) => r.id === rule.id);
    const other = same[at + by];
    if (!other) return;
    const ids = rules.map((r) => r.id);
    const i = ids.indexOf(rule.id);
    const j = ids.indexOf(other.id);
    [ids[i], ids[j]] = [ids[j], ids[i]];
    void run(() => reorderEngineRules(e.id, ids));
  }

  if (!e.delegate_ready) {
    return (
      <section className="engines-group" aria-label="When I use it">
        <h3 className="engines-group-title">When I use it</h3>
        <p className="engines-muted">
          I can&apos;t hand work to {e.name} yet. It is set up and testable here.
        </p>
      </section>
    );
  }

  const perChat = PER_CHAT.includes(e.guards.max_per_conversation)
    ? PER_CHAT
    : [...PER_CHAT, e.guards.max_per_conversation].sort((a, b) => a - b);

  return (
    <section className="engines-group" aria-label="When I use it">
      <h3 className="engines-group-title">When I use it</h3>

      {suggested.map((p) => (
        <ProposalCard key={p.id} id={p.id} />
      ))}

      <div className="segmented rule-modes" role="radiogroup" aria-label={`When I use ${e.name}`}>
        {MODES.map((m) => (
          <button
            key={m}
            role="radio"
            aria-checked={e.use_mode === m}
            className={`segmented-item ${e.use_mode === m ? "on" : ""}`}
            onClick={() => e.use_mode !== m && void updateEngine(e.id, { use_mode: m }).catch((err) => setError(errText(err)))}
          >
            {MODE_LABELS[m]}
          </button>
        ))}
      </div>
      <p className="engines-muted">{modeHelp(e.use_mode, e.name, e.id)}</p>

      {(["use", "avoid"] as const).map((kind) => {
        const list = rules.filter((r) => r.kind === kind);
        return (
          <div className="rule-block" key={kind}>
            <h4 className="rule-title">{KIND_TITLE[kind]}</h4>
            {list.length === 0 && <p className="engines-muted">None yet.</p>}
            <ul className="rule-list">
              {list.map((r, i) => (
                <RuleItem
                  key={r.id}
                  rule={r}
                  early={early[`${e.id}:${r.label}`]}
                  first={i === 0}
                  last={i === list.length - 1}
                  onMove={(by) => move(r, by)}
                  run={run}
                />
              ))}
            </ul>
            {rules.length < RULES_MAX && (
              <AddRule
                kind={kind}
                hint={KIND_HINT[kind]}
                onAdd={(text) => run(() => addEngineRule(e.id, kind, text))}
              />
            )}
          </div>
        );
      })}

      <div className="rule-block">
        <h4 className="rule-title">Always checked, whatever I decide</h4>
        <div className="rule-guards">
          <Opt
            className="locked"
            title="In a folder"
            help="Agents always work in a folder, so I never hand work over in a chat without one."
            hint="Agents always work in a folder"
          >
            <span className="engines-tag">Always</span>
          </Opt>
          <Opt
            as="label"
            title="Code projects only"
            help="Only in a folder that looks like a code project."
          >
            <Toggle
              checked={e.guards.code_projects_only}
              onChange={(on) => void saveGuards({ code_projects_only: on })}
            />
          </Opt>
          <Opt
            as="label"
            title="Folder not read-only"
            help="Not in a folder where I may only read."
          >
            <Toggle
              checked={e.guards.min_trust !== "read-only"}
              onChange={(on) => void saveGuards({ min_trust: on ? "confirm" : "read-only" })}
            />
          </Opt>
          <Opt title="Jobs per chat" help="The most it is given in one conversation.">
            <Select
              label={`${e.name}: jobs per chat`}
              value={String(e.guards.max_per_conversation)}
              options={perChat.map((n) => ({ value: String(n), label: String(n) }))}
              onChange={(v) => void saveGuards({ max_per_conversation: Number(v) })}
            />
          </Opt>
        </div>
      </div>

      <div className="rule-foot">
        <button className="btn-text" onClick={() => setConfirming(true)}>
          Reset to defaults
        </button>
      </div>
      {error && (
        <p className="engines-error" role="alert">
          {error}
        </p>
      )}
      {confirming && (
        <ConfirmDialog
          title={`Reset the rules for ${e.name}?`}
          body="I put back my own rules and the three settings above, and forget the ones you wrote."
          confirmLabel="Reset"
          onConfirm={() => {
            setConfirming(false);
            void run(() => resetEngineRules(e.id));
          }}
          onCancel={() => setConfirming(false)}
        />
      )}
    </section>
  );
}

function RuleItem({
  rule: r,
  early,
  first,
  last,
  onMove,
  run,
}: {
  rule: EngineRule;
  early?: PrematchCount;
  first: boolean;
  last: boolean;
  onMove: (by: -1 | 1) => void;
  run: (work: () => Promise<EngineInfo[]>) => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(r.text);
  const other = r.kind === "use" ? "avoid" : "use";

  const commit = () => {
    const text = draft.trim();
    setEditing(false);
    if (!text || text === r.text) {
      setDraft(r.text);
      return;
    }
    void run(() => updateEngineRule(r.id, { text }));
  };

  return (
    <li className={`rule-item ${r.enabled ? "" : "off"}`}>
      <span className="rule-label">{r.label}</span>
      <div className="rule-text">
        {editing ? (
          <div className="rule-edit">
            <input
              className="field-input"
              autoFocus
              aria-label={`Rule ${r.label}`}
              value={draft}
              maxLength={RULE_MAX}
              onChange={(ev) => setDraft(ev.target.value)}
              onKeyDown={(ev) => {
                if (ev.key === "Enter") commit();
                if (ev.key === "Escape") {
                  setDraft(r.text);
                  setEditing(false);
                }
              }}
            />
            <span className="rule-count" aria-live="polite">
              {draft.length}/{RULE_MAX}
            </span>
          </div>
        ) : (
          <button className="rule-words" onClick={() => setEditing(true)} title="Edit">
            {r.text}
          </button>
        )}
        <div className="rule-meta">
          {r.source === "proposed" && <span className="rule-tag">suggested by me</span>}
          {r.auto && (
            <button
              className="rule-tag auto"
              title="A handover for this rule starts without asking. Press to ask again."
              onClick={() => void run(() => updateEngineRule(r.id, { auto: false }))}
            >
              starts without asking ×
            </button>
          )}
          {r.hits + r.overrides > 0 && (
            <span className={`rule-uses ${ruleLooksWrong(r) ? "warn" : ""}`}>
              used {r.hits} · declined {r.overrides}
              {ruleLooksWrong(r) && " · maybe reword it"}
            </span>
          )}
          {matchCounter(early) && <span className="rule-uses">{matchCounter(early)}</span>}
        </div>
        {!editing && (
          <div className="rule-tools">
            <button className="btn-text" aria-label={`Move ${r.label} up`} disabled={first} onClick={() => onMove(-1)}>
              ↑
            </button>
            <button className="btn-text" aria-label={`Move ${r.label} down`} disabled={last} onClick={() => onMove(1)}>
              ↓
            </button>
            <button className="btn-text" onClick={() => void run(() => updateEngineRule(r.id, { kind: other }))}>
              Move to {KIND_TITLE[other].toLowerCase()}
            </button>
            <button className="btn-text danger" onClick={() => void run(() => deleteEngineRule(r.id))}>
              Delete
            </button>
          </div>
        )}
      </div>
      <label className="rule-on" title={r.enabled ? "On" : "Off"}>
        <Toggle
          checked={r.enabled}
          label={`Rule ${r.label} is on`}
          onChange={(on) => void run(() => updateEngineRule(r.id, { enabled: on }))}
        />
      </label>
    </li>
  );
}

function AddRule({ kind, hint, onAdd }: { kind: "use" | "avoid"; hint: string; onAdd: (text: string) => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");

  const add = async () => {
    const t = text.trim();
    if (!t) return;
    await onAdd(t);
    setText("");
    setOpen(false);
  };

  if (!open) {
    return (
      <button className="btn-text rule-add" onClick={() => setOpen(true)}>
        + Add a rule
      </button>
    );
  }
  return (
    <div className="rule-edit">
      <input
        className="field-input"
        autoFocus
        aria-label={`New rule: ${KIND_TITLE[kind]}`}
        placeholder={hint}
        value={text}
        maxLength={RULE_MAX}
        onChange={(ev) => setText(ev.target.value)}
        onKeyDown={(ev) => {
          if (ev.key === "Enter") void add();
          if (ev.key === "Escape") {
            setText("");
            setOpen(false);
          }
        }}
      />
      <span className="rule-count" aria-live="polite">
        {text.length}/{RULE_MAX}
      </span>
      <button className="btn-secondary" onClick={() => void add()} disabled={!text.trim()}>
        Add
      </button>
    </div>
  );
}

/** `CLI-10`: how the last jobs went, in one line, with the agent's own last
 * words behind a fold. Nothing in it is stored on disk. */
export function RecentJobs({ engine: e }: { engine: EngineInfo }) {
  const d = e.diagnostics;
  if (!e.delegate_ready) return null;
  const line =
    d.runs === 0
      ? "No jobs since I started."
      : `${d.runs} ${d.runs === 1 ? "job" : "jobs"} since I started, ${d.failed} failed` +
        (d.unknown_events > 0 ? `, ${d.unknown_events} messages I didn't understand` : "") +
        ".";
  return (
    <section className="engines-group" aria-label="Recent jobs">
      <h3 className="engines-group-title">Recent jobs</h3>
      <p className="engines-muted">{line}</p>
      {/* `CLI-UI-8`: the early warning that an update changed how it talks. */}
      {d.unknown_events > 0 && (
        <p className="engines-muted warn">
          If this number keeps rising after an update, {e.name} now talks to me differently. It still runs, and I
          count what I don't understand instead of guessing.
        </p>
      )}
      {d.last_error && <p className="engines-muted">Last problem: {d.last_error}</p>}
      {d.last_stderr && (
        <details className="rule-stderr">
          <summary>What it printed</summary>
          <pre>{d.last_stderr}</pre>
        </details>
      )}
    </section>
  );
}

/** `RTE-UI-4`: type a request, see what I would do with it. One cheap call on the
 * model the user picked; nothing is started. */
export function TryRequest() {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [answer, setAnswer] = useState<EngineRoutePreview | null>(null);
  const [error, setError] = useState<string | null>(null);

  const ask = async () => {
    const t = text.trim();
    if (!t || busy) return;
    setBusy(true);
    setError(null);
    setAnswer(null);
    try {
      setAnswer(await previewEngineRoute(t));
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="engines-limits rule-try">
      <h2 className="section-title">Try a request</h2>
      <p className="engines-muted">
        Type something you might ask me. I&apos;ll say who I would give it to and why. Nothing starts.
      </p>
      <div className="rule-edit">
        <input
          className="field-input"
          aria-label="A request to try"
          placeholder="Rename the config options across the repo"
          value={text}
          onChange={(ev) => setText(ev.target.value)}
          onKeyDown={(ev) => ev.key === "Enter" && void ask()}
        />
        <button className="btn-secondary" onClick={() => void ask()} disabled={busy || !text.trim()}>
          {busy ? "Thinking…" : "Try"}
        </button>
      </div>
      {answer && (
        <p className="engines-value" role="status">
          {previewLine(answer)}
          {answer.rule_text && <span className="engines-muted"> “{answer.rule_text}”</span>}
          <span className="engines-muted"> {answeredBy(answer)}</span>
        </p>
      )}
      {error && (
        <p className="engines-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
