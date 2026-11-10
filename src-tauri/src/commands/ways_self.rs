//! `WRK-16`/`WRK-17`/`WRK-18`: what the Self view's "How I work" tab reads and
//! changes: every way of working, the presets, where each place starts, and the
//! presets set aside.
//!
//! The lists a person picks from (triggers, moves, scopes) come from here, so the
//! window never offers a sentence the loop cannot carry out.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::ways::PresetView;
use crate::db::Db;
use crate::agent::evolve;
use crate::autonomy::{autonomy_gate, Rung};
use crate::memory::ways::{
    sentence, Back, Move, PresetEdit, PresetRule, Scope, ScopeCtx, Status, Trigger, WayEdit, AD_HOC,
};
use crate::memory::MemoryStore;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

/// The kinds of work a way can be narrowed to, as the self model names them.
const KINDS: &[(&str, &str)] = &[("mail", "Mail"), ("code", "Code"), ("report", "Reports")];

fn err(e: String) -> PoiesisError {
    PoiesisError::Message(e)
}

#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

/// A conversation a way came from, so it can be opened.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceView {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WayView {
    pub slug: String,
    /// "When a check fails, I fix that before anything else."
    pub text: String,
    pub when: String,
    #[serde(rename = "move")]
    pub mv: String,
    pub scope: String,
    pub scope_label: String,
    /// `always` | `learned` | `trying` | `retired`.
    pub status: String,
    /// `shipped`, `you`, or what proposed it.
    pub origin: String,
    pub since: String,
    pub evidence: Vec<EvidenceView>,
    /// While it is being tried out.
    pub trial: Option<TrialView>,
}

/// How a trial stands, in what the tab says: tasks of its kind so far, and days left.
#[derive(Debug, Clone, Serialize)]
pub struct TrialView {
    pub done: usize,
    pub tasks: u32,
    pub days_left: i64,
}

/// A change evolution would like to make, waiting for an answer.
#[derive(Debug, Clone, Serialize)]
pub struct WayProposalView {
    pub id: String,
    /// The kind of change, so the card knows which answers to offer.
    pub action: String,
    pub lead: String,
    pub quote: String,
    pub why: String,
    /// What can be answered, in order, `no` last: `[answer, label]`.
    pub answers: Vec<[String; 2]>,
}

/// One day to go back to, and what happened then.
#[derive(Debug, Clone, Serialize)]
pub struct MomentView {
    pub id: String,
    pub at: i64,
    /// `YYYY-MM-DD`, for the growth rings.
    pub date: String,
    pub label: String,
}

/// Something evolution did lately, with Undo when it did it on its own.
#[derive(Debug, Clone, Serialize)]
pub struct RecentView {
    pub id: String,
    pub said: String,
    pub undoable: bool,
}

/// One rule of the preset being shown. A built-in's rule that was switched off
/// is still listed, `on: false`, so it can be switched on again.
#[derive(Debug, Clone, Serialize)]
pub struct RuleView {
    pub when: String,
    #[serde(rename = "move")]
    pub mv: String,
    pub text: String,
    pub on: bool,
    /// The preset shipped with it. What is on and not original was added.
    pub original: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DefaultView {
    pub scope: String,
    pub label: String,
    pub preset: String,
    pub preset_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletedView {
    pub file: String,
    pub name: String,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WaysView {
    pub narrative: String,
    pub ways: Vec<WayView>,
    pub presets: Vec<PresetView>,
    /// The preset this view shows.
    pub preset: String,
    pub rules: Vec<RuleView>,
    /// The preset new chats start with everywhere, unless a place says otherwise.
    pub global_default: String,
    pub defaults: Vec<DefaultView>,
    pub deleted: Vec<DeletedView>,
    pub proposals: Vec<WayProposalView>,
    /// Every point there is a copy of how I worked to go back to, newest first.
    pub moments: Vec<MomentView>,
    pub recent: Vec<RecentView>,
    pub triggers: Vec<Choice>,
    pub moves: Vec<Choice>,
    pub scopes: Vec<Choice>,
    pub kinds: Vec<Choice>,
}

/// A way's reach, in words.
fn scope_label(scope: &str, names: &[Choice]) -> String {
    names.iter().find(|c| c.value == scope).map(|c| c.label.clone()).unwrap_or_else(|| "Everywhere".into())
}

/// Every place a rule or a default can be narrowed to, as a pick list.
fn scope_choices(db: &Db) -> Vec<Choice> {
    let mut out = vec![Choice { value: "everywhere".into(), label: "Everywhere".into() }];
    for p in db.list_projects(false).unwrap_or_default() {
        out.push(Choice { value: format!("project:{}", p.id), label: format!("Project: {}", p.name) });
    }
    for p in db.list_personas().unwrap_or_default() {
        out.push(Choice { value: format!("persona:{}", p.id), label: format!("Persona: {}", p.name) });
    }
    for (k, label) in KINDS {
        out.push(Choice { value: format!("kind:{k}"), label: format!("Kind of work: {label}") });
    }
    out
}

fn way_view(w: crate::memory::ways::Way, db: &Db, scopes: &[Choice], facts: &evolve::Facts, now: i64) -> WayView {
    let text = w.sentence();
    let trial = w.trial.filter(|_| w.status == Status::Trying).map(|t| TrialView {
        done: evolve::standing(facts, &w.scope, t.from).tasks,
        tasks: t.tasks,
        days_left: (((t.from + t.days as i64 * 86_400_000) - now) / 86_400_000).max(0),
    });
    let scope = w.scope.as_text();
    WayView {
        slug: w.slug,
        text,
        when: w.when.slug().into(),
        mv: w.mv.slug().into(),
        scope_label: scope_label(&scope, scopes),
        scope,
        status: w.status.slug().into(),
        origin: w.origin,
        trial,
        since: w.since,
        evidence: w
            .evidence
            .into_iter()
            .filter_map(|id| {
                let c = db.get_conversation(&id).ok().flatten()?;
                Some(EvidenceView { id, title: c.title })
            })
            .collect(),
    }
}

/// What the tab shows. `preset` is the one it is showing; absent, the one new
/// chats start with, or Lean.
#[tauri::command]
pub fn ways_view_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, preset: Option<String>) -> WaysView {
    mem.ensure_ways();
    let now = crate::db::now_ms();
    let facts = evolve::Facts::gather(&db, now);
    let scopes = scope_choices(&db);
    let presets = mem.presets();
    let global = mem.default_for(&ScopeCtx::default()).map(|p| p.slug).unwrap_or_else(|| AD_HOC.to_string());
    let shown = preset
        .as_deref()
        .and_then(|p| presets.iter().find(|x| x.slug == p))
        .or_else(|| presets.iter().find(|x| x.slug == global && x.slug != AD_HOC))
        .or_else(|| presets.iter().find(|x| x.slug == "lean"))
        .or_else(|| presets.first())
        .cloned();
    let (shown_slug, rules) = match &shown {
        Some(p) => {
            let original = mem.shipped_rules(&p.slug);
            let mut rules: Vec<RuleView> = p
                .rules
                .iter()
                .map(|r| RuleView {
                    when: r.when.slug().into(),
                    mv: r.mv.slug().into(),
                    text: sentence(r.when, r.mv),
                    on: true,
                    original: original.contains(r),
                })
                .collect();
            rules.extend(original.iter().filter(|r| !p.rules.contains(r)).map(|r| RuleView {
                when: r.when.slug().into(),
                mv: r.mv.slug().into(),
                text: sentence(r.when, r.mv),
                on: false,
                original: true,
            }));
            (p.slug.clone(), rules)
        }
        None => (AD_HOC.to_string(), Vec::new()),
    };
    let defaults = mem
        .defaults()
        .into_iter()
        .filter_map(|(scope, slug)| {
            let preset_name = presets.iter().find(|p| p.slug == slug)?.name.clone();
            let label = if scope == "global" { "New chats everywhere".to_string() } else { scope_label(&scope, &scopes) };
            Some(DefaultView { scope, label, preset: slug, preset_name })
        })
        .collect();
    WaysView {
        narrative: narrative(&mem, &db, now),
        ways: mem.ways().into_iter().map(|w| way_view(w, &db, &scopes, &facts, now)).collect(),
        presets: presets.iter().cloned().map(|p| PresetView::of(p, &presets)).collect(),
        preset: shown_slug,
        rules,
        global_default: global,
        defaults,
        proposals: way_proposals(&db),
        moments: mem.moments().into_iter().map(moment_view).collect(),
        recent: evolve::recent(&db, now)
            .into_iter()
            .map(|r| RecentView { id: r.id, said: r.said, undoable: r.auto && r.undo != evolve::Undo::Nothing })
            .collect(),
        deleted: mem.deleted_presets().into_iter().map(|d| DeletedView { file: d.file, name: d.name, at: d.at }).collect(),
        triggers: Trigger::ALL.iter().map(|t| Choice { value: t.slug().into(), label: t.words().into() }).collect(),
        moves: Move::ALL.iter().map(|m| Choice { value: m.slug().into(), label: m.words().into() }).collect(),
        scopes,
        kinds: KINDS.iter().map(|(k, l)| Choice { value: (*k).into(), label: (*l).into() }).collect(),
    }
}

/// The sentence at the top of the tab: how often I changed how I work, and what I did lately.
fn narrative(mem: &MemoryStore, db: &Db, now: i64) -> String {
    let base = mem.ways_narrative();
    match evolve::recent(db, now).first() {
        Some(r) => format!("{base} Lately I {}.", r.said),
        None => base,
    }
}

/// What a card offers. A new way can be tried first; a trial that ended is kept or let go;
/// the rest are a yes or a no.
fn answers_for(change: &evolve::Change) -> Vec<[String; 2]> {
    let pair = |a: &str, l: &str| [a.to_string(), l.to_string()];
    match change {
        evolve::Change::AddWay { .. } => vec![pair("try", "Try it"), pair("always", "Always"), pair("no", "No")],
        evolve::Change::TrialEnd { early: true, .. } => {
            vec![pair("keep-trying", "Keep trying"), pair("keep", "Keep it"), pair("retire", "Retire it"), pair("no", "Not now")]
        }
        evolve::Change::TrialEnd { .. } => vec![pair("keep", "Keep it"), pair("retire", "Retire it"), pair("no", "Not now")],
        _ => vec![pair("yes", "Yes"), pair("no", "No")],
    }
}

fn way_proposals(db: &Db) -> Vec<WayProposalView> {
    db.list_change_proposals()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.target == evolve::TARGET)
        .filter_map(|p| {
            let stored = evolve::read(&p.proposed_text)?;
            let action = serde_json::to_value(&stored.change).ok()?["action"].as_str()?.to_string();
            let answers = answers_for(&stored.change);
            Some(WayProposalView { id: p.id, action, lead: stored.lead, quote: stored.quote, why: p.rationale, answers })
        })
        .collect()
}

fn trigger_of(s: &str) -> Cmd<Trigger> {
    Trigger::parse(s).ok_or_else(|| err("I don't know that trigger.".into()))
}

fn move_of(s: &str) -> Cmd<Move> {
    Move::parse(s).ok_or_else(|| err("I don't know that move.".into()))
}

fn scope_of(s: &str) -> Scope {
    if s.trim() == "everywhere" {
        Scope::Everywhere
    } else {
        Scope::parse(s)
    }
}

/// `+ Add a way of working`.
#[tauri::command]
pub fn way_add_cmd(mem: State<'_, MemoryStore>, when: String, r#move: String, scope: String) -> Cmd<String> {
    mem.add_way(trigger_of(&when)?, move_of(&r#move)?, scope_of(&scope)).map(|w| w.slug).map_err(err)
}

/// Change one way: its words, its reach, or whether it is on. Anything left
/// out stays.
#[tauri::command]
pub fn way_update_cmd(
    mem: State<'_, MemoryStore>,
    slug: String,
    when: Option<String>,
    r#move: Option<String>,
    scope: Option<String>,
    status: Option<String>,
) -> Cmd<()> {
    let edit = WayEdit {
        when: when.as_deref().map(trigger_of).transpose()?,
        mv: r#move.as_deref().map(move_of).transpose()?,
        scope: scope.as_deref().map(scope_of),
        status: match status.as_deref() {
            None => None,
            Some(s) => Some(Status::parse(s).ok_or_else(|| err("I don't know that state.".into()))?),
        },
    };
    mem.update_way(&slug, edit).map(|_| ()).map_err(err)
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleArg {
    pub when: String,
    #[serde(rename = "move")]
    pub mv: String,
}

impl RuleArg {
    fn rule(&self) -> Cmd<PresetRule> {
        Ok(PresetRule { when: trigger_of(&self.when)?, mv: move_of(&self.mv)? })
    }
}

/// Switch one rule of a preset on or off, or add it with `on`.
#[tauri::command]
pub fn preset_rule_cmd(mem: State<'_, MemoryStore>, slug: String, rule: RuleArg, on: bool) -> Cmd<()> {
    mem.set_preset_rule(&slug, rule.rule()?, on).map(|_| ()).map_err(err)
}

/// A preset of your own. From a preset (`from`), from nothing, or from a run
/// (`from` is the preset it worked under and `add` the rules that fired on top).
/// Returns its slug.
#[tauri::command]
pub fn preset_create_cmd(
    mem: State<'_, MemoryStore>,
    name: String,
    line: String,
    from: Option<String>,
    add: Option<Vec<RuleArg>>,
    locked: Option<bool>,
) -> Cmd<String> {
    let add_given = add.is_some();
    let add: Vec<PresetRule> = add.unwrap_or_default().iter().map(RuleArg::rule).collect::<Cmd<_>>()?;
    // What a run kept (`add` given, even empty) came from a chat; otherwise from a preset or nothing.
    let how = if add_given { "chat" } else if from.as_deref().is_some_and(|f| !f.trim().is_empty()) { "preset" } else { "" };
    mem.create_preset_how(&name, &line, from.as_deref(), &add, locked.unwrap_or(true), how).map(|p| p.slug).map_err(err)
}

#[tauri::command]
pub fn preset_update_cmd(
    mem: State<'_, MemoryStore>,
    slug: String,
    name: Option<String>,
    line: Option<String>,
    locked: Option<bool>,
) -> Cmd<()> {
    mem.edit_preset(&slug, PresetEdit { name, line, locked }).map(|_| ()).map_err(err)
}

/// What deleting a preset would touch, or (confirmed) the deletion.
#[derive(Debug, Clone, Serialize)]
pub struct DeleteView {
    pub deleted: bool,
    /// Where it is the default, and the tasks that run under it, in words.
    pub used_by: Vec<String>,
}

/// Where a preset is in use that a person chose on purpose: the defaults that name
/// it and the scheduled tasks that run under it.
fn used_by(mem: &MemoryStore, db: &Db, slug: &str) -> Vec<String> {
    let scopes = scope_choices(db);
    let mut out: Vec<String> = mem
        .defaults_using(slug)
        .into_iter()
        .map(|scope| {
            if scope == "global" {
                "new chats".to_string()
            } else {
                scope_label(&scope, &scopes).replacen(": ", " ", 1).to_lowercase()
            }
        })
        .collect();
    for job in crate::commands::scheduler::jobs_using_style(db, slug) {
        out.push(format!("the task \u{201c}{job}\u{201d}"));
    }
    out
}

/// Delete a custom preset. Without `confirmed`, only says where it is in use; a preset
/// in use is deleted once the person has seen where, and then those places go back to
/// Ad hoc. The file is set aside, not destroyed.
#[tauri::command]
pub fn preset_delete_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, slug: String, confirmed: bool) -> Cmd<DeleteView> {
    let used = used_by(&mem, &db, &slug);
    if !used.is_empty() && !confirmed {
        return Ok(DeleteView { deleted: false, used_by: used });
    }
    mem.delete_preset(&slug).map_err(err)?;
    let _ = mem.drop_defaults_for(&slug);
    crate::commands::scheduler::clear_job_style(&db, &slug);
    Ok(DeleteView { deleted: true, used_by: used })
}

/// Bring a deleted preset back. Returns its slug.
#[tauri::command]
pub fn preset_restore_cmd(mem: State<'_, MemoryStore>, file: String) -> Cmd<String> {
    mem.restore_preset(&file).map(|p| p.slug).map_err(err)
}

/// Say which preset a place starts with (`global`, `project:<id>`, `persona:<id>`,
/// `kind:<kind>`), or take its default away.
#[tauri::command]
pub fn style_default_set_cmd(mem: State<'_, MemoryStore>, scope: String, preset: Option<String>) -> Cmd<()> {
    mem.set_default(&scope, preset.as_deref()).map_err(err)
}

/// The preset a chat starts with when nobody chose one for it: what the next run in
/// it will hold. `None` is Ad hoc with nothing set.
#[tauri::command]
pub fn conversation_default_style_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, id: String) -> Option<String> {
    let conv = db.get_conversation(&id).ok().flatten()?;
    if conv.style.is_some() {
        return None;
    }
    let project = db.conversation_project(&id).ok().flatten().map(|p| p.id);
    let kind = crate::agent::selfmodel::kind_now(&db, &id);
    let ctx = ScopeCtx { project: project.as_deref(), persona: conv.persona_id.as_deref(), kind };
    mem.default_for(&ctx).map(|p| p.slug).filter(|s| s != AD_HOC)
}

// ---- WRK-20: versions ----

/// One line of a preset's history, as the dialog draws it.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryView {
    pub seq: u32,
    pub at: i64,
    /// `you`, `evolution` or `original`.
    pub by: String,
    /// `original`, `version`, `back`, `locked`, `unlocked`.
    pub what: String,
    pub why: String,
    pub version: u32,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// The rules it had, so "Compare with now" has something to compare.
    pub rules: Vec<String>,
    /// The version the preset has now.
    pub current: bool,
}

#[tauri::command]
pub fn preset_history_cmd(mem: State<'_, MemoryStore>, slug: String) -> Vec<HistoryView> {
    let list = mem.preset_history(&slug);
    let has_rules = |what: &str| matches!(what, "original" | "version" | "back");
    let current = list.iter().find(|e| has_rules(&e.what)).map(|e| e.seq);
    let words = |rules: &[PresetRule]| rules.iter().map(|r| sentence(r.when, r.mv)).collect::<Vec<_>>();
    list.into_iter()
        .map(|e| HistoryView {
            current: Some(e.seq) == current,
            added: words(&e.added),
            removed: words(&e.removed),
            rules: words(&e.rules),
            seq: e.seq,
            at: e.at,
            by: e.by,
            what: e.what,
            why: e.why,
            version: e.version,
        })
        .collect()
}

/// Go back to the original (`to` absent) or to the version at that place in the history.
#[tauri::command]
pub fn preset_back_cmd(mem: State<'_, MemoryStore>, slug: String, to: Option<u32>) -> Cmd<()> {
    let back = to.map_or(Back::Original, Back::Entry);
    mem.go_back(&slug, back).map(|_| ()).map_err(err)
}

/// The organization's growth, for the rings in Self and the layers on the mark.
#[derive(Debug, Clone, Serialize)]
pub struct GrowthView {
    /// Every day there is a copy of how I worked to go back to.
    pub moments: Vec<MomentView>,
    /// How many times I changed how I work on my own.
    pub layers: usize,
}

#[tauri::command]
pub fn ways_growth_cmd(mem: State<'_, MemoryStore>) -> GrowthView {
    GrowthView { moments: mem.moments().into_iter().map(moment_view).collect(), layers: mem.evolved_versions() }
}

fn moment_view(m: crate::memory::ways::Moment) -> MomentView {
    MomentView {
        date: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(m.at).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default(),
        id: m.id,
        at: m.at,
        label: m.label,
    }
}

/// "Go back to how I worked on ...": the copy taken at that moment comes back.
#[tauri::command]
pub fn ways_go_back_cmd(mem: State<'_, MemoryStore>, id: String) -> Cmd<()> {
    let at: i64 = id.parse().map_err(|_| err("I can't find that day.".into()))?;
    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(at)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .and_then(|d| crate::memory::ways::short_date(&d))
        .unwrap_or_else(|| "then".into());
    mem.go_back_to(&id, &date).map_err(err)
}

/// "Bring back into Lean". Returns the parent's slug.
#[tauri::command]
pub fn preset_merge_cmd(mem: State<'_, MemoryStore>, slug: String) -> Cmd<String> {
    mem.merge_offspring(&slug).map(|p| p.slug).map_err(err)
}

// ---- WRK-19: evolution ----

/// Make a change under the golden-set guard, like every other self-change: a change
/// that breaks the fixed behaviour checks is put back.
async fn apply_guarded(
    app: &tauri::AppHandle,
    change: &evolve::Change,
    answer: &str,
    why: &str,
) -> Result<evolve::Applied, String> {
    use tauri::{Emitter, Manager};
    let db = app.state::<Db>();
    let mem = app.state::<MemoryStore>();
    let mgr = app.state::<crate::runtime::RuntimeManager>();
    let slot: std::sync::Mutex<Option<evolve::Applied>> = std::sync::Mutex::new(None);
    let regressed = crate::agent::golden::guard_self_change(
        &mgr.client,
        &mgr,
        &db,
        &mem,
        None,
        || {
            *slot.lock().unwrap() = Some(evolve::apply(&mem, change, answer, why)?);
            Ok(())
        },
        || match slot.lock().unwrap().take() {
            Some(done) => evolve::undo(&mem, &done.undo),
            None => Ok(()),
        },
    )
    .await?;
    if let Some(n) = regressed {
        let _ = db.log_activity(None, "memory", &format!("a change to how I work made me worse at {n} thing(s), so I put it back"));
        let _ = app.emit("poiesis-golden-reverted", serde_json::json!({ "count": n }));
        return Err("That change made me worse at something I check myself on, so I put it back.".into());
    }
    let done = slot.lock().unwrap().take();
    done.ok_or_else(|| "Nothing was changed.".to_string())
}

/// Answer one of the proposals in the tab. `answer` is `no`, or what the card offered:
/// `try`, `always`, `yes`, `keep`, `retire`, `keep-trying`.
#[tauri::command]
pub async fn way_proposal_answer_cmd(app: tauri::AppHandle, id: String, answer: String) -> Cmd<()> {
    use tauri::Manager;
    let db = app.state::<Db>();
    let proposal = db
        .get_change_proposal(&id)
        .map_err(|e| err(e.to_string()))?
        .filter(|p| p.target == evolve::TARGET)
        .ok_or_else(|| err("That proposal no longer exists.".into()))?;
    if proposal.status != "pending" {
        return Err(err("That proposal was already answered.".into()));
    }
    if answer == "no" {
        db.resolve_change_proposal(&id, "dismissed").map_err(|e| err(e.to_string()))?;
        return Ok(());
    }
    let stored = evolve::read(&proposal.proposed_text).ok_or_else(|| err("I can't read that proposal any more.".into()))?;
    let applied = apply_guarded(&app, &stored.change, &answer, &proposal.rationale).await.map_err(err)?;
    db.resolve_change_proposal(&id, "applied").map_err(|e| err(e.to_string()))?;
    evolve::note(&db, &applied, false, crate::db::now_ms());
    let _ = db.log_activity(None, "memory", &format!("I {}", applied.said));
    Ok(())
}

/// Take back something evolution did on its own.
#[tauri::command]
pub fn ways_undo_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, id: String) -> Cmd<()> {
    let found = evolve::take(&db, &id, crate::db::now_ms()).ok_or_else(|| err("That is too long ago to take back here.".into()))?;
    evolve::undo(&mem, &found.undo).map_err(err)
}

/// The daily pass: look at what happened, and raise what is due. Under Ask it waits
/// for a yes in the tab; under Auto it is done and told, with Undo; under Off nothing
/// is learned.
pub async fn evolve_daily(app: &tauri::AppHandle) {
    use tauri::Manager;
    let db = app.state::<Db>();
    let mem = app.state::<MemoryStore>();
    let rung = autonomy_gate(&db, evolve::CLASS);
    if rung == Rung::Off {
        return;
    }
    let now = crate::db::now_ms();
    for c in evolve::due(&db, &mem, now) {
        let Ok(id) = evolve::raise(&db, &c) else { continue };
        if rung != Rung::Auto {
            continue;
        }
        let answer = match &c.change {
            evolve::Change::TrialEnd { verdict, .. } if verdict == "worse" => "retire",
            evolve::Change::TrialEnd { .. } => "keep",
            evolve::Change::AddWay { .. } => "try",
            _ => "yes",
        };
        match apply_guarded(app, &c.change, answer, &c.why).await {
            Ok(done) => {
                let _ = db.resolve_change_proposal(&id, "applied");
                evolve::note(&db, &done, true, now);
                let _ = db.log_activity(None, "memory", &format!("I {}", done.said));
            }
            Err(_) => {
                let _ = db.resolve_change_proposal(&id, "dropped");
            }
        }
    }
}
