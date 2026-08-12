//! Reflection (REF-2): the agent learning from its own finished work.
//!
//! A *lesson* is the one kind of memory Poiesis writes about **itself** — a
//! generalizable correction drawn from a conversation that is over ("check a
//! directory exists before writing into it"). Three commitments shape this:
//!
//! - **Idempotent.** `reflected_at` is stamped before the model runs, so a
//!   hung or nonsense turn can never put the app in a retry loop.
//! - **Strict.** The output is parsed as JSON or discarded. A model that
//!   rambles teaches nothing; guessing at its intent would write junk into the
//!   prompt of every future conversation.
//! - **Gated.** Saving obeys `autonomy_gate("lessons")` — the user can turn
//!   self-teaching down to proposals, or off.
//! - **Criticized.** Before a draft is trusted, a second local call reviews
//!   it against the same transcript (`CRT-1`). One that fails is not thrown
//!   away — it is demoted to a `change_proposals` row regardless of the
//!   configured rung (`CRT-2`), and the verdict is logged to `tool_stats` so
//!   drift in reflection quality is visible later (`CRT-3`).

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::autonomy::{autonomy_gate, Rung};
use crate::cloud::{drive_turn, ChatEndpoint};
use crate::commands::agent::{build_remote_endpoint, ChatTarget};
use crate::db::Db;
use crate::memory::{Fact, MemoryStore, LESSONS};
use crate::runtime::proxy::{CancelFlag, TurnOutcome};
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

/// How many turns of the finished conversation reflection reads.
const WINDOW: usize = 30;
/// Per-turn clip. Reflection needs the shape of what happened, not the text.
const TURN_CLIP: usize = 400;
/// Never write more than this from one pass, whatever the model returns.
const MAX_LESSONS: usize = 3;
/// How many fail→fix pairs reach the prompt (`FIX-2`).
const MAX_FIXES: usize = 5;
/// Per-field clip for a fail→fix pair. Tool arguments are unbounded — a
/// `write_file` carries its whole body — and the rest of this prompt is
/// already capped turn by turn, so leaving these raw is what would blow the
/// context, not the number of pairs.
const FIX_FIELD_CLIP: usize = 200;

/// Clip one field of a fail→fix pair to `FIX_FIELD_CLIP`, on a char boundary.
fn clip(s: &str) -> String {
    if s.chars().count() <= FIX_FIELD_CLIP {
        return s.to_string();
    }
    let mut out: String = s.chars().take(FIX_FIELD_CLIP).collect();
    out.push('…');
    out
}

/// One lesson the model proposed. `confidence` decides whether it is worth
/// acting on at all — low-confidence drafts are dropped, not queued, because
/// v1 would rather learn nothing than fill the review queue with noise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonDraft {
    pub name: String,
    pub description: String,
    pub body: String,
    /// "high" | "low"
    #[serde(default)]
    pub confidence: String,
}

#[derive(Debug, Default, Deserialize)]
struct LessonBatch {
    #[serde(default)]
    lessons: Vec<LessonDraft>,
}

/// The critic's answer for one draft (`CRT-1`). `reason` is only meaningful
/// when `ok` is false — it becomes the rationale on the demoted proposal.
#[derive(Debug, Deserialize)]
struct CriticVerdict {
    ok: bool,
    #[serde(default)]
    reason: String,
}

/// What one reflection pass produced. `saved` is what went to disk and is in
/// effect now; `proposed` is waiting on the user. They are counted separately
/// because the UI says different things about them — claiming to have learned
/// something that is still pending approval would be a lie.
#[derive(Debug, Default, Serialize)]
pub struct Reflection {
    pub saved: Vec<LessonDraft>,
    pub proposed: Vec<LessonDraft>,
}

/// Run one self-reflection pass over a finished conversation.
#[tauri::command]
pub async fn reflect_conversation_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    app: tauri::AppHandle,
    conversation_id: String,
    target: Option<ChatTarget>,
) -> Result<Reflection, PoiesisError> {
    // First thing, before anything can fail: this conversation has had its turn.
    let _ = db.set_conversation_reflected(&conversation_id, crate::db::now_ms());

    // Nothing to route through — bail quietly rather than surfacing an error
    // for a background process the user didn't ask for.
    let target = target.unwrap_or_default();
    let endpoint = match build_remote_endpoint(&db, &target).map_err(PoiesisError::Message)? {
        Some(ep) => ep,
        None => {
            let Some((base_url, token)) = mgr.engine_endpoint().await else {
                return Ok(Reflection::default());
            };
            ChatEndpoint::OpenAi {
                base_url,
                api_key: Some(token),
                model: None,
            }
        }
    };

    // `OUT-2` answers to the `skills` rung, not this one: a user who turned
    // off self-taught lessons hasn't thereby said a skill may never ask to
    // fix itself. It reads `skill_runs`, not the transcript, so it needs
    // none of the work below.
    let rung = autonomy_gate(&db, "lessons");
    if rung == Rung::Off {
        propose_skill_revisions(&mgr, &endpoint, &db, &conversation_id).await;
        return Ok(Reflection::default());
    }

    let turns = db
        .list_messages_window(&conversation_id, WINDOW)
        .unwrap_or_default();
    if turns.is_empty() {
        return Ok(Reflection::default());
    }
    let transcript = turns
        .iter()
        .map(|m| {
            let mut text: String = m.content.chars().take(TURN_CLIP).collect();
            if m.content.chars().count() > TURN_CLIP {
                text.push('…');
            }
            format!("{}: {text}", m.role)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let failures = db.tool_failures_in(&conversation_id).unwrap_or_default();
    let failure_text = if failures.is_empty() {
        "none".to_string()
    } else {
        failures
            .iter()
            .map(|(tool, n)| format!("{tool} failed {n}×"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    // FIX-2: the highest-signal evidence reflection ever gets — a concrete
    // wrong action, the reason it was wrong, and the action that worked right
    // after, in this same conversation.
    let fixes = db.tool_fixes_in(&conversation_id).unwrap_or_default();
    let fixes_text = if fixes.is_empty() {
        "none".to_string()
    } else {
        fixes
            .iter()
            .take(MAX_FIXES)
            .map(|f| {
                format!(
                    "{} failed with \"{}\" for {}, then succeeded with {}",
                    f.tool_name,
                    clip(&f.error),
                    clip(&f.failed_args),
                    clip(&f.fixed_args)
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    };

    let prompt = format!(
        "Below is a finished conversation, the assistant's tool-failure counts, and any mistakes \
         it corrected itself within the conversation. Extract AT MOST {MAX_LESSONS} lessons about \
         how the assistant should work better next time.\n\
         A lesson must be: (a) generalizable beyond this one conversation, (b) actionable as a \
         behavior change, (c) grounded in an observed mistake, failure, or user correction. \
         Style rules the assistant was merely following are NOT lessons. If nothing qualifies, \
         return {{\"lessons\":[]}}.\n\
         JSON schema:\n\
         {{\"lessons\":[{{\"name\":\"kebab-case-slug\",\"description\":\"one line\",\
         \"body\":\"2-4 sentences, imperative voice\",\"confidence\":\"high|low\"}}]}}\n\
         Tool failures: {failure_text}\n\
         Mistakes you corrected yourself during this conversation: {fixes_text}\n\
         Conversation:\n{transcript}"
    );
    let msgs = vec![
        serde_json::json!({
            "role": "system",
            "content": "You are the self-reflection process of a local AI assistant. Output ONLY JSON, no preamble.",
        }),
        serde_json::json!({ "role": "user", "content": prompt }),
    ];

    let outcome = drive_turn(&mgr.client, &endpoint, &msgs, &[], 0.2, &CancelFlag::new(), |_| {})
        .await
        .map_err(|e| PoiesisError::Message(e.to_string()))?;
    let TurnOutcome::Final { content } = outcome else {
        return Ok(Reflection::default());
    };

    // Parse strictly: unreadable output means this conversation taught nothing.
    let batch: LessonBatch = serde_json::from_str(strip_fence(&content)).unwrap_or_default();

    // Key for tool-reliability stats (CRT-3): mirrors the naming `run_agent`
    // uses so the critic's pass/fail sits next to every other skill's.
    let model_name = if target.provenance.as_deref() == Some("cloud") {
        target.model.clone().unwrap_or_else(|| "cloud".to_string())
    } else {
        mgr.engine_model_name().await.unwrap_or_else(|| "local".to_string())
    };

    let existing_lessons = mem.list_lessons();
    let mut out = Reflection::default();
    for draft in batch.lessons.into_iter().take(MAX_LESSONS) {
        if draft.confidence.trim() != "high" {
            continue;
        }
        let Ok(slug) = crate::memory::slugify(&draft.name) else { continue };

        // RPT-1: has this already been learned — by exact slug, or close
        // enough in wording? Decided here, acted on only after the critic:
        // bumping a count and escalating to standing instructions are both
        // writes, and an unreviewed draft must not cause either.
        let recurrence_of = existing_lessons
            .iter()
            .find(|l| {
                l.name == slug
                    || lessons_look_alike(&l.name, &l.description, &draft.name, &draft.description)
            })
            .map(|l| (l.name.clone(), l.description.clone()));

        // CRT-1: a second local call reviews the draft against the same
        // transcript before it is trusted. CRT-2: a draft that fails is not
        // discarded — it is demoted to a proposal regardless of the
        // configured rung, the same landing a lesson gets under `Ask`.
        let verdict = critique(&mgr, &endpoint, &transcript, &draft).await;
        db.add_tool_stat(&model_name, "reflect.critic", &conversation_id, verdict.ok);
        if !verdict.ok {
            // `CRT-UI-1` shows the objection as the rationale, so it has to say
            // something even when the critic answers with a bare `ok:false`.
            let objection = if verdict.reason.trim().is_empty() {
                "I couldn't convince myself this held up.".to_string()
            } else {
                verdict.reason.clone()
            };
            // The objection is the *rationale*, never the lesson's own
            // description — the draft's description travels separately so
            // accepting the proposal doesn't file a criticism as the summary.
            if db
                .add_change_proposal(
                    "lesson-critic",
                    Some(&slug),
                    &draft.body,
                    &objection,
                    Some(&draft.description),
                )
                .is_ok()
            {
                out.proposed.push(draft);
            }
            continue;
        }

        // RPT-1: the critic cleared it, and it's something already learned —
        // so it isn't news, it's evidence the lesson isn't sticking. Bump the
        // count in place rather than writing a duplicate file.
        if let Some((name, description)) = recurrence_of {
            let Ok(recurrence) = mem.bump_lesson_recurrence(&db, &name) else { continue };
            let _ = db.log_activity(
                Some(&conversation_id),
                "reflect",
                &format!("relearned {name}: now {recurrence}×"),
            );
            // RPT-2: a lesson learned three times belongs in standing
            // instructions instead. That is a soul change, so it obeys the
            // `soul` rung like every other one — a user who closed off
            // standing-instruction changes must not receive these either —
            // and it is proposed exactly once, however often it recurs after.
            if recurrence >= 3
                && autonomy_gate(&db, "soul") != Rung::Off
                && !db.has_soul_escalation(&name).unwrap_or(true)
            {
                let soul = mem.soul();
                let addition = if soul.trim().is_empty() {
                    format!("- {description}")
                } else {
                    format!("{}\n- {description}", soul.trim_end())
                };
                // Over the cap there is no proposal the user could ever
                // accept, so say so in the log rather than retrying silently
                // on every future recurrence.
                if addition.chars().count() > crate::memory::SOUL_CAP {
                    let _ = db.log_activity(
                        Some(&conversation_id),
                        "reflect",
                        &format!(
                            "wanted to make {name} a standing instruction, but they're already full"
                        ),
                    );
                } else {
                    let _ = db.add_change_proposal(
                        "soul",
                        Some(&name),
                        &addition,
                        "I've learned this three times — it isn't sticking as a lesson.",
                        None,
                    );
                }
            }
            continue;
        }

        match rung {
            Rung::Off => break,
            Rung::Ask => {
                if db
                    .add_change_proposal(
                        "lesson",
                        Some(&slug),
                        &draft.body,
                        &draft.description,
                        Some(&draft.description),
                    )
                    .is_ok()
                {
                    out.proposed.push(draft);
                }
            }
            Rung::Auto => {
                let fact = Fact {
                    name: slug.clone(),
                    description: draft.description.clone(),
                    kind: "lesson".to_string(),
                    created: String::new(),
                    source_conversation: Some(conversation_id.clone()),
                    body: draft.body.clone(),
                    scope: None,
                    recurrence: None,
                    last_seen: None,
                    expires_at: None,
                };
                if mem.save_lesson(&db, &fact).is_err() {
                    continue;
                }
                // Reflection runs outside any chat stream, so the toast is
                // driven by an app-level event rather than the agent sink.
                let _ = app.emit(
                    "poiesis-memory-write",
                    serde_json::json!({
                        "op": "save",
                        "name": slug,
                        "description": draft.description,
                        "collection": LESSONS,
                        "undo_token": "",
                    }),
                );
                let _ = db.log_activity(
                    Some(&conversation_id),
                    "reflect",
                    &format!("learned {slug}"),
                );
                out.saved.push(draft);
            }
        }
    }

    // `OUT-1`: a lesson (saved, proposed, or demoted-but-proposed — everything
    // `out` counts) drawn from this conversation is the "corrected" signal for
    // any skill it activated.
    if !out.saved.is_empty() || !out.proposed.is_empty() {
        let _ = db.mark_skill_runs_corrected(&conversation_id);
    }
    propose_skill_revisions(&mgr, &endpoint, &db, &conversation_id).await;

    Ok(out)
}

/// `OUT-2`'s trigger: 3 or more of a skill's last 5 (or fewer, if it hasn't
/// run 5 times yet) activations hit a tool failure afterwards. Fewer than 3
/// runs total can never reach the threshold, so it's not worth evaluating.
fn is_rough(runs: &[crate::db::SkillRunRow]) -> bool {
    runs.len() >= 3 && runs.iter().filter(|r| r.tool_failures > 0).count() >= 3
}

/// `OUT-2`: when a skill's last 5 activations show 3 or more with tool
/// failures afterwards, draft a revised `SKILL.md` and propose it. Skills
/// sourced from Personal/Project are never rewritten in place — accepting the
/// proposal (`resolve_change_proposal_cmd`, target `skill`) always writes an
/// App-source copy, which supersedes the original in discovery order
/// (`skillpack::add_root`) without touching anything under the user's
/// the user's own folders. Proposed at most once ever per skill (`has_skill_revision_proposal`).
async fn propose_skill_revisions(
    mgr: &RuntimeManager,
    endpoint: &ChatEndpoint,
    db: &Db,
    conversation_id: &str,
) {
    if autonomy_gate(db, "skills") == Rung::Off {
        return;
    }
    let Ok(names) = db.skills_used_in(conversation_id) else { return };

    // Which of this conversation's skills are actually rough and unproposed —
    // settled before touching the filesystem, so the common case (nothing to
    // revise) costs two indexed queries per skill and no directory walk.
    let rough: Vec<(String, String, Vec<crate::db::SkillRunRow>)> = names
        .into_iter()
        .filter_map(|name| {
            // The name is third-party frontmatter and ends up as a directory
            // when the proposal is accepted, so the slug — not the name — is
            // what identifies the proposal.
            let slug = crate::memory::slugify(&name).ok()?;
            if db.has_skill_revision_proposal(&slug).unwrap_or(true) {
                return None;
            }
            let runs = db.recent_skill_runs(&name, 5).ok()?;
            is_rough(&runs).then_some((name, slug, runs))
        })
        .collect();
    if rough.is_empty() {
        return;
    }

    let folder = db
        .conversation_folder(conversation_id)
        .ok()
        .and_then(|(f, _)| f)
        .map(std::path::PathBuf::from);
    let packs = crate::agent::skillpack::discover(mgr.app_data_dir(), folder.as_deref());

    for (name, slug, runs) in rough {
        let Some(pack) = packs.iter().find(|p| p.name == name) else { continue };
        let Ok(body) = crate::agent::skillpack::load_body(pack) else { continue };

        let mut fixes_text = String::new();
        for run in runs.iter().filter(|r| r.tool_failures > 0) {
            for f in db
                .tool_fixes_in(&run.conversation_id)
                .unwrap_or_default()
                .into_iter()
                .take(3)
            {
                fixes_text.push_str(&format!(
                    "- {} failed with \"{}\" for {}, then succeeded with {}\n",
                    f.tool_name,
                    clip(&f.error),
                    clip(&f.failed_args),
                    clip(&f.fixed_args)
                ));
            }
        }
        if fixes_text.is_empty() {
            fixes_text.push_str("none recorded — the failures were never corrected within a run\n");
        }

        let prompt = format!(
            "The skill below has been rough lately — {} of its last {} uses hit tool failures \
             afterwards. Revise its instructions to prevent the mistakes listed, keeping its \
             purpose and structure. Output ONLY the revised markdown body: no frontmatter, no \
             preamble, no code fence.\n\n\
             Current skill body:\n{body}\n\n\
             Mistakes observed in recent uses:\n{fixes_text}",
            runs.iter().filter(|r| r.tool_failures > 0).count(),
            runs.len()
        );
        let msgs = vec![
            serde_json::json!({
                "role": "system",
                "content": "You revise a struggling AI assistant skill's own instructions. Output ONLY the revised markdown body.",
            }),
            serde_json::json!({ "role": "user", "content": prompt }),
        ];
        let Ok(TurnOutcome::Final { content }) =
            drive_turn(&mgr.client, endpoint, &msgs, &[], 0.2, &CancelFlag::new(), |_| {}).await
        else {
            continue;
        };
        let revised = strip_fence(&content).trim();
        if revised.is_empty() {
            continue;
        }

        let when_to_use = pack.when_to_use.as_deref().unwrap_or(pack.description.as_str());
        // Frontmatter keeps the skill's **original** name while the proposal's
        // slug carries the safe directory: discovery keys on the frontmatter
        // name, so this is what makes the accepted App copy supersede the
        // Personal/Project original instead of sitting beside it.
        let file = crate::agent::skillpack::render_skill_md(&name, &pack.description, when_to_use, revised);
        if db
            .add_change_proposal(
                "skill-revision",
                Some(&slug),
                &file,
                crate::db::SKILL_REVISION_RATIONALE,
                Some(&pack.description),
            )
            .is_ok()
        {
            let _ = db.log_activity(
                Some(conversation_id),
                "memory",
                &format!("proposed a revision to the skill {name}"),
            );
        }
    }
}

/// CRT-1: ask a second local call whether a drafted lesson actually holds up
/// against the conversation it came from. Any engine failure, cancellation,
/// or tool-call instead of an answer is read the same way as an explicit
/// `ok:false` — a critic that can't be reached hasn't cleared the lesson.
async fn critique(
    mgr: &RuntimeManager,
    endpoint: &ChatEndpoint,
    transcript: &str,
    draft: &LessonDraft,
) -> CriticVerdict {
    let prompt = format!(
        "A reflection pass drew the lesson below from the conversation that follows. Judge it: \
         is it actually supported by what happened, specific enough to act on, and generalizable \
         beyond this one conversation? If you raise ANY issue, you must answer ok:false.\n\
         Lesson \"{}\": {}\n\
         JSON schema: {{\"ok\":true|false,\"reason\":\"one line, only when ok is false\"}}\n\
         Conversation:\n{transcript}",
        draft.name, draft.body
    );
    let msgs = vec![
        serde_json::json!({
            "role": "system",
            "content": "You are a skeptical reviewer of another process's self-drafted lessons. Output ONLY JSON, no preamble.",
        }),
        serde_json::json!({ "role": "user", "content": prompt }),
    ];
    match drive_turn(&mgr.client, endpoint, &msgs, &[], 0.0, &CancelFlag::new(), |_| {}).await {
        Ok(TurnOutcome::Final { content }) => parse_critic(&content),
        _ => CriticVerdict {
            ok: false,
            reason: "the critic couldn't be reached".to_string(),
        },
    }
}

/// Parse a critic verdict, strictly first. A model that ignores the JSON
/// shape and answers in prose is still read for the one signal the contract
/// requires — an explicit `ok:false`, however it's quoted or spaced. No
/// signal found means no issue was raised.
fn parse_critic(raw: &str) -> CriticVerdict {
    if let Ok(v) = serde_json::from_str::<CriticVerdict>(strip_fence(raw)) {
        return v;
    }
    let flat: String = raw
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '"')
        .collect();
    let ok = !flat.contains("ok:false");
    CriticVerdict {
        ok,
        reason: if ok {
            String::new()
        } else {
            raw.chars().take(200).collect()
        },
    }
}

/// `RPT-1`: a dependency-free stand-in for semantic similarity — word-set
/// overlap (Jaccard) between "name description" pairs. Good enough to catch
/// "the same lesson, phrased differently" without needing an embedder loaded,
/// which reflection has no access to.
fn lessons_look_alike(name_a: &str, desc_a: &str, name_b: &str, desc_b: &str) -> bool {
    fn words(s: &str) -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .map(str::to_string)
            .collect()
    }
    let a = words(&format!("{name_a} {desc_a}"));
    let b = words(&format!("{name_b} {desc_b}"));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let intersection = a.intersection(&b).count();
    let union = a.union(&b).count();
    (intersection as f32 / union as f32) >= 0.5
}

/// Strip a ```json fence if the model wrapped its answer in one.
fn strip_fence(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else { return s };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.trim_start_matches('\n').trim_end_matches('`').trim()
}

/// Delete a lesson by hand (REF-UI-1). Returns the trash token that undoes it.
#[tauri::command]
pub fn forget_lesson_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
) -> Result<String, PoiesisError> {
    let file = mem.forget_lesson(&db, &name).map_err(PoiesisError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("forgot lesson {name}"));
    Ok(file)
}

#[tauri::command]
pub fn list_lessons_cmd(mem: State<'_, MemoryStore>) -> Vec<Fact> {
    mem.list_lessons()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_high_confidence_drafts_survive_parsing() {
        let raw = r#"```json
        {"lessons":[
          {"name":"Verify Paths","description":"check first","body":"Do it.","confidence":"high"},
          {"name":"maybe","description":"unsure","body":"Hmm.","confidence":"low"}
        ]}
        ```"#;
        let batch: LessonBatch = serde_json::from_str(strip_fence(raw)).unwrap();
        assert_eq!(batch.lessons.len(), 2);
        let kept: Vec<_> = batch
            .lessons
            .iter()
            .filter(|l| l.confidence == "high")
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(crate::memory::slugify(&kept[0].name).unwrap(), "verify-paths");
    }

    #[test]
    fn prose_teaches_nothing_rather_than_erroring() {
        let batch: LessonBatch =
            serde_json::from_str(strip_fence("Sure! Here are some lessons:")).unwrap_or_default();
        assert!(batch.lessons.is_empty());
        // A well-formed empty answer is also fine.
        let batch: LessonBatch = serde_json::from_str(r#"{"lessons":[]}"#).unwrap();
        assert!(batch.lessons.is_empty());
    }

    /// `RPT-1`: the same lesson, reworded, should still be recognized as a
    /// recurrence rather than a brand-new lesson — that's the whole point of
    /// the similarity fallback for a slug that doesn't match exactly.
    #[test]
    fn similar_lessons_are_recognized_as_the_same_one() {
        assert!(lessons_look_alike(
            "check-path-exists",
            "verify a path exists before writing to it",
            "verify-path-before-write",
            "check that a path exists before writing to it",
        ));
        assert!(!lessons_look_alike(
            "check-path-exists",
            "verify a path exists before writing to it",
            "ask-before-sending-mail",
            "always confirm with the user before sending an email",
        ));
    }

    #[test]
    fn critic_reads_well_formed_json() {
        let ok = parse_critic(r#"{"ok":true}"#);
        assert!(ok.ok);
        let bad = parse_critic(r#"{"ok":false,"reason":"not grounded in the conversation"}"#);
        assert!(!bad.ok);
        assert_eq!(bad.reason, "not grounded in the conversation");
    }

    /// `OUT-2`: the threshold is "3 of the last 5", not "3 total ever" — and
    /// fewer than 3 runs total can never trigger it, however few succeeded.
    #[test]
    fn skill_is_rough_at_three_of_five_failing_runs() {
        fn run(tool_failures: i64) -> crate::db::SkillRunRow {
            crate::db::SkillRunRow {
                conversation_id: "c".into(),
                tool_failures,
                corrected: false,
                created_at: 0,
            }
        }

        assert!(!is_rough(&[run(1), run(1)])); // only 2 runs, can't reach 3
        assert!(!is_rough(&[run(1), run(1), run(0), run(0), run(0)])); // 2 of 5
        assert!(is_rough(&[run(1), run(1), run(1), run(0), run(0)])); // 3 of 5
        assert!(is_rough(&[run(1), run(2), run(1)])); // 3 of 3, fewer than 5 runs so far
    }

    #[test]
    fn critic_falls_back_to_scanning_prose_for_the_contractual_signal() {
        // The contract obligates the model to write `ok:false` if it has any
        // objection at all, so prose without that signal reads as a pass —
        // whatever the model's degree of chattiness.
        let chatty_pass = parse_critic("Sure, ok: true, this lesson looks solid to me.");
        assert!(chatty_pass.ok);
        // Loosely quoted or spaced, the rejection signal still lands.
        let chatty_fail = parse_critic("I have concerns here — \"ok\" : false, too vague.");
        assert!(!chatty_fail.ok);
        // No signal at all: read as no issue raised, not as an error.
        let silent = parse_critic("Sure! Here is my review of the lesson.");
        assert!(silent.ok);
    }
}
