import { useCallback, useEffect, useState, type ReactNode } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import * as api from "../../lib/api";
import type { SessionReport, SessionRun } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import { CloseIcon } from "../Icons/Icons";
import Select from "../Select/Select";
import { count, duration, ending, money, stamp, summaryText, tokens } from "./summary";
import "./SessionDetails.css";

type Tab = "overview" | "runs" | "log" | "messages" | "records";

function Copy({ text, label = "Copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      className="btn-text sd-copy"
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(
          () => {
            setDone(true);
            setTimeout(() => setDone(false), 1500);
          },
          () => setDone(false)
        );
      }}
    >
      {done ? "Copied" : label}
    </button>
  );
}

/** Name and value pairs. A pair with no value is left out. */
function Facts({ rows }: { rows: [string, ReactNode, string?][] }) {
  return (
    <dl className="sd-facts">
      {rows
        .filter(([, value]) => value !== null && value !== undefined && value !== "")
        .map(([name, value, copy]) => (
          <div key={name}>
            <dt>{name}</dt>
            <dd>
              <span>{value}</span>
              {copy && <Copy text={copy} />}
            </dd>
          </div>
        ))}
    </dl>
  );
}

/** One log row whole, read only once it is asked for. */
function Row({ conversationId, seq }: { conversationId: string; seq: number }) {
  const [text, setText] = useState<string | null>(null);
  useEffect(() => {
    let gone = false;
    api
      .sessionEvent(conversationId, seq)
      .then((t) => !gone && setText(t ?? "This row is gone."))
      .catch((e) => !gone && setText(String(e)));
    return () => {
      gone = true;
    };
  }, [conversationId, seq]);
  if (text === null) return <p className="sd-quiet">Reading…</p>;
  return (
    <div className="sd-raw">
      <Copy text={text} />
      <pre>{text}</pre>
    </div>
  );
}

/** A line that opens to show more. Its content is not built until it is open. */
function Fold({ line, children, className = "" }: { line: ReactNode; children: () => ReactNode; className?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <li className={`sd-fold ${className}`}>
      <button className="sd-fold-line" aria-expanded={open} onClick={() => setOpen(!open)}>
        {line}
      </button>
      {open && <div className="sd-fold-body">{children()}</div>}
    </li>
  );
}

function Overview({ r }: { r: SessionReport }) {
  const open = useAppStore((s) => s.setSessionDetails);
  const reveal = useAppStore((s) => s.revealInSystem);
  const c = r.conversation;
  const t = r.totals;
  return (
    <>
      <Facts
        rows={[
          ["Session id", <code key="id">{c.id}</code>, c.id],
          ["Title", c.title],
          ["Created", stamp(c.created_at)],
          ["Updated", stamp(c.updated_at)],
          [
            "Parent session",
            c.parent_conversation_id && (
              <button className="btn-text sd-link" onClick={() => open(c.parent_conversation_id!)}>
                {c.parent_conversation_id}
              </button>
            ),
          ],
          ["Agent app session", r.engine_session && <code>{r.engine_session}</code>, r.engine_session ?? undefined],
          ["Model", c.model_id],
          ["Leader", c.leader ?? "not chosen"],
          ["Persona id", c.persona_id],
          ["Way of working", c.style ?? "not chosen"],
          ["Overrides", c.overrides_json],
          ["Project", r.project && `${r.project.name} (${r.project.id})`],
          ["Folder", r.folder],
          ["Folder trust", r.folder && r.folder_trust],
          ["Workspace mode", c.workspace ? "on" : null],
          ["External agents", c.engines_off ? "kept out of this chat" : null],
          ["Started from", c.remote_sender && `${c.remote_sender} (${c.remote_transport})`],
          ["Summary covers up to", c.summary_upto_message_id],
        ]}
      />
      <h3 className="sd-head">Totals</h3>
      <Facts
        rows={[
          ["Messages", `${t.user_messages} from you, ${t.assistant_messages} answers`],
          ["Runs", t.runs],
          ["Tool calls", `${t.tool_calls}, ${t.tool_errors} failed`],
          ["Tokens", `${t.prompt_tokens.toLocaleString("en-US")} in, ${t.output_tokens.toLocaleString("en-US")} out`],
          ["Helper tokens", t.helper_tokens ? t.helper_tokens.toLocaleString("en-US") : null],
          ["Cost at list prices", t.cost_usd === null ? null : money(t.cost_usd)],
          ["Artifacts", t.artifacts],
          ["Files changed", t.files_changed],
          ["Compactions", t.compactions],
          ["Log rows", t.events],
        ]}
      />
      <h3 className="sd-head">This app</h3>
      <Facts
        rows={[
          ["Version", r.app_version],
          ["System", r.os],
          ["Data version", r.schema_version],
          [
            "Data folder",
            <button key="data" className="btn-text sd-link" onClick={() => void reveal(r.data_dir)}>
              {r.data_dir}
            </button>,
            r.data_dir,
          ],
        ]}
      />
    </>
  );
}

const list = (v: unknown): string => (Array.isArray(v) ? v.join(", ") : "");

function Setup({ setup }: { setup: Record<string, unknown> }) {
  const flags = ["headless", "read_only", "plan_first", "remote"].filter((k) => setup[k] === true);
  const text = (k: string) => (setup[k] === null || setup[k] === undefined ? null : String(setup[k]));
  return (
    <Facts
      rows={[
        ["Tier", text("tier")],
        ["Effort", text("effort")],
        ["Temperature", text("temperature")],
        ["Context window", text("context_window") ?? "unknown"],
        ["Max steps", text("max_steps")],
        ["Plan ceiling", text("plan_ceiling")],
        ["Delegation depth", text("max_depth")],
        ["Time limit (s)", text("time_limit_s")],
        ["Plan mode", text("plan_mode")],
        ["Parent run", text("parent_run")],
        ["Skill", text("skill")],
        ["Delegate to", text("delegate_to")],
        ["Flags", flags.join(", ")],
        ["Tools on", setup.tools_enabled === false ? "no" : null],
        ["Toolsets", list(setup.toolsets) || "none"],
        ["Tools offered at start", list(setup.tools) || "none"],
        ["Held back", list(setup.held_back)],
      ]}
    />
  );
}

function Run({ conversationId, run }: { conversationId: string; run: SessionRun }) {
  const used = tokens(run);
  const failed = run.tool_calls.filter((c) => c.ok === false).length;
  const bad = !run.live && run.stop_reason !== "completed";
  return (
    <Fold
      line={
        <>
          <span className="sd-when">{stamp(run.started_at)}</span>
          <span className={bad ? "sd-bad" : ""}>{ending(run)}</span>
          <span className="sd-rest">
            {[
              duration(run),
              run.steps !== null ? count(run.steps, "step") : "",
              count(run.tool_calls.length, "tool call"),
              failed ? `${failed} failed` : "",
              run.model ?? "",
            ]
              .filter(Boolean)
              .join(" · ")}
          </span>
        </>
      }
    >
      {() => (
        <>
          <Facts
            rows={[
              ["Run id", <code key="id">{run.run_id}</code>, run.run_id],
              ["Ended", run.ended_at === null ? null : stamp(run.ended_at)],
              ["Model", [run.model, run.provenance].filter(Boolean).join(", ")],
              ["Tokens", `${used.prompt.toLocaleString("en-US")} in, ${used.output.toLocaleString("en-US")} out`],
              ["Cost at list prices", run.cost_usd === null ? null : money(run.cost_usd)],
              ["Paid by", run.usage[run.usage.length - 1]?.paid_by],
              ["Sent at start", run.prompt_seq === null ? null : `${count(run.prompt_messages, "message")}, ${count(run.prompt_chars, "character")}`],
              ["Said while it ran", run.steers.join(" | ")],
            ]}
          />
          <h4 className="sd-head">Setup</h4>
          {run.setup ? <Setup setup={run.setup} /> : <p className="sd-quiet">Not recorded for this run.</p>}
          {run.tool_calls.length > 0 && (
            <>
              <h4 className="sd-head">Tool calls</h4>
              <ul className="sd-list">
                {run.tool_calls.map((call, i) => (
                  <Fold
                    key={`${call.call_seq}:${i}`}
                    line={
                      <>
                        <span className="sd-kind">{call.name}</span>
                        <span className={call.ok === false ? "sd-bad" : ""}>
                          {call.ok === null ? "no result" : call.ok ? "ok" : "failed"}
                        </span>
                        <span className="sd-rest">{call.args}</span>
                      </>
                    }
                  >
                    {() => (
                      <>
                        <p className="sd-quiet">Call, row {call.call_seq}</p>
                        <Row conversationId={conversationId} seq={call.call_seq} />
                        {call.result_seq !== null && (
                          <>
                            <p className="sd-quiet">Result, row {call.result_seq}</p>
                            <Row conversationId={conversationId} seq={call.result_seq} />
                          </>
                        )}
                      </>
                    )}
                  </Fold>
                ))}
              </ul>
            </>
          )}
          {run.prompt_seq !== null && (
            <ul className="sd-list">
              <Fold line={<span>What the model was sent at the start</span>}>
                {() => <Row conversationId={conversationId} seq={run.prompt_seq!} />}
              </Fold>
            </ul>
          )}
        </>
      )}
    </Fold>
  );
}

function Log({ r }: { r: SessionReport }) {
  const [kind, setKind] = useState("");
  const kinds = [...new Set(r.events.map((e) => e.kind))].sort();
  const rows = r.events.filter((e) => !kind || e.kind === kind);
  if (r.events.length === 0) return <p className="sd-quiet">Nothing is in the log yet.</p>;
  return (
    <>
      <Select
        label="Kind of row"
        value={kind}
        onChange={setKind}
        options={[
          { value: "", label: "All rows", hint: String(r.events.length) },
          ...kinds.map((k) => ({ value: k, label: k, hint: String(r.events.filter((e) => e.kind === k).length) })),
        ]}
      />
      <ul className="sd-list">
        {rows.map((e) => (
          <Fold
            key={e.seq}
            line={
              <>
                <span className="sd-seq">{e.seq}</span>
                <span className="sd-when">{stamp(e.at)}</span>
                <span className="sd-kind">{e.kind}</span>
                <span className="sd-rest">{e.preview}</span>
              </>
            }
          >
            {() => (
              <>
                <Facts rows={[["Run id", e.run_id, e.run_id ?? undefined], ["Size", count(e.bytes, "byte")]]} />
                <Row conversationId={r.conversation.id} seq={e.seq} />
              </>
            )}
          </Fold>
        ))}
      </ul>
    </>
  );
}

function Messages({ r }: { r: SessionReport }) {
  if (r.messages.length === 0) return <p className="sd-quiet">No messages yet.</p>;
  return (
    <ul className="sd-list">
      {r.messages.map((m) => (
        <Fold
          key={m.id}
          line={
            <>
              <span className="sd-when">{stamp(m.created_at)}</span>
              <span className="sd-kind">{m.role}</span>
              {m.stop_reason && <span className={m.stop_reason === "completed" ? "" : "sd-bad"}>{m.stop_reason}</span>}
              <span className="sd-rest">{m.preview}</span>
            </>
          }
        >
          {() => (
            <>
              <Facts
                rows={[
                  ["Message id", <code key="id">{m.id}</code>, m.id],
                  ["Model", [m.model_name, m.model_provenance].filter(Boolean).join(", ")],
                  ["Author", m.author],
                  ["Spoken to", m.addressed],
                  ["Spoken", m.spoken ? "yes" : null],
                  ["Attachments", m.attachments || null],
                  ["Length", count(m.chars, "character")],
                ]}
              />
              {m.context !== null && (
                <>
                  <p className="sd-quiet">What reached this answer</p>
                  <div className="sd-raw">
                    <pre>{JSON.stringify(m.context, null, 2)}</pre>
                  </div>
                </>
              )}
            </>
          )}
        </Fold>
      ))}
    </ul>
  );
}

/** A heading and its list, left out when the list is empty. */
function Group({ title, size, children }: { title: string; size: number; children: ReactNode }) {
  if (size === 0) return null;
  return (
    <>
      <h3 className="sd-head">
        {title} ({size})
      </h3>
      <ul className="sd-list">{children}</ul>
    </>
  );
}

const json = (v: unknown) => (
  <div className="sd-raw">
    <pre>{JSON.stringify(v, null, 2)}</pre>
  </div>
);

function Records({ r }: { r: SessionReport }) {
  const open = useAppStore((s) => s.setSessionDetails);
  const nothing = [r.helpers, r.decisions, r.tool_counts, r.tool_fixes, r.skills, r.posts, r.activity].every((a) => a.length === 0);
  if (nothing) return <p className="sd-quiet">No other records for this chat.</p>;
  return (
    <>
      <Group title="Helpers" size={r.helpers.length}>
        {r.helpers.map((h) => (
          <Fold
            key={h.id}
            line={
              <>
                <span className="sd-when">{stamp(h.started_at)}</span>
                <span className="sd-kind">{h.member ?? h.agent}</span>
                <span className={h.status === "error" ? "sd-bad" : ""}>{h.status}</span>
                <span className="sd-rest">{h.task}</span>
              </>
            }
          >
            {() => (
              <>
                <button className="btn-secondary" onClick={() => open(h.child_conversation_id)}>
                  Open this helper's details
                </button>
                {json(h)}
              </>
            )}
          </Fold>
        ))}
      </Group>
      <Group title="Quick decisions" size={r.decisions.length}>
        {r.decisions.map((d) => (
          <Fold
            key={d.id}
            line={
              <>
                <span className="sd-when">{stamp(d.at)}</span>
                <span className="sd-kind">{d.purpose}</span>
                <span className="sd-rest">{[d.action, d.model, `${d.ms} ms`].filter(Boolean).join(" · ")}</span>
              </>
            }
          >
            {() => json(d)}
          </Fold>
        ))}
      </Group>
      <Group title="Tools, worked and tried" size={r.tool_counts.length}>
        {r.tool_counts.map((t) => (
          <li key={t.tool_name} className="sd-plain">
            <span className="sd-kind">{t.tool_name}</span>
            <span className={t.ok < t.total ? "sd-bad" : ""}>
              {t.ok} of {t.total}
            </span>
          </li>
        ))}
      </Group>
      <Group title="Failed calls that were then fixed" size={r.tool_fixes.length}>
        {r.tool_fixes.map((f, i) => (
          <Fold
            key={i}
            line={
              <>
                <span className="sd-kind">{f.tool_name}</span>
                <span className="sd-rest">{f.error}</span>
              </>
            }
          >
            {() => json(f)}
          </Fold>
        ))}
      </Group>
      <Group title="Skills used" size={r.skills.length}>
        {r.skills.map((s, i) => (
          <li key={i} className="sd-plain">
            <span className="sd-when">{stamp(s.created_at)}</span>
            <span className="sd-kind">{s.skill_name}</span>
            <span className="sd-rest">
              {count(s.tool_failures, "tool failure")}
              {s.corrected ? " · corrected" : ""}
            </span>
          </li>
        ))}
      </Group>
      <Group title="Team posts" size={r.posts.length}>
        {r.posts.map((p) => (
          <Fold
            key={p.id}
            line={
              <>
                <span className="sd-when">{stamp(p.created_at)}</span>
                <span className="sd-kind">{p.kind}</span>
                <span className="sd-rest">
                  {p.from_member} to {p.to_member}: {p.text}
                </span>
              </>
            }
          >
            {() => json(p)}
          </Fold>
        ))}
      </Group>
      <Group title="Activity" size={r.activity.length}>
        {r.activity.map((a) => (
          <li key={a.id} className="sd-plain">
            <span className="sd-when">{stamp(a.created_at)}</span>
            <span className="sd-kind">{a.kind}</span>
            <span className="sd-rest" title={a.detail}>
              {a.detail}
            </span>
          </li>
        ))}
      </Group>
    </>
  );
}

function Dialog({ id }: { id: string }) {
  const close = useAppStore((s) => s.setSessionDetails);
  const [report, setReport] = useState<SessionReport | null>(null);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState("");
  const [tab, setTab] = useState<Tab>("overview");

  const load = useCallback(() => {
    setError("");
    return api
      .sessionReport(id)
      .then(setReport)
      .catch((e) => setError(String(e).replace(/^Error:\s*/, "")));
  }, [id]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") close(null);
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [close]);

  async function saveReport() {
    setSaved("");
    setError("");
    try {
      const dest = await save({ defaultPath: `session-${id}.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
      if (!dest) return;
      const full = await api.sessionReport(id, true);
      await api.saveArtifactFile(dest, "code", JSON.stringify(full, null, 2));
      setSaved(`Saved to ${dest}`);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  }

  const tabs: [Tab, string][] = [
    ["overview", "Overview"],
    ["runs", report ? `Runs (${report.runs.length})` : "Runs"],
    ["log", report ? `Log (${report.events.length})` : "Log"],
    ["messages", report ? `Messages (${report.messages.length})` : "Messages"],
    ["records", "Records"],
  ];

  return (
    <div className="confirm-backdrop" onClick={() => close(null)}>
      <div
        className="confirm-card sd-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Session details"
        onClick={(e) => e.stopPropagation()}
      >
        <button className="sd-x" aria-label="Close session details" title="Close" onClick={() => close(null)}>
          <CloseIcon size={14} />
        </button>
        <h2 className="confirm-title">Session details</h2>
        <p className="confirm-body sd-sub">{report?.conversation.title ?? "Reading…"}</p>
        <div className="segmented sd-tabs" role="tablist" aria-label="Parts of the report">
          {tabs.map(([key, label]) => (
            <button
              key={key}
              role="tab"
              aria-selected={tab === key}
              className={`segmented-item ${tab === key ? "on" : ""}`}
              onClick={() => setTab(key)}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="sd-body" role="tabpanel" key={tab}>
          {report && tab === "overview" && <Overview r={report} />}
          {report && tab === "runs" && (
            <>
              {report.runs.length === 0 && <p className="sd-quiet">Nothing has run in this chat yet.</p>}
              <ul className="sd-list">
                {/* Newest first: the run that just went wrong is the one being looked for. */}
                {[...report.runs].reverse().map((run) => (
                  <Run key={run.run_id} conversationId={id} run={run} />
                ))}
              </ul>
            </>
          )}
          {report && tab === "log" && <Log r={report} />}
          {report && tab === "messages" && <Messages r={report} />}
          {report && tab === "records" && <Records r={report} />}
        </div>
        {/* One line, always there, so the dialog never changes height. */}
        <p className={`sd-status ${error ? "sd-bad" : ""}`} role="status">
          {error || saved || "A saved report holds the messages and tool output of this chat."}
        </p>
        <div className="confirm-actions sd-actions">
          <button className="confirm-cancel" onClick={() => void load()}>
            Refresh
          </button>
          <button
            className="confirm-cancel"
            disabled={!report}
            onClick={() => report && void navigator.clipboard?.writeText(summaryText(report)).then(() => setSaved("Summary copied."))}
          >
            Copy summary
          </button>
          <button className="confirm-cancel" disabled={!report} onClick={() => void saveReport()}>
            Save full report…
          </button>
          <button className="confirm-cancel sd-close" onClick={() => close(null)}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * `DBG-3`: what the app recorded about one chat, for finding a fault or checking how its runs
 * went. A dialog of a fixed size over everything; only its body scrolls. Opened from a chat's
 * menu, and from here for a helper's or a parent's chat.
 */
export default function SessionDetails() {
  const id = useAppStore((s) => s.sessionDetailsId);
  if (!id) return null;
  // Keyed, so another chat starts on a clean dialog.
  return <Dialog key={id} id={id} />;
}
