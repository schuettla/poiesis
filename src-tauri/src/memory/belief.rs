//! Beliefs with evidence (`BLF-2`, COGNITION_PLAN III.4).
//!
//! Before a new fact is written it is held against the entries that look like it.
//! Nothing here deletes a belief: a replaced one stays on disk as an earlier
//! version, a disputed one is marked `contested` in both entries, and a belief said
//! again only gains the conversation it was said in.
//!
//! `BeliefRelation` is meant to become a decision (COGNITION_PLAN, "What this plan
//! uses from the Dual-process plan"). Until a decision model answers it, this is the
//! fallback the plan names: wording overlap says whether two entries are about the
//! same thing, and the text says whether the new one repeats, extends or differs.

use std::collections::HashSet;

use super::{parse_entry, render_entry, slugify, today, Fact, MemoryStore, FACTS};
use crate::db::Db;

/// How the new entry stands to an existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// The same belief: nothing new is written.
    Same,
    /// The new text says everything the old did, and more.
    Refines,
    /// About the same thing, saying something else.
    Contradicts,
    Unrelated,
}

/// How many entries a new one is held against.
const TOP: usize = 5;
/// Word overlap (Jaccard over name and description) from which two entries are
/// about the same thing. Low on purpose: "lives in Hamburg" and "lives in Berlin"
/// share one word in three, and a false alarm only costs a question under Unsure.
const RELATED: f32 = 0.3;

fn words(s: &str) -> HashSet<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(str::to_string).collect()
}

/// The text with case and punctuation gone, so "Prefers tea." equals "prefers tea".
fn plain(s: &str) -> String {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

fn overlap(a: &Fact, b: &Fact) -> f32 {
    let wa = words(&format!("{} {}", a.name, a.description));
    let wb = words(&format!("{} {}", b.name, b.description));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    wa.intersection(&wb).count() as f32 / wa.union(&wb).count() as f32
}

/// Does `new` repeat, extend or differ from `old`?
pub fn relation(new: &Fact, old: &Fact) -> Relation {
    // A belief in one project says nothing about the same words in another.
    if new.project.is_some() && old.project.is_some() && new.project != old.project {
        return Relation::Unrelated;
    }
    let (a, b) = (plain(&new.body), plain(&old.body));
    if a.is_empty() || b.is_empty() {
        return Relation::Unrelated;
    }
    if a == b {
        return Relation::Same;
    }
    if overlap(new, old) < RELATED {
        return Relation::Unrelated;
    }
    if b.contains(&a) {
        // The old entry already says it, with more.
        Relation::Same
    } else if a.contains(&b) {
        Relation::Refines
    } else {
        Relation::Contradicts
    }
}

/// What `save_checked` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Written as it came.
    New,
    /// Already believed: the old entry gained this conversation as evidence.
    Merged(String),
    /// The old entry is now an earlier version of the new one.
    Replaced(String),
    /// Both entries are marked contested until the person decides.
    Contested(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// The entry that holds the belief now.
    pub name: String,
    pub outcome: Outcome,
}

impl MemoryStore {
    /// The entries `new` is held against: the closest few that are not unrelated,
    /// closest first. Superseded entries are not listed (`list_in`) and intentions
    /// are not beliefs about the world.
    fn related_to(&self, new: &Fact) -> Vec<(Fact, Relation)> {
        let mut found: Vec<(f32, Fact, Relation)> = self
            .list_in(FACTS)
            .into_iter()
            .filter(|f| f.in_prompt() && f.name != new.name)
            .filter_map(|f| {
                let r = relation(new, &f);
                (r != Relation::Unrelated).then(|| (overlap(new, &f), f, r))
            })
            .collect();
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        found.into_iter().take(TOP).map(|(_, f, r)| (f, r)).collect()
    }

    /// Save a fact the way the three write sites should: held against what is
    /// believed already (III.4). `by_user` says the person is the newer source
    /// (`/remember`, or the `memory` tool in a turn the person is in): then a
    /// contradicting answer replaces the old one. Otherwise both are contested.
    pub fn save_checked(&self, db: &Db, f: &Fact, by_user: bool) -> Result<Saved, String> {
        let related = self.related_to(f);
        // A repeat beats everything: say where it was said again and stop.
        if let Some((old, _)) = related.iter().find(|(_, r)| *r == Relation::Same) {
            if let Some(conversation) = f.source_conversation.as_deref() {
                self.add_evidence(db, FACTS, &old.name, conversation)?;
            }
            return Ok(Saved { name: old.name.clone(), outcome: Outcome::Merged(old.name.clone()) });
        }
        let Some((old, relation)) = related.into_iter().next() else {
            return Ok(Saved { name: self.save(db, f)?, outcome: Outcome::New });
        };
        if relation == Relation::Refines || by_user {
            let name = self.replace_fact(db, &old.name, f)?;
            Ok(Saved { name, outcome: Outcome::Replaced(old.name) })
        } else {
            let name = self.contest_fact(db, &old.name, f)?;
            Ok(Saved { name, outcome: Outcome::Contested(old.name) })
        }
    }

    /// Rewrite one entry's frontmatter in place, under the lock.
    fn rewrite(&self, collection: &str, name: &str, change: impl FnOnce(&mut Fact)) -> Result<(), String> {
        let name = slugify(name)?;
        let _guard = self.lock.lock().unwrap();
        let path = self.path_for(collection, &name);
        let text = std::fs::read_to_string(&path).map_err(|_| format!("no memory named {name}"))?;
        let mut entry = parse_entry(&name, &text);
        change(&mut entry);
        std::fs::write(&path, render_entry(&entry)).map_err(|e| e.to_string())
    }

    /// `BLF-1`: this entry was said again in `conversation_id`.
    pub fn add_evidence(&self, db: &Db, collection: &str, name: &str, conversation_id: &str) -> Result<(), String> {
        self.rewrite(collection, name, |f| f.add_evidence(conversation_id))?;
        self.write_index();
        self.sync_fts(db);
        Ok(())
    }

    /// Save `f` as the answer that replaces `old`. The old file stays, marked
    /// superseded and out of every prompt; the new one carries its evidence and says
    /// what it replaced. If the old one cannot be marked the new one is taken back,
    /// so a failure never leaves two active answers.
    pub(super) fn replace_fact(&self, db: &Db, old: &str, f: &Fact) -> Result<String, String> {
        let old = slugify(old)?;
        let before = self.read(&old).ok_or_else(|| format!("no memory named {old}"))?;
        if before.is_superseded() {
            return Err(format!("{old} was already replaced"));
        }
        let mut evidence = before.evidence.clone();
        let mut next = Fact {
            supersedes: Some(old.clone()),
            valid_from: Some(today()),
            scope: f.scope.clone().or_else(|| before.scope.clone()),
            project: f.project.clone().or_else(|| before.project.clone()),
            status: None,
            ..f.clone()
        };
        if let Some(source) = &f.source_conversation {
            if !evidence.contains(source) {
                evidence.push(source.clone());
            }
        }
        next.evidence = evidence;
        let saved = self.save(db, &next)?;
        let marked = self.rewrite(FACTS, &old, |e| {
            e.status = Some("superseded".into());
            e.superseded_by = Some(saved.clone());
        });
        if let Err(e) = marked {
            let _ = self.forget_in(db, FACTS, &saved);
            return Err(e);
        }
        // A replaced belief must stop matching: its vector goes, and it is no longer
        // listed, so it will not be embedded again.
        let _ = db.delete_vectors_for_ref("memory", FACTS, &old);
        self.write_index();
        self.sync_fts(db);
        Ok(saved)
    }

    /// Save `f` next to `old`, both marked contested. Same safety as `replace_fact`.
    fn contest_fact(&self, db: &Db, old: &str, f: &Fact) -> Result<String, String> {
        let old = slugify(old)?;
        self.read(&old).ok_or_else(|| format!("no memory named {old}"))?;
        let saved = self.save(db, &Fact { status: Some("contested".into()), ..f.clone() })?;
        if let Err(e) = self.rewrite(FACTS, &old, |e| e.status = Some("contested".into())) {
            let _ = self.forget_in(db, FACTS, &saved);
            return Err(e);
        }
        self.write_index();
        self.sync_fts(db);
        Ok(saved)
    }

    /// The groups of contested facts that dispute one another, each side with its
    /// evidence. A contested fact whose opponent is gone stands alone.
    pub fn unsure_groups(&self) -> Vec<Vec<Fact>> {
        let contested: Vec<Fact> = self.list_in(FACTS).into_iter().filter(|f| f.status.as_deref() == Some("contested")).collect();
        let mut groups: Vec<Vec<Fact>> = Vec::new();
        for f in contested {
            let at = groups.iter().position(|g| g.iter().any(|o| relation(&f, o) != Relation::Unrelated));
            match at {
                Some(i) => groups[i].push(f),
                None => groups.push(vec![f]),
            }
        }
        groups
    }

    /// Mark entries as disputed, both still in the prompt with the hedge marker. An
    /// entry that is gone or already replaced is skipped. Returns how many were marked.
    pub fn mark_contested(&self, db: &Db, names: &[String]) -> usize {
        let mut marked = 0;
        for name in names {
            let live = self.read(name).is_some_and(|f| !f.is_superseded());
            if live && self.rewrite(FACTS, name, |e| e.status = Some("contested".into())).is_ok() {
                marked += 1;
            }
        }
        if marked > 0 {
            self.write_index();
            self.sync_fts(db);
        }
        marked
    }

    /// The person decided about a contested group. `keep` is the side that stays;
    /// with `both` every side stays, each as true only in some situations (topical).
    pub fn resolve_contest(&self, db: &Db, keep: &str, both: bool) -> Result<(), String> {
        let keep = slugify(keep)?;
        let group = self
            .unsure_groups()
            .into_iter()
            .find(|g| g.iter().any(|f| f.name == keep))
            .ok_or_else(|| format!("{keep} is not in doubt"))?;
        for f in &group {
            if both || f.name == keep {
                self.rewrite(FACTS, &f.name, |e| {
                    e.status = None;
                    if both {
                        e.scope = Some("topical".into());
                    }
                })?;
            } else {
                self.rewrite(FACTS, &f.name, |e| {
                    e.status = Some("superseded".into());
                    e.superseded_by = Some(keep.clone());
                })?;
                let _ = db.delete_vectors_for_ref("memory", FACTS, &f.name);
            }
        }
        self.write_index();
        self.sync_fts(db);
        Ok(())
    }

    /// Take back what `save_checked` did: the new entry goes to the trash and the
    /// old one is believed again. Its vector was dropped when it was replaced; the
    /// next recall embeds it again.
    pub fn undo_change(&self, db: &Db, new: &str, old: &str) -> Result<(), String> {
        self.rewrite(FACTS, old, |e| {
            e.status = None;
            e.superseded_by = None;
        })?;
        self.forget_in(db, FACTS, new)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(name: &str, description: &str, body: &str) -> Fact {
        Fact {
            name: name.into(),
            description: description.into(),
            kind: "preference".into(),
            body: body.into(),
            source_conversation: Some("c-new".into()),
            ..Default::default()
        }
    }

    fn store() -> (tempfile::TempDir, MemoryStore, Db) {
        let dir = tempfile::tempdir().unwrap();
        let s = MemoryStore::new(dir.path()).unwrap();
        (dir, s, Db::open_in_memory().unwrap())
    }
    #[test]
    fn same_text_extension_and_a_different_answer_are_told_apart() {
        let tea = fact("prefers-tea", "Prefers tea", "Prefers tea.");
        assert_eq!(relation(&fact("likes-tea", "Likes tea", "prefers tea"), &tea), Relation::Same);
        assert_eq!(relation(&fact("prefers-tea", "Prefers tea", "Prefers tea in the morning."), &tea), Relation::Refines);
        assert_eq!(relation(&fact("prefers-coffee", "Prefers coffee", "Prefers coffee."), &tea), Relation::Contradicts);
        assert_eq!(relation(&fact("uses-sqlite", "Project uses SQLite", "Uses SQLite."), &tea), Relation::Unrelated);
        // The old entry already says it with more: a repeat, not news.
        let long = fact("prefers-tea", "Prefers tea", "Prefers tea in the morning.");
        assert_eq!(relation(&fact("prefers-tea-x", "Prefers tea", "prefers tea"), &long), Relation::Same);
    }

    #[test]
    fn two_projects_do_not_dispute() {
        let a = Fact { project: Some("p1".into()), ..fact("prefers-tea", "Prefers tea", "Prefers tea.") };
        let b = Fact { project: Some("p2".into()), ..fact("prefers-coffee", "Prefers coffee", "Prefers coffee.") };
        assert_eq!(relation(&b, &a), Relation::Unrelated);
        let global = fact("prefers-coffee", "Prefers coffee", "Prefers coffee.");
        assert_eq!(relation(&global, &a), Relation::Contradicts);
    }

    #[test]
    fn a_contradiction_from_the_user_replaces_and_keeps_the_old_version() {
        let (_d, s, db) = store();
        s.save(&db, &fact("prefers-tea", "Prefers tea", "Prefers tea.")).unwrap();
        let saved = s.save_checked(&db, &fact("prefers-coffee", "Prefers coffee", "Prefers coffee."), true).unwrap();
        assert_eq!(saved.outcome, Outcome::Replaced("prefers-tea".into()));
        let now = s.read("prefers-coffee").unwrap();
        assert_eq!(now.supersedes.as_deref(), Some("prefers-tea"));
        assert_eq!(now.valid_from.as_deref(), Some(today().as_str()));
        assert!(now.status.is_none());
        assert_eq!(s.list().iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["prefers-coffee"]);
        let earlier = s.list_superseded(FACTS);
        assert_eq!(earlier.len(), 1);
        assert_eq!(earlier[0].superseded_by.as_deref(), Some("prefers-coffee"));
        assert!(!s.index_markdown().contains("prefers-tea"));
    }

    #[test]
    fn a_contradiction_from_anyone_else_marks_both_contested_never_two_silent_actives() {
        let (_d, s, db) = store();
        s.save(&db, &fact("prefers-tea", "Prefers tea", "Prefers tea.")).unwrap();
        let saved = s.save_checked(&db, &fact("prefers-coffee", "Prefers coffee", "Prefers coffee."), false).unwrap();
        assert_eq!(saved.outcome, Outcome::Contested("prefers-tea".into()));
        assert_eq!(s.read("prefers-tea").unwrap().status.as_deref(), Some("contested"));
        assert_eq!(s.read("prefers-coffee").unwrap().status.as_deref(), Some("contested"));
        // Both still reach the prompt, each hedged.
        let index = s.index_markdown();
        assert!(index.contains("[contested, noted") && index.contains("prefers-tea") && index.contains("prefers-coffee"));
        let groups = s.unsure_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }

    #[test]
    fn a_repeat_writes_nothing_and_adds_the_conversation_as_evidence() {
        let (_d, s, db) = store();
        s.save(&db, &Fact { source_conversation: Some("c-old".into()), ..fact("prefers-tea", "Prefers tea", "Prefers tea.") }).unwrap();
        assert_eq!(s.read("prefers-tea").unwrap().evidence, vec!["c-old".to_string()]);
        let saved = s.save_checked(&db, &fact("likes-tea", "Likes tea", "prefers tea"), false).unwrap();
        assert_eq!(saved.outcome, Outcome::Merged("prefers-tea".into()));
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.read("prefers-tea").unwrap().evidence, vec!["c-old".to_string(), "c-new".to_string()]);
        // Said a third time in the same chat: still one source.
        s.save_checked(&db, &fact("tea-again", "Prefers tea", "Prefers tea."), false).unwrap();
        assert_eq!(s.read("prefers-tea").unwrap().evidence.len(), 2);
    }

    #[test]
    fn an_extension_replaces_the_shorter_version_and_keeps_its_evidence() {
        let (_d, s, db) = store();
        s.save(&db, &Fact { source_conversation: Some("c-old".into()), ..fact("prefers-tea", "Prefers tea", "Prefers tea.") }).unwrap();
        let saved = s
            .save_checked(&db, &fact("prefers-tea-morning", "Prefers tea in the morning", "Prefers tea in the morning."), false)
            .unwrap();
        assert_eq!(saved.outcome, Outcome::Replaced("prefers-tea".into()));
        assert_eq!(s.read("prefers-tea-morning").unwrap().evidence, vec!["c-old".to_string(), "c-new".to_string()]);
        assert_eq!(s.list().len(), 1);
    }

    #[test]
    fn unrelated_facts_are_written_as_before() {
        let (_d, s, db) = store();
        s.save(&db, &fact("prefers-tea", "Prefers tea", "Prefers tea.")).unwrap();
        let saved = s.save_checked(&db, &fact("uses-sqlite", "Project uses SQLite", "Uses SQLite."), true).unwrap();
        assert_eq!(saved.outcome, Outcome::New);
        assert_eq!(s.list().len(), 2);
        assert!(s.unsure_groups().is_empty());
    }

    #[test]
    fn the_person_settles_a_doubt_by_keeping_one_side_or_both() {
        let (_d, s, db) = store();
        s.save(&db, &fact("prefers-tea", "Prefers tea", "Prefers tea.")).unwrap();
        s.save_checked(&db, &fact("prefers-coffee", "Prefers coffee", "Prefers coffee."), false).unwrap();
        s.resolve_contest(&db, "prefers-coffee", false).unwrap();
        assert_eq!(s.read("prefers-coffee").unwrap().status, None);
        assert!(s.read("prefers-tea").unwrap().is_superseded());
        assert!(s.unsure_groups().is_empty());

        let (_d2, s2, db2) = store();
        s2.save(&db2, &fact("prefers-tea", "Prefers tea", "Prefers tea.")).unwrap();
        s2.save_checked(&db2, &fact("prefers-coffee", "Prefers coffee", "Prefers coffee."), false).unwrap();
        s2.resolve_contest(&db2, "prefers-tea", true).unwrap();
        for name in ["prefers-tea", "prefers-coffee"] {
            let f = s2.read(name).unwrap();
            assert_eq!(f.status, None);
            assert_eq!(f.scope.as_deref(), Some("topical"));
        }
        assert!(s2.resolve_contest(&db2, "prefers-tea", false).is_err(), "nothing left in doubt");
    }

    #[test]
    fn undo_takes_the_new_belief_back_and_believes_the_old_one_again() {
        let (_d, s, db) = store();
        s.save(&db, &fact("prefers-tea", "Prefers tea", "Prefers tea.")).unwrap();
        let saved = s.save_checked(&db, &fact("prefers-coffee", "Prefers coffee", "Prefers coffee."), true).unwrap();
        s.undo_change(&db, &saved.name, "prefers-tea").unwrap();
        let names: Vec<String> = s.list().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["prefers-tea".to_string()]);
        assert!(s.list_superseded(FACTS).is_empty());
        assert!(s.read("prefers-tea").unwrap().superseded_by.is_none());
    }

    #[test]
    fn evidence_and_valid_from_survive_a_round_trip_and_old_files_are_unchanged() {
        let f = Fact { evidence: vec!["a".into(), "b".into()], valid_from: Some("2026-10-01".into()), ..fact("x", "d", "body") };
        let again = parse_entry("x", &render_entry(&f));
        assert_eq!(again.evidence, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(again.valid_from.as_deref(), Some("2026-10-01"));
        let plain = render_entry(&fact("x", "d", "body"));
        assert!(!plain.contains("evidence") && !plain.contains("valid_from"));
    }
}
