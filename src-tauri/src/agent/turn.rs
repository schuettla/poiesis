//! `HRW-1`: one entry point for a user turn.
//!
//! Until this module existed, a typed message was put together in the React
//! store: the model's copy of the message, recall, the system prompt, the rows
//! in the database. This module is that, in Rust, so a typed message, a resumed
//! run and the eval harness can all start the same way.
//!
//! The rule (`HRW-0`): anything that changes what the model sees or does is
//! decided here. The window passes the user's input and choices, and shows what
//! comes back.
//!
//! The assistant row is finished here too: the steps are folded by
//! `agent::timeline` as events pass, saved with the row, and sent back in
//! `TurnFinalized`.

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tauri::ipc::Channel;

use crate::agent::context::{self, GatherOpts, KEEP_RECENT, KEEP_RECENT_WORKSPACE};
use crate::agent::fleet::StopReason;
use crate::agent::plan::Plan;
use crate::agent::run::{AgentEventSink, RunOptions};
use crate::agent::timeline::{self, Timeline};
use crate::agent::AgentEvent;
use crate::commands::agent::{execute_turn, resolve_turn, ChatTarget, TurnEnv};
use crate::commands::attachments;
use crate::db::{NewAttachment, NewMessage};

/// What the model reads of the user's PDF, at most (UTF-16 units, as the
/// window counted them).
const PDF_TEXT_CAP: usize = 20_000;

/// Said as the answer when a resume finds nothing to pick up.
pub const NOTHING_TO_RESUME: &str = "I have no record of that run to pick up \u{2014} it finished before I started keeping one. Ask me again and I will start it fresh.";

/// Temperature when neither the conversation nor its persona says.
const DEFAULT_TEMPERATURE: f32 = 0.7;

/// The window a turn is budgeted against when nothing better is known: a
/// hosted provider we have no figure for, a connected server that was added
/// without one, and an engine that has not reported its own.
const DEFAULT_CLOUD_CTX: usize = 32_000;
const DEFAULT_ENDPOINT_CTX: usize = 8192;
const DEFAULT_LOCAL_CTX: usize = 4096;

/// The setting behind "summarize older turns when they stop fitting". On
/// unless it says `false`.
const AUTOCOMPACT_KEY: &str = "context.autocompact";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAttachment {
    /// `image` | `pdf` | `file`.
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub path: String,
    /// A pasted or browser-dropped image carries its bytes inline; a picked or
    /// dropped file is read from `path`.
    #[serde(default)]
    pub data_uri: Option<String>,
}

/// `CMP-3`: something the user pointed at with `+`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRef {
    /// `conversation` | `artifact`.
    pub kind: String,
    pub id: String,
    pub label: String,
}

/// `EPI-UI-1`: "Do it the same way", as the composer holds it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastTimeTaken {
    pub at: i64,
    pub conversation_id: String,
    pub digest: String,
    #[serde(default)]
    pub skill: Option<String>,
}

/// The model the window picked, as far as a turn needs to know it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnModel {
    pub name: String,
    /// `local` | `cloud` | `endpoint`.
    pub provenance: String,
    #[serde(default)]
    pub vision: bool,
    /// The connected server's own window, which `/v1/models` does not report.
    #[serde(default)]
    pub ctx_size: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInput {
    pub conversation_id: String,
    /// What the user typed; saved on the user row.
    pub text: String,
    /// A block or surface action. It is the recall query in place of `text`.
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub attachments: Vec<TurnAttachment>,
    #[serde(default)]
    pub refs: Vec<TurnRef>,
    /// `W2`: workspace interactions since the last reply, already worded.
    #[serde(default)]
    pub pending_actions: Vec<String>,
    #[serde(default)]
    pub last_time: Option<LastTimeTaken>,
    /// `VTN-1`: the user spoke this turn and will hear the reply.
    #[serde(default)]
    pub spoken: bool,
    pub model: TurnModel,
    #[serde(default)]
    pub target: ChatTarget,
    #[serde(default)]
    pub options: RunOptions,
    #[serde(default)]
    pub tools_enabled: bool,
    /// The rows `save_turn` already wrote for this turn. The window saves first
    /// when it has to start the engine, so a failed or abandoned start still
    /// leaves the message behind.
    #[serde(default)]
    pub saved: Option<SavedTurn>,
}

/// The two rows of a turn, once saved.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTurn {
    pub user_message_id: String,
    pub assistant_message_id: String,
}

// ---------------------------------------------------------------------------
// The model's copy of the user's message
// ---------------------------------------------------------------------------

/// "12 Oct". Fixed English, because the date goes to the model and the model's
/// copy must not change with the language of the window.
pub fn short_date(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%-d %b").to_string())
        .unwrap_or_default()
}

/// What "Do it the same way" adds to this one message. It names the conversation
/// so the model can read the whole of it, and says nothing about the user that
/// the digest did not.
pub fn last_time_note(taken: &LastTimeTaken) -> String {
    let skill = taken
        .skill
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!(" The skill I used was \"{s}\"."))
        .unwrap_or_default();
    format!(
        "\n\n[Context: last time ({}) we did something like this (conversation id {}). What I recorded about it: {}{} Do it the same way unless this message says otherwise. Read that conversation with read_conversation if you need more.]",
        short_date(taken.at),
        taken.conversation_id,
        taken.digest.trim(),
        skill,
    )
}

/// What the model gets of one attached PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfBody {
    Text(String),
    Unreadable,
}

/// The text of the model's copy of the message: the user's words, with what
/// they attached and pointed at written in after them.
///
/// Word for word what the window wrote (`sendMessage` in `store.ts`), in the
/// same order: workspace updates in front, then `+` refs, the last-time note,
/// each PDF, and the sentence about images the model cannot see.
pub fn user_text(input: &TurnInput, pdfs: &[(String, PdfBody)]) -> String {
    let mut out = input.text.clone();
    if !input.pending_actions.is_empty() {
        out = format!(
            "(Workspace updates since your last reply: {}.)\n\n{out}",
            input.pending_actions.join("; ")
        );
    }
    for r in &input.refs {
        if r.kind == "conversation" {
            out.push_str(&format!(
                "\n\n[Context: the earlier conversation \"{}\" (id {}). Read it with read_conversation before answering.]",
                r.label, r.id
            ));
        } else {
            out.push_str(&format!(
                "\n\n[Context: \"{}\" from the library (artifact id {}). Read it with read_artifact before answering.]",
                r.label, r.id
            ));
        }
    }
    if let Some(taken) = &input.last_time {
        out.push_str(&last_time_note(taken));
    }
    for (name, body) in pdfs {
        match body {
            PdfBody::Text(text) => {
                let shown = if text.trim().is_empty() {
                    "(No selectable text \u{2014} this looks like a scanned PDF.)".to_string()
                } else {
                    context::take_chars(text, PDF_TEXT_CAP)
                };
                out.push_str(&format!("\n\n[Attached PDF: {name}]\n{shown}"));
            }
            PdfBody::Unreadable => {
                out.push_str(&format!("\n\n[Attached PDF: {name} \u{2014} couldn't be read]"));
            }
        }
    }
    let images = input.attachments.iter().filter(|a| a.kind == "image").count();
    if images > 0 && !input.model.vision {
        out.push_str(&format!(
            "\n\n(I attached {images} image(s), but \u{201c}{}\u{201d} can't see images. Pick a vision-capable model to use them.)",
            input.model.name
        ));
    }
    out
}

/// The message as the model receives it: plain text, or text plus the images
/// when the model can see them. An image that cannot be read is skipped.
pub fn user_content(text: String, images: Vec<String>) -> serde_json::Value {
    if images.is_empty() {
        return serde_json::Value::String(text);
    }
    let mut parts = vec![serde_json::json!({ "type": "text", "text": text })];
    parts.extend(
        images.into_iter().map(|url| serde_json::json!({ "type": "image_url", "image_url": { "url": url } })),
    );
    serde_json::Value::Array(parts)
}

/// Read what the model is to be shown of the attachments: the PDF texts, and the
/// images as data URIs when the model has eyes. Every read answers to the same
/// scope check the window's own reads do.
async fn read_attachments(env: &TurnEnv<'_>, input: &TurnInput) -> (Vec<(String, PdfBody)>, Vec<String>) {
    let mut pdfs = Vec::new();
    for a in input.attachments.iter().filter(|a| a.kind == "pdf") {
        let body = match attachments::pdf_text(env.db, env.grants, None, &a.path).await {
            Ok(text) => PdfBody::Text(text),
            Err(_) => PdfBody::Unreadable,
        };
        pdfs.push((a.name.clone(), body));
    }
    let mut images = Vec::new();
    if input.model.vision {
        for a in input.attachments.iter().filter(|a| a.kind == "image") {
            if let Some(uri) = &a.data_uri {
                images.push(uri.clone());
            } else if let Ok(uri) = attachments::image_data_uri(env.db, env.grants, None, &a.path).await {
                images.push(uri);
            }
        }
    }
    (pdfs, images)
}

// ---------------------------------------------------------------------------
// What the turn keeps of its own events
// ---------------------------------------------------------------------------

/// What a turn keeps of the events that pass, to save when it ends. Read through
/// the sink's tap, so it sees exactly what the window would have seen.
#[derive(Default)]
pub struct TurnFold {
    /// The prose as it streamed. This, and not the run's return value, is what
    /// the window has always saved.
    text: String,
    /// Lessons that spoke up at a step, in the order they did, once each.
    fired: Vec<String>,
    /// `HRW-5`: the steps, as the row will hold them.
    timeline: Timeline,
    /// `PLN-UI-5`: the plan as it last stood, from a `Plan` event or the run's end.
    plan: Option<Plan>,
    /// The row the turn was written to, once it has been. A background check
    /// that reports after that (`USE-8`) still reaches the saved timeline.
    saved_as: Option<String>,
}

impl TurnFold {
    fn see(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::Token { text } => self.text.push_str(text),
            AgentEvent::Fired { lesson, .. } if !self.fired.contains(lesson) => {
                self.fired.push(lesson.clone());
            }
            AgentEvent::Error { message } if self.text.is_empty() => {
                self.text = format!("That didn't work: {message}");
            }
            AgentEvent::Plan { plan, .. } => self.plan = Some(plan.clone()),
            AgentEvent::RunEnded { plan: Some(plan), .. } => self.plan = Some(plan.clone()),
            _ => {}
        }
        if timeline::reads(event) {
            if let Some(wire) = timeline::as_wire(event) {
                self.timeline.see(&wire);
            }
        }
    }
}

/// A sink whose events also feed a fresh `TurnFold`.
fn folding_sink(channel: Channel<AgentEvent>) -> (AgentEventSink, Arc<Mutex<TurnFold>>) {
    let fold = Arc::new(Mutex::new(TurnFold::default()));
    let tap = fold.clone();
    let sink = AgentEventSink::new(channel).with_tap(move |e| {
        let Ok(mut fold) = tap.lock() else { return };
        fold.see(e);
        if let (Some(id), AgentEvent::UntrustedFlag { .. }) = (&fold.saved_as, e) {
            if let (Some(db), Some(steps)) = (crate::agent::decide::db(), fold.timeline.steps_json()) {
                let _ = db.replace_message_steps(id, &steps);
            }
        }
    });
    (sink, fold)
}

// ---------------------------------------------------------------------------
// History, its budget and compaction
// ---------------------------------------------------------------------------

/// How many tokens of history, summary and prompt this turn may fill. Ported
/// from the window's `resolveBudget`, and deliberately not the run's own
/// `context_window` (the meter), which is read off the pricing table.
pub fn context_budget(model: &TurnModel, provider: Option<&str>, engine_ctx: Option<u32>) -> usize {
    match model.provenance.as_str() {
        // Published windows of the hosted providers; OpenRouter's is the default.
        "cloud" => match provider {
            Some("anthropic") => 200_000,
            Some("openai") => 128_000,
            _ => DEFAULT_CLOUD_CTX,
        },
        // `/v1/models` does not report a window, so the user's own figure is
        // the only one there is.
        "endpoint" => model.ctx_size.map_or(DEFAULT_ENDPOINT_CTX, |n| n as usize),
        _ => engine_ctx.map_or(DEFAULT_LOCAL_CTX, |n| n as usize),
    }
}

/// The saved turns still sent word for word: everything after the summary
/// boundary, without empty rows and without the turn being sent (which goes in
/// as `current`, and twice would put the user's words in front of the model
/// twice).
fn history(
    db: &crate::db::Db,
    conversation_id: &str,
    current_id: &str,
    boundary: Option<&str>,
) -> Vec<(String, serde_json::Value)> {
    let all: Vec<crate::db::Message> = db
        .list_messages(conversation_id)
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.id != current_id && !m.content.trim().is_empty())
        .collect();
    let from = boundary.and_then(|b| all.iter().position(|m| m.id == b)).map_or(0, |i| i + 1);
    all.into_iter()
        .skip(from)
        .map(|m| (m.id, serde_json::json!({ "role": m.role, "content": m.content })))
        .collect()
}

/// Fit the summary, the earlier turns and the new message into `budget`. The one
/// place a turn's transcript is cut: `run_turn` sends what it returns, and the
/// CTX-4 gate (`context_golden`) renders the same call.
pub fn fit(
    system: &str,
    summary: &str,
    prior: &[serde_json::Value],
    current: &serde_json::Value,
    budget: usize,
    keep: usize,
) -> context::Budgeted {
    context::budget_turns(&context::with_summary(system, summary), prior, current, budget, keep)
}

/// The turns to send: the system prompt, as much history as the window holds,
/// and the new message. When history overflows and auto-compact is on, the older
/// part is summarized first, once, and the window is told (`Compacted`).
/// Nothing is deleted. A summary that fails never blocks the send: the oldest
/// turns are simply dropped.
async fn budgeted_turns(
    env: &TurnEnv<'_>,
    input: &TurnInput,
    system: &str,
    current: serde_json::Value,
    current_id: &str,
    sink: &AgentEventSink,
) -> Vec<serde_json::Value> {
    let db = env.db;
    let conversation_id = input.conversation_id.as_str();
    let conv = db.get_conversation(conversation_id).ok().flatten();
    let keep = if conv.as_ref().is_some_and(|c| c.workspace) { KEEP_RECENT_WORKSPACE } else { KEEP_RECENT };
    let budget = context_budget(&input.model, input.target.provider.as_deref(), env.mgr.engine_ctx_size().await);

    let mut summary = conv.as_ref().and_then(|c| c.summary.clone()).unwrap_or_default();
    let boundary = conv.and_then(|c| c.summary_upto_message_id);
    let mut prior = history(db, conversation_id, current_id, boundary.as_deref());
    let turns_of = |prior: &[(String, serde_json::Value)]| -> Vec<serde_json::Value> {
        prior.iter().map(|(_, t)| t.clone()).collect()
    };
    let mut bt = fit(system, &summary, &turns_of(&prior), &current, budget, keep);

    let auto = db.get_setting(AUTOCOMPACT_KEY).ok().flatten().as_deref() != Some("false");
    if bt.needs_compaction && auto {
        // The overflow is the oldest prefix, so the boundary is its last turn.
        if let Some((upto, _)) = prior.get(bt.overflow - 1) {
            let upto = upto.clone();
            if let Ok(made) = crate::commands::conversations::compact_conversation(
                env.mgr,
                db,
                conversation_id,
                &upto,
                input.target.clone(),
                None,
            )
            .await
            {
                sink.emit(AgentEvent::Compacted {
                    conversation_id: conversation_id.to_string(),
                    summary: made.clone(),
                    summary_upto_message_id: upto.clone(),
                });
                summary = made;
                prior = history(db, conversation_id, current_id, Some(&upto));
                bt = fit(system, &summary, &turns_of(&prior), &current, budget, keep);
            }
        }
    }
    bt.turns
}

// ---------------------------------------------------------------------------
// Settings the turn reads
// ---------------------------------------------------------------------------

/// The conversation's own temperature, else its persona's, else the default.
pub fn temperature_for(db: &crate::db::Db, conversation_id: &str) -> f32 {
    let from_json = |json: Option<&str>| -> Option<f32> {
        let v: serde_json::Value = serde_json::from_str(json?).ok()?;
        v.get("temperature")?.as_f64().map(|t| t as f32)
    };
    let Some(conv) = db.get_conversation(conversation_id).ok().flatten() else {
        return DEFAULT_TEMPERATURE;
    };
    from_json(conv.overrides_json.as_deref())
        .or_else(|| {
            let persona = db.get_persona(conv.persona_id.as_deref()?).ok().flatten()?;
            from_json(persona.params_json.as_deref())
        })
        .unwrap_or(DEFAULT_TEMPERATURE)
}

// ---------------------------------------------------------------------------
// The two entry points
// ---------------------------------------------------------------------------

/// Write the turn's end onto its assistant row, and say so. `TurnFinalized` is
/// the last thing a turn sends: the window takes the saved timeline from it.
fn finalize(
    env: &TurnEnv<'_>,
    sink: &AgentEventSink,
    assistant_message_id: &str,
    fold: &Mutex<TurnFold>,
    context_json: Option<&str>,
    stop_reason: Option<StopReason>,
) {
    let stop = stop_reason.filter(|r| r.is_partial()).map(StopReason::as_str);
    // Written under the fold's lock, so a late flag lands either in this write
    // or in its own one after it, never between.
    let steps_json = fold.lock().ok().and_then(|mut f| {
        let plan = f.plan.as_ref().filter(|p| !p.is_empty()).and_then(|p| serde_json::to_string(p).ok());
        let steps_json = f.timeline.steps_json();
        let _ = env.db.finalize_message(
            assistant_message_id,
            &f.text,
            steps_json.as_deref(),
            context_json,
            stop,
            plan.as_deref(),
        );
        f.saved_as = Some(assistant_message_id.to_string());
        steps_json
    });
    sink.emit(AgentEvent::TurnFinalized { message_id: assistant_message_id.to_string(), steps_json });
}

/// The user row and an empty assistant row of a turn. `run_turn` does this
/// itself unless the window already did (`save_turn_cmd`).
pub fn save_turn(db: &crate::db::Db, input: &TurnInput) -> Result<SavedTurn, crate::db::DbError> {
    let conversation_id = input.conversation_id.as_str();
    let (user, assistant) = db
        .append_message(
            conversation_id,
            &NewMessage {
                role: "user".into(),
                content: input.text.clone(),
                model_name: None,
                model_provenance: None,
                steps_json: None,
                attachments: input
                    .attachments
                    .iter()
                    .map(|a| NewAttachment {
                        kind: a.kind.clone(),
                        name: a.name.clone(),
                        path: a.path.clone(),
                        artifact_id: None,
                    })
                    .collect(),
            },
        )
        .and_then(|user| {
            let assistant = db.append_message(
                conversation_id,
                &NewMessage {
                    role: "assistant".into(),
                    content: String::new(),
                    model_name: Some(input.model.name.clone()),
                    model_provenance: Some(input.model.provenance.clone()),
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )?;
            Ok((user, assistant))
        })?;
    if input.spoken {
        let _ = db.mark_spoken(&user.id);
        let _ = db.mark_spoken(&assistant.id);
    }
    Ok(SavedTurn { user_message_id: user.id, assistant_message_id: assistant.id })
}

/// `HRW-1`: start, run and save one user turn.
///
/// In order: the user row and an empty assistant row are saved (unless the
/// window saved them already), and `TurnSaved` goes out before anything slow;
/// the place to run is resolved; the model's copy of the message and the system
/// prompt are built; what recall brought in is announced; the run happens; the
/// assistant row is finished.
pub async fn run_turn(env: &TurnEnv<'_>, input: TurnInput, on_event: Channel<AgentEvent>) {
    let db = env.db;
    let conversation_id = input.conversation_id.as_str();
    let (sink, fold) = folding_sink(on_event);

    // 1. The two rows.
    let saved = match input.saved.clone().map_or_else(|| save_turn(db, &input), Ok) {
        Ok(saved) => saved,
        Err(e) => {
            sink.emit(AgentEvent::Error { message: format!("I couldn't save your message. {e}") });
            return;
        }
    };
    let SavedTurn { user_message_id: user_id, assistant_message_id: assistant_id } = saved;

    // 2. The window takes the real ids at once.
    sink.emit(AgentEvent::TurnSaved {
        user_message_id: Some(user_id.clone()),
        assistant_message_id: assistant_id.clone(),
    });

    // 3. Where this turn runs.
    let setup = match resolve_turn(env.mgr, db, &input.target).await {
        Ok(setup) => setup,
        Err(message) => {
            sink.emit(AgentEvent::Error { message });
            finalize(env, &sink, &assistant_id, &fold, None, None);
            return;
        }
    };

    // `DEC-4`/`CTL-2`: the decision engine warm by the first decision, and the
    // turn's decision budget started with it.
    crate::agent::decide::prewarm();
    crate::agent::decide::begin_turn(conversation_id);

    // 4. The model's copy of the message.
    let (pdfs, images) = read_attachments(env, &input).await;
    let content = user_content(user_text(&input, &pdfs), images);

    // 5. The system prompt, and what recall brought in.
    let query = input.action.as_deref().unwrap_or(&input.text);
    let mut inputs = context::gather(
        env.db,
        env.memory,
        env.mgr,
        env.embed_mgr,
        GatherOpts {
            conversation_id,
            tools_enabled: input.tools_enabled,
            model_name: &setup.model_name,
            query,
            attended: true,
        },
    )
    .await;
    inputs.spoken = input.spoken;
    let system = context::compose_system_prompt(&inputs);
    if !inputs.recalled_matches.is_empty() {
        sink.emit(AgentEvent::Recall {
            id: format!("{}{}", timeline::RECALL_STEP_PREFIX, crate::db::now_ms()),
            matches: inputs.recalled_matches.clone(),
        });
    }

    let current = serde_json::json!({ "role": "user", "content": content });
    let mut msgs = budgeted_turns(env, &input, &system, current, &user_id, &sink).await;
    context::insert_briefs(db, conversation_id, input.tools_enabled, false, &mut msgs);

    // 6. The run.
    let temperature = temperature_for(db, conversation_id);
    let outcome = execute_turn(
        env,
        &setup,
        &input.target,
        conversation_id,
        Some(&assistant_id),
        msgs,
        temperature,
        input.tools_enabled,
        None,
        input.options.clone(),
        &sink,
    )
    .await;

    // 7. The assistant row, finished.
    let mut refs = inputs.context_refs.clone();
    if let Ok(fold) = fold.lock() {
        refs.fired = fold.fired.clone();
    }
    let context_json = serde_json::to_string(&refs).ok();
    finalize(env, &sink, &assistant_id, &fold, context_json.as_deref(), Some(outcome.stop_reason));
}

/// `HRN-UI-5`, moved here by `HRW-1`: pick an interrupted run back up with its
/// work intact.
///
/// The transcript comes out of the session log rather than being rebuilt from
/// the visible messages: the messages have the prose, but the log has the tool
/// results, which are the part that cost minutes and money.
///
/// Like a typed turn it saves its own assistant row and says so with
/// `TurnSaved`; there is no user row. Returns `false` when there was nothing to
/// resume, in which case the row says so.
pub async fn resume_turn(
    env: &TurnEnv<'_>,
    conversation_id: &str,
    model: &TurnModel,
    target: ChatTarget,
    tools_enabled: bool,
    options: RunOptions,
    on_event: Channel<AgentEvent>,
) -> bool {
    let db = env.db;
    let (sink, fold) = folding_sink(on_event);

    let row = match db.append_message(
        conversation_id,
        &NewMessage {
            role: "assistant".into(),
            content: String::new(),
            model_name: Some(model.name.clone()),
            model_provenance: Some(model.provenance.clone()),
            steps_json: None,
            attachments: Vec::new(),
        },
    ) {
        Ok(row) => row,
        Err(e) => {
            sink.emit(AgentEvent::Error { message: format!("I couldn't save the answer. {e}") });
            return false;
        }
    };
    sink.emit(AgentEvent::TurnSaved { user_message_id: None, assistant_message_id: row.id.clone() });

    let nothing = |text: &str| {
        sink.emit(AgentEvent::Token { text: text.to_string() });
        finalize(env, &sink, &row.id, &fold, None, None);
        false
    };

    let Some((run_id, _reason)) = db.last_logged_run(conversation_id).ok().flatten() else {
        return nothing(NOTHING_TO_RESUME);
    };
    let mut msgs = crate::agent::log::replay(db, &run_id);
    if msgs.is_empty() {
        return nothing(NOTHING_TO_RESUME);
    }
    // `LRN-2`: never do again what has already left the machine. The model would
    // be told not to repeat its work, but "not to" is a request; this is a refusal.
    // Said as the reply, not as an error: nothing failed.
    if let Some(what) = crate::agent::log::outward_done(db, &run_id) {
        return nothing(&crate::agent::log::cut_off_after(&what));
    }
    msgs.push(serde_json::json!({
        "role": "system",
        "content": crate::agent::log::RESUME_PROMPT,
    }));

    let setup = match resolve_turn(env.mgr, db, &target).await {
        Ok(setup) => setup,
        Err(message) => {
            sink.emit(AgentEvent::Error { message });
            finalize(env, &sink, &row.id, &fold, None, None);
            return false;
        }
    };
    // `CRD-1`: the resumed answer was prepared from the interrupted run's log, so
    // it carries that answer's record of what reached it.
    let _ = db.carry_context_to(conversation_id, &row.id);

    let outcome = execute_turn(
        env,
        &setup,
        &target,
        conversation_id,
        Some(&row.id),
        msgs,
        temperature_for(db, conversation_id),
        tools_enabled,
        // `PLN-T4`: the plan is not in the replayed messages, so it is handed over
        // separately. Without it a resumed run would show its plan vanish at the
        // moment it was picked back up, and write a second one.
        crate::agent::log::last_plan(db, conversation_id),
        options,
        &sink,
    )
    .await;
    finalize(env, &sink, &row.id, &fold, None, Some(outcome.stop_reason));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> TurnInput {
        TurnInput {
            conversation_id: "c".into(),
            text: text.into(),
            action: None,
            attachments: Vec::new(),
            refs: Vec::new(),
            pending_actions: Vec::new(),
            last_time: None,
            spoken: false,
            model: TurnModel { name: "Tiny".into(), provenance: "local".into(), vision: false, ctx_size: None },
            target: ChatTarget::default(),
            options: RunOptions::default(),
            tools_enabled: false,
            saved: None,
        }
    }

    fn attachment(kind: &str, name: &str) -> TurnAttachment {
        TurnAttachment { kind: kind.into(), name: name.into(), path: String::new(), data_uri: None }
    }

    /// A plain message reaches the model as the user typed it, with nothing added.
    #[test]
    fn a_plain_message_is_left_alone() {
        assert_eq!(user_text(&input("hello"), &[]), "hello");
    }

    #[test]
    fn workspace_updates_go_in_front() {
        let mut i = input("next");
        i.pending_actions = vec!["checked step 2".into(), "pinned A".into()];
        assert_eq!(
            user_text(&i, &[]),
            "(Workspace updates since your last reply: checked step 2; pinned A.)\n\nnext"
        );
    }

    #[test]
    fn refs_name_what_to_read_and_how() {
        let mut i = input("compare");
        i.refs = vec![
            TurnRef { kind: "conversation".into(), id: "c1".into(), label: "Trip".into() },
            TurnRef { kind: "artifact".into(), id: "a9".into(), label: "Plan".into() },
        ];
        let out = user_text(&i, &[]);
        assert!(out.contains("\n\n[Context: the earlier conversation \"Trip\" (id c1). Read it with read_conversation before answering.]"), "{out}");
        assert!(out.contains("\n\n[Context: \"Plan\" from the library (artifact id a9). Read it with read_artifact before answering.]"), "{out}");
        assert!(out.find("Trip").unwrap() < out.find("Plan").unwrap(), "in the order given");
    }

    /// `EPI-UI-1`: the digest is trimmed, the skill is named only when there was one.
    #[test]
    fn the_last_time_note_has_the_date_the_conversation_and_the_digest() {
        let at = 1_760_270_400_000; // 12 Oct 2025, midday UTC: the 12th in any time zone
        let mut taken = LastTimeTaken { at, conversation_id: "c7".into(), digest: "  Ran the report.  ".into(), skill: None };
        let note = last_time_note(&taken);
        assert_eq!(
            note,
            "\n\n[Context: last time (12 Oct) we did something like this (conversation id c7). What I recorded about it: Ran the report. Do it the same way unless this message says otherwise. Read that conversation with read_conversation if you need more.]"
        );
        taken.skill = Some("weekly".into());
        assert!(last_time_note(&taken).contains("Ran the report. The skill I used was \"weekly\". Do it the same way"));
    }

    #[test]
    fn a_pdf_is_cut_at_its_cap_and_a_scan_and_a_failure_say_so() {
        let i = input("read");
        let big = "x".repeat(PDF_TEXT_CAP + 500);
        let out = user_text(&i, &[("big.pdf".into(), PdfBody::Text(big))]);
        assert!(out.contains("\n\n[Attached PDF: big.pdf]\n"), "{out}");
        assert_eq!(out.matches('x').count(), PDF_TEXT_CAP);

        let out = user_text(&i, &[("scan.pdf".into(), PdfBody::Text("  \n ".into()))]);
        assert!(out.ends_with("[Attached PDF: scan.pdf]\n(No selectable text \u{2014} this looks like a scanned PDF.)"), "{out}");

        let out = user_text(&i, &[("bad.pdf".into(), PdfBody::Unreadable)]);
        assert!(out.ends_with("\n\n[Attached PDF: bad.pdf \u{2014} couldn't be read]"), "{out}");
    }

    /// A model that cannot see is told so, in words, rather than sent nothing.
    #[test]
    fn images_for_a_model_without_eyes_become_a_sentence() {
        let mut i = input("what is this");
        i.attachments = vec![attachment("image", "a.png"), attachment("image", "b.png")];
        assert!(user_text(&i, &[]).ends_with(
            "\n\n(I attached 2 image(s), but \u{201c}Tiny\u{201d} can't see images. Pick a vision-capable model to use them.)"
        ));
        i.model.vision = true;
        assert_eq!(user_text(&i, &[]), "what is this", "a model that sees is sent the pictures instead");
    }

    #[test]
    fn images_ride_as_parts_after_the_text() {
        assert_eq!(user_content("hi".into(), vec![]), serde_json::json!("hi"));
        let v = user_content("hi".into(), vec!["data:image/png;base64,AA".into()]);
        assert_eq!(v[0], serde_json::json!({ "type": "text", "text": "hi" }));
        assert_eq!(v[1]["image_url"]["url"], "data:image/png;base64,AA");
    }

    fn hit(snippet: &str) -> crate::db::SearchHit {
        crate::db::SearchHit {
            source: "memory".into(),
            conversation_id: None,
            title: "t".into(),
            created_at: 0,
            snippet: snippet.into(),
            kind: Some("fact".into()),
            path: None,
            why: None,
        }
    }

    /// `CRD-1`: the record kept beside an answer names every lesson that came
    /// and the reason recall gave for each entry that has one.
    #[test]
    fn the_record_beside_an_answer_keeps_lessons_and_reasons() {
        use crate::commands::memory::ContextRefs;
        let named = |title: &str, kind: &str, why: Option<&str>| crate::db::SearchHit {
            title: title.into(),
            kind: Some(kind.into()),
            why: why.map(str::to_string),
            ..hit("")
        };
        let facts = ["be-concise".to_string(), "anna-contact".to_string()];
        let refs = ContextRefs::for_turn(
            None,
            false,
            false,
            &facts,
            &[
                named("anna-contact", "preference", Some("you named Anna")),
                named("check-header", "lesson", Some("about check header")),
            ],
        );
        assert_eq!(refs.facts, facts);
        assert_eq!(refs.lessons, ["check-header"]);
        assert_eq!(refs.why.get("anna-contact").map(String::as_str), Some("you named Anna"));
        assert_eq!(refs.why.get("check-header").map(String::as_str), Some("about check header"));

        let none = ContextRefs::for_turn(None, false, false, &[], &[named("a", "fact", None)]);
        assert!(none.why.is_empty(), "no reasons when recall gave none");
    }

    #[test]
    fn the_conversation_then_the_persona_then_the_default_sets_the_temperature() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let conv = db.create_conversation("t", None, false).unwrap().id;
        assert_eq!(temperature_for(&db, &conv), DEFAULT_TEMPERATURE);

        let persona = db
            .create_persona(&crate::db::NewPersona {
                name: "Cold".into(),
                system_prompt: "x".into(),
                model_id: None,
                params_json: Some(r#"{"temperature":0.2}"#.into()),
                tools_json: None,
                skills_json: None,
                description: None,
                spawnable: false,
            })
            .unwrap();
        db.set_conversation_persona(&conv, Some(&persona.id), None).unwrap();
        assert_eq!(temperature_for(&db, &conv), 0.2);

        db.set_conversation_persona(&conv, Some(&persona.id), Some(r#"{"temperature":1.1}"#)).unwrap();
        assert_eq!(temperature_for(&db, &conv), 1.1);
    }

    /// The fold keeps what the window kept: the prose as it streamed, an error
    /// only when nothing was said, and each lesson once.
    #[test]
    fn the_fold_keeps_the_prose_the_first_error_and_each_lesson_once() {
        let mut f = TurnFold::default();
        f.see(&AgentEvent::Token { text: "Hel".into() });
        f.see(&AgentEvent::Token { text: "lo".into() });
        f.see(&AgentEvent::Fired { id: "s".into(), lesson: "L1".into(), line: "x".into() });
        f.see(&AgentEvent::Fired { id: "t".into(), lesson: "L1".into(), line: "y".into() });
        f.see(&AgentEvent::Fired { id: "t".into(), lesson: "L2".into(), line: "y".into() });
        f.see(&AgentEvent::Error { message: "boom".into() });
        assert_eq!(f.text, "Hello", "something was said, so the error does not replace it");
        assert_eq!(f.fired, ["L1", "L2"]);

        let mut f = TurnFold::default();
        f.see(&AgentEvent::Error { message: "boom".into() });
        assert_eq!(f.text, "That didn't work: boom");
    }

    // -- the whole turn, against a fake model server ------------------------------

    use crate::agent::browser::BrowserPool;
    use crate::agent::fleet::Fleet;
    use crate::commands::files::DialogGrants;
    use crate::db::Db;
    use crate::memory::MemoryStore;
    use crate::permissions::PermissionManager;
    use crate::runtime::{EmbedManager, RerankManager, RuntimeManager};

    type Bodies = Arc<Mutex<Vec<serde_json::Value>>>;

    /// An OpenAI-shaped server that answers every chat request with `reply` as a
    /// stream, and keeps the bodies it was sent.
    async fn fake_model(reply: &'static str) -> (String, Bodies) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let bodies: Bodies = Arc::default();
        let seen = bodies.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    let (head_end, len) = loop {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..p]).to_ascii_lowercase();
                            let len = head
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break (p + 4, len);
                        }
                    };
                    while buf.len() < head_end + len {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf[head_end..head_end + len]) {
                        seen.lock().unwrap().push(v);
                    }
                    let frame = serde_json::json!({ "choices": [{ "delta": { "content": reply } }] });
                    let body = format!("data: {frame}\n\ndata: [DONE]\n\n");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), bodies)
    }

    /// Every long-lived handle a turn needs, over an empty database.
    struct Rig {
        db: Db,
        mgr: RuntimeManager,
        embed: EmbedManager,
        rerank: RerankManager,
        perms: PermissionManager,
        memory: MemoryStore,
        pool: BrowserPool,
        fleet: Fleet,
        grants: DialogGrants,
        tmp: tempfile::TempDir,
    }

    impl Rig {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            Rig {
                db: Db::open_in_memory().unwrap(),
                mgr: RuntimeManager::new(tmp.path().to_path_buf()),
                embed: EmbedManager::new(),
                rerank: RerankManager::new(),
                perms: PermissionManager::new(),
                memory: MemoryStore::new(tmp.path()).unwrap(),
                pool: BrowserPool::new(),
                fleet: Fleet::new(),
                grants: DialogGrants::new(),
                tmp,
            }
        }

        fn env(&self) -> TurnEnv<'_> {
            TurnEnv {
                mgr: &self.mgr,
                embed_mgr: &self.embed,
                rerank_mgr: &self.rerank,
                db: &self.db,
                perms: &self.perms,
                memory: &self.memory,
                browser_pool: &self.pool,
                fleet: &self.fleet,
                grants: &self.grants,
            }
        }

        /// A conversation, and a target pointing at the fake server.
        fn conversation_on(&self, base_url: &str) -> (String, ChatTarget) {
            let conv = self.db.create_conversation("A chat", None, false).unwrap().id;
            let ep = self.db.insert_local_endpoint("fake", base_url, 8192).unwrap();
            let target = ChatTarget {
                provenance: Some("endpoint".into()),
                provider: Some(ep.id),
                model: Some("fake-model".into()),
            };
            (conv, target)
        }
    }

    /// Events a channel was sent, as JSON.
    fn recording_channel() -> (Channel<AgentEvent>, Arc<Mutex<Vec<serde_json::Value>>>) {
        let seen: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
        let writer = seen.clone();
        let channel = Channel::new(move |body: tauri::ipc::InvokeResponseBody| {
            if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    writer.lock().unwrap().push(v);
                }
            }
            Ok(())
        });
        (channel, seen)
    }

    fn typed(conv: &str, target: ChatTarget, text: &str) -> TurnInput {
        let mut i = input(text);
        i.conversation_id = conv.into();
        i.target = target;
        i.model = TurnModel { name: "Fake".into(), provenance: "endpoint".into(), vision: false, ctx_size: Some(8192) };
        i
    }

    /// The first request that was a chat, as the model received it.
    fn first_chat(bodies: &Bodies) -> Vec<serde_json::Value> {
        let all = bodies.lock().unwrap();
        let body = all.iter().find(|b| b["messages"].is_array()).expect("a chat request");
        body["messages"].as_array().unwrap().clone()
    }

    /// `HRW-T-2`: a typed message is saved, run and finished, and the window is
    /// told the real ids before anything slow happens.
    #[tokio::test]
    async fn a_typed_turn_saves_its_rows_runs_and_finishes_the_answer() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("Hello there").await;
        let (conv, target) = rig.conversation_on(&url);
        let (channel, events) = recording_channel();
        let mut turn = typed(&conv, target, "Say hi");
        turn.spoken = true;

        run_turn(&rig.env(), turn, channel).await;

        let rows = rig.db.list_messages(&conv).unwrap();
        assert_eq!(rows.len(), 2, "the user's row and the answer's");
        assert_eq!((rows[0].role.as_str(), rows[0].content.as_str()), ("user", "Say hi"));
        assert_eq!(rows[1].role, "assistant");
        assert_eq!(rows[1].content, "Hello there");
        assert_eq!(rows[1].model_name.as_deref(), Some("Fake"));
        assert_eq!(rows[1].model_provenance.as_deref(), Some("endpoint"));
        assert_eq!(rows[1].stop_reason, None, "a finished run leaves no stop reason");
        assert!(rows[0].spoken && rows[1].spoken, "spoken is saved on both rows");
        assert!(rig.db.message_context_json(&rows[1].id).unwrap().is_some(), "the record of what reached it");

        let events = events.lock().unwrap();
        assert_eq!(events[0]["type"], "turn_saved", "before anything slow: {events:?}");
        assert_eq!(events[0]["user_message_id"], rows[0].id.as_str());
        assert_eq!(events[0]["assistant_message_id"], rows[1].id.as_str());
        assert!(events.iter().any(|e| e["type"] == "token" && e["text"] == "Hello there"));
        let last = events.last().unwrap();
        assert_eq!(last["type"], "turn_finalized", "the last thing a turn says");
        assert_eq!(last["message_id"], rows[1].id.as_str());
        assert!(last["steps_json"].is_null(), "no steps, nothing saved: {last}");

        let sent = first_chat(&bodies);
        assert_eq!(sent.first().unwrap()["role"], "system");
        let last = sent.last().unwrap();
        assert_eq!((last["role"].as_str(), last["content"].as_str()), (Some("user"), Some("Say hi")));
    }

    /// The window saved the rows before it started the engine (`save_turn_cmd`):
    /// the turn runs on them and adds none of its own.
    #[tokio::test]
    async fn a_turn_saved_ahead_runs_on_its_rows() {
        let rig = Rig::new();
        let (url, _bodies) = fake_model("Hello there").await;
        let (conv, target) = rig.conversation_on(&url);
        let (channel, events) = recording_channel();
        let mut turn = typed(&conv, target, "Say hi");
        let saved = save_turn(&rig.db, &turn).unwrap();
        turn.saved = Some(saved.clone());

        run_turn(&rig.env(), turn, channel).await;

        let rows = rig.db.list_messages(&conv).unwrap();
        assert_eq!(rows.len(), 2, "no second pair of rows");
        assert_eq!(rows[0].id, saved.user_message_id);
        assert_eq!((rows[1].id.as_str(), rows[1].content.as_str()), (saved.assistant_message_id.as_str(), "Hello there"));
        let events = events.lock().unwrap();
        assert_eq!(events[0]["type"], "turn_saved");
        assert_eq!(events[0]["assistant_message_id"], saved.assistant_message_id.as_str());
    }

    /// An image reaches a model that can see it as a part of the message.
    #[tokio::test]
    async fn an_image_goes_as_a_part_to_a_model_that_sees() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("A cat").await;
        let (conv, target) = rig.conversation_on(&url);
        let mut turn = typed(&conv, target, "What is this?");
        turn.model.vision = true;
        turn.attachments = vec![TurnAttachment {
            kind: "image".into(),
            name: "cat.png".into(),
            path: String::new(),
            data_uri: Some("data:image/png;base64,AAAA".into()),
        }];
        let (channel, _) = recording_channel();

        run_turn(&rig.env(), turn, channel).await;

        let sent = first_chat(&bodies);
        let content = &sent.last().unwrap()["content"];
        assert_eq!(content[0], serde_json::json!({ "type": "text", "text": "What is this?" }));
        assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,AAAA");
        let rows = rig.db.list_messages(&conv).unwrap();
        assert_eq!(rows[0].attachments.len(), 1, "the attachment is saved on the user's row");
        assert_eq!(rows[0].attachments[0].name, "cat.png");
    }

    /// A one-page PDF with the words "Hello from the PDF".
    fn tiny_pdf() -> Vec<u8> {
        let stream = "BT /F1 18 Tf 20 100 Td (Hello from the PDF) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for o in &offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes(),
        );
        out
    }

    /// `CHT-8`: the words of an attached PDF are part of the message the model
    /// reads, and one that cannot be read is named as such.
    #[tokio::test]
    async fn a_pdfs_words_are_in_the_message_the_model_reads() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("ok").await;
        let (conv, target) = rig.conversation_on(&url);
        let good = rig.tmp.path().join("good.pdf");
        std::fs::write(&good, tiny_pdf()).unwrap();
        rig.grants.remember(&good);
        let mut turn = typed(&conv, target, "Summarise");
        turn.attachments = vec![
            TurnAttachment { kind: "pdf".into(), name: "good.pdf".into(), path: good.to_string_lossy().into(), data_uri: None },
            TurnAttachment { kind: "pdf".into(), name: "gone.pdf".into(), path: "/no/such/file.pdf".into(), data_uri: None },
        ];
        let (channel, _) = recording_channel();

        run_turn(&rig.env(), turn, channel).await;

        let sent = first_chat(&bodies);
        let content = sent.last().unwrap()["content"].as_str().unwrap().to_string();
        assert!(content.starts_with("Summarise\n\n[Attached PDF: good.pdf]\n"), "{content}");
        assert!(content.contains("Hello from the PDF"), "{content}");
        assert!(content.ends_with("[Attached PDF: gone.pdf \u{2014} couldn't be read]"), "{content}");
    }

    /// With no model to run on, the turn still leaves both rows, and the answer's
    /// says why.
    #[tokio::test]
    async fn a_turn_with_nowhere_to_run_says_so_on_its_row() {
        let rig = Rig::new();
        let conv = rig.db.create_conversation("A chat", None, false).unwrap().id;
        let (channel, events) = recording_channel();
        let mut turn = typed(&conv, ChatTarget::default(), "Anyone there?");
        turn.model.provenance = "local".into();

        run_turn(&rig.env(), turn, channel).await;

        let rows = rig.db.list_messages(&conv).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1].content,
            "That didn't work: No model is loaded yet. Pick a model to get started."
        );
        let events = events.lock().unwrap();
        assert_eq!(events[0]["type"], "turn_saved");
        assert!(events.iter().any(|e| e["type"] == "error"));
    }

    /// `HRW-T-5`: with nothing to resume the answer is a row that says so.
    #[tokio::test]
    async fn resuming_with_nothing_to_resume_saves_the_fixed_words() {
        let rig = Rig::new();
        let conv = rig.db.create_conversation("A chat", None, false).unwrap().id;
        let (channel, events) = recording_channel();
        let model = TurnModel { name: "Fake".into(), provenance: "endpoint".into(), vision: false, ctx_size: None };

        let resumed =
            resume_turn(&rig.env(), &conv, &model, ChatTarget::default(), false, RunOptions::default(), channel).await;

        assert!(!resumed);
        let rows = rig.db.list_messages(&conv).unwrap();
        assert_eq!(rows.len(), 1, "one answer row, no user row");
        assert_eq!(rows[0].role, "assistant");
        assert_eq!(rows[0].content, NOTHING_TO_RESUME);
        let events = events.lock().unwrap();
        assert_eq!(events[0]["type"], "turn_saved");
        assert!(events[0]["user_message_id"].is_null());
        assert_eq!(events[0]["assistant_message_id"], rows[0].id.as_str());
        assert_eq!(events.last().unwrap()["type"], "turn_finalized");
    }

    /// `HRW-5`: the steps the events described are what the row holds, with the
    /// plan and the reason it stopped, and the window is handed the same text.
    #[tokio::test]
    async fn the_row_holds_the_timeline_the_plan_and_why_it_stopped() {
        let rig = Rig::new();
        let conv = rig.db.create_conversation("A chat", None, false).unwrap().id;
        let row = rig
            .db
            .append_message(
                &conv,
                &NewMessage {
                    role: "assistant".into(),
                    content: String::new(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        let (channel, events) = recording_channel();
        let (sink, fold) = folding_sink(channel);
        let plan = Plan {
            items: vec![crate::agent::plan::PlanItem {
                text: "read it".into(),
                status: crate::agent::plan::PlanStatus::Done,
                why: None,
                added: false,
                judged: None,
                decision: None,
            }],
            ..Default::default()
        };
        sink.emit(AgentEvent::Recall {
            id: "recall-1".into(),
            matches: vec![hit("Likes tea")],
        });
        sink.step_start("a", "read", "notes.md");
        sink.emit(AgentEvent::Fired { id: "a".into(), lesson: "L1".into(), line: "careful".into() });
        sink.emit(AgentEvent::StepDone { id: "a".into(), result: Some("— 4 lines".into()) });
        sink.emit(AgentEvent::Plan { run_id: "r".into(), plan });
        sink.token("Here you go");

        finalize(&rig.env(), &sink, &row.id, &fold, None, Some(StopReason::MaxSteps));

        let saved: serde_json::Value =
            serde_json::from_str(&rig.db.list_messages(&conv).unwrap()[0].steps_json.clone().unwrap()).unwrap();
        assert_eq!(saved[0]["verb"], "remembered", "recall first");
        assert_eq!(saved[0]["target"], "Likes tea");
        assert_eq!(saved[1]["id"], "a");
        assert_eq!(saved[1]["result"], "— 4 lines");
        assert_eq!(saved[1]["fired"][0]["lesson"], "L1");
        let rows = rig.db.list_messages(&conv).unwrap();
        assert_eq!(rows[0].content, "Here you go");
        assert_eq!(rows[0].stop_reason.as_deref(), Some("max_steps"));
        assert!(rig.db.message_plan_json(&row.id).unwrap().unwrap().contains("read it"));

        let events = events.lock().unwrap();
        let told = events.last().unwrap();
        assert_eq!(told["type"], "turn_finalized");
        let told_steps: serde_json::Value = serde_json::from_str(told["steps_json"].as_str().unwrap()).unwrap();
        assert_eq!(told_steps, saved, "the window gets exactly what was saved");
    }

    /// `HRW-T-6`: a spoken reply cut off keeps what was heard; the whole reply
    /// moves beside the timeline; the row says it was interrupted.
    #[test]
    fn a_cut_off_reply_keeps_what_was_heard_and_the_whole_beside_the_steps() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("A chat", None, false).unwrap().id;
        let row = db
            .append_message(
                &conv,
                &NewMessage {
                    role: "assistant".into(),
                    content: String::new(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        let steps = r#"[{"id":"a","verb":"read","target":"x","status":"done"}]"#;
        db.finalize_message(&row.id, "One. Two. Three.", Some(steps), None, None, None).unwrap();

        assert!(db.set_message_heard(&row.id, "One.").unwrap());

        let saved = &db.list_messages(&conv).unwrap()[0];
        assert_eq!(saved.content, "One.");
        assert_eq!(saved.stop_reason.as_deref(), Some("interrupted"));
        let parsed: serde_json::Value = serde_json::from_str(saved.steps_json.as_deref().unwrap()).unwrap();
        assert_eq!(parsed["unspoken"], "One. Two. Three.");
        assert_eq!(parsed["steps"][0]["id"], "a");

        // Said again, the whole reply is not overwritten by what was heard.
        db.set_message_heard(&row.id, "One. Two.").unwrap();
        let again: serde_json::Value =
            serde_json::from_str(db.list_messages(&conv).unwrap()[0].steps_json.as_deref().unwrap()).unwrap();
        assert_eq!(again["unspoken"], "One. Two. Three.");
        assert!(!db.set_message_heard("no-such-row", "x").unwrap());

        // `USE-8`: a check that reports late rewrites the steps, not the whole reply.
        let flagged = r#"[{"id":"a","verb":"read","target":"x","status":"done","untrusted":[]}]"#;
        db.replace_message_steps(&row.id, flagged).unwrap();
        let late: serde_json::Value =
            serde_json::from_str(db.list_messages(&conv).unwrap()[0].steps_json.as_deref().unwrap()).unwrap();
        assert_eq!(late["unspoken"], "One. Two. Three.");
        assert!(late["steps"][0]["untrusted"].is_array(), "{late}");

        // A reply with no steps still holds the whole of it.
        let bare = db
            .append_message(
                &conv,
                &NewMessage {
                    role: "assistant".into(),
                    content: "Only words.".into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        db.set_message_heard(&bare.id, "Only").unwrap();
        let bare = db.list_messages(&conv).unwrap().into_iter().find(|m| m.id == bare.id).unwrap();
        assert_eq!(bare.steps_json.as_deref().map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap()),
            Some(serde_json::json!({"steps": [], "unspoken": "Only words."})));
    }

    /// `HRW-T-5`: a run that was logged is picked up from the log, and the
    /// answer is a new row that is finished like any other.
    #[tokio::test]
    async fn resuming_a_logged_run_sends_its_work_back_and_finishes_a_new_row() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("Done now").await;
        let (conv, target) = rig.conversation_on(&url);
        let (channel, _) = recording_channel();
        run_turn(&rig.env(), typed(&conv, target.clone(), "Start the job"), channel).await;
        bodies.lock().unwrap().clear();

        let (channel, events) = recording_channel();
        let model = TurnModel { name: "Fake".into(), provenance: "endpoint".into(), vision: false, ctx_size: None };
        let resumed = resume_turn(&rig.env(), &conv, &model, target, false, RunOptions::default(), channel).await;

        assert!(resumed);
        let rows = rig.db.list_messages(&conv).unwrap();
        assert_eq!(rows.len(), 3, "user, first answer, resumed answer");
        assert_eq!(rows[2].role, "assistant");
        assert_eq!(rows[2].content, "Done now");
        let events = events.lock().unwrap();
        assert_eq!(events[0]["type"], "turn_saved");
        assert!(events[0]["user_message_id"].is_null());
        assert_eq!(events[0]["assistant_message_id"], rows[2].id.as_str());
        let sent = first_chat(&bodies);
        let told = sent.last().unwrap();
        assert_eq!(told["role"], "system");
        assert_eq!(told["content"], crate::agent::log::RESUME_PROMPT);
        assert!(
            sent.iter().any(|m| m["role"] == "user" && m["content"] == "Start the job"),
            "the interrupted run's own transcript came back: {sent:?}"
        );
    }

    // -- history, budget and compaction (HRW-2) ----------------------------------

    /// The last request that was a chat: for a turn that compacted first, the
    /// first one is the summary's.
    fn last_chat(bodies: &Bodies) -> Vec<serde_json::Value> {
        let all = bodies.lock().unwrap();
        let body = all.iter().rev().find(|b| b["messages"].is_array()).expect("a chat request");
        body["messages"].as_array().unwrap().clone()
    }

    /// Save `n` earlier turns, each about `len` characters, alternating roles.
    fn earlier_turns(rig: &Rig, conv: &str, n: usize, len: usize) -> Vec<String> {
        (0..n)
            .map(|i| {
                let role = if i % 2 == 0 { "user" } else { "assistant" };
                let content = format!("turn {i} {}", "x".repeat(len));
                rig.db
                    .append_message(
                        conv,
                        &NewMessage {
                            role: role.into(),
                            content,
                            model_name: None,
                            model_provenance: None,
                            steps_json: None,
                            attachments: Vec::new(),
                        },
                    )
                    .unwrap()
                    .id
            })
            .collect()
    }

    fn model_with(provenance: &str, ctx_size: Option<i64>) -> TurnModel {
        TurnModel { name: "m".into(), provenance: provenance.into(), vision: false, ctx_size }
    }

    /// The budget is what the window worked out, case by case.
    #[test]
    fn the_budget_is_the_providers_window_then_the_servers_then_the_engines() {
        let cloud = model_with("cloud", None);
        assert_eq!(context_budget(&cloud, Some("anthropic"), None), 200_000);
        assert_eq!(context_budget(&cloud, Some("openai"), None), 128_000);
        assert_eq!(context_budget(&cloud, Some("openrouter"), None), 32_000);
        assert_eq!(context_budget(&cloud, Some("somebody-else"), None), 32_000);
        assert_eq!(context_budget(&cloud, None, None), 32_000);

        assert_eq!(context_budget(&model_with("endpoint", Some(16_384)), None, Some(4096)), 16_384);
        assert_eq!(context_budget(&model_with("endpoint", None), None, Some(4096)), 8192, "added without a window");

        assert_eq!(context_budget(&model_with("local", None), None, Some(12_000)), 12_000);
        assert_eq!(context_budget(&model_with("local", None), None, None), 4096, "engine has not said");
    }

    /// Earlier turns go in order, between the prompt and the new message; an
    /// empty row and the turn being sent are not among them.
    #[tokio::test]
    async fn earlier_turns_are_sent_once_in_order_without_empty_rows() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("ok").await;
        let (conv, target) = rig.conversation_on(&url);
        earlier_turns(&rig, &conv, 2, 10);
        rig.db
            .append_message(
                &conv,
                &NewMessage {
                    role: "assistant".into(),
                    content: "  ".into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        let (channel, _) = recording_channel();

        run_turn(&rig.env(), typed(&conv, target, "and now?"), channel).await;

        let sent = last_chat(&bodies);
        let roles: Vec<_> = sent.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"], "{sent:?}");
        assert!(sent[1]["content"].as_str().unwrap().starts_with("turn 0"));
        assert!(sent[2]["content"].as_str().unwrap().starts_with("turn 1"));
        assert_eq!(sent[3]["content"], "and now?", "the new message once, not also as history");
    }

    /// Turns the summary already stands in for are not sent again; the summary
    /// rides in the system prompt.
    #[tokio::test]
    async fn a_summary_replaces_the_turns_it_covers() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("ok").await;
        let (conv, target) = rig.conversation_on(&url);
        let ids = earlier_turns(&rig, &conv, 4, 10);
        rig.db.set_conversation_summary(&conv, "FACTS: the sky is blue", &ids[1]).unwrap();
        let (channel, _) = recording_channel();

        run_turn(&rig.env(), typed(&conv, target, "next"), channel).await;

        let sent = last_chat(&bodies);
        let system = sent[0]["content"].as_str().unwrap();
        assert!(system.contains("## Conversation so far"), "{system}");
        assert!(system.contains("the sky is blue"));
        let texts: Vec<_> = sent[1..].iter().map(|m| m["content"].as_str().unwrap()).collect();
        assert!(texts[0].starts_with("turn 2") && texts[1].starts_with("turn 3"), "{texts:?}");
        assert_eq!(texts.len(), 3);
    }

    /// History that does not fit is summarized once, the window is told, and the
    /// turn runs on the summary plus the recent turns.
    #[tokio::test]
    async fn overflowing_history_is_summarized_and_the_window_is_told() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("SUMMARY TEXT").await;
        let (conv, target) = rig.conversation_on(&url);
        let ids = earlier_turns(&rig, &conv, 10, 400);
        let mut turn = typed(&conv, target, "go on");
        turn.model.ctx_size = Some(800);
        let (channel, events) = recording_channel();

        run_turn(&rig.env(), turn, channel).await;

        let events = events.lock().unwrap();
        let compacted: Vec<_> = events.iter().filter(|e| e["type"] == "compacted").collect();
        assert_eq!(compacted.len(), 1, "{events:?}");
        assert_eq!(compacted[0]["summary"], "SUMMARY TEXT");
        assert_eq!(compacted[0]["conversation_id"], conv.as_str());
        let upto = compacted[0]["summary_upto_message_id"].as_str().unwrap();
        assert_eq!(upto, ids[3], "the oldest four did not fit");
        assert!(
            events.iter().position(|e| e["type"] == "compacted") < events.iter().position(|e| e["type"] == "token"),
            "told before the answer streams"
        );

        let saved = rig.db.get_conversation(&conv).unwrap().unwrap();
        assert_eq!(saved.summary.as_deref(), Some("SUMMARY TEXT"));
        assert_eq!(saved.summary_upto_message_id.as_deref(), Some(upto));

        let sent = last_chat(&bodies);
        assert!(sent[0]["content"].as_str().unwrap().contains("SUMMARY TEXT"));
        assert_eq!(sent.len(), 1 + 6 + 1, "system, the six recent turns, the new message");
        assert!(sent[1]["content"].as_str().unwrap().starts_with("turn 4"));
        assert_eq!(rig.db.list_messages(&conv).unwrap().len(), 12, "nothing was deleted");
    }

    /// With auto-compact off the oldest turns are dropped from what is sent and
    /// the turn still runs; nothing is summarized or announced.
    #[tokio::test]
    async fn with_auto_compact_off_the_oldest_turns_are_dropped_not_summarized() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("fine").await;
        let (conv, target) = rig.conversation_on(&url);
        rig.db.set_setting(AUTOCOMPACT_KEY, "false").unwrap();
        earlier_turns(&rig, &conv, 10, 400);
        let mut turn = typed(&conv, target, "go on");
        turn.model.ctx_size = Some(800);
        let (channel, events) = recording_channel();

        run_turn(&rig.env(), turn, channel).await;

        assert!(!events.lock().unwrap().iter().any(|e| e["type"] == "compacted"));
        assert!(rig.db.get_conversation(&conv).unwrap().unwrap().summary.is_none());
        let sent = last_chat(&bodies);
        assert_eq!(sent.len(), 1 + 6 + 1, "the six recent turns are never dropped");
        assert_eq!(rig.db.list_messages(&conv).unwrap().last().unwrap().content, "fine");
    }

    /// A Workspace conversation keeps fewer turns word for word, because the
    /// live surface carries the task.
    #[tokio::test]
    async fn a_workspace_conversation_keeps_three_recent_turns_not_six() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("fine").await;
        let (conv, target) = rig.conversation_on(&url);
        rig.db.set_conversation_workspace(&conv, true).unwrap();
        rig.db.set_setting(AUTOCOMPACT_KEY, "false").unwrap();
        earlier_turns(&rig, &conv, 10, 400);
        let mut turn = typed(&conv, target, "go on");
        turn.model.ctx_size = Some(400);
        let (channel, _) = recording_channel();

        run_turn(&rig.env(), turn, channel).await;

        // The loop drops an assistant turn that would open the transcript, so
        // the third-newest can be missing; the fourth-newest must be.
        let told = last_chat(&bodies).iter().map(|m| m["content"].to_string()).collect::<String>();
        assert!(told.contains("turn 8") && told.contains("turn 9"), "{told}");
        assert!(!told.contains("turn 6"), "only three are kept: {told}");
    }

    /// With the composer's tools switch off the memory index still goes in,
    /// marked as the only detail there is, as the window always sent it.
    #[tokio::test]
    async fn the_memory_index_goes_in_with_tools_off() {
        let rig = Rig::new();
        let (url, bodies) = fake_model("ok").await;
        let (conv, target) = rig.conversation_on(&url);
        rig.memory
            .save(
                &rig.db,
                &crate::memory::Fact {
                    name: "tea".into(),
                    description: "drinks green tea".into(),
                    kind: "preference".into(),
                    body: "Drinks green tea every morning.".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let (channel, _) = recording_channel();

        run_turn(&rig.env(), typed(&conv, target, "what do I drink?"), channel).await;

        let sent = last_chat(&bodies);
        let system = sent[0]["content"].as_str().unwrap();
        assert!(system.contains("Your notes about the user"), "{system}");
        assert!(system.contains("Tools are off"), "{system}");
        assert!(system.contains("tea"));
    }
}
