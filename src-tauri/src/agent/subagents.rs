//! Delegation (`SUB-4`/`SUB-5`): one tool that runs several child agents at
//! once, each in its own conversation with its own fresh context.
//!
//! A child is a real `run_agent` call, not a prompt trick and not a second
//! loop. It gets a `conversations` row, a transcript, artifacts, blocks and a
//! working folder exactly like any run, which is what makes it openable,
//! steerable and stoppable in the UI while it works.
//!
//! Three rules hold the whole thing up:
//!
//! - **Narrowing only.** A child's toolsets are the parent's, intersected with
//!   its own persona's allowlist. Its folder and trust are the parent's. Depth
//!   increments and never resets.
//! - **One owner of the reply.** The lead synthesises. A child's text reaches
//!   the lead as a tool result and the user as an openable transcript — never
//!   as the answer.
//! - **The child cannot see the conversation.** It gets its task and nothing
//!   else. That is the entire point: context that does not grow with the job.

use futures_util::future::join_all;

use crate::agent::fleet::RunLimits;
use crate::agent::run::{run_agent, RunContext};
use crate::agent::toolsets::{set_step_note, ToolContext, Toolset};
use crate::agent::AgentEvent;
use crate::db::Db;
use crate::runtime::proxy::CancelFlag;

/// Appended to every child's system prompt, unchanged. A child that asks a
/// question is a child that has wasted its whole run: nobody is there.
const CONTRACT: &str = "You are a sub-agent working for a lead agent. You were given one task and you cannot see the conversation it came from. Do the task and finish. Your final message is the whole result the lead receives, so put everything that matters in it and do not ask questions, because nobody will answer. If you cannot finish, say what you found and what is missing.";

/// Added for a background child (`SUB-10`). The lead has already replied by the
/// time this run is halfway done, so the usual "the lead is waiting" reading of
/// the contract is wrong, and a child that behaves as if someone will react to a
/// half-answer will produce one.
const BACKGROUND_CONTRACT: &str = "You are running in the background. The lead has already moved on and will read your report later, so nothing you write reaches anyone until you finish. Anything that would need a person to approve it is refused here, so plan around that rather than trying.";

/// The built-in agent type: no extra system prompt, the parent's own toolsets.
/// It exists in code rather than in the `personas` table so delegation works
/// before the user has made a single persona.
pub const GENERAL: &str = "general";

/// Hard ceiling on children per `delegate` call, whatever Settings says.
const MAX_PARALLEL_CAP: usize = 5;
/// Hard ceiling on children per assistant turn, across several calls.
const MAX_PER_TURN: usize = 6;
/// How much of a child's answer the lead is handed. Past this the lead is
/// reading a transcript rather than a report.
const REPORT_CLIP: usize = 4000;

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([{
        "type": "function",
        "function": {
            "name": "delegate",
            "description": "Run one or more independent sub-tasks as separate agents, in parallel, each with its own fresh context. Use when a task splits into parts that do not depend on each other, or when a part would fill your context with material you only need the conclusion of. Do not use for a single short step you can do yourself.",
            "parameters": {
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "description": "One entry per agent. Two or three is normal.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent": {
                                    "type": "string",
                                    "description": "An agent type by name. Omit for a general agent."
                                },
                                "task": {
                                    "type": "string",
                                    "description": "The complete objective. The agent sees only this, not our conversation."
                                },
                                "output": {
                                    "type": "string",
                                    "description": "The shape of the answer you want back, e.g. 'a bullet per file with its purpose'."
                                }
                            },
                            "required": ["task"]
                        }
                    },
                    "background": {
                        "type": "boolean",
                        "description": "Leave the agents running and carry on without their answers. Use for long work whose result you do not need in this reply — you get run ids back at once, and check_agents or collect_agents reads them later. Default false, which waits for every agent and hands you their reports."
                    }
                },
                "required": ["tasks"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "check_agents",
            "description": "See how the agents you started are doing: what each one is, whether it is queued, working or finished, how many steps it has taken and for how long. Cheap and instant — it never waits.",
            "parameters": { "type": "object", "properties": {} }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "collect_agents",
            "description": "Read the reports of agents you started in the background. By default it waits until they are all finished, so only call it when you actually need their answers.",
            "parameters": {
                "type": "object",
                "properties": {
                    "run_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Which runs to collect. Omit for every agent of yours that has not been read yet."
                    },
                    "wait": {
                        "type": "boolean",
                        "description": "Wait for the unfinished ones (default true). False returns what is ready now and names what is not."
                    }
                }
            }
        }
    }])
}

pub fn handles(name: &str) -> bool {
    matches!(name, "delegate" | "check_agents" | "collect_agents")
}

pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
        "check_agents" => return ("checked".into(), "on my agents".into()),
        "collect_agents" => return ("collected".into(), "what my agents found".into()),
        "delegate" => {}
        _ => return (name.to_string(), String::new()),
    }
    let verb = if wants_background(args) { "started" } else { "delegated" };
    match parse_tasks(args) {
        Ok(tasks) if tasks.len() == 1 => {
            let t = &tasks[0];
            (verb.into(), format!("{} · {}", t.agent_label(), clip(&t.task, 48)))
        }
        Ok(tasks) => (verb.into(), format!("{} agents", tasks.len())),
        Err(_) => (verb.into(), "a sub-task".into()),
    }
}

/// `SUB-10`: did the lead ask for these to keep running without it?
///
/// Read loosely for the same reason `parse_tasks` is: a small model writes
/// `"true"` as often as `true`, and refusing that would silently run the work in
/// the foreground — the opposite of what was asked, with no way to tell.
pub fn wants_background(args: &serde_json::Value) -> bool {
    match args.get("background").or_else(|| args.get("async")) {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => matches!(s.trim(), "true" | "1" | "yes"),
        Some(serde_json::Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        _ => false,
    }
}

/// One requested child, after the arguments have been made sense of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    /// The agent type asked for, verbatim. `None` means the general agent.
    pub agent: Option<String>,
    pub task: String,
    pub output: Option<String>,
}

impl TaskSpec {
    fn agent_label(&self) -> &str {
        self.agent.as_deref().unwrap_or(GENERAL)
    }
}

/// Read `delegate`'s arguments generously.
///
/// Local models are the hard case here: a 4B model asked for an array of
/// objects will send a bare string, a single object, or `prompt` where `task`
/// belongs — and a refusal costs the user a whole turn to teach it nothing.
/// Every shape that unambiguously means one thing is accepted; only an empty
/// task is refused, because there is nothing to hand anyone.
pub fn parse_tasks(args: &serde_json::Value) -> Result<Vec<TaskSpec>, String> {
    let raw = args
        .get("tasks")
        .or_else(|| args.get("agents"))
        .cloned()
        // No `tasks` at all: the whole argument object is one task.
        .unwrap_or_else(|| args.clone());

    let entries: Vec<serde_json::Value> = match raw {
        serde_json::Value::Array(items) => items,
        other => vec![other],
    };

    let mut out = Vec::new();
    for entry in entries {
        let spec = match entry {
            serde_json::Value::String(task) => TaskSpec {
                agent: None,
                task: task.trim().to_string(),
                output: None,
            },
            serde_json::Value::Object(_) => {
                let task = ["task", "prompt", "instruction", "objective", "description"]
                    .iter()
                    .find_map(|k| entry.get(*k).and_then(|v| v.as_str()))
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                TaskSpec {
                    agent: entry
                        .get("agent")
                        .or_else(|| entry.get("agent_type"))
                        .or_else(|| entry.get("name"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                    task,
                    output: entry
                        .get("output")
                        .or_else(|| entry.get("output_format"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                }
            }
            _ => continue,
        };
        if spec.task.is_empty() {
            return Err(
                "Each task needs a `task` string saying what the agent should do. The agent cannot see this conversation, so write the whole objective out."
                    .to_string(),
            );
        }
        out.push(spec);
    }

    if out.is_empty() {
        return Err(
            "Nothing to delegate. Pass `tasks` as a list, each with a `task` describing the whole objective."
                .to_string(),
        );
    }
    Ok(out)
}

/// An agent type the lead may hand a job to.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentType {
    pub name: String,
    pub description: String,
    /// The persona backing it, or `None` for the built-in general agent.
    pub persona_id: Option<String>,
}

/// The general agent plus every persona the user marked delegatable (`SUB-3`).
pub fn agent_types(db: &Db) -> Vec<AgentType> {
    let mut out = vec![AgentType {
        name: GENERAL.to_string(),
        description: "A general agent with the same tools I have.".to_string(),
        persona_id: None,
    }];
    for p in db.list_personas().unwrap_or_default() {
        if !p.spawnable {
            continue;
        }
        out.push(AgentType {
            name: p.name.clone(),
            description: p.description.clone().unwrap_or_default(),
            persona_id: Some(p.id.clone()),
        });
    }
    out
}

/// A settings integer, clamped. Delegation's caps all have this shape.
fn capped(db: &Db, key: &str, default: usize, max: usize) -> usize {
    db.get_setting(key)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(1, max)
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "delegate" => delegate(ctx, args).await,
        "check_agents" => check_agents(ctx),
        "collect_agents" => collect_agents(ctx, args).await,
        _ => Err(format!("The Delegation toolset doesn't provide '{name}'.")),
    }
}

async fn delegate(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let Some(del) = ctx.delegation else {
        return Err("Delegation isn't available in this run. Do the work yourself.".to_string());
    };
    // `SUB-5` guard. The tool is normally withdrawn at max depth, so reaching
    // this means the model called a tool it wasn't offered.
    if del.max_depth == 0 {
        return Err(
            "You are already a delegated agent, so you can't hand this out again. Do the work yourself."
                .to_string(),
        );
    }
    if del.parent_run.cancel.is_cancelled() {
        return Err("The user stopped this run.".to_string());
    }

    let db = ctx.db;
    let mut tasks = parse_tasks(args)?;

    // Caps, and say so — a lead that silently loses a branch reports on work
    // nobody did.
    let mut clipped_note = String::new();
    let max_parallel = capped(db, "subagents.max_parallel", 3, MAX_PARALLEL_CAP);
    if tasks.len() > max_parallel {
        clipped_note = format!(
            "\nI only ran the first {max_parallel} of the {} tasks you asked for. Ask again for the rest if you still need them.",
            tasks.len()
        );
        tasks.truncate(max_parallel);
    }
    let already = db
        .list_subagent_runs(ctx.conversation_id)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.parent_message_id.as_deref() == ctx.assistant_message_id)
        .count();
    let room = MAX_PER_TURN.saturating_sub(already);
    if tasks.len() > room {
        clipped_note = format!(
            "\nThis turn has already used its {MAX_PER_TURN} agents, so I ran {room} of them. Finish with what you have."
        );
        tasks.truncate(room);
    }
    if tasks.is_empty() {
        return Err(format!(
            "This turn has already run {MAX_PER_TURN} agents. Answer with what you have."
        ));
    }

    // `SUB-10`: was this asked for as fire-and-forget, and can it be?
    //
    // Refused for a headless parent on purpose. A scheduled job is accountable
    // for its own run and ends when it ends; a child that outlives it would
    // report to nobody, and its permission prompts would already have been
    // refused for exactly the same reason.
    let asked_background = wants_background(args);
    let background = asked_background && !ctx.headless && crate::agent::background::available();
    if asked_background && !background {
        clipped_note.push_str(
            "\nI couldn't leave these running on their own here, so I waited for them instead. The answers below are complete.",
        );
    }

    let types = agent_types(db);
    let (folder, trust) = db.conversation_folder(ctx.conversation_id).unwrap_or((None, "confirm".into()));
    let max_steps = capped(db, "subagents.max_steps", 8, 24);
    let timeout_secs = capped(db, "subagents.timeout_secs", 300, 3600);
    // `SUB-UI-7`: whether a child may delegate further. Off is the default and
    // the safe answer — this is a fork-bomb guard, not a philosophy.
    let nested = db
        .get_setting("subagents.allow_nested")
        .ok()
        .flatten()
        .is_some_and(|v| v == "true" || v == "1");

    // --- start every child before running any of them, so the Fleet card is
    // complete the moment work begins rather than filling in one row at a time.
    struct Child {
        run: std::sync::Arc<crate::agent::fleet::RunHandle>,
        agent: String,
        conversation_id: String,
        messages: Vec<serde_json::Value>,
        sink: crate::agent::run::AgentEventSink,
    }

    let mut children = Vec::new();
    for (index, spec) in tasks.into_iter().enumerate() {
        let matched = spec.agent.as_ref().and_then(|want| {
            types
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(want.trim()))
        });
        let agent = matched.map(|t| t.name.clone()).unwrap_or_else(|| GENERAL.to_string());
        let persona = matched
            .and_then(|t| t.persona_id.clone())
            .and_then(|id| db.get_persona(&id).ok().flatten());

        let title = format!("{agent} · {}", clip(&spec.task, 40));
        let conv = db
            .create_conversation(&title, None, false)
            .map_err(|e| format!("I couldn't open a workspace for the agent: {e}"))?;
        let _ = db.set_conversation_parent(&conv.id, ctx.conversation_id);
        // Inherited, never widened: the child works where the parent works.
        if let Some(path) = folder.as_deref() {
            let _ = db.set_conversation_folder(&conv.id, Some(path));
            let _ = db.set_conversation_trust(&conv.id, &trust);
        }
        if let Some(p) = &persona {
            let _ = db.set_conversation_persona(&conv.id, Some(&p.id), None);
        }

        let run = del.fleet.open(
            &conv.id,
            CancelFlag::new(),
            Some(del.parent_run.id.clone()),
            del.parent_run.depth + 1,
        );
        let _ = db.create_subagent_run(
            &run.id,
            ctx.conversation_id,
            ctx.assistant_message_id,
            &conv.id,
            &agent,
            &spec.task,
        );
        ctx.sink.emit(AgentEvent::SubSpawned {
            run_id: run.id.clone(),
            conversation_id: conv.id.clone(),
            agent: agent.clone(),
            task: spec.task.clone(),
            index,
        });

        let mut system = String::new();
        if let Some(p) = &persona {
            if !p.system_prompt.trim().is_empty() {
                system.push_str(p.system_prompt.trim());
                system.push_str("\n\n");
            }
        }
        system.push_str(CONTRACT);
        if background {
            system.push_str("\n\n");
            system.push_str(BACKGROUND_CONTRACT);
        }
        if let Some(brief) = crate::agent::filesystem::working_folder_brief(db, &conv.id) {
            system.push_str("\n\n");
            system.push_str(&brief);
        }
        let mut user = spec.task.clone();
        if let Some(output) = &spec.output {
            user.push_str("\n\nGive the answer in this shape: ");
            user.push_str(output);
        }

        children.push(Child {
            sink: ctx.sink.child(&run.id),
            run,
            agent,
            conversation_id: conv.id,
            messages: vec![
                serde_json::json!({ "role": "system", "content": system }),
                serde_json::json!({ "role": "user", "content": user }),
            ],
        });
    }

    // --- `SUB-10`: hand them to the pool and return now.
    //
    // Everything above already happened — the conversations exist, the runs are
    // in the fleet, the rows are written and the Fleet card has its rows — so
    // the only difference from here is who waits.
    if background {
        let mut started = String::new();
        let spawns: Vec<crate::agent::background::Spawn> = children
            .into_iter()
            .map(|child| {
                let _ = db.set_subagent_status(&child.run.id, "queued");
                started.push_str(&format!("- {} · {}\n", child.run.id, child.agent));
                crate::agent::background::Spawn {
                    run_id: child.run.id.clone(),
                    child_conversation_id: child.conversation_id,
                    agent: child.agent,
                    messages: child.messages,
                    endpoint: del.endpoint.clone(),
                    local_endpoint: ctx.local_endpoint.cloned(),
                    model_name: del.model_name.to_string(),
                    temperature: del.temperature,
                    toolsets: del.toolsets.clone(),
                    provenance: del.provenance.to_string(),
                    context_window: del.context_window,
                    max_steps,
                    timeout_secs: timeout_secs as u64,
                }
            })
            .collect();
        let count = spawns.len();
        crate::agent::background::submit(spawns);
        set_step_note(ctx, format!("{count} agent{} in the background", plural(count)));
        return Ok(format!(
            "{count} agent{} {} now working in the background. You do not have their answers yet:\n{started}\nCarry on without them. Use check_agents to see how they are doing, or collect_agents to read their reports once you need them — that one waits, so do not call it until you have nothing else to do. If you have nothing left in this turn, tell the user what you started and finish; they are told the moment each one lands.{clipped_note}",
            plural(count),
            if count == 1 { "is" } else { "are" }
        ));
    }

    // --- run them together.
    let limits = RunLimits {
        max_iterations: max_steps,
        deadline: Some(std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs as u64)),
        max_depth: usize::from(nested),
    };
    let started = std::time::Instant::now();

    let outcomes = join_all(children.iter().map(|child| {
        let rc = RunContext {
            run: &child.run,
            limits: &limits,
            fleet: Some(del.fleet),
            ceiling: Some(&del.toolsets),
            provenance: del.provenance,
            context_window: del.context_window,
            // `PLN`: one plan per run. A child may write its own; it never
            // touches its lead's.
            plan: None,
        };
        // Boxed because this is `run_agent` calling itself through a tool call:
        // without it the future's size is defined in terms of itself.
        Box::pin(async move {
            run_agent(
                ctx.client,
                del.endpoint,
                ctx.local_endpoint,
                db,
                ctx.mgr,
                ctx.embed_mgr,
                ctx.rerank_mgr,
                ctx.perms,
                ctx.memory,
                ctx.browser_pool,
                &child.conversation_id,
                None,
                ctx.data_dir,
                del.model_name,
                child.messages.clone(),
                del.temperature,
                true,
                // `SUB-8`: a foreground child may prompt, and the panel says
                // which agent is asking. A child of a headless run stays headless.
                ctx.headless,
                &rc,
                &child.sink,
            )
            .await
        })
    }))
    .await;

    // --- report back.
    let mut report = String::new();
    for (i, (child, outcome)) in children.iter().zip(outcomes.iter()).enumerate() {
        let secs = child.run.elapsed_ms() / 1000;
        let status = match outcome.stop_reason {
            crate::agent::fleet::StopReason::Completed => "done",
            crate::agent::fleet::StopReason::Aborted => "stopped",
            crate::agent::fleet::StopReason::Error => "error",
            _ => "done",
        };
        let headline = headline(outcome.stop_reason.as_str(), outcome.steps, secs);
        let text = outcome.text.trim();
        let text = if text.is_empty() { "(it produced no text)" } else { text };

        let _ = db.finish_subagent_run(
            &child.run.id,
            status,
            outcome.stop_reason.as_str(),
            text,
            outcome.steps,
        );
        ctx.sink.emit(AgentEvent::SubEnded {
            run_id: child.run.id.clone(),
            status: status.to_string(),
            stop_reason: outcome.stop_reason.as_str().to_string(),
            summary: text.to_string(),
            steps: outcome.steps,
            ms: child.run.elapsed_ms(),
        });
        del.fleet.close(&child.run.id);

        report.push_str(&format!(
            "Agent {} ({}) — {headline}\n{}\n\n",
            i + 1,
            child.agent,
            clip(text, REPORT_CLIP)
        ));

        // What the child left behind, so the lead can point at it by name
        // instead of describing something the user cannot open.
        let artifacts = db.list_artifacts(&child.conversation_id).unwrap_or_default();
        if !artifacts.is_empty() {
            let names: Vec<String> = artifacts.iter().map(|a| a.title.clone()).collect();
            report.push_str(&format!("It made: {}\n\n", names.join(", ")));
        }
    }
    report.push_str(&clipped_note);

    let total = started.elapsed().as_secs();
    set_step_note(
        ctx,
        format!(
            "{} agent{}, {total}s",
            children.len(),
            if children.len() == 1 { "" } else { "s" }
        ),
    );
    Ok(report.trim_end().to_string())
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// How a finished child's run is summarised for the lead. One function so a
/// foreground report and a collected background report cannot describe the same
/// ending in two different ways — to the lead there is no difference between
/// them, and the wording is the only thing that could imply one.
fn headline(stop_reason: &str, steps: usize, secs: u64) -> String {
    match stop_reason {
        "completed" => format!("completed in {secs}s, {steps} steps"),
        "aborted" => format!("stopped by the user after {steps} steps — this is what it had"),
        "timeout" => format!("ran out of time after {steps} steps — this is what it had"),
        "max_steps" => format!("hit its step limit ({steps} steps) — this is what it had"),
        "error" => format!("failed after {steps} steps"),
        _ => format!("finished after {steps} steps"),
    }
}

/// `SUB-11`: how the agents you started are doing, right now, without waiting.
///
/// Reads the durable rows rather than the fleet, so it answers the same way
/// after a restart as during the turn that started them. The fleet is consulted
/// only for the one thing a row cannot know: how far a *live* run has got, since
/// the row is written at the start and at the end and never in between.
fn check_agents(ctx: &ToolContext<'_>) -> Result<String, String> {
    let rows = ctx.db.list_subagent_runs(ctx.conversation_id).unwrap_or_default();
    if rows.is_empty() {
        return Ok("You have not started any agents in this conversation.".to_string());
    }
    let now = crate::db::now_ms();
    let mut out = String::new();
    let mut live = 0usize;
    for row in &rows {
        let secs = (row.ended_at.unwrap_or(now) - row.started_at).max(0) / 1000;
        let line = match row.status.as_str() {
            "queued" => {
                live += 1;
                "queued, waiting for a free slot".to_string()
            }
            "running" => {
                live += 1;
                let steps = ctx
                    .delegation
                    .and_then(|d| d.fleet.get(&row.id))
                    .map(|r| r.steps())
                    .unwrap_or(row.steps);
                format!("working · {steps} steps · {secs}s")
            }
            _ => headline(row.stop_reason.as_deref().unwrap_or(""), row.steps, secs as u64),
        };
        out.push_str(&format!("{} · {} · {line}\n  {}\n", row.id, row.agent, clip(&row.task, 70)));
    }
    if live > 0 {
        out.push_str(&format!(
            "\n{live} still going. collect_agents waits for them; it is the only way to read a report.\n"
        ));
    }
    Ok(out.trim_end().to_string())
}

/// How long `collect_agents` will ever block, whatever it was asked for. A lead
/// that waits forever is a turn the user has to stop by hand, which is exactly
/// the situation background delegation exists to remove.
const COLLECT_CAP: std::time::Duration = std::time::Duration::from_secs(600);
/// How often the wait looks again. Long enough to cost nothing, short enough
/// that a child finishing does not sit unread.
const COLLECT_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// `SUB-11`: read what the agents found, waiting for them if asked.
async fn collect_agents(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<String, String> {
    let db = ctx.db;
    let wanted: Option<Vec<String>> = args
        .get("run_ids")
        .or_else(|| args.get("runs"))
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());

    let mine = |rows: Vec<crate::db::SubagentRun>| -> Vec<crate::db::SubagentRun> {
        match &wanted {
            Some(ids) => rows.into_iter().filter(|r| ids.iter().any(|w| w == &r.id)).collect(),
            None => rows,
        }
    };

    let rows = mine(db.list_subagent_runs(ctx.conversation_id).unwrap_or_default());
    if rows.is_empty() {
        return Ok(match &wanted {
            Some(_) => "None of those run ids belong to an agent you started here. check_agents lists the ones that do.".to_string(),
            None => "You have not started any agents in this conversation.".to_string(),
        });
    }

    // Wait, unless told not to. `wait` defaults to true because that is what
    // "collect" means; the flag exists for a lead that wants a snapshot.
    let wait = !matches!(args.get("wait"), Some(serde_json::Value::Bool(false)));
    let cancel = ctx.delegation.map(|d| d.parent_run.cancel.clone());
    let deadline = std::time::Instant::now() + COLLECT_CAP;
    let mut rows = rows;
    let mut gave_up = String::new();
    if wait {
        loop {
            if rows.iter().all(|r| r.ended_at.is_some()) {
                break;
            }
            if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                gave_up = "\nThe user stopped this run while I was waiting, so some of these are unfinished.".to_string();
                break;
            }
            if std::time::Instant::now() >= deadline {
                gave_up = "\nI stopped waiting after 10 minutes. The unfinished ones are still going — call collect_agents again later.".to_string();
                break;
            }
            tokio::time::sleep(COLLECT_POLL).await;
            rows = mine(db.list_subagent_runs(ctx.conversation_id).unwrap_or_default());
        }
    }

    let mut report = String::new();
    let mut pending = 0usize;
    for (i, row) in rows.iter().enumerate() {
        if row.ended_at.is_none() {
            pending += 1;
            report.push_str(&format!(
                "Agent {} ({}) — still working, no report yet [{}]\n\n",
                i + 1,
                row.agent,
                row.id
            ));
            continue;
        }
        let secs = ((row.ended_at.unwrap_or(row.started_at) - row.started_at).max(0) / 1000) as u64;
        let text = row.result.as_deref().unwrap_or("").trim();
        let text = if text.is_empty() { "(it produced no text)" } else { text };
        report.push_str(&format!(
            "Agent {} ({}) — {}\n{}\n\n",
            i + 1,
            row.agent,
            headline(row.stop_reason.as_deref().unwrap_or(""), row.steps, secs),
            clip(text, REPORT_CLIP)
        ));
        let artifacts = db.list_artifacts(&row.child_conversation_id).unwrap_or_default();
        if !artifacts.is_empty() {
            let names: Vec<String> = artifacts.iter().map(|a| a.title.clone()).collect();
            report.push_str(&format!("It made: {}\n\n", names.join(", ")));
        }
    }
    report.push_str(&gave_up);
    let done = rows.len() - pending;
    set_step_note(ctx, format!("{done} report{}", plural(done)));
    Ok(report.trim_end().to_string())
}

/// Kept so `Toolset::Subagents` and this module can't drift on the id.
#[allow(dead_code)]
const TOOLSET: Toolset = Toolset::Subagents;

#[cfg(test)]
mod tests {
    use super::*;

    fn tasks(json: serde_json::Value) -> Vec<TaskSpec> {
        parse_tasks(&json).expect("should parse")
    }

    /// `SUB-T3`: a bare string, one object and an array all mean the same thing.
    /// A local model sends all three, and refusing any of them costs a turn.
    #[test]
    fn every_shape_a_small_model_sends_means_the_same_thing() {
        let array = tasks(serde_json::json!({ "tasks": [{ "task": "read the readme" }] }));
        let object = tasks(serde_json::json!({ "tasks": { "task": "read the readme" } }));
        let string = tasks(serde_json::json!({ "tasks": "read the readme" }));
        let bare = tasks(serde_json::json!({ "task": "read the readme" }));
        let alias = tasks(serde_json::json!({ "tasks": [{ "prompt": "read the readme" }] }));
        for got in [object, string, bare, alias] {
            assert_eq!(got, array);
        }
        assert_eq!(array[0].task, "read the readme");
        assert_eq!(array[0].agent, None);
    }

    #[test]
    fn an_empty_task_is_refused_with_something_the_model_can_act_on() {
        let err = parse_tasks(&serde_json::json!({ "tasks": [{ "task": "  " }] })).unwrap_err();
        assert!(err.contains("task"), "{err}");
        assert!(parse_tasks(&serde_json::json!({})).is_err());
    }

    #[test]
    fn an_agent_and_an_output_shape_survive_parsing() {
        let got = tasks(serde_json::json!({
            "tasks": [{ "agent": "researcher", "task": "find the docs", "output": "one bullet each" }]
        }));
        assert_eq!(got[0].agent.as_deref(), Some("researcher"));
        assert_eq!(got[0].output.as_deref(), Some("one bullet each"));
    }

    /// `SUB-T4`: the cap is a clamp, not a rejection — and Settings can lower it
    /// but never raise it past the hard ceiling.
    #[test]
    fn the_parallel_cap_cannot_be_raised_past_the_hard_ceiling() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(capped(&db, "subagents.max_parallel", 3, MAX_PARALLEL_CAP), 3);
        db.set_setting("subagents.max_parallel", "99").unwrap();
        assert_eq!(capped(&db, "subagents.max_parallel", 3, MAX_PARALLEL_CAP), MAX_PARALLEL_CAP);
        db.set_setting("subagents.max_parallel", "nonsense").unwrap();
        assert_eq!(capped(&db, "subagents.max_parallel", 3, MAX_PARALLEL_CAP), 3);
    }

    /// Only personas the user marked delegatable are offered, and the general
    /// agent is always there so delegation works with no personas at all.
    #[test]
    fn only_personas_the_user_opened_up_are_offered() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(agent_types(&db).len(), 1);
        assert_eq!(agent_types(&db)[0].name, GENERAL);

        let mut p = db
            .create_persona(&crate::db::NewPersona {
                name: "researcher".into(),
                system_prompt: "You research.".into(),
                model_id: None,
                params_json: None,
                tools_json: None,
                skills_json: None,
                description: Some("Reads sources and reports back.".into()),
                spawnable: false,
            })
            .unwrap();
        assert_eq!(agent_types(&db).len(), 1, "off by default");

        p.spawnable = true;
        db.update_persona(&p).unwrap();
        let types = agent_types(&db);
        assert_eq!(types.len(), 2);
        assert_eq!(types[1].name, "researcher");
        assert_eq!(types[1].description, "Reads sources and reports back.");
    }

    /// `SUB-10`: the flag has to survive the same sloppiness `parse_tasks`
    /// forgives. Read too strictly, `"background": "true"` would run the work in
    /// the foreground — the opposite of the request, silently.
    #[test]
    fn every_shape_of_the_background_flag_means_the_same_thing() {
        for yes in [
            serde_json::json!({ "background": true }),
            serde_json::json!({ "background": "true" }),
            serde_json::json!({ "background": "yes" }),
            serde_json::json!({ "background": 1 }),
            serde_json::json!({ "async": true }),
        ] {
            assert!(wants_background(&yes), "{yes} should mean background");
        }
        for no in [
            serde_json::json!({}),
            serde_json::json!({ "background": false }),
            serde_json::json!({ "background": "false" }),
            serde_json::json!({ "background": 0 }),
            serde_json::json!({ "tasks": [{ "task": "a" }] }),
        ] {
            assert!(!wants_background(&no), "{no} should mean wait");
        }
    }

    /// `SUB-11`: the same ending reads the same way whether the lead waited for
    /// it or collected it an hour later. A collected report that said "finished"
    /// where a waited one says "hit its step limit" would hide a partial answer.
    #[test]
    fn an_ending_is_described_the_same_way_however_it_was_read() {
        assert_eq!(headline("completed", 3, 12), "completed in 12s, 3 steps");
        assert!(headline("max_steps", 8, 90).contains("this is what it had"));
        assert!(headline("timeout", 8, 90).contains("this is what it had"));
        assert!(headline("aborted", 2, 5).contains("this is what it had"));
        assert!(headline("error", 1, 2).starts_with("failed"));
        // An unknown reason still says how much work there was, and claims
        // nothing about whether the answer is whole.
        assert_eq!(headline("", 4, 9), "finished after 4 steps");
    }

    #[test]
    fn the_toolset_answers_for_all_three_of_its_tools() {
        for name in ["delegate", "check_agents", "collect_agents"] {
            assert!(handles(name), "{name} should be handled here");
        }
        assert!(!handles("read_file"));
        let (verb, _) = describe("check_agents", &serde_json::json!({}));
        assert_eq!(verb, "checked");
        let (verb, _) = describe("collect_agents", &serde_json::json!({}));
        assert_eq!(verb, "collected");
        // Starting agents and waiting for them are different acts, and the
        // timeline says which one happened.
        let (verb, _) = describe(
            "delegate",
            &serde_json::json!({ "tasks": [{ "task": "a" }], "background": true }),
        );
        assert_eq!(verb, "started");
    }

    #[test]
    fn the_timeline_says_how_many_agents_went_out() {
        let (verb, target) = describe(
            "delegate",
            &serde_json::json!({ "tasks": [{ "task": "a" }, { "task": "b" }] }),
        );
        assert_eq!(verb, "delegated");
        assert_eq!(target, "2 agents");
        let (_, one) = describe("delegate", &serde_json::json!({ "tasks": [{ "task": "read the readme" }] }));
        assert_eq!(one, "general · read the readme");
    }
}
