//! The tidy-up (`SLP-1`, `SLP-2`, COGNITION_PLAN III.6): one pass that the daily job
//! runs over the new conversations and that Tidy up / `/tidy` run over everything.
//!
//! The pass needs no model to find what it finds:
//! - **promotes** a candidate lesson that waited on its episode (`held`) once the
//!   same thing was seen in 2 or more conversations;
//! - **merges** notes that say the same, joining their evidence;
//! - **lets go** of what tested badly (5 or more chances, usefulness under 0.3) and,
//!   for lessons, of what has had no chance to help for 120 days.
//!
//! Where the change lands is the `consolidate` rung: `ask` leaves one pending proposal
//! on the Memory tab, `auto` applies it with a snapshot and the golden check, `off`
//! does nothing. Skills from success (2 or more finished conversations of one kind
//! with similar "what worked") are drafted by a model and always land as a proposal.

use std::collections::HashSet;

use tauri::{AppHandle, Emitter, Manager};

use crate::autonomy::{autonomy_gate, Rung};
use crate::commands::memory::{
    apply_guarded, db_pending, Applied, Consolidation, ConsolidationMerge, ConsolidationPromote, PENDING_CONSOLIDATION,
};
use crate::commands::reflect::lessons_look_alike;
use crate::db::episodes::Episode;
use crate::db::Db;
use crate::memory::{belief, civil_date, expiry_date, Fact, MemoryStore, FACTS, LESSONS};
use crate::runtime::proxy::{CancelFlag, TurnOutcome};
use crate::runtime::RuntimeManager;

/// How many conversations one daily pass folds in.
const NEW_EPISODES: usize = 5;
/// How far back a held candidate can still be promoted by a later conversation.
const POOL_DAYS: i64 = 180;
/// Chances before a record can let an item go, and the usefulness it must fall under.
const FORGET_CHANCES: i64 = 5;
const FORGET_BELOW: f64 = 0.3;
/// A lesson with no chance to help for this long is let go (the lesson cap already
/// uses the same number, `memory::PRUNE_IDLE_DAYS`).
const IDLE_DAYS: i64 = crate::memory::PRUNE_IDLE_DAYS;
/// Word overlap of two "what worked" lines from which they are the same job.
const SAME_WORK: f32 = 0.4;
const DAY_MS: i64 = 86_400_000;
/// The setting that remembers which conversations already produced a skill draft.
const SKILL_SOURCES: &str = "tidy.skill_sources";

/// How far a pass reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The daily pass: up to 5 conversations no tidy-up has folded in. A group is
    /// promoted only when one of them is in it.
    New,
    /// Tidy up and `/tidy`: everything still held.
    Everything,
}

/// A candidate lesson that waited on its episode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub conversation_id: String,
    pub name: String,
    pub description: String,
    pub body: String,
    pub cue: String,
}

/// The candidates of these episodes that no one has acted on (`held`).
pub fn held_candidates(episodes: &[Episode]) -> Vec<Held> {
    let mut out = Vec::new();
    for e in episodes {
        let Some(raw) = e.candidates_json.as_deref() else { continue };
        let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else { continue };
        for c in list {
            if c.get("status").and_then(|s| s.as_str()) != Some("held") {
                continue;
            }
            let text = |k: &str| c.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            let (name, body) = (text("name"), text("body"));
            if name.is_empty() || body.is_empty() {
                continue;
            }
            out.push(Held {
                conversation_id: e.conversation_id.clone(),
                name,
                description: text("description"),
                body,
                cue: text("cue"),
            });
        }
    }
    out
}

/// Group candidates that say the same thing. Greedy: a candidate joins the first group
/// any member of which it looks like.
fn group_alike(held: &[Held]) -> Vec<Vec<&Held>> {
    let mut groups: Vec<Vec<&Held>> = Vec::new();
    for h in held {
        let at = groups
            .iter()
            .position(|g| g.iter().any(|o| lessons_look_alike(&o.name, &o.description, &h.name, &h.description)));
        match at {
            Some(i) => groups[i].push(h),
            None => groups.push(vec![h]),
        }
    }
    groups
}

/// The promotion ladder's first rung: a candidate seen in 2 or more conversations.
/// `newest_first` is the pool in the order the episodes came (newest first). When
/// `must_include` is set, a group counts only if it holds one of those conversations.
pub fn promotions(
    held: &[Held],
    lessons: &[Fact],
    must_include: Option<&HashSet<String>>,
) -> Vec<ConsolidationPromote> {
    let mut out = Vec::new();
    for group in group_alike(held) {
        let mut conversations: Vec<String> = Vec::new();
        for h in &group {
            if !conversations.contains(&h.conversation_id) {
                conversations.push(h.conversation_id.clone());
            }
        }
        if conversations.len() < 2 {
            continue;
        }
        if let Some(only) = must_include {
            if !conversations.iter().any(|c| only.contains(c)) {
                continue;
            }
        }
        // Oldest source first, as evidence is kept.
        conversations.reverse();
        let first = group[0];
        let slug = crate::memory::slugify(&first.name).unwrap_or_else(|_| first.name.clone());
        let existing = lessons
            .iter()
            .find(|l| l.name == slug || lessons_look_alike(&l.name, &l.description, &first.name, &first.description));
        if let Some(l) = existing {
            // Already believed: only worth saying when there is a source it lacks.
            if conversations.iter().all(|c| l.evidence.contains(c)) {
                continue;
            }
        }
        out.push(ConsolidationPromote {
            name: slug,
            description: first.description.clone(),
            body: first.body.clone(),
            cue: Some(first.cue.clone()).filter(|c| !c.is_empty()),
            evidence: conversations,
            into: existing.map(|l| l.name.clone()),
            members: group.iter().map(|h| (h.conversation_id.clone(), h.name.clone())).collect(),
        });
    }
    out
}

/// Notes that say the same: the older survives and the newer folds into it. Lessons
/// use the wording check reflection already uses for "the same lesson again".
pub fn duplicate_merges(facts: &[Fact], lessons: &[Fact]) -> Vec<ConsolidationMerge> {
    fn fold(
        entries: &[&Fact],
        same: impl Fn(&Fact, &Fact) -> bool,
        collection: Option<&str>,
    ) -> Vec<ConsolidationMerge> {
        let mut oldest_first: Vec<&Fact> = entries.to_vec();
        oldest_first.sort_by(|a, b| a.created.cmp(&b.created).then(a.name.cmp(&b.name)));
        let mut gone: HashSet<&str> = HashSet::new();
        let mut out = Vec::new();
        for (i, keep) in oldest_first.iter().enumerate() {
            if gone.contains(keep.name.as_str()) {
                continue;
            }
            let drop: Vec<String> = oldest_first[i + 1..]
                .iter()
                .filter(|o| !gone.contains(o.name.as_str()) && same(o, keep))
                .map(|o| o.name.clone())
                .collect();
            if drop.is_empty() {
                continue;
            }
            gone.extend(oldest_first[i + 1..].iter().filter(|o| drop.contains(&o.name)).map(|o| o.name.as_str()));
            out.push(ConsolidationMerge {
                keep: keep.name.clone(),
                drop,
                text: keep.body.clone(),
                collection: collection.map(str::to_string),
            });
        }
        out
    }
    let active: Vec<&Fact> = facts.iter().filter(|f| f.in_prompt()).collect();
    let mut out = fold(&active, |a, b| belief::relation(a, b) == belief::Relation::Same, None);
    let lessons: Vec<&Fact> = lessons.iter().collect();
    out.extend(fold(
        &lessons,
        |a, b| lessons_look_alike(&a.name, &a.description, &b.name, &b.description),
        Some(LESSONS),
    ));
    out
}

/// What to let go of: `(facts, lessons)`. An item that tested badly goes whatever its
/// kind; a lesson that has had no chance to help for a long time goes too.
pub fn forgets(
    facts: &[Fact],
    lessons: &[Fact],
    fact_credit: &std::collections::HashMap<String, crate::db::episodes::Credit>,
    lesson_credit: &std::collections::HashMap<String, crate::db::episodes::Credit>,
    surfaced: &std::collections::HashMap<String, i64>,
    idle_before: &str,
) -> (Vec<String>, Vec<String>) {
    let bad = |credit: &std::collections::HashMap<String, crate::db::episodes::Credit>, name: &str| {
        credit.get(name).is_some_and(|c| c.chances >= FORGET_CHANCES && c.usefulness() < FORGET_BELOW)
    };
    let facts_out = facts.iter().filter(|f| f.in_prompt() && bad(fact_credit, &f.name)).map(|f| f.name.clone()).collect();
    let lessons_out = lessons
        .iter()
        .filter(|l| {
            let last_chance =
                surfaced.get(&l.name).map(|ms| civil_date(ms / DAY_MS)).unwrap_or_else(|| l.created.clone());
            bad(lesson_credit, &l.name) || last_chance.as_str() < idle_before
        })
        .map(|l| l.name.clone())
        .collect();
    (facts_out, lessons_out)
}

/// What the rules found, and the conversations they looked at.
pub struct Found {
    pub proposal: Consolidation,
    /// The new conversations this pass considered: stamped once it has acted.
    pub seen: Vec<String>,
}

pub fn rules_found(db: &Db, mem: &MemoryStore, reach: Reach) -> Found {
    let now = crate::db::now_ms();
    let new = db.unconsolidated_episodes(NEW_EPISODES).unwrap_or_default();
    let pool = db.episodes_since(now - POOL_DAYS * DAY_MS).unwrap_or_default();
    let new_ids: HashSet<String> = new.iter().map(|e| e.conversation_id.clone()).collect();
    let must = (reach == Reach::New).then_some(&new_ids);

    let facts = mem.list();
    let lessons = mem.list_in(LESSONS);
    let mut proposal = Consolidation {
        promotes: promotions(&held_candidates(&pool), &lessons, must),
        merges: duplicate_merges(&facts, &lessons),
        ..Default::default()
    };
    // Something folded away by a merge is not also let go.
    let merged_away: HashSet<&str> = proposal.merges.iter().flat_map(|m| m.drop.iter().map(String::as_str)).collect();
    let (facts_out, lessons_out) = forgets(
        &facts,
        &lessons,
        &db.memory_credit_map(FACTS).unwrap_or_default(),
        &db.memory_credit_map(LESSONS).unwrap_or_default(),
        &db.memory_usage_map(LESSONS).unwrap_or_default(),
        &expiry_date(-IDLE_DAYS),
    );
    proposal.deletes = facts_out.into_iter().filter(|n| !merged_away.contains(n.as_str())).collect();
    proposal.lesson_deletes = lessons_out.into_iter().filter(|n| !merged_away.contains(n.as_str())).collect();
    Found { proposal, seen: new_ids.into_iter().collect() }
}

/// The rules' proposal alone, for Tidy up to add the model's own look to.
pub fn rules_proposal(db: &Db, mem: &MemoryStore, reach: Reach) -> Consolidation {
    rules_found(db, mem, reach).proposal
}

/// Say where each candidate got to on its episode row (`promoted`).
pub fn mark_candidates(db: &Db, members: &[(String, String)], status: &str) {
    for conversation_id in members.iter().map(|(c, _)| c).collect::<HashSet<_>>() {
        let Ok(Some(episode)) = db.episode(conversation_id) else { continue };
        let Some(raw) = episode.candidates_json else { continue };
        let Ok(mut list) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) else { continue };
        for c in list.iter_mut() {
            let name = c.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if members.iter().any(|(conv, n)| conv == conversation_id && n == name) {
                c["status"] = serde_json::Value::String(status.to_string());
            }
        }
        if let Ok(json) = serde_json::to_string(&list) {
            let _ = db.set_episode_candidates(conversation_id, &json);
        }
    }
}

/// Keep a pass's rows as the pending proposal, joined to any that already waits.
pub fn keep_pending(db: &Db, found: &Consolidation) {
    if found.is_empty() {
        return;
    }
    let mut merged = db_pending(db).unwrap_or_default();
    merged.absorb(found.clone());
    store_pending(db, &merged);
}

/// Write the pending proposal, or clear it when nothing is left in it.
pub fn store_pending(db: &Db, proposal: &Consolidation) {
    let json = if proposal.is_empty() { String::new() } else { serde_json::to_string(proposal).unwrap_or_default() };
    let _ = db.set_setting(PENDING_CONSOLIDATION, &json);
}

/// The digest no longer offers Undo (the snapshot was used).
pub fn clear_undo(db: &Db) {
    crate::commands::scheduler::clear_digest_undo(db);
}

// ---- skills from success (SLP-2) ----

/// Finished conversations of one kind whose "what worked" lines read alike: the same
/// job done the same way twice or more. Conversations that already produced a draft
/// are left out.
pub fn skill_groups(episodes: &[Episode], used: &HashSet<String>) -> Vec<Vec<Episode>> {
    fn words(s: &str) -> HashSet<String> {
        s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(str::to_string).collect()
    }
    fn alike(a: &str, b: &str) -> bool {
        let (wa, wb) = (words(a), words(b));
        if wa.is_empty() || wb.is_empty() {
            return false;
        }
        wa.intersection(&wb).count() as f32 / wa.union(&wb).count() as f32 >= SAME_WORK
    }
    let mut groups: Vec<Vec<Episode>> = Vec::new();
    for e in episodes {
        let kind = e.task_kind.as_deref().unwrap_or("unknown");
        let worked = e.worked.as_deref().unwrap_or("").trim();
        if e.outcome.as_deref() != Some("done") || kind == "unknown" || worked.is_empty() || used.contains(&e.conversation_id)
        {
            continue;
        }
        let at = groups.iter().position(|g| {
            g[0].task_kind.as_deref() == Some(kind) && g.iter().any(|o| alike(o.worked.as_deref().unwrap_or(""), worked))
        });
        match at {
            Some(i) => groups[i].push(e.clone()),
            None => groups.push(vec![e.clone()]),
        }
    }
    groups.retain(|g| g.len() >= 2);
    groups
}

fn used_sources(db: &Db) -> HashSet<String> {
    db.get_setting(SKILL_SOURCES)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

#[derive(Debug, serde::Deserialize)]
struct SkillDraft {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    when_to_use: String,
    #[serde(default)]
    body: String,
}

/// The skill-writing prompt: the same ask `/skillify` makes of a conversation, put to
/// what two or more of them did.
fn skill_prompt(group: &[Episode]) -> String {
    let jobs = group
        .iter()
        .map(|e| {
            format!(
                "- goal: {}\n  what I did: {}\n  what worked: {}",
                e.goal.as_deref().unwrap_or(""),
                e.digest.as_deref().unwrap_or(""),
                e.worked.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "These finished conversations were the same kind of job, done the same way. Turn the \
         repeatable part into a skill: the steps that would do the job again, leaving out what \
         was specific to one time, as short imperative instructions a later run can follow \
         without these conversations. Output ONLY JSON: {{\"name\":\"kebab-case\",\"description\":\"one line\",\
         \"when_to_use\":\"one line\",\"body\":\"the steps\"}}. If nothing here is worth repeating, \
         output {{\"name\":\"\"}}.\n{jobs}"
    )
}

/// `SLP-2`: draft one skill from a group of finished jobs and leave it as a proposal
/// (the `skills` rung is `ask`, so a skill never keeps itself). At most one per pass.
/// Returns how many proposals were made.
pub async fn propose_skills(mgr: &RuntimeManager, db: &Db, endpoint: Option<(String, String)>) -> usize {
    if autonomy_gate(db, "skills") == Rung::Off {
        return 0;
    }
    let Some((base_url, token)) = endpoint else { return 0 };
    let now = crate::db::now_ms();
    let pool = db.episodes_since(now - POOL_DAYS * DAY_MS).unwrap_or_default();
    let mut used = used_sources(db);
    let Some(group) = skill_groups(&pool, &used).into_iter().next() else { return 0 };

    let endpoint = crate::cloud::ChatEndpoint::OpenAi { base_url, api_key: Some(token), model: None };
    let msgs = vec![
        serde_json::json!({ "role": "system", "content": "You write reusable skills for an AI assistant. Output ONLY JSON." }),
        serde_json::json!({ "role": "user", "content": skill_prompt(&group) }),
    ];
    let Ok(TurnOutcome::Final { content, .. }) =
        crate::cloud::drive_turn(&mgr.client, &endpoint, &msgs, &[], 0.2, crate::cloud::Effort::Off, &CancelFlag::new(), |_| {})
            .await
    else {
        return 0;
    };
    // Whatever came of it, these conversations are not drafted from again.
    used.extend(group.iter().map(|e| e.conversation_id.clone()));
    let _ = db.set_setting(SKILL_SOURCES, &serde_json::to_string(&used.iter().collect::<Vec<_>>()).unwrap_or_default());

    let draft: SkillDraft = serde_json::from_str(crate::commands::reflect::strip_fence(&content)).unwrap_or(SkillDraft {
        name: String::new(),
        description: String::new(),
        when_to_use: String::new(),
        body: String::new(),
    });
    propose_draft(mgr, db, &draft)
}

/// File a draft as a `skill` proposal unless it is empty, too long, or its name is
/// already a skill or a pending proposal.
fn propose_draft(mgr: &RuntimeManager, db: &Db, draft: &SkillDraft) -> usize {
    let Ok(slug) = crate::memory::slugify(&draft.name) else { return 0 };
    if draft.description.trim().is_empty() || draft.body.trim().is_empty() || draft.body.chars().count() > 8000 {
        return 0;
    }
    if crate::agent::skillpack::discover(mgr.app_data_dir(), None).iter().any(|p| p.name == slug) {
        return 0;
    }
    if db.list_change_proposals().unwrap_or_default().iter().any(|p| p.target == "skill" && p.slug.as_deref() == Some(&slug)) {
        return 0;
    }
    let when = if draft.when_to_use.trim().is_empty() { draft.description.as_str() } else { draft.when_to_use.as_str() };
    let file = crate::agent::skillpack::render_skill_md_with(&slug, draft.description.trim(), when.trim(), &draft.body, Some("poiesis"));
    let made = db
        .add_change_proposal("skill", Some(&slug), &file, draft.description.trim(), Some(draft.description.trim()))
        .is_ok();
    if made {
        let _ = db.log_activity(None, "memory", &format!("proposed the skill {slug}"));
    }
    usize::from(made)
}

// ---- the pass (SLP-1) ----

/// What one daily tidy-up did, for the digest. It claims nothing else.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassResult {
    pub applied: Applied,
    /// Rows waiting for a yes (the whole pending proposal), under `ask`.
    pub waiting: usize,
    /// Skill drafts left on the Lessons tab.
    pub skills: usize,
    /// The golden check found it made me worse, and I put it back.
    pub reverted: bool,
    /// Input arrived before the pass was done: the rest stays due.
    pub stopped: bool,
    /// The pass ran at all (`consolidate` was not off).
    pub ran: bool,
    /// Something was applied that Undo can put back.
    pub undoable: bool,
}

/// Step 3 of the daily job. `cancel` is checked before each stage; the stage in flight
/// finishes, the rest stays due. New conversations are stamped as folded in only when
/// the pass got as far as acting on them.
pub async fn run_pass(app: &AppHandle, cancel: &CancelFlag) -> PassResult {
    let db = app.state::<Db>();
    let mem = app.state::<MemoryStore>();
    let mgr = app.state::<RuntimeManager>();
    let rung = autonomy_gate(&db, "consolidate");
    let mut result = PassResult::default();
    if rung == Rung::Off {
        return result;
    }
    result.ran = true;
    let _ = app.emit("poiesis-tending", serde_json::json!({ "on": true }));

    let found = rules_found(&db, &mem, Reach::New);
    if cancel.is_cancelled() {
        result.stopped = true;
    } else {
        match rung {
            Rung::Auto if !found.proposal.is_empty() => match apply_guarded(app, &found.proposal, None).await {
                Ok((applied, None)) => {
                    result.applied = applied;
                    result.undoable = applied.total() > 0;
                }
                Ok((_, Some(_))) => result.reverted = true,
                Err(_) => {}
            },
            Rung::Ask => keep_pending(&db, &found.proposal),
            _ => {}
        }
        if !result.reverted {
            let now = crate::db::now_ms();
            for id in &found.seen {
                let _ = db.mark_episode_consolidated(id, now);
            }
        }
        result.waiting = db_pending(&db).map(|p| p.rows()).unwrap_or(0);
    }
    if !cancel.is_cancelled() {
        result.skills = propose_skills(&mgr, &db, mgr.engine_endpoint().await).await;
    } else {
        result.stopped = true;
    }
    let _ = app.emit("poiesis-tending", serde_json::json!({ "on": false }));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::episodes::Credit;

    fn ep(id: &str, outcome: &str, kind: &str, worked: &str, candidates: &str) -> Episode {
        Episode {
            conversation_id: id.into(),
            goal: Some("g".into()),
            outcome: Some(outcome.into()),
            task_kind: Some(kind.into()),
            digest: Some("I did it.".into()),
            worked: Some(worked.into()),
            candidates_json: Some(candidates.into()),
            encode_version: 1,
            ..Default::default()
        }
    }

    fn cand(name: &str, desc: &str, status: &str) -> String {
        format!(r#"{{"name":"{name}","description":"{desc}","body":"Do {name}.","cue":"tool:read_file","confidence":"low","status":"{status}"}}"#)
    }

    fn lesson(name: &str, desc: &str, created: &str) -> Fact {
        Fact { name: name.into(), description: desc.into(), kind: "lesson".into(), created: created.into(), body: format!("Do {name}."), ..Default::default() }
    }

    #[test]
    fn only_held_candidates_are_in_the_pool() {
        let c = format!("[{},{},{}]", cand("a-b", "x y z", "held"), cand("c-d", "p q r", "saved"), cand("e-f", "s t u", "promoted"));
        let held = held_candidates(&[ep("c1", "done", "report", "w", &c)]);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].name, "a-b");
        assert!(held_candidates(&[ep("c1", "done", "report", "w", "not json")]).is_empty());
    }

    #[test]
    fn the_same_small_correction_in_three_conversations_becomes_one_lesson_with_three_sources() {
        let c = |n: &str, d: &str| format!("[{}]", cand(n, d, "held"));
        let eps = vec![
            ep("c3", "done", "report", "w", &c("check-header-row", "check the header row first")),
            ep("c2", "done", "report", "w", &c("check-header", "check the header row first always")),
            ep("c1", "done", "report", "w", &c("header-check", "check the header row first")),
            ep("c9", "done", "report", "w", &c("use-tabs", "use tabs for indents")),
        ];
        let promoted = promotions(&held_candidates(&eps), &[], None);
        assert_eq!(promoted.len(), 1, "the one-off candidate stays held");
        assert_eq!(promoted[0].evidence, vec!["c1", "c2", "c3"], "oldest source first");
        assert_eq!(promoted[0].into, None);
        assert_eq!(promoted[0].members.len(), 3);
    }

    #[test]
    fn two_candidates_from_one_conversation_are_one_source() {
        let c = format!("[{},{}]", cand("check-header", "check the header row first", "held"), cand("header-check", "check the header row first", "held"));
        assert!(promotions(&held_candidates(&[ep("c1", "done", "report", "w", &c)]), &[], None).is_empty());
    }

    #[test]
    fn a_daily_pass_needs_a_new_conversation_in_the_group() {
        let c = |n: &str| format!("[{}]", cand(n, "check the header row first", "held"));
        let eps = vec![ep("c2", "done", "report", "w", &c("check-header")), ep("c1", "done", "report", "w", &c("header-check"))];
        let held = held_candidates(&eps);
        let only_old: HashSet<String> = ["other".to_string()].into();
        assert!(promotions(&held, &[], Some(&only_old)).is_empty());
        let with_new: HashSet<String> = ["c2".to_string()].into();
        assert_eq!(promotions(&held, &[], Some(&with_new)).len(), 1);
    }

    #[test]
    fn a_group_that_matches_a_lesson_adds_sources_only_when_it_lacks_them() {
        let c = |n: &str| format!("[{}]", cand(n, "check the header row first", "held"));
        let eps = vec![ep("c2", "done", "report", "w", &c("check-header")), ep("c1", "done", "report", "w", &c("header-check"))];
        let held = held_candidates(&eps);
        let known = Fact { evidence: vec!["c1".into()], ..lesson("check-the-header", "check the header row first", "2026-09-01") };
        let p = promotions(&held, &[known.clone()], None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].into.as_deref(), Some("check-the-header"));
        let complete = Fact { evidence: vec!["c1".into(), "c2".into()], ..known };
        assert!(promotions(&held, &[complete], None).is_empty(), "nothing new to say");
    }

    #[test]
    fn the_older_of_two_duplicates_survives_and_the_newer_folds_in() {
        let a = Fact { name: "prefers-tea".into(), description: "Prefers tea".into(), body: "Prefers tea.".into(), created: "2026-01-01".into(), kind: "preference".into(), ..Default::default() };
        let b = Fact { name: "likes-tea".into(), description: "Likes tea".into(), body: "prefers tea".into(), created: "2026-03-01".into(), kind: "preference".into(), ..Default::default() };
        let other = Fact { name: "uses-sqlite".into(), description: "Uses SQLite".into(), body: "Uses SQLite.".into(), created: "2026-02-01".into(), kind: "fact".into(), ..Default::default() };
        let merges = duplicate_merges(&[b, other, a], &[]);
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].keep, "prefers-tea");
        assert_eq!(merges[0].drop, vec!["likes-tea"]);
        assert!(merges[0].collection.is_none());

        let l1 = lesson("check-header", "check the header row first", "2026-01-01");
        let l2 = lesson("header-check", "check the header row first", "2026-02-01");
        let m = duplicate_merges(&[], &[l2, l1]);
        assert_eq!(m[0].keep, "check-header");
        assert_eq!(m[0].collection.as_deref(), Some("lessons"));
    }

    #[test]
    fn what_tested_badly_or_sat_unused_for_long_is_let_go() {
        let facts = vec![lesson("bad-fact", "x", "2026-09-01"), lesson("good-fact", "y", "2026-09-01")];
        let lessons = vec![
            lesson("bad-lesson", "a", "2026-09-01"),
            lesson("idle-lesson", "b", "2026-01-01"),
            lesson("used-lesson", "c", "2026-01-01"),
            lesson("new-lesson", "d", "2026-10-20"),
        ];
        let credit = |pairs: &[(&str, Credit)]| pairs.iter().map(|(n, c)| (n.to_string(), *c)).collect::<std::collections::HashMap<_, _>>();
        let bad = Credit { chances: 6, helped: 0, hurt: 5, last_outcome_at: None };
        let few = Credit { chances: 2, helped: 0, hurt: 2, last_outcome_at: None };
        let fc = credit(&[("bad-fact", bad), ("good-fact", few)]);
        let lc = credit(&[("bad-lesson", bad)]);
        let mut surfaced = std::collections::HashMap::new();
        surfaced.insert("used-lesson".to_string(), 1_793_000_000_000i64);
        let (f, l) = forgets(&facts, &lessons, &fc, &lc, &surfaced, "2026-06-29");
        assert_eq!(f, vec!["bad-fact"], "two bad chances are not a record");
        assert_eq!(l, vec!["bad-lesson", "idle-lesson"]);
    }

    #[test]
    fn rules_find_a_promotion_a_merge_and_a_forget_in_a_real_store() {
        let tmp = tempfile::tempdir().unwrap();
        let mem = MemoryStore::new(tmp.path()).unwrap();
        let db = Db::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..3 {
            let id = db.create_conversation(&format!("Chat {i}"), None, false).unwrap().id;
            db.save_episode_rules(&id, "done", "report", "{}").unwrap();
            let c = format!("[{}]", cand(&format!("check-header-{i}"), "check the header row first", "held"));
            db.save_episode_draft(&id, "g", "I did it.", "I read the header first.", "", &c).unwrap();
            ids.push(id);
        }
        mem.save(&db, &Fact { name: "prefers-tea".into(), description: "Prefers tea".into(), kind: "preference".into(), body: "Prefers tea.".into(), ..Default::default() }).unwrap();
        mem.save(&db, &Fact { name: "likes-tea".into(), description: "Likes tea".into(), kind: "preference".into(), body: "prefers tea".into(), created: "2099-01-01".into(), ..Default::default() }).unwrap();

        let found = rules_found(&db, &mem, Reach::New);
        assert_eq!(found.proposal.promotes.len(), 1);
        assert_eq!(found.proposal.promotes[0].evidence.len(), 3);
        assert_eq!(found.proposal.merges.len(), 1);
        assert_eq!(found.seen.len(), 3);

        // Applied: one lesson with three sources, one note folded in, and the episodes
        // say where the candidates got to.
        let done = crate::commands::memory::apply_proposal(&db, &mem, &found.proposal);
        assert_eq!((done.promoted, done.merged), (1, 1));
        let l = mem.list_lessons();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].evidence.len(), 3);
        assert!(db.episode(&ids[0]).unwrap().unwrap().candidates_json.unwrap().contains("promoted"));
        // Nothing left to say the next night.
        assert!(rules_found(&db, &mem, Reach::Everything).proposal.promotes.is_empty());
    }

    #[test]
    fn a_merge_joins_the_evidence_and_a_contest_marks_both_sides() {
        let tmp = tempfile::tempdir().unwrap();
        let mem = MemoryStore::new(tmp.path()).unwrap();
        let db = Db::open_in_memory().unwrap();
        let f = |name: &str, body: &str, conv: &str, created: &str| Fact {
            name: name.into(),
            description: format!("About {name}"),
            kind: "preference".into(),
            body: body.into(),
            source_conversation: Some(conv.into()),
            created: created.into(),
            ..Default::default()
        };
        mem.save(&db, &f("prefers-tea", "Prefers tea.", "c1", "2026-01-01")).unwrap();
        mem.save(&db, &f("likes-tea", "prefers tea", "c2", "2026-02-01")).unwrap();
        mem.save(&db, &f("prefers-coffee", "Prefers coffee.", "c3", "2026-03-01")).unwrap();
        let proposal = Consolidation {
            merges: vec![ConsolidationMerge { keep: "prefers-tea".into(), drop: vec!["likes-tea".into()], text: "Prefers tea.".into(), collection: None }],
            contests: vec![vec!["prefers-tea".into(), "prefers-coffee".into(), "gone".into()]],
            ..Default::default()
        };
        let done = crate::commands::memory::apply_proposal(&db, &mem, &proposal);
        assert_eq!((done.merged, done.contested), (1, 1));
        assert_eq!(mem.read("prefers-tea").unwrap().evidence, vec!["c1".to_string(), "c2".to_string()]);
        assert!(mem.read("likes-tea").is_none());
        assert_eq!(mem.read("prefers-tea").unwrap().status.as_deref(), Some("contested"));
        assert_eq!(mem.read("prefers-coffee").unwrap().status.as_deref(), Some("contested"));
    }

    #[test]
    fn a_tidy_up_that_changed_nothing_claims_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let mem = MemoryStore::new(tmp.path()).unwrap();
        let db = Db::open_in_memory().unwrap();
        let proposal = Consolidation {
            deletes: vec!["not-there".into()],
            lesson_deletes: vec!["nor-this".into()],
            merges: vec![ConsolidationMerge { keep: "missing".into(), drop: vec!["x".into()], text: "t".into(), collection: None }],
            ..Default::default()
        };
        assert_eq!(crate::commands::memory::apply_proposal(&db, &mem, &proposal).total(), 0);
    }

    #[test]
    fn two_finished_jobs_done_the_same_way_make_a_skill_group() {
        let mk = |id: &str, kind: &str, outcome: &str, worked: &str| ep(id, outcome, kind, worked, "[]");
        let eps = vec![
            mk("c1", "report", "done", "I grouped the rows by category and summed them"),
            mk("c2", "report", "done", "grouped rows by category then summed them"),
            mk("c3", "report", "failed", "grouped rows by category and summed them"),
            mk("c4", "mail", "done", "grouped rows by category and summed them"),
            mk("c5", "report", "done", ""),
        ];
        let groups = skill_groups(&eps, &HashSet::new());
        assert_eq!(groups.len(), 1);
        let ids: Vec<&str> = groups[0].iter().map(|e| e.conversation_id.as_str()).collect();
        assert_eq!(ids, vec!["c1", "c2"], "only finished jobs of one kind with a worked line");
        let used: HashSet<String> = ["c1".to_string()].into();
        assert!(skill_groups(&eps, &used).is_empty(), "a used conversation does not count twice");
        assert!(skill_prompt(&groups[0]).contains("grouped rows"));
    }

    #[test]
    fn a_skill_draft_lands_as_a_proposal_once_and_only_when_whole() {
        let db = Db::open_in_memory().unwrap();
        let mgr = RuntimeManager::new(tempfile::tempdir().unwrap().path().to_path_buf());
        let draft = |name: &str, body: &str| SkillDraft {
            name: name.into(),
            description: "Make the expense summary".into(),
            when_to_use: "When asked for an expense summary".into(),
            body: body.into(),
        };
        assert_eq!(propose_draft(&mgr, &db, &draft("", "Group rows.")), 0, "no name");
        assert_eq!(propose_draft(&mgr, &db, &draft("expense-summary", "  ")), 0, "no body");
        assert_eq!(propose_draft(&mgr, &db, &draft("expense-summary", &"x".repeat(8001))), 0, "too long");
        assert_eq!(propose_draft(&mgr, &db, &draft("Expense Summary", "1. Group rows by category.")), 1);
        let p = db.list_change_proposals().unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].target, "skill");
        assert_eq!(p[0].slug.as_deref(), Some("expense-summary"));
        assert!(p[0].proposed_text.contains("origin: poiesis"));
        assert_eq!(propose_draft(&mgr, &db, &draft("expense-summary", "1. Again.")), 0, "already waiting");
    }

    #[test]
    fn a_pending_proposal_takes_rows_once_and_a_row_can_be_dropped() {
        let db = Db::open_in_memory().unwrap();
        let row = Consolidation { lesson_deletes: vec!["old".into()], contests: vec![vec!["a".into(), "b".into()]], ..Default::default() };
        keep_pending(&db, &row);
        keep_pending(&db, &row);
        let pending = db_pending(&db).unwrap();
        assert_eq!(pending.rows(), 2, "a second pass does not double a row");
        let mut p = pending;
        assert!(p.remove_row("let-go", "old"));
        assert!(p.remove_row("contest", "a|b"));
        assert!(!p.remove_row("merge", "nothing"));
        store_pending(&db, &p);
        assert!(db_pending(&db).is_none(), "an empty proposal is cleared, not kept");
    }
}
