import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { AUTONOMY_CLASSES, useAppStore } from "../../lib/store";
import MemoryPanel from "../Memory/MemoryPanel";
import "../../routes/Models.css";
import "../Context/Context.css";
import "./Self.css";

type Tab = "memory" | "lessons" | "health" | "autonomy";

const TABS: { id: Tab; label: string }[] = [
  { id: "memory", label: "Memory" },
  { id: "lessons", label: "Lessons" },
  { id: "health", label: "Health" },
  { id: "autonomy", label: "Autonomy" },
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

  return (
    <>
      {activeConversationId && (
        <button
          className="why-link self-why-link"
          onClick={() => openContextPanel({ conversationId: activeConversationId })}
        >
          What I'm working from
        </button>
      )}
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

      <div className="self-tabpanel" role="tabpanel">
        {tab === "memory" && <MemoryPanel />}
        {tab === "lessons" && <LessonsTab />}
        {tab === "health" && <HealthTab />}
        {tab === "autonomy" && <AutonomyTab />}
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

  const pending = proposals.filter((p) => p.target === "lesson" || p.target === "lesson-critic");

  async function answer(id: string, accept: boolean) {
    await resolve(id, accept);
    await refreshSelf();
  }

  if (!lessons.length && !pending.length) {
    return (
      <p className="empty-hint">
        I haven't learned anything yet. When a conversation goes wrong and you put me
        right, I'll draw a lesson from it once we're done and keep it here.
      </p>
    );
  }

  return (
    <div className="self-list">
      {pending.map((p) => (
        <div className="memory-proposal" key={p.id}>
          <p className="memory-proposal-head">
            I'd like to learn this: {p.rationale}
          </p>
          <pre className="self-steps">{p.proposed_text}</pre>
          <div className="setting-actions">
            <button className="btn-primary" onClick={() => answer(p.id, true)}>
              Learn it
            </button>
            <button className="btn-text" onClick={() => answer(p.id, false)}>
              Not now
            </button>
          </div>
        </div>
      ))}

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
          <p className="memory-desc">{l.description}</p>
          {expanded === l.name && <p className="memory-body">{l.body}</p>}
          <div className="memory-card-actions">
            <button
              className="btn-text"
              onClick={() => setExpanded(expanded === l.name ? null : l.name)}
            >
              {expanded === l.name ? "Hide" : "Read it"}
            </button>
            {l.source_conversation && (
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

  if (!vitality) return <p className="empty-hint">Nothing to report yet.</p>;

  return (
    <div className="self-list">
      <div className="self-block">
        <p className="self-line">
          {vitality.engine_restarts_session > 0
            ? `I've restarted my engine ${vitality.engine_restarts_session}× this session.`
            : "My engine hasn't needed restarting this session."}
        </p>
        <p className="self-line">Last time I reflected: {formatDate(vitality.last_reflection)}.</p>
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
      </div>

      <div className="self-block">
        <p className="self-subhead">Whether a recent change made me worse</p>
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
          <p className="empty-hint">I haven't checked myself yet.</p>
        )}
        <div className="setting-actions">
          <button className="btn-secondary" onClick={checkGoldenNow} disabled={checkingGolden}>
            {checkingGolden ? "Checking…" : "Check me now"}
          </button>
        </div>
        {goldenError && <p className="self-line self-note">{goldenError}</p>}
      </div>

      <div className="self-block">
        <p className="self-subhead">How my tools have run this week</p>
        {vitality.tool_health.length === 0 ? (
          <p className="empty-hint">No tool calls recorded yet.</p>
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
      </div>

      {vitality.quarantined.length > 0 && (
        <div className="self-block">
          <p className="self-subhead">Files I couldn't read</p>
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
        </div>
      )}
    </div>
  );
}

/** AUT-UI-1: the membrane, as five plain choices. */
function AutonomyTab() {
  const autonomy = useAppStore((s) => s.autonomy);
  const setAutonomy = useAppStore((s) => s.setAutonomy);

  return (
    <div className="self-list">
      <p className="setting-help">
        I maintain myself. You decide how much I may change without asking.
      </p>
      {AUTONOMY_CLASSES.map((c) => (
        <div className="self-rung" key={c.id}>
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
