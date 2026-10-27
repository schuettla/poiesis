//! Commands over the durable self (MEM-3, MEM-5, SOUL-3, MEM-UI).
//!
//! The user-facing half of the memory system: read the index for injection,
//! browse and edit facts by hand, and review what the agent proposed. Nothing
//! here lets the agent apply a change on its own — `consolidate` and
//! `propose_soul_edit` both stop at a proposal the user answers.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::autonomy::{autonomy_gate, Rung};
use crate::cloud::{drive_turn, ChatEndpoint};
use crate::commands::agent::{build_remote_endpoint, ChatTarget};
use crate::commands::embedgen::embed_texts_or_none;
use crate::db::vectors::NewVector;
use crate::db::{ChangeProposal, Db, SearchHit};
use crate::memory::belief::Outcome;
use crate::memory::{Fact, MemoryStore, Profile, FACTS, LESSONS};
use crate::runtime::proxy::{CancelFlag, TurnOutcome};
use crate::runtime::{EmbedManager, RuntimeManager};
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

/// Settings key holding a consolidation the user hasn't answered yet.
const PENDING_CONSOLIDATION: &str = "memory.pending_consolidation";
/// `PRO-9`: the snapshot name to undo the most recent automatic or manual
/// rebuild. Overwritten by every rebuild; a plain setting is enough for one
/// level of undo, matching every other memory-write toast in this app.
const PROFILE_LAST_SNAPSHOT: &str = "profile.last_snapshot";

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// What gets prepended to every conversation (MEM-3).
#[derive(Debug, Serialize)]
pub struct MemoryContext {
    /// The generated MEMORY.md body — one line per entry.
    pub index: String,
    /// Standing instructions the user approved (SOUL.md).
    pub soul: String,
    /// `PRO-6`: the synthesized style profile's body, or empty when none
    /// exists yet — `PROFILE.md` read fresh, not cached, since a rebuild can
    /// land between turns.
    pub about_you: String,
    pub fact_count: usize,
}

#[tauri::command]
pub fn get_memory_context_cmd(mem: State<'_, MemoryStore>) -> MemoryContext {
    MemoryContext {
        index: mem.index_markdown(),
        soul: mem.soul(),
        about_you: mem.profile().map(|p| p.body).unwrap_or_default(),
        fact_count: mem.list().len(),
    }
}

/// What `recall_for` produces for one turn (SEM-3): the always-injected
/// block (facts, and — with no embedder — lessons too, unchanged
/// from today, SEM-4) plus what was actually retrieved by meaning, shaped as
/// `SearchHit`s so the timeline renders them through the existing recall
/// provenance UI (SEM-5) with no new component.
#[derive(Debug, Serialize)]
pub struct RecallResult {
    pub index: String,
    pub matches: Vec<SearchHit>,
    /// Names of the facts that actually reached the prompt this turn (WHY-2:
    /// the client has no other way to know which facts the cap kept).
    pub injected_facts: Vec<String>,
}

/// `SCP-3`: pick up at most 3 facts nobody has classified yet and ask the
/// current chat model, one call each, to bound how much latency a backlog
/// can add to a single turn. No model loaded ⇒ skip entirely this round
/// rather than burn a slot on a request that can't be made; a call that
/// fails is marked `global` so it doesn't retry forever (`SCP-2`).
async fn backfill_scope(mgr: &RuntimeManager, db: &Db, mem: &MemoryStore) {
    let missing = mem.facts_missing_scope();
    if missing.is_empty() {
        return;
    }
    let Some((base_url, token)) = mgr.engine_endpoint().await else {
        return;
    };
    let endpoint = ChatEndpoint::OpenAi { base_url, api_key: Some(token), model: None };
    for f in missing.into_iter().take(3) {
        let scope = crate::agent::memory_skill::scope_for(
            &mgr.client,
            Some(&endpoint),
            &f.name,
            &f.description,
            &f.body,
            None,
        )
        .await
        .unwrap_or_else(|| "global".to_string());
        let _ = mem.set_fact_scope(db, &f.name, &scope);
    }
}

/// The no-embedder path (SEM-4): today's whole index, unchanged. Goes through
/// `recall_for(None)` rather than `index_markdown()` so the "last surfaced"
/// mark covers exactly the facts that fit inside the character cap — a fact
/// the cap dropped never reached the prompt and must not claim it did.
fn wholesale(db: &Db, mem: &MemoryStore, project: Option<&str>) -> RecallResult {
    let set = mem.recall_for_project(db, None, project);
    let _ = db.touch_memory_usage(FACTS, &set.injected_facts);
    RecallResult { index: set.index, matches: Vec::new(), injected_facts: set.injected_facts }
}

/// Embed `query`, backfill any fact/lesson missing a vector under the
/// current recall model in the same round trip (SEM-1/SEM-2), then retrieve.
/// Degrades to `index_markdown` unchanged whenever an embedder isn't ready —
/// no model, no engine, or a failed request (EMB-5, SEM-4).
#[tauri::command]
pub async fn recall_for_cmd(
    mgr: State<'_, RuntimeManager>,
    embed_mgr: State<'_, EmbedManager>,
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    query: String,
    conversation_id: Option<String>,
) -> Cmd<RecallResult> {
    // `PRJ-8`: scoped to the conversation's project when one is named. The
    // caller may legitimately have none — a manual search of memory is not
    // happening anywhere in particular.
    let project = conversation_id
        .as_deref()
        .and_then(|id| db.conversation_project(id).ok().flatten())
        .map(|p| p.id);
    Ok(recall_for(&mgr, &embed_mgr, &db, &mem, &query, project.as_deref(), conversation_id.as_deref()).await)
}

/// The body of `recall_for_cmd`, callable without a Tauri `State`.
///
/// `CTX-3` needs the same scoped recall the chat window gets, from inside the
/// backend's own prompt assembly. Two copies of this would mean a scheduled job
/// recalling different memories than a typed message, which is exactly the drift
/// that track exists to end. Anything that fails here degrades to the whole
/// index rather than to no memory at all.
pub async fn recall_for(
    mgr: &RuntimeManager,
    embed_mgr: &EmbedManager,
    db: &Db,
    mem: &MemoryStore,
    query: &str,
    project: Option<&str>,
    // `CUE-1`: the chat the turn is in, for the names its notes hold.
    conversation_id: Option<&str>,
) -> RecallResult {
    backfill_scope(mgr, db, mem).await;

    let Ok(Some(model)) = db.default_model_by_role("embed") else {
        return wholesale(db, mem, project);
    };
    let query = query.to_string();

    let missing = mem.missing_vector_texts(db, &model.name);
    let mut texts = vec![query.clone()];
    texts.extend(missing.iter().map(|(_, _, t)| t.clone()));

    let Some((vectors, model_name, dim)) = embed_texts_or_none(mgr, embed_mgr, db, &texts).await
    else {
        return wholesale(db, mem, project);
    };

    if !missing.is_empty() {
        let rows: Vec<NewVector> = missing
            .iter()
            .zip(&vectors[1..])
            .map(|((collection, name, text), v)| NewVector {
                owner_kind: "memory".into(),
                scope_key: collection.clone(),
                ref_key: name.clone(),
                chunk_ix: 0,
                text: text.clone(),
                model: model_name.clone(),
                dim,
                vec: v.clone(),
                mtime: None,
            })
            .collect();
        let _ = db.insert_vectors(&rows);
    }

    let session_names = conversation_id
        .and_then(|id| db.get_session_state(id).ok().flatten())
        .map(|state| crate::agent::episode::state_entities(&state))
        .unwrap_or_default();
    let cues = crate::memory::Cues::from_turn(&query, session_names);
    let set = mem.recall_with_cues(db, Some((&vectors[0], &model_name, dim)), project, &cues);
    let _ = db.touch_memory_usage(FACTS, &set.injected_facts);
    let matches = set
        .retrieved
        .iter()
        .map(|r| SearchHit {
            source: "memory".to_string(),
            // The chat it was learned in, when that chat still exists: the
            // timeline links to it (`CUE-UI-1`).
            conversation_id: r
                .source_conversation
                .clone()
                .filter(|id| db.get_conversation(id).ok().flatten().is_some()),
            title: r.name.clone(),
            created_at: date_ms(&r.created),
            snippet: r.description.clone(),
            kind: Some(r.kind.clone()),
            path: None,
            why: Some(r.why.clone()).filter(|w| !w.is_empty()),
        })
        .collect();
    RecallResult { index: set.index, matches, injected_facts: set.injected_facts }
}

/// A `YYYY-MM-DD` memory date as milliseconds, or 0 when it does not read.
fn date_ms(date: &str) -> i64 {
    chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(12, 0, 0))
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0)
}

// ---- context manifest (WHY-1/2): explaining what shaped an answer ----

/// The compact, per-message record `finalize_message_cmd` stores as
/// `context_json` (WHY-2) — slugs and an id, not prompt text, so a past
/// answer can be explained without re-storing the whole prompt. Built
/// client-side from the same `memory`/`matches` the turn already computed,
/// then handed back opaquely; only `context_manifest_cmd` ever parses it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextRefs {
    pub persona_id: Option<String>,
    #[serde(default)]
    pub soul_present: bool,
    /// `PRO-6`/`WHY-2`: whether the synthesized profile actually reached this
    /// turn's prompt. `#[serde(default)]` so history recorded before `PRO`
    /// shipped reads as `false` — that turn genuinely had none.
    #[serde(default)]
    pub about_you_present: bool,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub lessons: Vec<String>,
    /// `RET` file paths — always empty until Part III lands (`SMP` deferral).
    #[serde(default)]
    pub files: Vec<String>,
    /// `CRD-1`: why each retrieved entry reached this answer, in words, by
    /// name ("same project", "you named Anna"). Empty until recall can say
    /// (`CUE-2`); history written before then reads as "no reason recorded".
    #[serde(default)]
    pub why: std::collections::BTreeMap<String, String>,
    /// `CRD-1`: lessons that fired at a tool call during this answer (`HAB-1`).
    #[serde(default)]
    pub fired: Vec<String>,
}

impl ContextRefs {
    /// What one turn used: the same record the chat window builds for its own
    /// turns (`buildContextRefs` in `store.ts`), for the runs the backend
    /// assembles itself. Two builders, one shape.
    pub fn for_turn(
        persona_id: Option<String>,
        soul_present: bool,
        about_you_present: bool,
        injected_facts: &[String],
        matches: &[SearchHit],
    ) -> Self {
        ContextRefs {
            persona_id,
            soul_present,
            about_you_present,
            facts: injected_facts.to_vec(),
            lessons: matches.iter().filter(|m| m.kind.as_deref() == Some("lesson")).map(|m| m.title.clone()).collect(),
            why: matches
                .iter()
                .filter_map(|m| m.why.clone().map(|w| (m.title.clone(), w)))
                .collect(),
            ..Default::default()
        }
    }
}

/// One labelled slice of what shaped an answer (WHY-1). `always_on` layers
/// (soul, persona, about_you, remembered, session) ride on every turn;
/// retrieved layers (learned, procedures, from_files) were brought in for
/// this question specifically (WHY-3). Empty `text` is rendered by the panel
/// as "nothing from here" (WHY-5) rather than the layer being omitted.
#[derive(Debug, Serialize)]
pub struct ContextLayer {
    pub label: String,
    pub text: String,
    pub sources: Vec<String>,
    pub always_on: bool,
}

#[derive(Debug, Serialize)]
pub struct ContextManifest {
    /// `false` only when `message_id` was given and nothing was recorded for
    /// it — pre-`WHY-2` history, or a turn that failed before finalizing. The
    /// panel shows "I didn't record this one" instead of a guess (WHY-5).
    pub recorded: bool,
    pub layers: Vec<ContextLayer>,
}

/// One index-style line per named entry still on disk; a name the caller
/// listed that no longer exists (forgotten since) is silently dropped rather
/// than guessed at.
///
/// `why` (`CUE-UI-1`) is the reason recall gave for an entry, by name; an entry
/// that has one carries it at the end of its line.
fn rehydrate(mem: &MemoryStore, collection: &str, names: &[String], why: &std::collections::BTreeMap<String, String>) -> String {
    names
        .iter()
        .filter_map(|n| mem.read_in(collection, n))
        .map(|f| match why.get(&f.name) {
            Some(w) => format!("- [{}] ({}) {} \u{2014} {w}\n", f.name, f.kind, f.description),
            None => format!("- [{}] ({}) {}\n", f.name, f.kind, f.description),
        })
        .collect()
}

/// The two fact layers (`SCP` + `WHY-3`). Facts used to be one always-on
/// block; scoping split them in half, and the panel has to say so — a topical
/// fact is exactly the kind that *doesn't* ride every turn, and `SMP-6b` makes
/// this panel the answer to "why did you bring that up?". Listing them all as
/// "in every answer" would make the one surface that explains the prompt the
/// one surface that overstates it. A name with no file left is dropped from
/// both, the same as `rehydrate` does.
fn fact_layers(mem: &MemoryStore, names: &[String], why: &std::collections::BTreeMap<String, String>) -> [ContextLayer; 2] {
    let (mut global, mut topical) = (Vec::new(), Vec::new());
    for n in names {
        let Some(f) = mem.read_in(FACTS, n) else { continue };
        if f.scope.as_deref() == Some("topical") {
            topical.push(n.clone());
        } else {
            global.push(n.clone());
        }
    }
    [
        ContextLayer {
            label: "Remembered (notes)".into(),
            text: rehydrate(mem, FACTS, &global, why),
            sources: global,
            always_on: true,
        },
        ContextLayer {
            label: "Remembered (when relevant)".into(),
            text: rehydrate(mem, FACTS, &topical, why),
            sources: topical,
            always_on: false,
        },
    ]
}

fn persona_layer(db: &Db, persona_id: Option<&str>) -> ContextLayer {
    let persona = persona_id.and_then(|id| db.get_persona(id).ok().flatten());
    ContextLayer {
        label: "Persona / system prompt".into(),
        text: persona.as_ref().map(|p| p.system_prompt.clone()).unwrap_or_default(),
        sources: persona.map(|p| vec![p.name]).unwrap_or_default(),
        always_on: true,
    }
}

/// The live manifest (`message_id: None`): today's actual composition —
/// wholesale facts/lessons (`recall_for(None)`, same as an unread
/// turn would get) plus whichever persona and rolling summary the
/// conversation carries right now. This is what the composer chip shows.
fn live_manifest(db: &Db, mem: &MemoryStore, conversation_id: &str) -> Vec<ContextLayer> {
    let conv = db.get_conversation(conversation_id).ok().flatten();
    // `PRJ-8`: the manifest has to describe the prompt that will actually be
    // built, so it is scoped exactly as recall is.
    let project = conv.as_ref().and_then(|c| c.project_id.clone());
    let set = mem.recall_for_project(db, None, project.as_deref());
    let none = std::collections::BTreeMap::new();
    let [notes, when_relevant] = fact_layers(mem, &set.injected_facts, &none);
    let lessons = mem.list_in(LESSONS).into_iter().map(|f| f.name).collect::<Vec<_>>();
    vec![
        ContextLayer {
            label: "Soul (standing instructions)".into(),
            text: mem.soul(),
            sources: Vec::new(),
            always_on: true,
        },
        persona_layer(db, conv.as_ref().and_then(|c| c.persona_id.as_deref())),
        ContextLayer {
            label: "About you".into(),
            text: mem.profile().map(|p| p.body).unwrap_or_default(),
            sources: Vec::new(),
            always_on: true,
        },
        notes,
        when_relevant,
        ContextLayer {
            label: "Learned (lessons)".into(),
            text: rehydrate(mem, LESSONS, &lessons, &none),
            sources: lessons,
            always_on: false,
        },
        ContextLayer {
            label: "From your files".into(),
            text: String::new(),
            sources: Vec::new(),
            always_on: false,
        },
        ContextLayer {
            label: "Session".into(),
            text: conv.and_then(|c| c.summary).unwrap_or_default(),
            sources: Vec::new(),
            always_on: true,
        },
    ]
}

/// The historical manifest: rehydrate display text for exactly the slugs
/// `WHY-2` recorded for this message, from whatever those entries currently
/// say (an edited-since fact shows its current wording, not a frozen copy).
fn historical_manifest(db: &Db, mem: &MemoryStore, conversation_id: &str, refs: &ContextRefs) -> Vec<ContextLayer> {
    let conv = db.get_conversation(conversation_id).ok().flatten();
    // Split on what those facts say *now*, matching how this function already
    // rehydrates their wording from the current file rather than a frozen copy.
    let [notes, when_relevant] = fact_layers(mem, &refs.facts, &refs.why);
    vec![
        ContextLayer {
            label: "Soul (standing instructions)".into(),
            text: if refs.soul_present { mem.soul() } else { String::new() },
            sources: Vec::new(),
            always_on: true,
        },
        persona_layer(db, refs.persona_id.as_deref()),
        ContextLayer {
            label: "About you".into(),
            text: if refs.about_you_present { mem.profile().map(|p| p.body).unwrap_or_default() } else { String::new() },
            sources: Vec::new(),
            always_on: true,
        },
        notes,
        when_relevant,
        ContextLayer {
            label: "Learned (lessons)".into(),
            text: rehydrate(mem, LESSONS, &refs.lessons, &refs.why),
            sources: refs.lessons.clone(),
            always_on: false,
        },
        ContextLayer {
            label: "From your files".into(),
            text: String::new(),
            sources: refs.files.clone(),
            always_on: false,
        },
        ContextLayer {
            label: "Session".into(),
            text: conv.and_then(|c| c.summary).unwrap_or_default(),
            sources: Vec::new(),
            always_on: true,
        },
    ]
}

#[tauri::command]
pub fn context_manifest_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    conversation_id: String,
    message_id: Option<String>,
) -> Cmd<ContextManifest> {
    let Some(message_id) = message_id else {
        return Ok(ContextManifest { recorded: true, layers: live_manifest(&db, &mem, &conversation_id) });
    };
    let Some(raw) = db.message_context_json(&message_id).map_err(err)? else {
        return Ok(ContextManifest { recorded: false, layers: Vec::new() });
    };
    let Ok(refs) = serde_json::from_str::<ContextRefs>(&raw) else {
        return Ok(ContextManifest { recorded: false, layers: Vec::new() });
    };
    Ok(ContextManifest {
        recorded: true,
        layers: historical_manifest(&db, &mem, &conversation_id, &refs),
    })
}

/// A fact plus when it last actually reached a prompt (SEM-UI-4).
#[derive(Debug, Serialize)]
pub struct FactRow {
    #[serde(flatten)]
    pub fact: Fact,
    pub last_used_at: Option<i64>,
}

#[tauri::command]
pub fn list_memory_facts_cmd(mem: State<'_, MemoryStore>, db: State<'_, Db>) -> Cmd<Vec<FactRow>> {
    let usage = db.memory_usage_map(FACTS).map_err(err)?;
    Ok(mem
        .list()
        .into_iter()
        .map(|f| {
            let last_used_at = usage.get(&f.name).copied();
            FactRow { fact: f, last_used_at }
        })
        .collect())
}

#[tauri::command]
pub fn update_memory_fact_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
    description: Option<String>,
    body: String,
) -> Cmd<()> {
    mem.update(&db, &name, description.as_deref(), &body)
        .map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("edited {name}"));
    Ok(())
}

/// What `/remember` saved, for the toast that offers to take it back.
#[derive(Debug, Serialize)]
pub struct RememberedFact {
    pub name: String,
    pub description: String,
    /// `new | merged | replaced | contested` (`BLF-2`): what the relation check
    /// did. `other` is the entry it touched, for the last three.
    pub outcome: String,
    pub other: Option<String>,
}

/// `UCM-5`: `/remember <fact>`. The user is the author, so this is not behind
/// the `facts` autonomy rung — that rung is about what *I* may write unasked.
/// It is the same `MemoryStore` write the `memory` tool makes, so the toast and
/// its undo fire exactly as they do for a fact I saved myself.
///
/// The text becomes the body whole; the name is its first few words, made
/// unique, and the description is its first line. Scope is global: someone who
/// types `/remember` means *always*, not *in this topic*.
#[tauri::command]
pub fn remember_fact_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    conversation_id: Option<String>,
    text: String,
) -> Cmd<RememberedFact> {
    let body = text.trim();
    if body.is_empty() {
        return Err(PoiesisError::Message("Tell me what to remember.".into()));
    }
    let stem: String = body.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
    let first_line = body.lines().next().unwrap_or(body);
    let description: String = first_line.chars().take(120).collect();
    let project = conversation_id
        .as_deref()
        .and_then(|id| db.conversation_project(id).ok().flatten())
        .map(|p| p.id);

    let mut attempt = 0;
    let saved = loop {
        let name = if attempt == 0 { stem.clone() } else { format!("{stem} {}", attempt + 1) };
        let fact = Fact {
            name,
            description: description.clone(),
            kind: "fact".into(),
            source_conversation: conversation_id.clone(),
            body: body.to_string(),
            scope: Some("global".into()),
            project: project.clone(),
            ..Default::default()
        };
        // `BLF-2`: the user is the newer source, so an answer that differs from
        // what I believed replaces it (and the old one stays as an earlier version).
        match mem.save_checked(&db, &fact, true) {
            Ok(saved) => break saved,
            // A name already taken is the one error worth retrying; the rest
            // (too long, refused as poisoned) are the user's to hear.
            Err(e) if e.contains("exists") && attempt < 20 => attempt += 1,
            Err(e) => return Err(PoiesisError::Message(e)),
        }
    };
    let (outcome, other) = match &saved.outcome {
        Outcome::New => ("new", None),
        Outcome::Merged(o) => ("merged", Some(o.clone())),
        Outcome::Replaced(o) => ("replaced", Some(o.clone())),
        Outcome::Contested(o) => ("contested", Some(o.clone())),
    };
    let logged = match (&saved.outcome, &other) {
        (Outcome::Merged(_), Some(old)) => format!("merge {old}"),
        (Outcome::Replaced(_), _) => format!("replace {}", saved.name),
        (Outcome::Contested(_), _) => format!("contest {}", saved.name),
        _ => format!("save {}", saved.name),
    };
    let _ = db.log_activity(conversation_id.as_deref(), "memory", &logged);
    Ok(RememberedFact { name: saved.name, description, outcome: outcome.into(), other })
}

/// The user overriding a fact's scope by hand (`SCP-UI-1`) — they are the
/// final authority on their own standing instructions, classifier or not.
#[tauri::command]
pub fn set_fact_scope_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
    scope: String,
) -> Cmd<()> {
    mem.set_fact_scope(&db, &name, &scope).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("set {name}'s scope to {scope}"));
    Ok(())
}

/// Move a fact to `.trash/`. Returns the trash filename, which the undo strip
/// hands back to `restore_memory_fact_cmd`.
#[tauri::command]
pub fn forget_memory_fact_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
) -> Cmd<String> {
    let file = mem.forget(&db, &name).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("forgot {name}"));
    Ok(file)
}

/// A button on a recalled entry, as a command trace in the chat's session log: the
/// same place `/forget` is written, so `user_signals` and credit read it without
/// a new channel (`CUE-UI-1`).
fn trace_press(db: &Db, conversation_id: Option<&str>, name: &str, entry: &str, note: &str) {
    let Some(conversation_id) = conversation_id else { return };
    crate::agent::log::record_command(
        db,
        conversation_id,
        None,
        &crate::agent::log::CommandTrace {
            name: name.to_string(),
            args: entry.to_string(),
            by: "user".to_string(),
            outcome: "done".to_string(),
            note: Some(note.to_string()),
            message_id: None,
        },
    );
}

/// `CUE-UI-1`, Outdated: the person said a recalled fact is out of date and what
/// is right now. The answer becomes the new fact, the old one stays as an earlier
/// version (`MemoryStore::supersede_fact`). Returns the new fact's name.
#[tauri::command]
pub fn outdate_memory_fact_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
    right_now: String,
    conversation_id: Option<String>,
) -> Cmd<String> {
    let saved = mem
        .supersede_fact(&db, &name, &right_now, conversation_id.as_deref())
        .map_err(PoiesisError::Message)?;
    trace_press(
        &db,
        conversation_id.as_deref(),
        crate::agent::credit::OUTDATED,
        &name,
        &format!("{name} is out of date; it is now {saved}"),
    );
    let _ = db.log_activity(conversation_id.as_deref(), "memory", &format!("replaced {name} with {saved}"));
    Ok(saved)
}

/// `CUE-UI-1`, Not relevant here: it counts against the entry for this answer
/// and changes nothing else.
#[tauri::command]
pub fn not_relevant_memory_cmd(db: State<'_, Db>, name: String, conversation_id: Option<String>) -> Cmd<()> {
    trace_press(
        &db,
        conversation_id.as_deref(),
        crate::agent::credit::NOT_RELEVANT,
        &name,
        &format!("{name} was not relevant here"),
    );
    Ok(())
}

/// Facts a newer one replaced, newest first: the Memory tab's "Earlier versions".
#[tauri::command]
pub fn list_earlier_versions_cmd(mem: State<'_, MemoryStore>) -> Vec<Fact> {
    mem.list_superseded(FACTS)
}

/// `BLF-UI-1`, Unsure: contested facts, grouped by who disputes whom. Each group is
/// two or more sides; a side whose opponent is gone stands alone.
#[tauri::command]
pub fn list_unsure_cmd(mem: State<'_, MemoryStore>) -> Vec<Vec<Fact>> {
    mem.unsure_groups()
}

/// The person settled a doubt: keep `keep` (and let the other sides become earlier
/// versions), or with `both` keep every side as true in some situations only.
#[tauri::command]
pub fn resolve_unsure_cmd(mem: State<'_, MemoryStore>, db: State<'_, Db>, keep: String, both: bool) -> Cmd<()> {
    mem.resolve_contest(&db, &keep, both).map_err(PoiesisError::Message)?;
    let what = if both { "kept both sides of a doubt" } else { "settled a doubt" };
    let _ = db.log_activity(None, "memory", &format!("{what} about {keep}"));
    Ok(())
}

/// Undo of a replace or a contest (the toast's Undo): the new fact goes to the
/// trash and the old one is believed again.
#[tauri::command]
pub fn undo_belief_change_cmd(mem: State<'_, MemoryStore>, db: State<'_, Db>, new: String, old: String) -> Cmd<()> {
    mem.undo_change(&db, &new, &old).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("took back {new}; {old} stands"));
    Ok(())
}

/// `BLF-UI-1`, Let go (G9): everything set aside in `.trash/`, newest first.
#[tauri::command]
pub fn list_let_go_cmd(mem: State<'_, MemoryStore>) -> Vec<crate::memory::TrashItem> {
    mem.list_trash()
}

/// One line of "Recent changes", in the first person.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RecentChange {
    pub at: i64,
    pub line: String,
    /// `facts | lessons`: which tab it belongs on.
    pub collection: String,
}

fn spoken(name: &str) -> String {
    name.replace('-', " ")
}

/// What the activity log says about memory, in my own words. Log lines that are
/// not a change to what I believe (a read, a scope tweak) are not listed.
pub(crate) fn recent_change(kind: &str, detail: &str) -> Option<String> {
    let detail = detail.trim();
    match kind {
        "memory" => {
            if let Some((old, new)) = detail.strip_prefix("replaced ").and_then(|r| r.split_once(" with ")) {
                return Some(format!("I replaced \"{}\" with \"{}\"; the old one is under Earlier versions.", spoken(old), spoken(new)));
            }
            if let Some(rest) = detail.strip_prefix("took back ") {
                let new = rest.split(';').next().unwrap_or(rest);
                return Some(format!("I took back \"{}\".", spoken(new)));
            }
            let (verb, name) = detail.split_once(' ')?;
            let name = spoken(name);
            match verb {
                "save" => Some(format!("I remembered \"{name}\".")),
                "update" | "edited" => Some(format!("I updated \"{name}\".")),
                "forget" | "forgot" => Some(format!("I forgot \"{name}\".")),
                "merge" => Some(format!("I already knew \"{name}\"; I noted this chat as another source.")),
                "replace" => Some(format!("I replaced an older note with \"{name}\"; the old one is under Earlier versions.")),
                "contest" => Some(format!("I am not sure about \"{name}\": it disagrees with an older note.")),
                "settled" | "kept" => Some(format!("I {detail}.")),
                _ => None,
            }
        }
        "reflect" => {
            if let Some(name) = detail.strip_prefix("learned ") {
                return Some(format!("I learned \"{}\".", spoken(name)));
            }
            if let Some(rest) = detail.strip_prefix("relearned ") {
                let name = rest.split(':').next().unwrap_or(rest);
                return Some(format!("I met \"{}\" again.", spoken(name)));
            }
            detail.strip_prefix("pruned an old lesson: ").map(|n| format!("I let go of the lesson \"{}\".", spoken(n)))
        }
        _ => None,
    }
}

/// `BLF-UI-1`, Recent changes: what I last did to what I know, newest first.
#[tauri::command]
pub fn list_recent_changes_cmd(db: State<'_, Db>, limit: Option<usize>) -> Cmd<Vec<RecentChange>> {
    let entries = db.list_activity(300).map_err(err)?;
    Ok(entries
        .iter()
        .filter_map(|a| {
            let collection = if a.kind == "reflect" { LESSONS } else { FACTS };
            recent_change(&a.kind, &a.detail).map(|line| RecentChange { at: a.created_at, line, collection: collection.into() })
        })
        .take(limit.unwrap_or(12))
        .collect())
}

#[tauri::command]
pub fn restore_memory_fact_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    file: String,
) -> Cmd<()> {
    mem.restore_trash(&db, &file).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", "restored a forgotten memory");
    Ok(())
}

#[tauri::command]
pub fn set_soul_cmd(mem: State<'_, MemoryStore>, db: State<'_, Db>, text: String) -> Cmd<()> {
    mem.set_soul(&text).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", "edited standing instructions");
    Ok(())
}

// ---- PRO: the synthesized profile (SMP-5's untitled "About you") ----

#[tauri::command]
pub fn get_profile_cmd(mem: State<'_, MemoryStore>) -> Cmd<Option<Profile>> {
    Ok(mem.profile())
}

/// `PRO-2`: one local call over every global-scoped fact, asked explicitly to
/// cover only tone/format/length/language/units and never who the user is.
/// `None` on any failure — no engine, no response — so this ambient rebuild
/// never reaches for the turn's cloud endpoint the way `classify_scope`
/// doesn't either; it simply tries again next trigger.
async fn synthesize_profile(mgr: &RuntimeManager, facts: &[Fact]) -> Option<String> {
    let Some((base_url, token)) = mgr.engine_endpoint().await else { return None };
    let endpoint = ChatEndpoint::OpenAi { base_url, api_key: Some(token), model: None };
    let listed: String = facts
        .iter()
        .map(|f| format!("- ({}) {}: {}", f.kind, f.description, f.body))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "This user's standing preferences and instructions, gathered over time:\n{listed}\n\n\
         Write 1 to 3 plain sentences, third person, present tense, describing ONLY how this user \
         likes answers delivered — tone, format, length, language, units. Do not include or infer \
         anything about who they are, what they do, or what they work on. Output only the sentences \
         and nothing else."
    );
    let msgs = vec![serde_json::json!({ "role": "user", "content": prompt })];
    let outcome = drive_turn(&mgr.client, &endpoint, &msgs, &[], 0.2, crate::cloud::Effort::Off, &CancelFlag::new(), |_| {})
        .await
        .ok()?;
    let TurnOutcome::Final { content, .. } = outcome else { return None };
    let text = content.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Rebuild the profile from every global-scoped fact (`PRO-2`), snapshotting
/// first so `PRO-UI-5`'s `Undo` has something to restore (`PRO-9`).
///
/// `force` is the difference between the two triggers `PRO-4`/`PRO-UI-2`
/// define: an automatic trigger (debounce or daily tick) respects the
/// `PRO-3` volume gate and the `profile` autonomy rung, and returns `Ok(None)`
/// — not an error — whenever it decides not to act. A user's own `Rewrite
/// this` click ignores both: they asked directly, which is its own
/// authorization.
#[tauri::command]
pub async fn rebuild_profile_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    force: bool,
) -> Cmd<Option<Profile>> {
    if !force && autonomy_gate(&db, "profile") == Rung::Off {
        return Ok(None);
    }
    let facts = mem.profile_sources();
    if !force && facts.len() < crate::memory::PROFILE_MIN_SOURCES {
        return Ok(None);
    }
    if facts.is_empty() {
        return Err(PoiesisError::Message(
            "I don't have any preferences saved yet to draw from — tell me how you like answers \
             written and I'll remember it."
                .into(),
        ));
    }
    let Some(body) = synthesize_profile(&mgr, &facts).await else {
        return Err(PoiesisError::Message(
            "couldn't reach the local model to do this — try again once a model is running.".into(),
        ));
    };
    let snapshot = mem.snapshot().map_err(PoiesisError::Message)?;
    let _ = db.set_setting(PROFILE_LAST_SNAPSHOT, &snapshot);
    mem.set_profile(&body, facts.len(), false).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", "updated how I picture you");
    Ok(mem.profile())
}

/// The user overwriting the synthesis with their own words (`PRO-UI-2`).
/// Always allowed, same as editing a fact or `SOUL.md` directly — this is the
/// user's own memory folder.
#[tauri::command]
pub fn edit_profile_cmd(mem: State<'_, MemoryStore>, db: State<'_, Db>, text: String) -> Cmd<Profile> {
    let source_count = mem.profile().map(|p| p.source_count).unwrap_or(0);
    mem.set_profile(&text, source_count, true).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", "edited how I picture you");
    mem.profile().ok_or_else(|| PoiesisError::Message("that didn't save".into()))
}

/// `PRO-9`: undo the most recent rebuild. `None` if there's nothing to undo
/// (already used, or nothing has rebuilt this run).
#[tauri::command]
pub fn undo_profile_rebuild_cmd(mem: State<'_, MemoryStore>, db: State<'_, Db>) -> Cmd<Option<Profile>> {
    let Some(snapshot) = db.get_setting(PROFILE_LAST_SNAPSHOT).map_err(err)?.filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    mem.restore_profile(&snapshot).map_err(PoiesisError::Message)?;
    // One-shot: a second Undo click with nothing new to revert should do
    // nothing, not silently repeat the same restore.
    let _ = db.set_setting(PROFILE_LAST_SNAPSHOT, "");
    Ok(mem.profile())
}

/// Reveal the memory folder in the OS file manager — the point of storing the
/// self as markdown is that the user can open it.
#[tauri::command]
pub fn open_memory_dir_cmd(app: tauri::AppHandle, mem: State<'_, MemoryStore>) -> Cmd<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(mem.dir().to_string_lossy().to_string(), None::<&str>)
        .map_err(err)
}

/// Zip the whole self into the data directory and reveal it (MEM-UI-1).
///
/// Deliberately not a save dialog: the export lands beside the memory folder
/// the user already knows, and the file manager opens on it. `.snapshots/` is
/// skipped — those are copies of what's already in the archive.
#[tauri::command]
pub fn export_memory_zip_cmd(app: tauri::AppHandle, mem: State<'_, MemoryStore>) -> Cmd<String> {
    use std::io::Write;
    use tauri_plugin_opener::OpenerExt;

    let dir = mem.dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = dir
        .parent()
        .unwrap_or(dir)
        .join(format!("poiesis-memory-{stamp}.zip"));

    let file = std::fs::File::create(&out).map_err(err)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();

    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((from, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&from) else { continue };
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".snapshots" {
                continue;
            }
            let inner = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if entry.path().is_dir() {
                stack.push((entry.path(), inner));
            } else if let Ok(bytes) = std::fs::read(entry.path()) {
                zip.start_file(&inner, opts).map_err(err)?;
                zip.write_all(&bytes).map_err(err)?;
            }
        }
    }
    zip.finish().map_err(err)?;

    let path = out.to_string_lossy().to_string();
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path)
}

// ---- proposals (SOUL-3) ----

#[tauri::command]
pub fn list_change_proposals_cmd(db: State<'_, Db>) -> Cmd<Vec<ChangeProposal>> {
    db.list_change_proposals().map_err(err)
}

/// `MAIL-UI-2`'s `Edit`: rewrite a pending proposal's text before accepting it.
#[tauri::command]
pub fn update_change_proposal_text_cmd(db: State<'_, Db>, id: String, proposed_text: String) -> Cmd<()> {
    db.update_change_proposal_text(&id, &proposed_text).map_err(err)
}

/// Answer a proposal. Accepting a `soul` proposal is the only path by which
/// standing instructions ever change on the agent's initiative — and it runs
/// here, on an explicit user action, never inside the agent loop.
///
/// `persona` proposals are deliberately unhandled: the table's `target` and
/// `persona_id` columns future-proof per-persona prompt edits, which are out of
/// scope for v1.
#[tauri::command]
pub async fn resolve_change_proposal_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    mgr: State<'_, RuntimeManager>,
    app: tauri::AppHandle,
    id: String,
    accept: bool,
    target: Option<ChatTarget>,
) -> Cmd<()> {
    let proposal = db
        .get_change_proposal(&id)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("That proposal no longer exists.".into()))?;

    // Already answered (double-click, stale card): don't apply a second time.
    if proposal.status != "pending" {
        return Err(PoiesisError::Message("That proposal was already answered.".into()));
    }

    if !accept {
        db.resolve_change_proposal(&id, "dismissed").map_err(err)?;
        let _ = db.log_activity(None, "memory", "dismissed a proposed change");
        return Ok(());
    }

    match proposal.target.as_str() {
        "soul" => {
            use tauri::Emitter;
            // `GLD-2`: a standing instruction changes every future prompt —
            // check it against the golden set right after, and put the prior
            // text back on a confirmed regression.
            let prior = mem.soul();
            let regressed = crate::agent::golden::guard_self_change(
                &mgr.client,
                &mgr,
                &db,
                &mem,
                target.as_ref(),
                || mem.set_soul(&proposal.proposed_text),
                || mem.set_soul(&prior),
            )
            .await
            .map_err(PoiesisError::Message)?;
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            match regressed {
                None => {
                    let _ = db.log_activity(None, "memory", "accepted a standing instruction");
                }
                Some(n) => {
                    let _ = db.log_activity(
                        None,
                        "memory",
                        &format!("that standing instruction made me worse at {n} thing(s) — I put it back"),
                    );
                    let _ = app.emit("poiesis-golden-reverted", serde_json::json!({ "count": n }));
                }
            }
            Ok(())
        }
        "recipe" => {
            // `SKL-5`: recipe proposals aren't written any more, but one may
            // have been sitting unanswered across the upgrade. Accepting it
            // still honours the user's yes — the text they reviewed becomes a
            // skill, converted by exactly the same code that migrated the
            // recipes already on disk.
            let slug = proposal.slug.as_deref().unwrap_or("recipe");
            let recipe = crate::memory::recipe_legacy::parse_recipe(slug, &proposal.proposed_text);
            let (skill_md, surface) = crate::memory::recipe_legacy::to_skill_md(&recipe);
            let name = crate::memory::slugify(&recipe.name).map_err(PoiesisError::Message)?;
            let dir = mgr.skills_dir().join(&name);
            std::fs::create_dir_all(&dir).map_err(err)?;
            std::fs::write(dir.join("SKILL.md"), &skill_md).map_err(err)?;
            if let Some(json) = surface {
                let assets = dir.join("assets");
                std::fs::create_dir_all(&assets).map_err(err)?;
                std::fs::write(assets.join("surface.json"), json).map_err(err)?;
            }
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "memory", &format!("kept the skill {name}"));
            Ok(())
        }
        "project_instructions" => {
            // `SKC-4`: the frontend applies these with `setProjectInstructions`
            // (the one place that also refreshes the project and its prompt)
            // before answering, so all that is left here is to close the row.
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "memory", "accepted instructions for a project");
            Ok(())
        }
        // `MCPS-2`: a memory an agent app suggested with its `remember` tool. It
        // lands exactly as a lesson does, after the user has read it and said yes.
        "lesson" | "lesson-critic" | "engine-memory" => {
            // Reflection at rung `ask`, or a draft the critic demoted
            // (`CRT-2`). The body is the lesson and `description` is its own
            // one-line summary — deliberately *not* the rationale, which for a
            // demoted draft is the critic's objection and would otherwise be
            // filed as the lesson's description. Rows written before schema v8
            // have no description, so they fall back to the old behaviour.
            let slug = proposal
                .slug
                .clone()
                .ok_or_else(|| PoiesisError::Message("That lesson has no name.".into()))?;
            mem.save_lesson(
                &db,
                &Fact {
                    name: slug.clone(),
                    description: proposal
                        .description
                        .clone()
                        .unwrap_or_else(|| proposal.rationale.clone()),
                    kind: "lesson".into(),
                    created: String::new(),
                    source_conversation: None,
                    body: proposal.proposed_text.clone(),
                    scope: None,
                    recurrence: None,
                    last_seen: None,
                    expires_at: None,
                    origin: None,
                    cue: None,
                    confidence: None,
                    status: None,
                    supersedes: None,
                    superseded_by: None,
                    evidence: Vec::new(),
                    valid_from: None,
                    // A lesson the user accepted from the review queue is not
                    // fenced to a project: they were shown it out of context
                    // and said yes to it as a standing lesson.
                    project: None,
                },
            )
            .map_err(PoiesisError::Message)?;
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "reflect", &format!("learned {slug}"));
            Ok(())
        }
        // `RTE-6`: a change to the rules for when to hand work to an agent app.
        "engine-rule" => {
            let what = crate::engines::learn::apply(&db, &proposal).map_err(PoiesisError::Message)?;
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "engines", &format!("accepted a suggestion: {what}"));
            Ok(())
        }
        "email" => {
            // `MAIL-3`: the proposal holds the complete rendered message
            // (account + headers + body, `render_email_proposal`) — accepting
            // parses that back and sends exactly what the user reviewed.
            let fields = crate::agent::mail::parse_email_proposal(&proposal.proposed_text)
                .ok_or_else(|| PoiesisError::Message("That proposal's message couldn't be read back.".into()))?;
            crate::agent::mail::send_now(
                &db,
                &fields.account_id,
                &fields.to,
                fields.cc.as_deref(),
                &fields.subject,
                &fields.body,
                fields.in_reply_to.as_deref(),
            )
            .await
            .map_err(PoiesisError::Message)?;
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "mail", &format!("sent mail to {}", fields.to));
            Ok(())
        }
        "app" => {
            // `MCG-3`: a connector call a headless run could not ask about. The
            // proposal holds the exact arguments the person reviewed, and
            // accepting runs those, nothing recomputed.
            let call = crate::agent::connector_gate::parse_app_proposal(&proposal.proposed_text)
                .ok_or_else(|| PoiesisError::Message("That request couldn't be read back.".into()))?;
            crate::agent::connector_gate::run_approved(&db, &mgr.client, &call)
                .await
                .map_err(PoiesisError::Message)?;
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            Ok(())
        }
        // `SKL-5` (install) and `OUT-2` (revision) land the same way: the
        // proposal holds the complete future `SKILL.md`, and accepting is a
        // write of the exact text the user reviewed. For a revision of a
        // Personal/Project skill this always creates an **App-source copy** —
        // it supersedes the original in discovery order (`skillpack::add_root`
        // keys on the frontmatter name, which the revision preserves) without
        // touching anything under the user's own `.poiesis/skills/`.
        "skill" | "skill-revision" => {
            let slug = proposal.slug.as_deref().ok_or_else(|| PoiesisError::Message("That skill has no name.".into()))?;
            // Slugified even though every writer already does: this is the
            // sink that turns a proposal into a path, and a skill's name can
            // come from third-party frontmatter. An absolute or `..`-bearing
            // name would otherwise escape the skills folder entirely — a
            // `join` on an absolute path discards the base outright.
            let slug = crate::memory::slugify(slug).map_err(PoiesisError::Message)?;
            let dir = mgr.skills_dir().join(&slug);
            std::fs::create_dir_all(&dir).map_err(err)?;
            std::fs::write(dir.join("SKILL.md"), &proposal.proposed_text).map_err(err)?;
            // Enable under the name discovery will actually key this pack by
            // (its frontmatter name), which is not necessarily the directory
            // slug — reading it back is the only way to know for sure.
            let enable_as = crate::agent::skillpack::parse_pack(&dir, crate::agent::skillpack::SkillSource::App)
                .map(|p| p.name)
                .unwrap_or_else(|| slug.clone());
            crate::agent::skillpack::set_enabled(&db, crate::agent::skillpack::SkillSource::App, &enable_as, true);
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "memory", &format!("kept the skill {enable_as}"));
            Ok(())
        }
        other => Err(PoiesisError::Message(format!(
            "Proposals for '{other}' can't be applied in this version."
        ))),
    }
}

// ---- consolidation (MEM-5) ----

/// A cleanup the model proposes over the whole fact set. Never applied without
/// the user pressing Apply.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Consolidation {
    #[serde(default)]
    pub deletes: Vec<String>,
    #[serde(default)]
    pub edits: Vec<ConsolidationEdit>,
    #[serde(default)]
    pub merges: Vec<ConsolidationMerge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationEdit {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationMerge {
    /// The entry that survives, rewritten with `text`.
    pub keep: String,
    /// Entries folded into `keep` and moved to trash.
    pub drop: Vec<String>,
    pub text: String,
}

/// Ask the model to propose a tidy-up of the memory set. Returns the proposal
/// and stores it as a pending setting; **nothing is changed on disk here.**
#[tauri::command]
pub async fn consolidate_memory_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    target: Option<ChatTarget>,
) -> Cmd<Consolidation> {
    // AUT-1: tidying up rewrites and drops entries wholesale, so `off` has to
    // stop it at the source — not merely hide the button.
    if crate::autonomy::autonomy_gate(&db, "consolidate") == crate::autonomy::Rung::Off {
        return Err(PoiesisError::Message(
            "Tidying up my memory is turned off in my Self panel.".into(),
        ));
    }
    let target = target.unwrap_or_default();
    let endpoint = match build_remote_endpoint(&db, &target).map_err(PoiesisError::Message)? {
        Some(ep) => ep,
        None => {
            let Some((base_url, token)) = mgr.engine_endpoint().await else {
                return Err(PoiesisError::Message(
                    "No model is loaded, so memory can't be tidied up right now.".into(),
                ));
            };
            ChatEndpoint::OpenAi {
                base_url,
                api_key: Some(token),
                model: None,
            }
        }
    };

    let mut entries = mem.list();
    entries.extend(mem.list_in(LESSONS));
    if entries.is_empty() {
        return Ok(Consolidation::default());
    }
    let listing = entries
        .iter()
        .map(|f| format!("- {} ({}): {}", f.name, f.kind, f.body))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You maintain a personal memory file set. Propose a cleanup as JSON only:\n\
         {{\"deletes\":[\"name\"],\"edits\":[{{\"name\":\"...\",\"text\":\"...\"}}],\
         \"merges\":[{{\"keep\":\"name\",\"drop\":[\"name\"],\"text\":\"merged body\"}}]}}\n\
         Merge duplicates, drop facts superseded by newer ones, tighten wording.\n\
         Propose nothing you are unsure about.\nFacts:\n{listing}"
    );
    let msgs = vec![
        serde_json::json!({
            "role": "system",
            "content": "You output only JSON. No preamble, no code fences.",
        }),
        serde_json::json!({ "role": "user", "content": prompt }),
    ];

    let outcome = drive_turn(&mgr.client, &endpoint, &msgs, &[], 0.2, crate::cloud::Effort::Off, &CancelFlag::new(), |_| {})
        .await
        .map_err(err)?;
    let raw = match outcome {
        TurnOutcome::Final { content, .. } => content,
        _ => return Ok(Consolidation::default()),
    };

    // Parse strictly. A model that returns something we can't read proposes
    // nothing — guessing at its intent is exactly the wrong move here.
    let proposal: Consolidation =
        serde_json::from_str(strip_fence(&raw)).unwrap_or_default();

    let json = serde_json::to_string(&proposal).map_err(err)?;
    db.set_setting(PENDING_CONSOLIDATION, &json).map_err(err)?;
    Ok(proposal)
}

/// Strip a ```json fence if the model wrapped its answer in one.
fn strip_fence(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else { return s };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.trim_start_matches('\n').trim_end_matches('`').trim()
}

#[tauri::command]
pub fn get_pending_consolidation_cmd(db: State<'_, Db>) -> Cmd<Option<Consolidation>> {
    let Some(raw) = db.get_setting(PENDING_CONSOLIDATION).map_err(err)? else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&raw).ok())
}

/// Apply or discard the pending consolidation. Applying snapshots the whole
/// memory folder first, so a bad tidy-up is always recoverable — and
/// (`GLD-2`) is checked against the golden set immediately after; a
/// confirmed regression reverts to that same snapshot automatically.
#[tauri::command]
pub async fn apply_consolidation_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    mgr: State<'_, RuntimeManager>,
    app: tauri::AppHandle,
    accept: bool,
    target: Option<ChatTarget>,
) -> Cmd<()> {
    use tauri::Emitter;

    let Some(raw) = db.get_setting(PENDING_CONSOLIDATION).map_err(err)? else {
        return Ok(());
    };
    if !accept {
        db.set_setting(PENDING_CONSOLIDATION, "").map_err(err)?;
        return Ok(());
    }

    // Turning consolidation off between proposing and applying withdraws
    // consent for the whole batch; the pending proposal is dropped with it.
    if crate::autonomy::autonomy_gate(&db, "consolidate") == crate::autonomy::Rung::Off {
        db.set_setting(PENDING_CONSOLIDATION, "").map_err(err)?;
        return Err(PoiesisError::Message(
            "Tidying up my memory is turned off in my Self panel.".into(),
        ));
    }

    let proposal: Consolidation = serde_json::from_str(&raw).map_err(err)?;
    let snapshot_name: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

    let regressed = crate::agent::golden::guard_self_change(
        &mgr.client,
        &mgr,
        &db,
        &mem,
        target.as_ref(),
        || {
            let name = mem.snapshot()?;
            *snapshot_name.lock().unwrap() = name;

            for edit in &proposal.edits {
                if mem.update(&db, &edit.name, None, &edit.text).is_ok() {
                    let _ = db.log_activity(None, "memory", &format!("tidied {}", edit.name));
                }
            }
            for merge in &proposal.merges {
                // Only drop the sources once the target has actually absorbed
                // them. A model that names a keep that doesn't exist must not
                // delete facts into a merge that never happened.
                if mem.update(&db, &merge.keep, None, &merge.text).is_ok() {
                    let _ = db.log_activity(None, "memory", &format!("merged into {}", merge.keep));
                    for name in &merge.drop {
                        if name == &merge.keep {
                            continue; // never trash the entry we just merged into
                        }
                        if mem.forget(&db, name).is_ok() {
                            let _ = db.log_activity(None, "memory", &format!("merged away {name}"));
                        }
                    }
                }
            }
            for name in &proposal.deletes {
                if mem.forget(&db, name).is_ok() {
                    let _ = db.log_activity(None, "memory", &format!("dropped {name}"));
                }
            }
            Ok(())
        },
        || {
            let name = snapshot_name.lock().unwrap().clone();
            mem.restore_snapshot(&db, &name)
        },
    )
    .await
    .map_err(PoiesisError::Message)?;

    if let Some(n) = regressed {
        let _ = db.log_activity(
            None,
            "memory",
            &format!("a tidy-up made me worse at {n} thing(s) — I put it back"),
        );
        let _ = app.emit("poiesis-golden-reverted", serde_json::json!({ "count": n }));
    }

    db.set_setting(PENDING_CONSOLIDATION, "").map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_activity_log_reads_as_what_i_did() {
        assert_eq!(recent_change("memory", "save prefers-tea").as_deref(), Some("I remembered \"prefers tea\"."));
        assert_eq!(
            recent_change("memory", "replaced prefers-tea with prefers-coffee").as_deref(),
            Some("I replaced \"prefers tea\" with \"prefers coffee\"; the old one is under Earlier versions.")
        );
        assert!(recent_change("memory", "replace prefers-coffee").unwrap().contains("older note"));
        assert!(recent_change("memory", "contest prefers-coffee").unwrap().contains("not sure"));
        assert!(recent_change("memory", "merge prefers-tea").unwrap().contains("another source"));
        assert_eq!(recent_change("memory", "took back x; y stands").as_deref(), Some("I took back \"x\"."));
        assert_eq!(recent_change("reflect", "learned check-header").as_deref(), Some("I learned \"check header\"."));
        assert_eq!(
            recent_change("reflect", "pruned an old lesson: old-one").as_deref(),
            Some("I let go of the lesson \"old one\".")
        );
        assert!(recent_change("reflect", "relearned a-b: now 3\u{d7}").unwrap().contains("again"));
        // Not a change to what I believe.
        assert_eq!(recent_change("memory", "read prefers-tea"), None);
        assert_eq!(recent_change("memory", "set x's scope to topical"), None);
        assert_eq!(recent_change("heal", "quarantined a"), None);
    }

    #[test]
    fn parses_a_fenced_proposal() {
        let raw = "```json\n{\"deletes\":[\"a\"],\"edits\":[{\"name\":\"b\",\"text\":\"t\"}]}\n```";
        let p: Consolidation = serde_json::from_str(strip_fence(raw)).unwrap();
        assert_eq!(p.deletes, vec!["a"]);
        assert_eq!(p.edits[0].name, "b");
        assert!(p.merges.is_empty(), "missing keys default to empty");
    }

    #[test]
    fn unparseable_output_proposes_nothing() {
        let p: Consolidation = serde_json::from_str(strip_fence("sure! here you go")).unwrap_or_default();
        assert!(p.deletes.is_empty() && p.edits.is_empty() && p.merges.is_empty());
    }

    fn store() -> (MemoryStore, Db, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        (store, Db::open_in_memory().unwrap(), tmp)
    }

    fn fact(name: &str, kind: &str) -> Fact {
        Fact {
            name: name.into(),
            description: "a description".into(),
            kind: kind.into(),
            created: "2026-07-31".into(),
            source_conversation: None,
            body: "body text".into(),
            scope: None,
            recurrence: None,
            last_seen: None,
            expires_at: None,
            origin: None,
            cue: None,
            confidence: None,
            status: None,
            supersedes: None,
            superseded_by: None,
            evidence: Vec::new(),
            valid_from: None,
            project: None,
        }
    }

    #[test]
    fn live_manifest_reflects_soul_facts_and_persona() {
        let (mem, db, _tmp) = store();
        mem.set_soul("Always answer in metric.").unwrap();
        mem.save(&db, &fact("likes-rust", "preference")).unwrap();
        let persona = db
            .create_persona(&crate::db::NewPersona {
                name: "The Editor".into(),
                system_prompt: "You are a careful editor.".into(),
                model_id: None,
                params_json: None,
                tools_json: None,
                skills_json: None,
                description: None,
                spawnable: false,
            })
            .unwrap();
        let conv = db.create_conversation("test", None, false).unwrap();
        db.set_conversation_persona(&conv.id, Some(&persona.id), None).unwrap();

        // `context_manifest_cmd` itself takes `State<'_, T>`, which can't be
        // built outside a running app — exercise the pure builder instead.
        let layers = live_manifest(&db, &mem, &conv.id);
        let soul = layers.iter().find(|l| l.label.starts_with("Soul")).unwrap();
        assert!(soul.text.contains("metric"));
        assert!(soul.always_on);
        let notes = layers.iter().find(|l| l.label.starts_with("Remembered")).unwrap();
        assert!(notes.text.contains("likes-rust"));
        assert_eq!(notes.sources, vec!["likes-rust".to_string()]);
        let p = layers.iter().find(|l| l.label.starts_with("Persona")).unwrap();
        assert!(p.text.contains("careful editor"));
        assert_eq!(p.sources, vec!["The Editor".to_string()]);
    }

    #[test]
    fn a_topical_fact_is_not_listed_as_riding_every_answer() {
        let (mem, db, _tmp) = store();
        mem.save(&db, &fact("be-concise", "preference")).unwrap();
        mem.save(&db, &fact("pricing-currency", "preference")).unwrap();
        mem.set_fact_scope(&db, "pricing-currency", "topical").unwrap();
        let conv = db.create_conversation("test", None, false).unwrap();

        let layers = live_manifest(&db, &mem, &conv.id);
        let notes = layers.iter().find(|l| l.label == "Remembered (notes)").unwrap();
        assert!(notes.always_on);
        assert_eq!(notes.sources, vec!["be-concise".to_string()]);
        assert!(!notes.text.contains("pricing-currency"));

        let gated = layers.iter().find(|l| l.label == "Remembered (when relevant)").unwrap();
        assert!(!gated.always_on, "a topical fact must not claim it rides every answer");
        assert_eq!(gated.sources, vec!["pricing-currency".to_string()]);
    }

    #[test]
    fn the_historical_manifest_carries_the_reason_recall_gave() {
        let (mem, db, _tmp) = store();
        mem.save(&db, &fact("anna-contact", "preference")).unwrap();
        let refs = ContextRefs {
            facts: vec!["anna-contact".into()],
            why: [("anna-contact".to_string(), "you named Anna".to_string())].into(),
            ..Default::default()
        };
        let conv = db.create_conversation("test", None, false).unwrap();
        let layers = historical_manifest(&db, &mem, &conv.id, &refs);
        let all: String = layers.iter().map(|l| l.text.clone()).collect();
        assert!(all.contains("anna-contact") && all.contains("you named Anna"), "{all}");
        // An answer recorded before recall could say why reads as before.
        let old = historical_manifest(&db, &mem, &conv.id, &ContextRefs { facts: vec!["anna-contact".into()], ..Default::default() });
        assert!(!old.iter().any(|l| l.text.contains("you named")));
    }

    /// `CUE-UI-1`: Outdated replaces the belief and is counted against it; the
    /// next recall has only the new answer.
    #[test]
    fn pressing_outdated_replaces_the_fact_and_counts_against_it() {
        let (mem, db, _tmp) = store();
        mem.save(&db, &fact("lives-in-hamburg", "preference")).unwrap();
        let conv = db.create_conversation("test", None, false).unwrap().id;
        let answer = db
            .append_message(
                &conv,
                &crate::db::NewMessage {
                    role: "assistant".into(),
                    content: "Hello from Hamburg".into(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        let refs = ContextRefs { facts: vec!["lives-in-hamburg".into()], ..Default::default() };
        db.finalize_message(&answer.id, "Hello from Hamburg", None, Some(&serde_json::to_string(&refs).unwrap()), None, None)
            .unwrap();

        let new = mem.supersede_fact(&db, "lives-in-hamburg", "Lives in Berlin", Some(&conv)).unwrap();
        trace_press(&db, Some(&conv), crate::agent::credit::OUTDATED, "lives-in-hamburg", "out of date");

        let index = mem.recall_for(&db, None).index;
        assert!(index.contains(&new) && !index.contains("lives-in-hamburg"), "{index}");
        assert!(crate::agent::log::user_signals(&db, &conv)[0].contains("out of date"));
        // The old entry is gone from the prompt but still on disk, so credit has nothing to read:
        // it is the answer's record that is credited, and the name is no longer a live entry.
        crate::agent::credit::credit_conversation(&db, &mem, &conv);
        assert!(db.memory_credit_map("facts").unwrap().get("lives-in-hamburg").is_some_and(|c| c.hurt == 1));
    }

    #[test]
    fn historical_manifest_rehydrates_from_stored_refs_and_skips_forgotten_names() {
        let (mem, db, _tmp) = store();
        mem.save(&db, &fact("kept-fact", "preference")).unwrap();
        let refs = ContextRefs {
            persona_id: None,
            soul_present: false,
            about_you_present: false,
            facts: vec!["kept-fact".into(), "forgotten-fact".into()],
            lessons: Vec::new(),
            files: Vec::new(),
            ..Default::default()
        };
        let conv = db.create_conversation("test", None, false).unwrap();
        let layers = historical_manifest(&db, &mem, &conv.id, &refs);
        let notes = layers.iter().find(|l| l.label.starts_with("Remembered")).unwrap();
        assert!(notes.text.contains("kept-fact"));
        assert!(!notes.text.contains("forgotten-fact"), "a name with no file must be dropped, not guessed at");
        let soul = layers.iter().find(|l| l.label.starts_with("Soul")).unwrap();
        assert!(soul.text.is_empty(), "soul_present: false must not leak the current soul text");
    }
}
