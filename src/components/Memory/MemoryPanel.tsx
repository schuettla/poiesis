import { useEffect, useState } from "react";
import * as api from "../../lib/api";
import { cloudTarget, useAppStore, useExpert } from "../../lib/store";
import "./Memory.css";

/** How long the undo strip stays after a delete. */
const UNDO_MS = 5000;

/** SEM-UI-4: a quiet relative date, not a ranking — "a date is enough to see
 * what is alive and what has gone quiet." */
function ago(ts: number): string {
  const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/**
 * The durable self, laid open (MEM-UI-1). Everything the agent remembers is
 * listed here, editable and deletable by hand — the folder on disk is the
 * source of truth and this is just a comfortable window onto it.
 */
export default function MemoryPanel() {
  const [facts, setFacts] = useState<api.Fact[]>([]);
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [undo, setUndo] = useState<{ name: string; file: string } | null>(null);
  const [pending, setPending] = useState<api.Consolidation | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  // PRO-UI: the untitled synthesis at the top of the page (SMP-5).
  const [profile, setProfile] = useState<api.Profile | null>(null);
  const [profileEditing, setProfileEditing] = useState(false);
  const [profileDraft, setProfileDraft] = useState("");
  const [profileBusy, setProfileBusy] = useState(false);
  const [profileError, setProfileError] = useState("");

  const refreshMemoryContext = useAppStore((s) => s.refreshMemoryContext);
  const refreshChangeProposals = useAppStore((s) => s.refreshChangeProposals);
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);
  const setView = useAppStore((s) => s.setView);
  const noteGlobalFactChange = useAppStore((s) => s.noteGlobalFactChange);
  const expert = useExpert();

  async function refresh() {
    try {
      setFacts(await api.listMemoryFacts());
      setPending(await api.getPendingConsolidation());
      setProfile(await api.getProfile());
    } catch {
      /* memory folder unreadable */
    }
    refreshMemoryContext();
    refreshChangeProposals();
  }

  /** PRO-UI-2: "Rewrite this" / "Let me rewrite it" — ignores the volume gate,
   * since the user asked directly. */
  async function rewriteProfile() {
    setProfileBusy(true);
    setProfileError("");
    try {
      setProfile(await api.rebuildProfile(true));
      await refreshMemoryContext();
    } catch (e) {
      setProfileError(String(e));
    } finally {
      setProfileBusy(false);
    }
  }

  function startProfileEdit() {
    setProfileDraft(profile?.body ?? "");
    setProfileEditing(true);
  }

  async function saveProfileEdit() {
    setProfile(await api.editProfile(profileDraft));
    setProfileEditing(false);
    await refreshMemoryContext();
  }

  useEffect(() => {
    if (!api.inTauri()) return;
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!undo) return;
    const t = setTimeout(() => setUndo(null), UNDO_MS);
    return () => clearTimeout(t);
  }, [undo]);

  async function saveEdit(name: string) {
    await api.updateMemoryFact(name, draft);
    setEditing(null);
    await refresh();
    noteGlobalFactChange();
  }

  /** SCP-UI-1: the user is the final authority on their own standing
   * instructions, classifier or not. */
  async function setScope(name: string, scope: "global" | "topical") {
    await api.setFactScope(name, scope);
    await refresh();
    // PRO-4: which facts count as global just changed.
    noteGlobalFactChange();
  }

  async function remove(name: string) {
    const file = await api.forgetMemoryFact(name);
    setUndo({ name, file });
    await refresh();
    noteGlobalFactChange();
  }

  async function restore() {
    if (!undo) return;
    await api.restoreMemoryFact(undo.file);
    setUndo(null);
    await refresh();
  }

  async function tidyUp() {
    setBusy(true);
    setError("");
    try {
      // Proposing a tidy-up is a real model call, routed like any other — a
      // cloud-only setup has no local engine to fall back to.
      setPending(await api.consolidateMemory(cloudTarget()));
      refreshChangeProposals();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function resolveConsolidation(accept: boolean) {
    // Applying runs `GLD-2`'s before/after check, which routes to the model
    // in play the same way a chat turn does.
    await api.applyConsolidation(accept, cloudTarget());
    setPending(null);
    await refresh();
  }

  function openSource(conversationId: string) {
    setActiveConversation(conversationId);
    setView("chat");
  }

  const needle = query.trim().toLowerCase();
  const shown = needle
    ? facts.filter(
        (f) =>
          f.name.toLowerCase().includes(needle) ||
          f.description.toLowerCase().includes(needle) ||
          f.body.toLowerCase().includes(needle)
      )
    : facts;

  const hasProposal =
    !!pending && (pending.deletes.length > 0 || pending.edits.length > 0 || pending.merges.length > 0);

  return (
    <div className="memory-panel">
      <div className="memory-head">
        <span className="memory-count">
          Memory · {facts.length} {facts.length === 1 ? "fact" : "facts"}
        </span>
        <button className="btn-secondary" onClick={tidyUp} disabled={busy || facts.length === 0}>
          {busy ? "Thinking…" : "Tidy up"}
        </button>
        <button className="btn-secondary" onClick={() => api.openMemoryDir()}>
          Open folder
        </button>
        <button
          className="btn-secondary"
          onClick={() => api.exportMemoryZip().catch((e) => setError(String(e)))}
        >
          Export zip
        </button>
      </div>

      {error && <p className="memory-error">{error}</p>}

      {hasProposal && pending && (
        <div className="memory-proposal">
          <p className="memory-proposal-head">
            A tidy-up is proposed. Nothing has changed yet.
          </p>
          {pending.merges.map((m) => (
            <div className="memory-proposal-row" key={`merge-${m.keep}`}>
              <span className="memory-proposal-kind">merge</span>
              <span className="memory-proposal-body">
                keep <strong>{m.keep}</strong>, fold in {m.drop.join(", ")}
                <pre>{m.text}</pre>
              </span>
            </div>
          ))}
          {pending.edits.map((e) => (
            <div className="memory-proposal-row" key={`edit-${e.name}`}>
              <span className="memory-proposal-kind">edit</span>
              <span className="memory-proposal-body">
                <strong>{e.name}</strong>
                <pre>{e.text}</pre>
              </span>
            </div>
          ))}
          {pending.deletes.map((name) => (
            <div className="memory-proposal-row" key={`del-${name}`}>
              <span className="memory-proposal-kind">drop</span>
              <span className="memory-proposal-body">
                <strong>{name}</strong>
              </span>
            </div>
          ))}
          <div className="setting-actions">
            <button className="btn-primary" onClick={() => resolveConsolidation(true)}>
              Apply all
            </button>
            <button className="btn-text" onClick={() => resolveConsolidation(false)}>
              Dismiss
            </button>
          </div>
        </div>
      )}

      {undo && (
        <div className="memory-undo" role="status">
          <span>Forgot “{undo.name}”.</span>
          <button className="btn-text" onClick={restore}>
            Undo
          </button>
        </div>
      )}

      {/* PRO-UI-1/SMP-5: untitled prose — never a tab, never a labelled card.
          "About you" is a WHY-2 exception, not a name that belongs here. */}
      <div className={`memory-about-you ${profileBusy ? "busy" : ""}`}>
        {profileEditing ? (
          <>
            <textarea
              className="system-prompt"
              rows={3}
              value={profileDraft}
              onChange={(e) => setProfileDraft(e.target.value)}
            />
            <div className="setting-actions">
              <button className="btn-primary" onClick={saveProfileEdit}>
                Save
              </button>
              <button className="btn-text" onClick={() => setProfileEditing(false)}>
                Cancel
              </button>
            </div>
          </>
        ) : (
          <>
            {profile ? (
              <>
                <p className="memory-about-you-text">{profile.body}</p>
                {profile.edited && <p className="memory-about-you-meta">you wrote this</p>}
                {!profile.edited && (
                  <p className="memory-about-you-meta">
                    drawn from {profile.source_count} {profile.source_count === 1 ? "thing" : "things"} you've
                    told me
                  </p>
                )}
              </>
            ) : (
              <p className="memory-about-you-text memory-about-you-empty">
                I haven't formed a picture of this yet.
              </p>
            )}
            <div className="memory-about-you-actions">
              <button className="btn-text" onClick={rewriteProfile} disabled={profileBusy}>
                {profileBusy ? "Rewriting…" : profile?.edited ? "Let me rewrite it" : "Rewrite this"}
              </button>
              {profile && !profileBusy && (
                <button className="btn-text" onClick={startProfileEdit}>
                  Edit
                </button>
              )}
            </div>
            {profileError && <p className="memory-error">{profileError}</p>}
          </>
        )}
      </div>

      {facts.length > 0 && (
        <input
          className="field-input memory-search"
          type="search"
          placeholder="Search memories…"
          aria-label="Search memories"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      )}

      {facts.length === 0 && (
        <p className="empty-hint">
          Nothing remembered yet. Facts appear here when you confirm a lasting preference in chat.
        </p>
      )}

      {shown.map((f) => (
        <div className="memory-card" key={f.name}>
          <div className="memory-card-head">
            <span className="memory-name">{f.name}</span>
            <span className="memory-kind">{f.kind}</span>
            <span className="memory-created">{f.created}</span>
          </div>
          <p className="memory-desc">{f.description}</p>
          {expert ? (
            <div className="memory-scope" role="group" aria-label={`when "${f.name}" applies`}>
              <button
                className={`memory-scope-segment ${f.scope !== "topical" ? "active" : ""}`}
                aria-pressed={f.scope !== "topical"}
                onClick={() => setScope(f.name, "global")}
              >
                Always
              </button>
              <button
                className={`memory-scope-segment ${f.scope === "topical" ? "active" : ""}`}
                aria-pressed={f.scope === "topical"}
                onClick={() => setScope(f.name, "topical")}
              >
                When relevant
              </button>
            </div>
          ) : (
            <p className="memory-scope-plain">
              {f.scope === "topical" ? "only when it's relevant" : "applies to every answer"}
            </p>
          )}
          {f.last_used_at ? (
            <p className="memory-last-used">last surfaced {ago(f.last_used_at)}</p>
          ) : null}
          {editing === f.name ? (
            <>
              <textarea
                className="system-prompt"
                rows={4}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
              />
              <div className="setting-actions">
                <button className="btn-primary" onClick={() => saveEdit(f.name)}>
                  Save
                </button>
                <button className="btn-text" onClick={() => setEditing(null)}>
                  Cancel
                </button>
              </div>
            </>
          ) : (
            <>
              <p className="memory-body">{f.body}</p>
              <div className="memory-card-actions">
                <button
                  className="btn-text"
                  onClick={() => {
                    setEditing(f.name);
                    setDraft(f.body);
                  }}
                >
                  Edit
                </button>
                {f.source_conversation && (
                  <button className="btn-text" onClick={() => openSource(f.source_conversation!)}>
                    Where this came from
                  </button>
                )}
                <button className="btn-text danger" onClick={() => remove(f.name)}>
                  Delete
                </button>
              </div>
            </>
          )}
        </div>
      ))}
    </div>
  );
}
