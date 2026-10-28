//! `PRJ-S1` (`COGNITION_PLAN` III.12): a project's setup is derived, not stored.
//!
//! - **Proven lessons.** The lessons that reached answers, or fired, in a project's
//!   finished (`done`) conversations. `recall_with_cues` ranks them a little higher
//!   in that project. No model call and no table.
//! - **A project like this one.** When a project has no instructions yet, another
//!   project that does and looks like it can offer them. Looking alike is a name
//!   word they share or a folder they share a parent with; the kind of work the
//!   first chats were adds to it. Facts never travel: only the instructions are
//!   offered, and the person decides.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use crate::commands::memory::ContextRefs;
use crate::db::{Db, Project};

/// Lessons that reached an answer in a `done` conversation of this project.
pub fn proven_lessons(db: &Db, project_id: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for raw in db.done_contexts_of_project(project_id).unwrap_or_default() {
        if let Ok(refs) = serde_json::from_str::<ContextRefs>(&raw) {
            out.extend(refs.lessons);
            out.extend(refs.fired);
        }
    }
    out
}

/// Words too common in project names to say two projects are alike.
const COMMON: [&str; 14] =
    ["the", "and", "for", "new", "project", "my", "old", "app", "work", "test", "notes", "docs", "stuff", "misc"];

fn name_words(name: &str) -> HashSet<String> {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 && !COMMON.contains(w))
        .map(str::to_string)
        .collect()
}

fn folder_parent(root: &str) -> Option<&Path> {
    Path::new(root).parent().filter(|p| p.components().count() > 2)
}

/// Another project this one looks like, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Similar {
    pub project_id: String,
    /// In words, for the offer: "both have <word> in the name".
    pub why: String,
}

/// The best project to offer instructions from, for a project that has none.
/// `None` when this project already has instructions, or nothing looks like it.
pub fn similar_project(db: &Db, project_id: &str) -> Option<Similar> {
    let me = db.get_project(project_id).ok().flatten()?;
    if me.instructions.as_deref().is_some_and(|i| !i.trim().is_empty()) {
        return None;
    }
    let my_words = name_words(&me.name);
    let my_kind = db.task_kinds_of_project(project_id).unwrap_or_default().into_iter().next();

    let mut best: Option<(u32, Similar)> = None;
    // Newest first, so a tie goes to the project worked on most recently.
    for other in db.list_projects(false).unwrap_or_default() {
        if other.id == me.id || other.instructions.as_deref().is_none_or(|i| i.trim().is_empty()) {
            continue;
        }
        let Some((score, why)) = alike(db, &me, &my_words, my_kind.as_deref(), &other) else { continue };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, Similar { project_id: other.id, why }));
        }
    }
    best.map(|(_, s)| s)
}

fn alike(db: &Db, me: &Project, my_words: &HashSet<String>, my_kind: Option<&str>, other: &Project) -> Option<(u32, String)> {
    let mut score = 0;
    let mut reasons = Vec::new();
    let mut shared: Vec<String> = my_words.intersection(&name_words(&other.name)).cloned().collect();
    shared.sort();
    if let Some(word) = shared.first() {
        score += 2 * shared.len() as u32;
        reasons.push(format!("both have \"{word}\" in the name"));
    }
    if let (Some(a), Some(b)) = (me.root_path.as_deref(), other.root_path.as_deref()) {
        if folder_parent(a).is_some() && folder_parent(a) == folder_parent(b) {
            score += 2;
            reasons.push("their folders sit side by side".to_string());
        }
    }
    // Looking alike needs a name or a folder; the kind of work only adds to it,
    // because there are only a few kinds and most projects share one.
    if reasons.is_empty() {
        return None;
    }
    if let Some(kind) = my_kind {
        let theirs = db.task_kinds_of_project(&other.id).unwrap_or_default();
        if theirs.iter().filter(|k| *k == kind).count() * 2 > theirs.len() {
            score += 1;
            reasons.push(format!("both start with {} work", crate::agent::selfmodel::words(kind)));
        }
    }
    Some((score, reasons.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(db: &Db, name: &str, folder: Option<&str>, instructions: Option<&str>) -> String {
        let p = db.create_project(name, None).unwrap();
        if let Some(f) = folder {
            db.exec_for_tests("UPDATE projects SET root_path = ?2 WHERE id = ?1", rusqlite::params![p.id, f]);
        }
        if let Some(i) = instructions {
            db.set_project_instructions(&p.id, Some(i)).unwrap();
        }
        p.id
    }

    fn chat_in(db: &Db, project: &str, outcome: &str, kind: &str, context: Option<&str>) {
        let id = db.create_conversation("A chat", None, false).unwrap().id;
        db.set_conversation_project(&id, Some(project)).unwrap();
        db.save_episode_rules(&id, outcome, kind, "{}").unwrap();
        if let Some(ctx) = context {
            let m = db
                .append_message(
                    &id,
                    &crate::db::NewMessage {
                        role: "assistant".into(),
                        content: "ok".into(),
                        model_name: None,
                        model_provenance: None,
                        steps_json: None,
                        attachments: Vec::new(),
                    },
                )
                .unwrap();
            db.finalize_message(&m.id, "ok", None, Some(ctx), None, None).unwrap();
        }
    }

    #[test]
    fn lessons_from_finished_chats_of_this_project_only_are_proven() {
        let db = Db::open_in_memory().unwrap();
        let (a, b) = (project(&db, "Taxes", None, None), project(&db, "Book", None, None));
        chat_in(&db, &a, "done", "report", Some(r#"{"lessons":["check-header"],"fired":["read-first"]}"#));
        chat_in(&db, &a, "failed", "report", Some(r#"{"lessons":["bad-idea"]}"#));
        chat_in(&db, &b, "done", "report", Some(r#"{"lessons":["other-project"]}"#));
        let got = proven_lessons(&db, &a);
        assert_eq!(got, HashSet::from(["check-header".to_string(), "read-first".to_string()]));
        assert!(proven_lessons(&db, "none").is_empty());
    }

    #[test]
    fn a_shared_name_word_offers_that_projects_instructions() {
        let db = Db::open_in_memory().unwrap();
        let old = project(&db, "Taxes 2024", None, Some("Use the bank export in /data."));
        project(&db, "Garden", None, Some("Plants only."));
        let new = project(&db, "Taxes 2025", None, None);
        let got = similar_project(&db, &new).unwrap();
        assert_eq!(got.project_id, old);
        assert!(got.why.contains("\"taxes\""));
    }

    #[test]
    fn side_by_side_folders_look_alike_and_unrelated_ones_do_not() {
        let db = Db::open_in_memory().unwrap();
        let old = project(&db, "Alpha", Some("/home/me/code/alpha"), Some("Rust, tests first."));
        let new = project(&db, "Beta", Some("/home/me/code/beta"), None);
        assert_eq!(similar_project(&db, &new).unwrap().project_id, old);
        let far = project(&db, "Gamma", Some("/srv/other/gamma"), None);
        assert_eq!(similar_project(&db, &far), None);
    }

    #[test]
    fn a_project_with_instructions_or_without_a_match_offers_nothing() {
        let db = Db::open_in_memory().unwrap();
        project(&db, "Taxes 2024", None, Some("Rules."));
        let has = project(&db, "Taxes 2025", None, Some("Mine already."));
        assert_eq!(similar_project(&db, &has), None, "it has its own");
        let plain = project(&db, "Holiday", None, None);
        assert_eq!(similar_project(&db, &plain), None, "nothing alike");
        assert_eq!(similar_project(&db, "missing"), None);
    }

    #[test]
    fn a_project_without_instructions_is_not_offered_as_a_source() {
        let db = Db::open_in_memory().unwrap();
        project(&db, "Taxes 2024", None, None);
        let new = project(&db, "Taxes 2025", None, None);
        assert_eq!(similar_project(&db, &new), None);
    }

    #[test]
    fn the_kind_of_work_only_breaks_ties_between_projects_that_already_look_alike() {
        let db = Db::open_in_memory().unwrap();
        let mail = project(&db, "Client Mail", None, Some("Be brief."));
        let docs = project(&db, "Client Reports", None, Some("Use headings."));
        chat_in(&db, &mail, "done", "mail", None);
        chat_in(&db, &docs, "done", "report", None);
        let new = project(&db, "Client Notes", None, None);
        chat_in(&db, &new, "done", "report", None);
        let got = similar_project(&db, &new).unwrap();
        assert_eq!(got.project_id, docs);
        assert!(got.why.contains("reports work"));
        // With no chats yet in the new project the kind says nothing, and the
        // newest of the equal matches wins.
        let fresh = project(&db, "Client Plans", None, None);
        assert!(similar_project(&db, &fresh).is_some());
    }
}
