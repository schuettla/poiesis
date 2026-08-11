import { useEffect, useState } from "react";
import * as api from "../lib/api";
import { useAppStore } from "../lib/store";
import "./Surface.css";
import "../components/Self/Self.css";
import "./Tasks.css";

/**
 * Tasks (SCH): work Poiesis does on a schedule, while nobody is watching.
 *
 * This used to be a sixth tab inside the Self panel, on the reasoning that
 * "the self is a place, not a settings tab". Half of that held up and half
 * didn't: nightly reflection really is self-upkeep, but *your* tasks aren't
 * part of what Poiesis is made of — they're work you asked it to do on a
 * timer, and burying them three levels down behind a cog meant nobody found
 * them. They get their own section now.
 *
 * Every run happens in its own conversation, listed under the task and
 * openable in the rail. A task whose output you can't read is a black box,
 * and a one-line summary you have to take on faith is the same box with a
 * label on it.
 */
export default function Tasks() {
  const jobs = useAppStore((s) => s.scheduledJobs);
  const runningJob = useAppStore((s) => s.runningJob);
  const digest = useAppStore((s) => s.digest);
  const refreshScheduler = useAppStore((s) => s.refreshScheduler);
  const dismissDigest = useAppStore((s) => s.dismissDigest);
  const createJob = useAppStore((s) => s.createScheduledJob);
  const updateJob = useAppStore((s) => s.updateScheduledJob);
  const deleteJob = useAppStore((s) => s.deleteScheduledJob);
  const runNow = useAppStore((s) => s.runScheduledJobNow);
  const draft = useAppStore((s) => s.taskDraft);
  const clearDraft = useAppStore((s) => s.clearTaskDraft);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [runNote, setRunNote] = useState<string | null>(null);

  // Opening this section is what marks the digest read (SCH-UI-2) — no button,
  // and nothing nags if it's never opened. Sequenced so a slow refresh can't
  // overwrite the local "read" flip with the stale unread copy it fetched.
  useEffect(() => {
    (async () => {
      await dismissDigest();
      await refreshScheduler();
    })();
  }, [refreshScheduler, dismissDigest]);

  // Arriving from "Schedule this" in a chat: open the editor straight away,
  // prefilled. Landing on a list and having to find "+ New task" would lose
  // the thing the button was for.
  useEffect(() => {
    if (draft) setEditingId("new");
  }, [draft]);

  const nightly = jobs.find((j) => j.built_in) ?? null;
  const custom = jobs.filter((j) => !j.built_in);
  const editingJob = custom.find((j) => j.id === editingId) ?? null;

  async function toggleNightly(enabled: boolean) {
    if (!nightly) return;
    await updateJob(nightly.id, {
      name: nightly.name,
      prompt: nightly.prompt,
      cadence: nightly.cadence,
      scope: nightly.scope,
      enabled,
    });
  }

  async function handleRunNow(id: string) {
    setBusyId(id);
    setRunNote(null);
    try {
      setRunNote(await runNow(id));
    } catch (e) {
      setRunNote(e instanceof Error ? e.message : "That task couldn't run.");
    } finally {
      setBusyId(null);
    }
  }

  function closeEditor() {
    setEditingId(null);
    clearDraft();
  }

  return (
    <div className="surface">
      <div className="surface-inner">
      <h1>Tasks</h1>
      <p className="lede">
        Work I do on a schedule, on my own. Each run happens in its own chat you can
        open and read. A task can read a folder you point it at, but it can never
        change, delete or move anything, and it never interrupts you with a question
        while it runs — if it would need to ask, it stops and tells you so.
      </p>

      {digest && (
        <div className="self-block self-digest">
          <p className="self-line self-note">{digest.text}</p>
        </div>
      )}

      {runningJob && (
        <p className="self-line self-note">Running “{runningJob.job_name}” now…</p>
      )}

      {nightly && (
        <section className="self-block">
          <p className="self-subhead">Nightly reflection</p>
          <p className="self-line">
            Once a day, read back over finished conversations, learn what I'm sure
            enough of, and leave a short summary here in the morning.
          </p>
          <div className="setting-actions">
            <label className="toggle-line">
              <input
                type="checkbox"
                checked={nightly.enabled}
                onChange={(e) => toggleNightly(e.target.checked)}
              />
              <span>Run it nightly</span>
            </label>
            <button
              className="btn-secondary"
              onClick={() => handleRunNow(nightly.id)}
              disabled={!!runningJob || busyId === nightly.id}
            >
              {busyId === nightly.id ? "Running…" : "Run now"}
            </button>
          </div>
          {nightly.last_run_at && (
            <p className="self-line task-meta">
              Last run {formatWhen(nightly.last_run_at)}
              {nightly.last_result ? ` — ${nightly.last_result}` : ""}
            </p>
          )}
        </section>
      )}

      <section className="self-block">
        <div className="tasks-head">
          <p className="self-subhead">Your tasks</p>
          {!editingId && (
            <button className="btn-secondary" onClick={() => setEditingId("new")}>
              + New task
            </button>
          )}
        </div>

        {custom.length === 0 && !editingId && (
          <p className="empty-hint">
            No tasks yet. Anything you'd otherwise ask for again every morning is a
            good first one — or turn an open chat into one with “Schedule this”.
          </p>
        )}

        <div className="task-list">
          {custom.map((j) => (
            <TaskCard
              key={j.id}
              job={j}
              running={runningJob?.job_id === j.id}
              busy={busyId === j.id}
              disabled={!!runningJob}
              onEdit={() => setEditingId(j.id)}
              onRun={() => handleRunNow(j.id)}
              onDelete={() => deleteJob(j.id)}
            />
          ))}
        </div>

        {runNote && <p className="self-line self-note">{runNote}</p>}

        {editingId && (editingJob || editingId === "new") && (
          <TaskEditor
            // The form seeds its fields once, on mount. Without a key, switching
            // from one task to another reuses the same instance — showing the
            // previous task's text and saving it onto this one.
            key={editingId}
            job={editingJob}
            draft={editingId === "new" ? draft : null}
            onCancel={closeEditor}
            onSave={async (input) => {
              if (editingJob) await updateJob(editingJob.id, input);
              else await createJob(input);
              closeEditor();
            }}
          />
        )}
      </section>
      </div>
    </div>
  );
}

const CADENCE_LABELS: Record<api.Cadence, string> = {
  hourly: "Hourly",
  "six-hourly": "Every 6 hours",
  daily: "Daily",
  weekly: "Weekly",
};

/** Dates the way you'd say them, not the way they're stored. */
function formatWhen(at: number): string {
  const d = new Date(at);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  return sameDay ? `today at ${time}` : `${d.toLocaleDateString()} at ${time}`;
}

/** One task: what it does, when it next runs, and every run you can open. */
function TaskCard({
  job,
  running,
  busy,
  disabled,
  onEdit,
  onRun,
  onDelete,
}: {
  job: api.ScheduledJob;
  running: boolean;
  busy: boolean;
  disabled: boolean;
  onEdit: () => void;
  onRun: () => void;
  onDelete: () => void;
}) {
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);
  const setView = useAppStore((s) => s.setView);
  const [showRuns, setShowRuns] = useState(false);

  function openRun(conversationId: string) {
    setActiveConversation(conversationId);
    setView("chat");
  }

  return (
    <article className={`task-card ${job.enabled ? "" : "paused"}`}>
      <div className="task-card-head">
        <span className="task-name">{job.name}</span>
        <span className="task-cadence">{CADENCE_LABELS[job.cadence]}</span>
        {!job.enabled && <span className="task-paused">Paused</span>}
      </div>
      <p className="task-prompt">{job.prompt}</p>
      {job.scope && <p className="task-meta">Reads {job.scope}</p>}
      <p className="task-meta">
        {running
          ? "Running now…"
          : job.enabled
            ? `Next run ${formatWhen(job.next_run_at)}`
            : "Won't run until you turn it back on"}
        {job.last_run_at ? ` · last run ${formatWhen(job.last_run_at)}` : ""}
      </p>

      <div className="task-actions">
        <button className="btn-text" onClick={onRun} disabled={disabled || busy}>
          {busy ? "Running…" : "Run now"}
        </button>
        <button className="btn-text" onClick={onEdit}>
          Edit
        </button>
        {job.runs.length > 0 && (
          <button className="btn-text" onClick={() => setShowRuns((s) => !s)}>
            {showRuns ? "Hide runs" : `${job.runs.length} run${job.runs.length === 1 ? "" : "s"}`}
          </button>
        )}
        <button className="btn-text danger" onClick={onDelete}>
          Delete
        </button>
      </div>

      {showRuns && (
        <ul className="task-runs">
          {job.runs.map((r) => (
            <li key={r.conversation_id}>
              <button className="task-run-open" onClick={() => openRun(r.conversation_id)}>
                <span className="task-run-when">{formatWhen(r.at)}</span>
                <span className="task-run-summary">{r.summary}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}

/** The task form: name, instructions, schedule, folder, on/off. */
function TaskEditor({
  job,
  draft,
  onCancel,
  onSave,
}: {
  job: api.ScheduledJob | null;
  draft: { name: string; prompt: string; conversationId: string } | null;
  onCancel: () => void;
  onSave: (input: api.ScheduledJobInput) => Promise<void>;
}) {
  const [name, setName] = useState(job?.name ?? draft?.name ?? "");
  const [prompt, setPrompt] = useState(job?.prompt ?? draft?.prompt ?? "");
  const [cadence, setCadence] = useState<api.Cadence>(job?.cadence ?? "daily");
  const [scope, setScope] = useState<string | null>(job?.scope ?? null);
  const [enabled, setEnabled] = useState(job?.enabled ?? true);
  const [saving, setSaving] = useState(false);

  async function pickScope() {
    try {
      const picked = await api.pickFolder();
      if (picked) setScope(picked);
    } catch {
      /* cancelled or refused — leave the folder as it was */
    }
  }

  async function save() {
    if (!name.trim() || !prompt.trim()) return;
    setSaving(true);
    try {
      await onSave({
        name: name.trim(),
        prompt: prompt.trim(),
        cadence,
        scope,
        enabled,
        source_conversation_id: job ? job.source_conversation_id : (draft?.conversationId ?? null),
      });
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="self-block task-editor">
      {draft && !job && (
        <p className="self-line self-note">
          From the chat you had open — edit anything here before saving.
        </p>
      )}
      <label className="self-field">
        <span>Name</span>
        <input
          type="text"
          value={name}
          placeholder="What to call it"
          onChange={(e) => setName(e.target.value)}
        />
      </label>
      <label className="self-field">
        <span>What I should do</span>
        <textarea
          rows={5}
          value={prompt}
          placeholder="Describe it the way you'd ask me in a chat."
          onChange={(e) => setPrompt(e.target.value)}
        />
      </label>
      <div className="self-segmented" role="group" aria-label="How often">
        {(Object.keys(CADENCE_LABELS) as api.Cadence[]).map((c) => (
          <button
            key={c}
            className={`self-segment ${cadence === c ? "active" : ""}`}
            aria-pressed={cadence === c}
            onClick={() => setCadence(c)}
          >
            {CADENCE_LABELS[c]}
          </button>
        ))}
      </div>
      <div className="setting-actions">
        <button className="btn-text" onClick={pickScope}>
          {scope ? `Folder: ${scope}` : "Give it a folder to read (optional)"}
        </button>
        {scope && (
          <button className="btn-text" onClick={() => setScope(null)}>
            Clear folder
          </button>
        )}
      </div>
      <label className="toggle-line">
        <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
        <span>Run it on this schedule</span>
      </label>
      <div className="setting-actions">
        <button
          className="btn-primary"
          onClick={save}
          disabled={saving || !name.trim() || !prompt.trim()}
        >
          {job ? "Save changes" : "Create task"}
        </button>
        <button className="btn-text" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
