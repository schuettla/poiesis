import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { AUTONOMY_CLASSES, useAppStore } from "../../lib/store";
import { firesLine, habitLine, learningLines } from "../../lib/learning";
import { Evidence, LetGo, RecentChanges } from "../Memory/BeliefSections";
import MemoryPanel from "../Memory/MemoryPanel";
import ProposalCard from "../Conversation/ProposalCard";
import TriedFixes from "./TriedFixes";
import WaysTab from "./WaysTab";
import "../../routes/Models.css";
import "../Context/Context.css";
import "./Self.css";

type Tab = "memory" | "lessons" | "health" | "autonomy" | "ways";

const TABS: { id: Tab; label: string }[] = [
  { id: "memory", label: "Memory" },
  { id: "lessons", label: "Lessons" },
  { id: "health", label: "Health" },
  { id: "autonomy", label: "Autonomy" },
  { id: "ways", label: "How I work" },
];

const RUNG_LABELS: Record<string, string> = {
  auto: "Auto with undo",
  ask: "Ask first",
  off: "Off",
};

function formatDate(at: number | null): string {
  if (!at) return "never";
  return new Date(at).toLocaleDateString();
}

/**
 * The Self view's body (ORG-UI-1): everything Poiesis is made of, in four
 * plain tabs. Counts and words only — no gauges, no green/red. A page about a
 * self, not a server dashboard.
 */
export default function SelfPanel() {
  const [tab, setTab] = useState<Tab>("memory");
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const openContextPanel = useAppStore((s) => s.openContextPanel);
  // `/autonomy` and `/self` land on a tab, not just on the page.
  const tabRequest = useAppStore((s) => s.selfTabRequest);
  useEffect(() => {
    if (tabRequest) setTab(tabRequest.tab);
  }, [tabRequest]);

  return (
    <>
      <div className="self-toolbar">
        {/* The segmented control the rest of the app uses for view switching
            (Models, Engine) — Self is a page like those, so it switches like them. */}
        <div className="model-tabs" role="tablist" aria-label="What I'm made of">
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={tab === t.id}
              className={`model-tab ${tab === t.id ? "on" : ""}`}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
        {activeConversationId && (
          <button
            className="why-link self-why-link"
            onClick={() => openContextPanel({ conversationId: activeConversationId })}
          >
            What I'm working from
          </button>
        )}
      </div>

      <div className="self-tabpanel" role="tabpanel">
        {tab === "memory" && <MemoryPanel />}
        {tab === "lessons" && <LessonsTab />}
        {tab === "health" && <HealthTab />}
        {tab === "autonomy" && <AutonomyTab />}
        {tab === "ways" && <WaysTab />}
      </div>
    </>
  );
}

/** REF-UI-1: what I learned from my own mistakes, and — when the `lessons` rung
 * is set to ask-first — what I'd like to learn but haven't. */
function LessonsTab() {
  const lessons = useAppStore((s) => s.lessons);
  const forget = useAppStore((s) => s.forgetLesson);
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);
  const setView = useAppStore((s) => s.setView);
  const proposals = useAppStore((s) => s.changeProposals);
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const refreshSelf = useAppStore((s) => s.refreshSelf);
  const [expanded, setExpanded] = useState<string | null>(null);
  // Reloads the lists that read their own source after a lesson is forgotten or brought back.
  const [version, setVersion] = useState(0);
  useEffect(() => setVersion((v) => v + 1), [lessons]);

  const pending = proposals.filter(
    (p) =>
      p.target === "lesson" ||
      p.target === "lesson-critic" ||
      p.target === "engine-memory" ||
      p.target === "habit" ||
      // `IMP-4`: a line for a project's instructions is answered where it can be set.
      (p.target === "project_instructions" && !!p.group_id)
  );
  const [improve, setImprove] = useState<api.ImproveSummary | null>(null);
  useEffect(() => {
    if (!api.inTauri()) return;
    api.improveSummary().then(setImprove).catch(() => setImprove(null));
  }, [version, proposals]);

  async function dropRule(key: string) {
    await api.deleteImproveRule(key);
    setImprove(await api.improveSummary());
  }
  const standing = useAppStore((s) => s.makeStandingInstead);
  const [proposalError, setProposalError] = useState("");

  async function standingInstead(id: string) {
    setProposalError("");
    try {
      await standing(id);
      await refreshSelf();
    } catch (e) {
      setProposalError(String(e));
    }
  }

  async function answer(id: string, accept: boolean) {
    await resolve(id, accept);
    await refreshSelf();
  }

  if (!lessons.length && !pending.length) {
    return (
      <>
        <div className="memory-empty">
          <strong>Nothing learned yet</strong>
          <span>
            When a conversation goes wrong and you put me right, I'll draw a lesson from it once
            we're done and keep it here.
          </span>
        </div>
        <LetGo collection="lessons" version={version} onChanged={() => void refreshSelf()} />
      </>
    );
  }

  return (
    <div className="self-list">
      {pending.map((p) =>
        p.target === "project_instructions" ? (
          <ProposalCard key={p.id} id={p.id} />
        ) : p.target === "habit" ? (
          <div className="memory-proposal" key={p.id}>
            <TriedFixes proposal={p} />
            <p className="memory-proposal-head">I keep relearning {p.slug ?? "this one"}. {p.rationale}</p>
            <p className="memory-desc">{habitLine(p.proposed_text) ?? ""}</p>
            {proposalError && <p className="memory-error">{proposalError}</p>}
            <div className="setting-actions">
              <button className="btn-primary" onClick={() => answer(p.id, true)}>
                Check it first
              </button>
              <button className="btn-text" onClick={() => standingInstead(p.id)}>
                Make it a standing instruction instead
              </button>
              <button className="btn-text" onClick={() => answer(p.id, false)}>
                Not now
              </button>
            </div>
          </div>
        ) : (
          <div className="memory-proposal" key={p.id}>
            <TriedFixes proposal={p} />
            <p className="memory-proposal-head">
              {p.target === "engine-memory" ? `${p.rationale} Keep this?` : `I'd like to learn this: ${p.rationale}`}
            </p>
            <pre className="self-steps">{p.proposed_text}</pre>
            <div className="setting-actions">
              <button className="btn-primary" onClick={() => answer(p.id, true)}>
                {p.target === "engine-memory" ? "Keep it" : "Learn it"}
              </button>
              <button className="btn-text" onClick={() => answer(p.id, false)}>
                Not now
              </button>
            </div>
          </div>
        )
      )}

      <RecentChanges collection="lessons" version={version} />

      {lessons.length > 0 && <h3 className="memory-section-title">What I've learned</h3>}
      {lessons.map((l) => (
        <div className="memory-card" key={l.name}>
          <div className="memory-card-head">
            <span className="memory-name">{l.name}</span>
            <span className="memory-created">{l.created}</span>
            {/* RPT-2: plain words, no badge or colour — a lesson learned
                again is a sign it isn't sticking, not an achievement. */}
            {!!l.recurrence && l.recurrence > 1 && (
              <span className="memory-created">learned {l.recurrence}×</span>
            )}
          </div>
          {/* `CPX-5`: where it came from, in one plain line: no badge, no colour.
              A lesson I drew because you went back on my work says so. */}
          {l.origin === "rewind" && (
            <p className="memory-created">learned when you went back{l.created ? ` on ${l.created}` : ""}</p>
          )}
          <p className="memory-desc">{l.description}</p>
          {/* `HAB-UI-1`: a lesson that fires at a tool says so, in plain words. */}
          {l.fires && <p className="memory-created">{firesLine(l.fires)}</p>}
          {expanded === l.name && <p className="memory-body">{l.body}</p>}
          <Evidence ids={l.evidence} />
          <div className="memory-card-actions">
            <button
              className="btn-text"
              onClick={() => setExpanded(expanded === l.name ? null : l.name)}
            >
              {expanded === l.name ? "Hide" : "Read it"}
            </button>
            {l.source_conversation && !l.evidence?.length && (
              <button
                className="btn-text"
                onClick={() => {
                  setActiveConversation(l.source_conversation!);
                  setView("chat");
                }}
              >
                Where I learned it
              </button>
            )}
            <button className="btn-text danger" onClick={() => forget(l.name)}>
              Delete
            </button>
          </div>
        </div>
      ))}

      {improve && improve.rules.length > 0 && (
        <section className="memory-recent" aria-label="How I improve">
          <h3 className="memory-section-title">How I improve</h3>
          <ul>
            {improve.rules.map((r) => (
              <li key={r.key}>
                {r.text}{" "}
                <button className="btn-text danger" onClick={() => void dropRule(r.key)} aria-label={`Delete: ${r.text}`}>
                  Delete
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <LetGo collection="lessons" version={version} onChanged={() => void refreshSelf()} />
    </div>
  );
}

/** HEAL + REF: how well I'm actually running, and what I set aside. */
function HealthTab() {
  const vitality = useAppStore((s) => s.vitality);
  const refreshSelf = useAppStore((s) => s.refreshSelf);
  const conversations = useAppStore((s) => s.conversations);
  const reflect = useAppStore((s) => s.reflectConversation);
  const reflecting = useAppStore((s) => s.reflectingIds.length > 0);
  const autoReflect = useAppStore((s) => s.autoReflect);
  const setAutoReflect = useAppStore((s) => s.setAutoReflect);
  const goldenStatus = useAppStore((s) => s.goldenStatus);
  const goldenError = useAppStore((s) => s.goldenError);
  const checkingGolden = useAppStore((s) => s.checkingGolden);
  const checkGoldenNow = useAppStore((s) => s.checkGoldenNow);
  const [note, setNote] = useState("");
  const [learning, setLearning] = useState<api.LearningReport | null>(null);
  useEffect(() => {
    api.getLearningReport().then(setLearning).catch(() => setLearning(null));
  }, []);
  const [heldRate, setHeldRate] = useState<number | null>(null);
  useEffect(() => {
    if (!api.inTauri()) return;
    api.improveSummary().then((x) => setHeldRate(x.hold_rate)).catch(() => setHeldRate(null));
  }, []);
  const lines = learningLines(learning, heldRate === null ? null : `${Math.round(heldRate * 100)}%`);

  const latest = conversations[0];

  async function reflectNow() {
    if (!latest) return;
    setNote("");
    const { learned, proposed } = await reflect(latest.id);
    if (learned > 0) {
      setNote(`I learned ${learned} thing${learned === 1 ? "" : "s"} from “${latest.title}”.`);
    } else if (proposed > 0) {
      // Ask-first: saying "nothing to learn" here would be false — the lesson
      // exists, it's just waiting on an answer in the Lessons tab.
      setNote(
        `I'd like to learn ${proposed} thing${proposed === 1 ? "" : "s"} from “${latest.title}” — it's waiting in Lessons.`
      );
    } else {
      setNote("I found nothing new to learn from that one.");
    }
  }

  async function resolveQuarantine(file: string, restore: boolean) {
    if (restore) await api.restoreQuarantined(file);
    else await api.deleteQuarantined(file);
    await refreshSelf();
  }

  if (!vitality) {
    return (
      <div className="memory-empty">
        <strong>Nothing to report yet</strong>
        <span>Once I've been running for a while, how I'm doing shows up here.</span>
      </div>
    );
  }

  return (
    <div className="self-list">
      <section className="self-card">
        <h3 className="self-card-title">How I'm running</h3>
        <div className="self-row">
          <span className="self-row-label">Runtime</span>
          <span className="self-row-value">
            {vitality.engine_restarts_session > 0
              ? `I've restarted my runtime ${vitality.engine_restarts_session}× this session.`
              : "My runtime hasn't needed restarting this session."}
          </span>
        </div>
        <div className="self-row">
          <span className="self-row-label">Last reflected</span>
          <span className="self-row-value">{formatDate(vitality.last_reflection)}</span>
        </div>
        <div className="setting-actions">
          <button className="btn-secondary" onClick={reflectNow} disabled={reflecting || !latest}>
            {reflecting ? "Reflecting…" : "Reflect now"}
          </button>
          <label className="toggle-line">
            <input
              type="checkbox"
              checked={autoReflect}
              onChange={(e) => setAutoReflect(e.target.checked)}
            />
            <span>Let me reflect on conversations when we leave them</span>
          </label>
        </div>
        {note && <p className="self-line self-note">{note}</p>}
      </section>

      <section className="self-card">
        <h3 className="self-card-title">Whether I'm learning</h3>
        {lines ? (
          lines.map((l) => <LineRow key={l} line={l} />)
        ) : (
          <p className="self-quiet">No data yet. I need about two weeks to tell.</p>
        )}
      </section>

      <section className="self-card">
        <h3 className="self-card-title">Whether a recent change made me worse</h3>
        {goldenStatus ? (
          <>
            <p className="self-line">
              {goldenStatus.passed}/{goldenStatus.total} checks passing, last checked{" "}
              {formatDate(goldenStatus.checked_at)}.
            </p>
            {goldenStatus.failing.length > 0 && (
              <ul className="self-table">
                {goldenStatus.failing.map((id) => (
                  <li key={id}>
                    <span className="self-tool">{id}</span>
                  </li>
                ))}
              </ul>
            )}
          </>
        ) : (
          <p className="self-quiet">I haven't checked myself yet.</p>
        )}
        <div className="setting-actions">
          <button className="btn-secondary" onClick={checkGoldenNow} disabled={checkingGolden}>
            {checkingGolden ? "Checking…" : "Check me now"}
          </button>
        </div>
        {goldenError && <p className="self-line self-note">{goldenError}</p>}
      </section>

      <section className="self-card">
        <h3 className="self-card-title">How my tools have run this week</h3>
        {vitality.tool_health.length === 0 ? (
          <p className="self-quiet">No tool calls recorded yet.</p>
        ) : (
          <ul className="self-table">
            {vitality.tool_health.map((t) => (
              <li key={t.tool_name}>
                <span className="self-tool">{t.tool_name}</span>
                <span className="self-tool-count">
                  {t.ok}/{t.total}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {vitality.quarantined.length > 0 && (
        <section className="self-card">
          <h3 className="self-card-title">Files I couldn't read</h3>
          <p className="self-line">
            I set these aside rather than delete them. Fix one in an editor and put it
            back, or discard it.
          </p>
          <ul className="self-table">
            {vitality.quarantined.map((f) => (
              <li key={f}>
                <span className="self-tool">{f}</span>
                <button className="btn-text" onClick={() => resolveQuarantine(f, true)}>
                  Put it back
                </button>
                <button className="btn-text danger" onClick={() => resolveQuarantine(f, false)}>
                  Discard
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

/** One "label: value" line of the learning numbers, set like a row of details. */
function LineRow({ line }: { line: string }) {
  const at = line.indexOf(": ");
  if (at < 0) return <p className="self-line">{line}</p>;
  return (
    <div className="self-row">
      <span className="self-row-label">{line.slice(0, at)}</span>
      <span className="self-row-value">{line.slice(at + 2)}</span>
    </div>
  );
}

/** AUT-UI-1: the membrane, as plain choices, one per kind of change. */
function AutonomyTab() {
  const autonomy = useAppStore((s) => s.autonomy);
  const setAutonomy = useAppStore((s) => s.setAutonomy);
  // `CPX-6`: a `◆` in the command menu lands on the class it stands for.
  const focus = useAppStore((s) => s.selfTabRequest?.focus);
  const nonce = useAppStore((s) => s.selfTabRequest?.nonce);
  useEffect(() => {
    if (focus) document.getElementById(`autonomy-${focus}`)?.scrollIntoView?.({ block: "center" });
  }, [focus, nonce]);

  return (
    <div className="self-list">
      <p className="setting-help">
        I maintain myself. You decide how much I may change without asking.
      </p>
      <div className="self-card self-rungs">
        {AUTONOMY_CLASSES.map((c) => (
          <div className="self-rung" key={c.id} id={`autonomy-${c.id}`}>
            <div className="self-rung-text">
              <span className="self-rung-label">{c.label}</span>
              <span className="skill-desc">{c.blurb}</span>
            </div>
            <div className="self-segmented" role="group" aria-label={c.label}>
              {c.rungs.map((r) => (
                <button
                  key={r}
                  className={`self-segment ${(autonomy[c.id] ?? c.fallback) === r ? "active" : ""}`}
                  aria-pressed={(autonomy[c.id] ?? c.fallback) === r}
                  onClick={() => setAutonomy(c.id, r)}
                >
                  {RUNG_LABELS[r]}
                </button>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}


/** Refresh the Self view's data whenever it is opened. The digest and its
 * mark pulse moved to the Tasks section, which is where they're read now. */
export function useSelfRefresh() {
  const refreshSelf = useAppStore((s) => s.refreshSelf);
  useEffect(() => {
    refreshSelf();
  }, [refreshSelf]);
}
