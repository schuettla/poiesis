//! `CTX-3`: prompt assembly, moved off the frontend.
//!
//! Until this module existed, the system prompt was built in `store.ts` and
//! handed to the backend already finished. That worked for exactly one caller —
//! a person typing in the chat window. Every other way a run can start (a
//! scheduled job, a resumed run, a background subagent) had no frontend to ask,
//! so `scheduler::run_custom_job` sent one bare user message with no persona, no
//! standing instructions, no memory index, no skills list and no tool guidance.
//! That is `CTX-1`'s drift, and it is what this module exists to end.
//!
//! Two rules shape the code below.
//!
//! **Same text, byte for byte.** This is a port, not a rewrite. Every block here
//! reproduces its TypeScript original exactly, wording and blank lines included,
//! because `CTX-4` gates the switchover on the two agreeing character for
//! character. Where a comment says "matches `store.ts`", that is a promise, not
//! a note.
//!
//! **JSON text passes through, it is not re-serialized.** Session state, a
//! surface tree and a block's payload are all stored as JSON *text*. JavaScript
//! keeps object keys in insertion order; `serde_json` sorts them. Re-serializing
//! would reorder keys and break byte-identity for no gain, so this module takes
//! those fields as `String` and emits them unchanged. It parses them only when
//! it has to ask a question about them — "is this object empty" — and throws the
//! parse away again.

/// Rough token estimate: 1 token is about 4 characters. Deliberately
/// conservative, matching `estimateTokens` in `context.ts`.
///
/// The length is counted in UTF-16 code units, not bytes and not `char`s,
/// because that is what JavaScript's `String.length` counts. For plain text the
/// three agree; for an emoji they do not, and a budget that disagrees with the
/// meter the user is looking at is worse than a slightly wrong one.
pub fn estimate_tokens(text: &str) -> usize {
    text.encode_utf16().count().div_ceil(4)
}

/// Flat per-image estimate; real cost is model-specific, this is a safe floor.
const IMAGE_TOKEN_COST: usize = 800;

/// Share of the budget reserved for the response plus tool traffic.
const RESPONSE_RESERVE: f64 = 0.25;

/// Recent turns that are never dropped: the thread of the live exchange.
pub const KEEP_RECENT: usize = 6;

/// Fewer in workspace mode: the surface and session state carry the task state.
pub const KEEP_RECENT_WORKSPACE: usize = 3;

/// Estimated tokens for one turn. Text is charged by length; an image is charged
/// a flat `IMAGE_TOKEN_COST` rather than the meaningless character count of its
/// data URI.
pub fn turn_tokens(turn: &serde_json::Value) -> usize {
    match &turn["content"] {
        serde_json::Value::String(s) => estimate_tokens(s),
        serde_json::Value::Array(parts) => parts
            .iter()
            .map(|p| match p["type"].as_str() {
                Some("text") => estimate_tokens(p["text"].as_str().unwrap_or_default()),
                _ => IMAGE_TOKEN_COST,
            })
            .sum(),
        _ => 0,
    }
}

/// What `budget_turns` decided, mirroring `BudgetedTurns` in `context.ts`.
pub struct Budgeted {
    /// What to send, in order: system, kept prior turns, current.
    pub turns: Vec<serde_json::Value>,
    /// How many prior turns did not fit. The overflow is always the oldest
    /// prefix, so a count says everything a slice would.
    pub overflow: usize,
    pub used_tokens: usize,
    pub budget: usize,
    /// History alone exceeds the threshold; the caller should compact first.
    pub needs_compaction: bool,
}

/// Fit `prior` into `budget`, newest first.
///
/// Never drops the system turn, the last `keep_recent` turns, or the current
/// user turn. If those alone overflow we send them anyway and let the engine
/// deal with it, because dropping the user's actual question is never the right
/// answer.
///
/// This exists because llama.cpp truncates from the *front* when a request
/// overflows its window, which quietly eats the system prompt: the surface tree,
/// session state and standing guidance. Deciding here means the engine is never
/// handed more than it can hold.
pub fn budget_turns(
    system: &str,
    prior: &[serde_json::Value],
    current: &serde_json::Value,
    budget: usize,
    keep_recent: usize,
) -> Budgeted {
    let ceiling = budget as f64 * (1.0 - RESPONSE_RESERVE);
    let fixed = estimate_tokens(system) + turn_tokens(current);

    let mut kept: Vec<serde_json::Value> = Vec::new();
    let mut acc = 0usize;

    for (i, turn) in prior.iter().enumerate().rev() {
        let cost = turn_tokens(turn);
        let must_keep = prior.len() - i <= keep_recent;
        if !must_keep && (fixed + acc + cost) as f64 > ceiling {
            break;
        }
        acc += cost;
        kept.insert(0, turn.clone());
    }

    let overflow = prior.len() - kept.len();
    let mut turns = vec![serde_json::json!({ "role": "system", "content": system })];
    turns.extend(kept);
    turns.push(current.clone());

    Budgeted { turns, overflow, used_tokens: fixed + acc, budget, needs_compaction: overflow > 0 }
}

/// Append a conversation summary to the system prompt.
pub fn with_summary(system: &str, summary: &str) -> String {
    if summary.trim().is_empty() {
        return system.to_string();
    }
    format!(
        "{system}\n\n## Conversation so far (older turns were summarized)\n{}",
        summary.trim()
    )
}

// ---------------------------------------------------------------------------
// The system prompt
// ---------------------------------------------------------------------------

/// One skill, as the "Skills available" block needs it.
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
}

/// One workspace block already visible to the user.
pub struct BlockEntry {
    pub id: String,
    pub title: String,
    pub kind: String,
    /// The block's payload as stored, never re-serialized.
    pub data_json: String,
    /// The user's interaction state as stored, or `None`.
    pub state_json: Option<String>,
}

/// The live surface the model composed with `render_ui`.
pub struct SurfaceView {
    /// The tree as stored, never re-serialized.
    pub data_json: String,
    /// Bound state (inputs, choices, toggles) as stored, or `None`.
    pub state_json: Option<String>,
}

/// One tool's recent reliability, from `db.tool_health`.
pub struct HealthEntry {
    pub tool_name: String,
    pub ok: i64,
    pub total: i64,
}

/// Everything the system prompt is built from. Deliberately plain data with no
/// database in sight: that is what lets `CTX-4` compare this assembly against
/// the frontend's from a fixture, with no app running.
#[derive(Default)]
pub struct PromptInputs {
    /// The persona's system prompt, or the global default.
    pub base: String,
    /// `PRO-6`: the agent's synthesis of how this user likes to be talked to.
    pub about_you: Option<String>,
    /// Standing instructions the user approved (SOUL.md).
    pub soul: Option<String>,
    /// `PRJ-7`: the project this session is in, and what it says to do. Both
    /// or neither — a name with no instructions has nothing to inject, and
    /// instructions with no name have nothing to attribute them to.
    pub project_name: Option<String>,
    pub project_instructions: Option<String>,
    /// The durable memory index (MEMORY.md).
    pub memory_index: Option<String>,
    /// How many facts are remembered in total. Not the same question as whether
    /// `memory_index` is empty: scoped recall can leave the index empty on a
    /// turn where facts do exist, and that is not a cold start.
    pub fact_count: usize,
    /// Can the model call tools at all on this turn.
    pub tools_enabled: bool,
    /// Is the Memory toolset on. Distinct from `memory_index` being present:
    /// with the toolset off, soul and the synthesis are still injected, just
    /// without an index.
    pub memory_enabled: bool,
    pub skills: Vec<SkillEntry>,
    pub blocks: Vec<BlockEntry>,
    pub surface: Option<SurfaceView>,
    /// Durable session state as stored, never re-serialized.
    pub session_state_json: Option<String>,
    pub tool_health: Vec<HealthEntry>,
    /// `PLN-3`: whether — and how firmly — this turn is told to plan first.
    /// Defaults to *when it helps*, which is what an unset setting means.
    pub plan_mode: super::plan::PlanMode,
}

/// Per-entry cap (description plus when-to-use) and whole-block cap for the
/// `SKL-2` stage-1 disclosure. These match the Agent Skills standard's own
/// numbers, so a skill written for another agent is not truncated differently
/// here than it would be there.
pub const SKILL_ENTRY_CAP: usize = 1536;
pub const SKILLS_BLOCK_CAP: usize = 4000;

/// `PRJ-7`: the same order as the skills block, and well under any model's
/// patience. Instructions past this are clipped rather than dropped — a
/// project whose instructions vanished for being long would be worse.
pub const PROJECT_INSTRUCTIONS_CAP: usize = 4000;

/// Assemble the full system prompt for a turn.
///
/// Order is not cosmetic. The slowest-changing blocks come first so the prefix
/// cache stays warm turn to turn, and the block that tells the model to save
/// memories comes last because it is the one most often ignored when buried.
pub fn compose_system_prompt(inputs: &PromptInputs) -> String {
    let mut out = inputs.base.clone();
    let mut push = |block: String| {
        if !block.is_empty() {
            out.push_str("\n\n");
            out.push_str(&block);
        }
    };

    // The durable self comes first, right after the base prompt. The synthesis
    // leads (`PRO-6`) — it is the slowest-changing of these blocks — then the
    // standing instructions the user approved, then the index of what is
    // remembered.
    push(about_you_block(inputs.about_you.as_deref()));
    push(soul_block(inputs.soul.as_deref()));
    // `PRJ-7`: after the standing instructions, which apply everywhere, and
    // before the memory index, which this narrows the meaning of.
    push(project_block(
        inputs.project_name.as_deref(),
        inputs.project_instructions.as_deref(),
    ));
    push(memory_index_block(inputs.memory_index.as_deref(), inputs.tools_enabled));

    // Only mention blocks and surface machinery when the model can actually call
    // the tools — otherwise it imitates tool-call JSON as prose and it leaks raw.
    if inputs.tools_enabled {
        push(skills_block(&inputs.skills));
        push(block_registry(&inputs.blocks));
        push(surface_context(inputs.surface.as_ref()));
    }
    push(session_state_block(inputs.session_state_json.as_deref()));
    if inputs.tools_enabled {
        push(tool_guidance_block(inputs.plan_mode));
        // `MEM-COLD`: last of the guidance, and sent even at zero facts — that
        // is precisely the state it exists to break out of.
        if inputs.memory_enabled {
            push(memory_guidance_block(inputs.fact_count > 0));
        }
        push(tool_cautions(&inputs.tool_health));
    }
    out
}

/// `PRO-6`/`PRO-7`: unlike SOUL.md this is a background inference, not something
/// the user just decided, so a persona always wins.
fn about_you_block(text: Option<&str>) -> String {
    let t = text.unwrap_or_default().trim();
    if t.is_empty() {
        return String::new();
    }
    format!("## About you, as I understand it (apply it; don't mention it unless asked; the persona/system prompt above always wins if they conflict)\n{t}")
}

/// Standing instructions, framed so the model knows they outrank the persona
/// prompt above them when the two pull in different directions (SOUL constrains,
/// persona styles — persona still governs voice, format and depth).
fn soul_block(soul: Option<&str>) -> String {
    let s = soul.unwrap_or_default().trim();
    if s.is_empty() {
        return String::new();
    }
    format!("## Standing instructions (SOUL.md — the user approved these; they take precedence over the persona/system prompt above when the two conflict)\n{s}")
}

/// `PRJ-7`: what this project is, carried by every session in it.
///
/// Sits after SOUL.md because standing instructions the user approved apply
/// everywhere, and this applies only here. Framed like the other blocks so the
/// precedence is stated rather than guessed: this is context about the work,
/// not a voice, and the persona still governs how it is said.
fn project_block(name: Option<&str>, instructions: Option<&str>) -> String {
    let (Some(name), Some(text)) = (name, instructions) else {
        return String::new();
    };
    let (name, text) = (name.trim(), text.trim());
    if name.is_empty() || text.is_empty() {
        return String::new();
    }
    let text = clip(text, PROJECT_INSTRUCTIONS_CAP);
    format!("## Project: {name} (instructions for this project; the persona/system prompt above still governs voice, format and depth)\n{text}")
}

/// The durable memory index, with a caveat when tools (and so `memory` reads)
/// are off.
fn memory_index_block(index: Option<&str>, tools_enabled: bool) -> String {
    let i = index.unwrap_or_default().trim();
    if i.is_empty() {
        return String::new();
    }
    let detail = if tools_enabled {
        ""
    } else {
        " Tools are off — treat descriptions as the only available detail."
    };
    format!("## Your notes about the user (durable facts)\n{i}\n(Read a note's full text with memory(op:\"read\", name:…) before relying on its details.{detail})")
}

/// A compact rendering of durable session state, passed through as stored.
fn session_state_block(state_json: Option<&str>) -> String {
    let Some(text) = non_empty_object(state_json) else {
        return String::new();
    };
    format!("## Session state (durable; update with the remember tool)\n{text}")
}

/// The standing guidance only sent when the model can actually call tools.
///
/// `PLN-3`: the planning sentence rides on the end of the same block, because
/// it is the same kind of thing — how to go about the work — and because
/// `plan.mode = never` must be able to remove it without leaving a gap.
fn tool_guidance_block(plan_mode: super::plan::PlanMode) -> String {
    let mut out = format!("{SURFACE_GUIDANCE}\n\n{BLOCK_GUIDANCE}\n\n{PLAN_FIRST_GUIDANCE}");
    let planning = plan_mode.guidance();
    if !planning.is_empty() {
        out.push('\n');
        out.push_str(planning);
    }
    out
}

/// `MEM-COLD`: the instruction that makes durable memory actually happen.
///
/// Without this the only thing telling the model to save is one tool description
/// buried among forty others, and the index block goes silent at zero facts — so
/// an empty memory never mentions memory, the model never saves, and it stays
/// empty forever. This block is therefore sent whenever the Memory toolset is
/// on, *especially* when there is nothing remembered yet.
fn memory_guidance_block(has_facts: bool) -> String {
    let mut lines = vec![
        "## Remembering",
        "You keep durable notes about the user across conversations with the `memory` tool. When the user says something that will still be true next week and would change how you answer later, call memory(op:\"save\") in the same turn — do not wait to be asked, and do not announce it at length; the save shows up in their timeline on its own.",
        "Worth saving: how they want you to work (tone, length, format, language), tools/stacks/services they use, what they're building and why, stable personal or professional facts, standing decisions they've made.",
        "Never save: task state, one-off requests, anything you inferred rather than heard, or anything they haven't actually confirmed. When in doubt, don't.",
        "One fact per save, in their own terms, with a slug you would search for later.",
    ];
    if !has_facts {
        lines.push("You have not saved anything about this user yet, so the bar for the first few notes is simply: would knowing this next month make you better here? If so, save it.");
    }
    lines.join("\n")
}

/// `SKL-2` stage 1: name and description of every enabled skill, so the model
/// knows what exists before spending a turn on `skill` to read one. Lowest
/// priority (last to fit) is whichever skill sorts last — a full priority
/// ranking by source is not worth the complexity at the skill counts this is
/// ever exercised at.
///
/// The caller passes only the skills that are actually enabled (and, under
/// `SKL-6`, only those a persona's allowlist admits). The remainder line counts
/// against that same list, so a filtered-out skill is not advertised as "one
/// more" the model could ask for.
fn skills_block(skills: &[SkillEntry]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let header =
        "Skills available (read one with the `skill` tool before doing the work it covers):";
    let mut lines = vec![header.to_string()];
    let mut used = char_len(header);
    let mut shown = 0usize;
    for s in skills {
        let desc = match s.when_to_use.as_deref().filter(|w| !w.is_empty()) {
            Some(w) if !s.description.is_empty() => format!("{} — {w}", s.description),
            Some(w) => w.to_string(),
            None => s.description.clone(),
        };
        let clipped = clip(&desc, SKILL_ENTRY_CAP);
        let line = format!("- {}: {clipped}", s.name);
        if used + char_len(&line) + 1 > SKILLS_BLOCK_CAP {
            break;
        }
        used += char_len(&line) + 1;
        lines.push(line);
        shown += 1;
    }
    let remaining = skills.len() - shown;
    if remaining > 0 {
        lines.push(format!("(+{remaining} more)"));
    }
    lines.join("\n")
}

/// `HEAL-2`: tell the agent which of its own tools have been failing lately, so
/// it can route around the damage. Informational self-repair — it changes only
/// this prompt, stores nothing, and needs no setting. Worst two tools only: a
/// wall of cautions would just teach the model to distrust every tool.
pub fn tool_cautions(health: &[HealthEntry]) -> String {
    let mut failing: Vec<&HealthEntry> = health
        .iter()
        .filter(|t| t.total >= 8 && (t.ok as f64) / (t.total as f64) < 0.4)
        .collect();
    failing.sort_by(|a, b| {
        let (ra, rb) = (a.ok as f64 / a.total as f64, b.ok as f64 / b.total as f64);
        ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal)
    });
    failing
        .into_iter()
        .take(2)
        .map(|t| format!("Note: your \"{}\" tool has failed often recently — double-check its arguments, and prefer an alternative when one exists.", t.tool_name))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The current composed surface, injected so the model can revise it by
/// `node_id` instead of re-rendering blind. Capped so a huge tree cannot flood
/// the context — past the cap the model should just re-render whole regions.
fn surface_context(surface: Option<&SurfaceView>) -> String {
    let Some(surface) = surface else {
        return String::new();
    };
    let tree = if char_len(&surface.data_json) > 4000 {
        format!(
            "{}…(truncated — re-render regions you need to change)",
            take_chars(&surface.data_json, 4000)
        )
    } else {
        surface.data_json.clone()
    };
    let bound = match non_empty_object(surface.state_json.as_deref()) {
        Some(state) => format!("\nUser's bound state (from inputs/choices/toggles): {state}"),
        None => String::new(),
    };
    format!("## Workspace surface (the live interface you composed with render_ui)\nCurrent tree: {tree}{bound}")
}

/// Teach the model that the Workspace is a composable surface it owns.
const SURFACE_GUIDANCE: &str = concat!(
    "## Composing the workspace\n",
    "The Workspace view renders whatever interface tree you pass to `render_ui` — compose a real interface for the task (a dashboard, a board, a picker, a wizard, a tracker) instead of describing things in prose or emitting fixed chat blocks.\n",
    "Keep the surface CURRENT: as the task evolves, revise it (render_ui with node_id for one region, or re-render the whole tree) rather than accumulating chat.\n",
    "When a `ui_action` message arrives, it carries the user's bound state — revise the surface to reflect the interaction and reply in at most one sentence.\n",
    // `ORG-UI-2`: the data is already in this prompt (notes, lessons) and
    // render_ui already renders — so "show me what you've learned" becomes the
    // organism examining itself in its own body, with no new machinery.
    "If the user asks how you are, what you remember, or what you've learned, you may render your notes and lessons as a workspace surface.",
);

/// `W4`/`W5`: teach the model to treat blocks as the surface, not to narrate
/// them, and to acknowledge bare interactions briefly.
const BLOCK_GUIDANCE: &str = concat!(
    "## Presenting blocks\n",
    "When you present a block (comparison, plan, collection, form, progress, document), the user sees it rendered in full in their workspace. Do NOT restate the block's contents in prose — after presenting, conclude in at most two sentences.\n",
    "To change a block that already exists, call `present` with that block's existing `block_id` (see the workspace-block list above) rather than creating a new one.\n",
    "If the user's message is only a block interaction (a workspace update, or a `poiesis-action`), acknowledge it in one short sentence and do not present a menu of follow-up options.",
);

/// `LOOP-3`: a multi-step run reads as deliberate rather than flailing when the
/// model says what it intends before the first tool call. One line, tools only.
const PLAN_FIRST_GUIDANCE: &str = concat!(
    "## Working through a task\n",
    "For multi-step tasks, state a one-line plan before your first tool call.",
);

/// `W3`: a compact registry of the blocks already on the user's workspace, so
/// the model can update them by id instead of recreating (the duplicate-block
/// bug).
fn block_registry(blocks: &[BlockEntry]) -> String {
    if blocks.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = blocks
        .iter()
        .map(|b| {
            let summary = block_summary(b);
            let tail = if summary.is_empty() { String::new() } else { format!(", {summary}") };
            format!("[{}] \"{}\" ({}{tail})", b.id, b.title, b.kind)
        })
        .collect();
    format!("## Workspace blocks (already visible to the user — update these by passing their block_id to present, do not recreate)\n{}", lines.join("\n"))
}

fn block_summary(b: &BlockEntry) -> String {
    let data: serde_json::Value =
        serde_json::from_str(&b.data_json).unwrap_or(serde_json::Value::Null);
    let arr = |v: &serde_json::Value| v.as_array().cloned().unwrap_or_default();
    match b.kind.as_str() {
        "plan" => {
            let steps = arr(&data["steps"]);
            let state: serde_json::Value = b
                .state_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);
            let checked = &state["checked"];
            let done = steps
                .iter()
                .filter(|s| {
                    // The user's tick wins over the model's stored status, and
                    // an absent tick falls back to it — `checked[id] ?? status`.
                    let key = match &s["id"] {
                        serde_json::Value::String(s) => s.clone(),
                        other if other.is_null() => "undefined".to_string(),
                        other => other.to_string(),
                    };
                    let ticked = &checked[&key];
                    let effective = if ticked.is_null() { &s["status"] } else { ticked };
                    effective.as_str() == Some("done")
                })
                .count();
            format!("{done}/{} done", steps.len())
        }
        "comparison" => format!("{} options", arr(&data["options"]).len()),
        "collection" => format!("{} items", arr(&data["items"]).len()),
        "form" => format!("{} fields", arr(&data["fields"]).len()),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Reading the inputs out of the database
// ---------------------------------------------------------------------------

/// Reserved kind and title of the block that holds the composed surface. The
/// surface rides on the blocks table rather than a schema of its own, so it has
/// to be told apart from the ordinary blocks here, exactly as `present.rs` and
/// the frontend both do.
const SURFACE_KIND: &str = "surface";
const SURFACE_TITLE: &str = "Workspace";

/// The setting holding the user's global system prompt, and the text used when
/// they have never changed it. Must stay word for word what `store.ts` sends.
pub const SYSTEM_PROMPT_KEY: &str = "system_prompt";
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Poiesis Agent, a local-first assistant that maintains itself: you keep durable memory, learn lessons from your own mistakes, and propose — never impose — changes to how you work. Be concise and clear.";

/// Everything `gather` needs that is not a long-lived handle.
pub struct GatherOpts<'a> {
    pub conversation_id: &'a str,
    /// Can the model call tools on this turn.
    pub tools_enabled: bool,
    /// Which model's reliability record the cautions are drawn from.
    pub model_name: &'a str,
    /// This turn's user message, used to scope memory recall to what is
    /// actually being asked about.
    pub query: &'a str,
}

/// Read the whole system prompt's worth of state out of the database.
///
/// This is the half of `CTX-3` that ends the drift. Every fact the frontend used
/// to look up in its own store is fetched here instead, so a scheduled job, a
/// resumed run and a typed message all get the same prompt built the same way.
pub async fn gather(
    db: &crate::db::Db,
    memory: &crate::memory::MemoryStore,
    mgr: &crate::runtime::RuntimeManager,
    embed_mgr: &crate::runtime::EmbedManager,
    opts: GatherOpts<'_>,
) -> PromptInputs {
    let mut inputs = from_db(db, &opts);

    // With the Memory toolset off, the standing instructions and the synthesis
    // still go in — the user approved those, and they are not something the
    // model has to call a tool to use. Only the index needs the toolset, since
    // it is a table of contents for reads that could not happen.
    inputs.about_you = memory.profile().map(|p| p.body);
    inputs.soul = Some(memory.soul());
    inputs.fact_count = memory.list().len();
    if inputs.memory_enabled && opts.tools_enabled {
        let recalled =
            // `PRJ-8`: recall is scoped to the project this turn is in, so a
            // fact learned in another project is not eligible here.
            crate::commands::memory::recall_for(
                mgr,
                embed_mgr,
                db,
                memory,
                opts.query,
                db.conversation_project(opts.conversation_id)
                    .ok()
                    .flatten()
                    .map(|p| p.id)
                    .as_deref(),
            )
            .await;
        inputs.memory_index = Some(recalled.index);
    }

    // A persona may narrow the advertised skills, never widen them (`SKL-6`).
    // Telling the model about a skill the `skill` tool will then refuse burns a
    // turn to learn what the prompt could have said.
    if opts.tools_enabled {
        let conv = db.get_conversation(opts.conversation_id).ok().flatten();
        let allowed: Option<Vec<String>> = conv
            .as_ref()
            .and_then(|c| c.persona_id.as_deref())
            .and_then(|id| db.get_persona(id).ok().flatten())
            .and_then(|p| p.skills_json)
            .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok());
        let folder = conv.as_ref().and_then(|c| c.folder_path.as_deref()).map(std::path::Path::new);
        inputs.skills = super::skillpack::discover(mgr.app_data_dir(), folder)
            .into_iter()
            .filter(|p| super::skillpack::is_enabled(db, p))
            // Spelled out rather than `is_none_or`, which needs a newer Rust
            // than this crate's stated minimum.
            .filter(|p| match &allowed {
                Some(names) => names.contains(&p.name),
                None => true,
            })
            .map(|p| SkillEntry {
                name: p.name,
                description: p.description,
                when_to_use: p.when_to_use,
            })
            .collect();
    }

    inputs
}

/// The half of `gather` that needs nothing but the database.
///
/// Split out so it can be tested against a real schema without a loaded engine
/// or an embedder. The fields it leaves alone — the durable self and the skills
/// list — are the ones that need a `MemoryStore` and an app data directory.
fn from_db(db: &crate::db::Db, opts: &GatherOpts<'_>) -> PromptInputs {
    let conv = db.get_conversation(opts.conversation_id).ok().flatten();

    // The conversation's persona wins over the global prompt, the same
    // precedence the chat window applies (`CHT-4`/`CHT-7`).
    let base = conv
        .as_ref()
        .and_then(|c| c.persona_id.as_deref())
        .and_then(|id| db.get_persona(id).ok().flatten())
        .map(|p| p.system_prompt)
        .or_else(|| db.get_setting(SYSTEM_PROMPT_KEY).ok().flatten())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string());

    // The surface shares the blocks table with the ordinary blocks, so one read
    // serves both and the surface is lifted out by its reserved kind and title.
    let mut blocks = Vec::new();
    let mut surface = None;
    let mut tool_health = Vec::new();
    if opts.tools_enabled {
        for b in db.list_blocks(opts.conversation_id).unwrap_or_default() {
            if b.kind == SURFACE_KIND && b.title == SURFACE_TITLE {
                surface = Some(SurfaceView { data_json: b.data_json, state_json: b.state_json });
            } else {
                blocks.push(BlockEntry {
                    id: b.id,
                    title: b.title,
                    kind: b.kind,
                    data_json: b.data_json,
                    state_json: b.state_json,
                });
            }
        }
        tool_health = db
            .tool_health(opts.model_name, 7)
            .unwrap_or_default()
            .into_iter()
            .map(|r| HealthEntry { tool_name: r.tool_name, ok: r.ok, total: r.total })
            .collect();
    }

    // `PRJ-7`: read once here, beside the persona, because both answer the
    // same question — what standing context this turn starts from.
    let project = conv
        .as_ref()
        .and_then(|c| c.project_id.as_deref())
        .and_then(|id| db.get_project(id).ok().flatten());

    PromptInputs {
        base,
        project_name: project.as_ref().map(|p| p.name.clone()),
        project_instructions: project.and_then(|p| p.instructions),
        tools_enabled: opts.tools_enabled,
        memory_enabled: super::toolsets::Toolset::Memory.is_enabled(db),
        blocks,
        surface,
        session_state_json: db.get_session_state(opts.conversation_id).ok().flatten(),
        tool_health,
        plan_mode: super::plan::PlanMode::current(db),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Briefs that describe the run's own surroundings
// ---------------------------------------------------------------------------

/// Put the working-folder and Canvas briefs in front of the transcript.
///
/// These are written here, not in the frontend's prompt, so they can never drift
/// from what the file tools will actually enforce. `ART-4` is the same argument
/// for the Canvas: an artifact id the model never saw is an id it cannot pass,
/// so without this `read_artifact` and `update_artifact` exist but are
/// unreachable, and "fix that page" goes back to guessing at a file that was
/// never on disk.
///
/// Both are only worth saying when the model has tools to act on them. Each
/// lands after any leading system messages and before the first real turn, so
/// the standing prompt stays one uninterrupted prefix and the cache stays warm.
pub fn insert_briefs(
    db: &crate::db::Db,
    conversation_id: &str,
    tools_enabled: bool,
    headless: bool,
    msgs: &mut Vec<serde_json::Value>,
) {
    if !tools_enabled {
        return;
    }
    let file_access = crate::agent::toolsets::Toolset::FileSystem.is_enabled(db);
    let briefs = [
        file_access
            .then(|| super::filesystem::working_folder_brief(db, conversation_id))
            .flatten(),
        // `COD-2`/`COD-3`: what the project is and what its authors ask of an
        // agent, read off its folder. Here rather than in the composed prompt
        // because every fact in it comes from the disk.
        file_access
            .then(|| super::project::project_brief(db, conversation_id, headless))
            .flatten(),
        (crate::agent::toolsets::Toolset::Artifacts.is_enabled(db))
            .then(|| super::artifacts::artifacts_brief(db, conversation_id))
            .flatten(),
    ];
    for brief in briefs.into_iter().flatten() {
        let at = msgs.iter().position(|m| m["role"] != "system").unwrap_or(msgs.len());
        msgs.insert(at, serde_json::json!({ "role": "system", "content": brief }));
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Stored JSON text, but only if it is an object with at least one key.
///
/// The parse is thrown away: the *text* is what gets emitted, so key order
/// survives. Anything unparseable is treated as absent rather than passed to the
/// model as a broken fragment.
fn non_empty_object(json: Option<&str>) -> Option<String> {
    let text = json?.trim();
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    let map = parsed.as_object()?;
    if map.is_empty() {
        return None;
    }
    Some(text.to_string())
}

/// Length in UTF-16 code units, matching JavaScript's `String.length`. Every cap
/// in this module is a port of one measured that way.
fn char_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The first `n` UTF-16 code units, matching `String.prototype.slice(0, n)`.
/// Falls back to the whole string rather than splitting a surrogate pair, which
/// would emit an unpaired half.
fn take_chars(s: &str, n: usize) -> String {
    if char_len(s) <= n {
        return s.to_string();
    }
    let units: Vec<u16> = s.encode_utf16().take(n).collect();
    String::from_utf16(&units).unwrap_or_else(|_| s.to_string())
}

fn clip(s: &str, cap: usize) -> String {
    if char_len(s) > cap {
        format!("{}…", take_chars(s, cap))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, description: &str, when: Option<&str>) -> SkillEntry {
        SkillEntry {
            name: name.to_string(),
            description: description.to_string(),
            when_to_use: when.map(str::to_string),
        }
    }

    fn base() -> PromptInputs {
        PromptInputs { base: "You are Poiesis Agent.".to_string(), ..Default::default() }
    }

    /// `PRJ-7`: the project block needs both a name and instructions, and it
    /// costs a project with neither exactly nothing. Most projects have no
    /// instructions, and every folder-only project made before this existed
    /// has none — none of them should pay a token for the feature.
    #[test]
    fn the_project_block_is_absent_unless_there_is_something_to_say() {
        let mut inputs = base();
        assert_eq!(compose_system_prompt(&inputs), "You are Poiesis Agent.");

        // A name alone has nothing to attribute; instructions alone have
        // nothing to attribute them to.
        inputs.project_name = Some("Kitchen rebuild".into());
        assert_eq!(compose_system_prompt(&inputs), "You are Poiesis Agent.");
        inputs.project_name = None;
        inputs.project_instructions = Some("Quotes are in EUR.".into());
        assert_eq!(compose_system_prompt(&inputs), "You are Poiesis Agent.");

        inputs.project_name = Some("Kitchen rebuild".into());
        let out = compose_system_prompt(&inputs);
        assert!(out.contains("## Project: Kitchen rebuild"), "{out}");
        assert!(out.contains("Quotes are in EUR."), "{out}");

        // Whitespace is not something to say.
        inputs.project_instructions = Some("   \n  ".into());
        assert_eq!(compose_system_prompt(&inputs), "You are Poiesis Agent.");
    }

    /// It sits after the standing instructions, which apply everywhere, and
    /// before the memory index, which it narrows the meaning of. Order is not
    /// cosmetic here: it is what tells the model which of two instructions
    /// wins when they pull apart.
    #[test]
    fn the_project_block_sits_between_soul_and_the_memory_index() {
        let mut inputs = base();
        inputs.soul = Some("Always answer in one word.".into());
        inputs.project_name = Some("Book".into());
        inputs.project_instructions = Some("Past tense.".into());
        inputs.memory_index = Some("- [a-fact] something".into());

        let out = compose_system_prompt(&inputs);
        let at = |needle: &str| out.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at("Standing instructions") < at("## Project: Book"));
        assert!(at("## Project: Book") < at("Your notes about the user"));
    }

    /// The one rule the whole module rests on: with tools off, nothing about the
    /// surface, blocks or tool machinery is mentioned at all. A model told about
    /// tools it cannot call writes tool-call JSON as prose and it leaks raw.
    #[test]
    fn tool_machinery_is_silent_when_the_model_cannot_call_tools() {
        let mut inputs = base();
        inputs.skills.push(skill("pdf", "Read PDFs", None));
        inputs.blocks.push(BlockEntry {
            id: "b1".into(),
            title: "Options".into(),
            kind: "comparison".into(),
            data_json: r#"{"options":[1,2]}"#.into(),
            state_json: None,
        });
        inputs.surface =
            Some(SurfaceView { data_json: r#"{"type":"stack"}"#.into(), state_json: None });
        inputs.memory_enabled = true;

        let out = compose_system_prompt(&inputs);
        assert_eq!(out, "You are Poiesis Agent.");

        inputs.tools_enabled = true;
        let on = compose_system_prompt(&inputs);
        for expected in ["Skills available", "Workspace blocks", "Workspace surface", "Remembering"]
        {
            assert!(on.contains(expected), "missing {expected}");
        }
    }

    /// `MEM-COLD`: an empty memory is exactly the state that needs the nudge, so
    /// silence at zero facts would make the emptiness permanent.
    #[test]
    fn an_empty_memory_still_gets_told_to_remember() {
        let mut inputs = base();
        inputs.tools_enabled = true;
        inputs.memory_enabled = true;
        let cold = compose_system_prompt(&inputs);
        assert!(cold.contains("You have not saved anything about this user yet"));

        inputs.fact_count = 3;
        let warm = compose_system_prompt(&inputs);
        assert!(warm.contains("## Remembering"));
        assert!(!warm.contains("You have not saved anything"));
    }

    /// Scoped recall can leave the index empty on a turn where facts do exist.
    /// Reading the cold-start state off the index rather than the count would
    /// then nag a user whose memory is full.
    #[test]
    fn a_full_memory_with_nothing_recalled_this_turn_is_not_a_cold_start() {
        let mut inputs = base();
        inputs.tools_enabled = true;
        inputs.memory_enabled = true;
        inputs.memory_index = Some("   ".to_string());
        inputs.fact_count = 12;
        let out = compose_system_prompt(&inputs);
        assert!(!out.contains("## Your notes about the user"));
        assert!(!out.contains("You have not saved anything"));
    }

    #[test]
    fn the_skills_block_stops_at_its_cap_and_says_how_many_it_dropped() {
        let mut inputs = base();
        inputs.tools_enabled = true;
        for i in 0..20 {
            inputs.skills.push(skill(&format!("skill{i}"), &"x".repeat(600), None));
        }
        let out = compose_system_prompt(&inputs);
        let listed = out.lines().filter(|l| l.starts_with("- skill")).count();
        assert!(listed > 0 && listed < 20, "some fit, not all: {listed}");
        assert!(out.contains(&format!("(+{} more)", 20 - listed)));
        // The block itself stays under its cap; everything after it is other
        // blocks, so measure from the header to the remainder line.
        let block = out
            .split("Skills available")
            .nth(1)
            .and_then(|t| t.split("(+").next())
            .unwrap();
        assert!(char_len(block) < SKILLS_BLOCK_CAP);
    }

    #[test]
    fn a_long_skill_description_is_clipped_rather_than_dropped() {
        let mut inputs = base();
        inputs.tools_enabled = true;
        inputs.skills.push(skill("big", &"y".repeat(SKILL_ENTRY_CAP + 50), None));
        let out = compose_system_prompt(&inputs);
        assert!(out.contains("- big: "));
        assert!(out.contains('…'));
    }

    /// A skill's two sentences are joined the way `store.ts` joins them, and an
    /// absent second half must not leave a dangling separator.
    #[test]
    fn a_skill_without_a_when_to_use_has_no_dangling_dash() {
        let mut inputs = base();
        inputs.tools_enabled = true;
        inputs.skills.push(skill("a", "Does a thing", None));
        inputs.skills.push(skill("b", "Does another", Some("when asked")));
        let out = compose_system_prompt(&inputs);
        assert!(out.contains("- a: Does a thing\n"));
        assert!(out.contains("- b: Does another — when asked"));
    }

    /// Key order is the whole reason these fields are strings. Re-serializing
    /// through `serde_json` would sort them and break `CTX-4`'s byte-identity.
    #[test]
    fn stored_json_reaches_the_model_with_its_key_order_intact() {
        let mut inputs = base();
        inputs.session_state_json = Some(r#"{"zebra":1,"apple":2}"#.to_string());
        let out = compose_system_prompt(&inputs);
        assert!(out.contains(r#"{"zebra":1,"apple":2}"#));
    }

    #[test]
    fn empty_or_broken_session_state_is_left_out_entirely() {
        for text in ["{}", "   ", "not json", "[]"] {
            let mut inputs = base();
            inputs.session_state_json = Some(text.to_string());
            assert!(!compose_system_prompt(&inputs).contains("## Session state"), "for {text:?}");
        }
    }

    /// A plan block's progress is the user's ticks first, the model's stored
    /// status second — the same fallback the workspace itself renders.
    #[test]
    fn a_plans_progress_counts_the_users_ticks_over_the_models_status() {
        let mut inputs = base();
        inputs.tools_enabled = true;
        inputs.blocks.push(BlockEntry {
            id: "b1".into(),
            title: "Ship it".into(),
            kind: "plan".into(),
            data_json: r#"{"steps":[{"id":"1","status":"done"},{"id":"2","status":"todo"},{"id":"3","status":"todo"}]}"#.into(),
            state_json: Some(r#"{"checked":{"2":"done"}}"#.into()),
        });
        let out = compose_system_prompt(&inputs);
        assert!(out.contains(r#"[b1] "Ship it" (plan, 2/3 done)"#), "{out}");
    }

    #[test]
    fn a_block_kind_with_no_summary_has_no_trailing_comma() {
        let mut inputs = base();
        inputs.tools_enabled = true;
        inputs.blocks.push(BlockEntry {
            id: "b9".into(),
            title: "Notes".into(),
            kind: "document".into(),
            data_json: r#"{"body":"hi"}"#.into(),
            state_json: None,
        });
        let out = compose_system_prompt(&inputs);
        assert!(out.contains(r#"[b9] "Notes" (document)"#), "{out}");
    }

    #[test]
    fn a_huge_surface_tree_is_truncated_with_an_instruction_not_just_cut() {
        let mut inputs = base();
        inputs.tools_enabled = true;
        inputs.surface = Some(SurfaceView { data_json: "z".repeat(5000), state_json: None });
        let out = compose_system_prompt(&inputs);
        assert!(out.contains("re-render regions you need to change"));
        assert!(!out.contains(&"z".repeat(4001)));
    }

    #[test]
    fn the_index_says_tools_are_off_only_when_they_are() {
        let mut inputs = base();
        inputs.memory_index = Some("- likes tea".to_string());
        assert!(compose_system_prompt(&inputs).contains("Tools are off"));
        inputs.tools_enabled = true;
        assert!(!compose_system_prompt(&inputs).contains("Tools are off"));
    }

    /// Only the two worst, and only ones with enough calls to mean anything —
    /// a wall of cautions teaches the model to distrust every tool it has.
    #[test]
    fn only_the_two_worst_tools_are_warned_about() {
        let health = vec![
            HealthEntry { tool_name: "a".into(), ok: 1, total: 10 },
            HealthEntry { tool_name: "b".into(), ok: 3, total: 10 },
            HealthEntry { tool_name: "c".into(), ok: 0, total: 10 },
            HealthEntry { tool_name: "rare".into(), ok: 0, total: 3 },
            HealthEntry { tool_name: "fine".into(), ok: 9, total: 10 },
        ];
        let out = tool_cautions(&health);
        assert_eq!(out.lines().count(), 2);
        assert!(out.contains("\"c\""), "worst first");
        assert!(out.contains("\"a\""));
        assert!(!out.contains("rare"), "too few calls to mean anything");
    }

    // -- budgeting -----------------------------------------------------------

    fn turn(role: &str, text: &str) -> serde_json::Value {
        serde_json::json!({ "role": role, "content": text })
    }

    #[test]
    fn a_token_estimate_rounds_up_because_over_estimating_is_the_safe_side() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    /// An image costs a flat rate. Charging its data URI by length would make one
    /// screenshot look like a whole conversation.
    #[test]
    fn an_image_costs_a_flat_rate_not_the_length_of_its_data_uri() {
        let with_image = serde_json::json!({
            "role": "user",
            "content": [
                { "type": "text", "text": "abcd" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,".to_string() + &"A".repeat(100_000) } },
            ]
        });
        assert_eq!(turn_tokens(&with_image), 1 + IMAGE_TOKEN_COST);
    }

    #[test]
    fn everything_fits_when_there_is_room() {
        let prior: Vec<_> = (0..4).map(|i| turn("user", &format!("turn {i}"))).collect();
        let bt = budget_turns("system", &prior, &turn("user", "what's next?"), 4096, KEEP_RECENT);
        assert_eq!(bt.turns.len(), 6, "system + 4 prior + current");
        assert_eq!(bt.overflow, 0);
        assert!(!bt.needs_compaction);
    }

    /// The live exchange is never dropped, even when it alone overflows.
    /// Dropping the user's actual question is never the right answer.
    #[test]
    fn the_recent_thread_and_the_question_survive_a_budget_they_do_not_fit() {
        let prior: Vec<_> = (0..30).map(|_| turn("user", &"x".repeat(4000))).collect();
        let bt = budget_turns("system", &prior, &turn("user", "the actual question"), 128, KEEP_RECENT);
        assert_eq!(bt.turns.len(), 1 + KEEP_RECENT + 1);
        assert_eq!(bt.turns[0]["role"], "system");
        assert_eq!(bt.turns.last().unwrap()["content"], "the actual question");
        assert!(bt.needs_compaction);
        assert_eq!(bt.overflow, 30 - KEEP_RECENT);
    }

    #[test]
    fn workspace_mode_keeps_fewer_turns_because_the_surface_carries_the_state() {
        let prior: Vec<_> = (0..30).map(|_| turn("user", &"x".repeat(4000))).collect();
        let bt = budget_turns("system", &prior, &turn("user", "q"), 128, KEEP_RECENT_WORKSPACE);
        assert_eq!(bt.turns.len(), 1 + KEEP_RECENT_WORKSPACE + 1);
    }

    /// The kept turns are the *newest* ones, in their original order. A budget
    /// that kept the oldest would hand the model an exchange that stops before
    /// the thing being discussed.
    #[test]
    fn the_turns_that_survive_are_the_newest_ones_still_in_order() {
        let prior: Vec<_> = (0..10).map(|i| turn("user", &format!("{i}"))).collect();
        let bt = budget_turns("", &prior, &turn("user", "q"), 40, 2);
        let kept: Vec<&str> =
            bt.turns[1..bt.turns.len() - 1].iter().map(|t| t["content"].as_str().unwrap()).collect();
        let expected: Vec<String> = (10 - kept.len()..10).map(|i| i.to_string()).collect();
        assert_eq!(kept, expected);
    }

    // -- reading it back out of a real database --------------------------------

    use crate::db::Db;

    fn seeded() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("A run", None, false).unwrap();
        (db, conv.id)
    }

    fn opts<'a>(conversation_id: &'a str, tools_enabled: bool) -> GatherOpts<'a> {
        GatherOpts { conversation_id, tools_enabled, model_name: "local", query: "do the thing" }
    }

    /// A conversation with no persona falls back to the user's own prompt, and a
    /// user who never changed it falls back to the built-in one. Getting this
    /// order wrong would silently replace someone's persona with boilerplate.
    #[test]
    fn the_persona_outranks_the_global_prompt_which_outranks_the_default() {
        let (db, conv) = seeded();
        assert_eq!(from_db(&db, &opts(&conv, true)).base, DEFAULT_SYSTEM_PROMPT);

        db.set_setting(SYSTEM_PROMPT_KEY, "You are a careful assistant.").unwrap();
        assert_eq!(from_db(&db, &opts(&conv, true)).base, "You are a careful assistant.");

        let persona = db
            .create_persona(&crate::db::NewPersona {
                name: "Editor".into(),
                system_prompt: "You are a ruthless editor.".into(),
                model_id: None,
                params_json: None,
                tools_json: None,
                skills_json: None,
                description: None,
                spawnable: false,
            })
            .unwrap();
        db.set_conversation_persona(&conv, Some(&persona.id), None).unwrap();
        assert_eq!(from_db(&db, &opts(&conv, true)).base, "You are a ruthless editor.");
    }

    /// The surface rides on the blocks table, so it has to be told apart from
    /// the ordinary blocks. Left in the registry it would be advertised as
    /// something to update with `present`, which is not how it is revised.
    #[test]
    fn the_surface_is_lifted_out_of_the_block_list_rather_than_listed_with_it() {
        let (db, conv) = seeded();
        db.add_block(&conv, None, "surface", "Workspace", r#"{"type":"stack"}"#).unwrap();
        db.add_block(&conv, None, "comparison", "Which laptop", r#"{"options":[1]}"#).unwrap();

        let inputs = from_db(&db, &opts(&conv, true));
        assert_eq!(inputs.blocks.len(), 1);
        assert_eq!(inputs.blocks[0].title, "Which laptop");
        assert!(inputs.surface.is_some());
    }

    /// With tools off none of this can be acted on, so none of it is read —
    /// and, more to the point, none of it is sent.
    #[test]
    fn nothing_tool_shaped_is_read_when_the_model_cannot_call_tools() {
        let (db, conv) = seeded();
        db.add_block(&conv, None, "comparison", "Which laptop", r#"{"options":[1]}"#).unwrap();
        db.set_session_state(&conv, r#"{"a":1}"#).unwrap();

        let inputs = from_db(&db, &opts(&conv, false));
        assert!(inputs.blocks.is_empty());
        assert!(inputs.surface.is_none());
        assert!(inputs.tool_health.is_empty());
        // Session state is not tool machinery: it is the task's own state, and
        // it goes in either way. Only its *editing* needs a tool.
        assert_eq!(inputs.session_state_json.as_deref(), Some(r#"{"a":1}"#));
    }

    #[test]
    fn a_blank_summary_leaves_the_system_prompt_exactly_as_it_was() {
        assert_eq!(with_summary("base", "   "), "base");
        assert_eq!(
            with_summary("base", " FACTS: a "),
            "base\n\n## Conversation so far (older turns were summarized)\nFACTS: a"
        );
    }
}
