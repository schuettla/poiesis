import type { SessionReport, SessionRun } from "../../lib/api";

/** Date and time to the second, in this computer's time. Debugging lines up against logs. */
export function stamp(ms: number): string {
  if (!ms) return "";
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}

export function duration(run: SessionRun): string {
  if (run.ended_at === null) return "";
  const s = Math.max(0, run.ended_at - run.started_at) / 1000;
  return s < 60 ? `${s.toFixed(1)} s` : `${Math.floor(s / 60)} min ${Math.round(s % 60)} s`;
}

export const count = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
export const money = (usd: number) => `$${usd.toFixed(usd < 0.01 ? 5 : 3)}`;

/** The stop reason as logged, or what its absence means. */
export function ending(run: SessionRun): string {
  if (run.stop_reason) return run.stop_reason;
  if (run.live) return "running";
  // An agent app's job has usage and no log of its own.
  return run.prompt_seq === null ? "no log" : "no stop recorded";
}

export function tokens(run: SessionRun): { prompt: number; output: number } {
  return run.usage.reduce(
    (sum, u) => ({ prompt: sum.prompt + u.prompt_tokens, output: sum.output + u.output_tokens }),
    { prompt: 0, output: 0 }
  );
}

/** The report as a short text, to paste into a bug report. Ids, versions and what each run
 * did; no message text and no tool output. */
export function summaryText(r: SessionReport): string {
  const c = r.conversation;
  const t = r.totals;
  const lines = [
    `Session ${c.id}`,
    `Poiesis ${r.app_version} · ${r.os} · data v${r.schema_version}`,
    `Created ${stamp(c.created_at)} · updated ${stamp(c.updated_at)}`,
    `Model ${c.model_id ?? "not set"} · leader ${c.leader ?? "not chosen"} · style ${c.style ?? "not chosen"}`,
    c.parent_conversation_id ? `Parent session ${c.parent_conversation_id}` : null,
    r.engine_session ? `Agent app session ${r.engine_session}` : null,
    `${count(t.user_messages + t.assistant_messages, "message")} · ${count(t.runs, "run")} · ${count(t.tool_calls, "tool call")} (${t.tool_errors} failed) · ${t.prompt_tokens} tokens in, ${t.output_tokens} out`,
    "",
    ...r.runs.flatMap((run) => {
      const used = tokens(run);
      const setup = run.setup;
      const failed = run.tool_calls.filter((call) => call.ok === false);
      return [
        [
          `Run ${run.run_id}`,
          stamp(run.started_at),
          ending(run),
          duration(run),
          run.steps !== null ? count(run.steps, "step") : "",
          [run.model, run.provenance].filter(Boolean).join(" "),
          `${used.prompt} in, ${used.output} out`,
        ]
          .filter(Boolean)
          .join(" · "),
        setup ? `  tier ${setup.tier} · effort ${setup.effort} · max steps ${setup.max_steps} · window ${setup.context_window ?? "unknown"}` : null,
        run.tool_calls.length ? `  tools: ${run.tool_calls.map((call) => call.name).join(", ")}` : null,
        ...failed.map((call) => `  failed ${call.name} (row ${call.result_seq})`),
      ];
    }),
    ...r.helpers.map((h) => `Helper ${h.id} · ${h.agent} · ${h.status}${h.stop_reason ? ` (${h.stop_reason})` : ""} · session ${h.child_conversation_id}`),
  ];
  return lines.filter((l): l is string => l !== null).join("\n");
}
