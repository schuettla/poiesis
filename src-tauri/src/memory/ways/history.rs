//! `WRK-20`: a preset's versions, and going back to one.
//!
//! Every change to a preset is a new version, and the name stays. Each one is a
//! plain file under `memory/ways/history/<preset>/`, the whole preset as it was
//! then plus who changed it and why, so nothing is lost and going back is only
//! writing an old version as a new one. Locking and unlocking are entries of their
//! own, so it is clear why nothing changed for a while.
//!
//! ```text
//! history/lean/
//!    0001.md   original       (what shipped)
//!    0002.md   version 2      (you, switched something off)
//!    0003.md   locked
//! ```

use std::path::PathBuf;

use super::edit::NewPreset;
use super::{header_value, parse_preset, render_preset, shipped_preset, split, Preset, PresetRule, SHIPPED};
use crate::memory::{one_line, MemoryStore};

/// Entries that carry a version of the preset's rules (the others only say a lock changed).
fn carries_rules(what: &str) -> bool {
    matches!(what, "original" | "version" | "back")
}

/// One line of a preset's history.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Its place in the history. Stable, so a click can name it.
    pub seq: u32,
    pub at: i64,
    /// `you`, `evolution` or `original`.
    pub by: String,
    /// `original`, `version`, `back`, `locked`, `unlocked`.
    pub what: String,
    pub why: String,
    pub version: u32,
    pub rules: Vec<PresetRule>,
    pub added: Vec<PresetRule>,
    pub removed: Vec<PresetRule>,
}

/// Where a preset goes back to.
#[derive(Debug, Clone, Copy)]
pub enum Back {
    Original,
    Entry(u32),
}

impl MemoryStore {
    fn history_dir(&self, slug: &str) -> PathBuf {
        self.ways_dir().join("history").join(slug)
    }

    fn history_files(&self, slug: &str) -> Vec<(u32, PathBuf)> {
        let Ok(entries) = std::fs::read_dir(self.history_dir(slug)) else { return Vec::new() };
        let mut out: Vec<(u32, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter_map(|p| Some((p.file_stem()?.to_str()?.parse().ok()?, p)))
            .collect();
        out.sort_by_key(|(seq, _)| *seq);
        out
    }

    /// Write one entry: the preset as it is now, and what happened to it.
    pub(super) fn log_preset(&self, p: &Preset, by: &str, what: &str, why: &str) {
        let dir = self.history_dir(&p.slug);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let seq = self.history_files(&p.slug).last().map_or(1, |(s, _)| s + 1);
        let body = render_preset(p).replacen(
            "---\n",
            &format!(
                "---\nat: {}\nby: {}\nwhat: {}\nwhy: {}\n",
                chrono::Utc::now().timestamp_millis(),
                one_line(by),
                one_line(what),
                one_line(why)
            ),
            1,
        );
        let _ = std::fs::write(dir.join(format!("{seq:04}.md")), body);
    }

    /// The history of a preset that has none yet starts with what it was before
    /// this change: what shipped, for a built-in, and then whatever the person had
    /// already done to it.
    pub(super) fn ensure_baseline(&self, before: &Preset) {
        if !self.history_files(&before.slug).is_empty() {
            return;
        }
        if before.built_in {
            if let Some(s) = SHIPPED.iter().find(|s| s.slug == before.slug) {
                let original = shipped_preset(s);
                self.log_preset(&original, "original", "original", "");
                if before.rules != original.rules || before.version > 1 {
                    self.log_preset(before, "you", "version", "earlier changes");
                }
                return;
            }
        }
        self.log_preset(before, "original", "original", "");
    }

    /// Change a preset's content: the next version, kept in the history. A built-in's
    /// file is the person's from then on.
    pub(super) fn commit_preset(&self, before: &Preset, after: Preset, by: &str, why: &str) -> Result<Preset, String> {
        self.commit_as(before, after, by, "version", why)
    }

    fn commit_as(&self, before: &Preset, after: Preset, by: &str, what: &str, why: &str) -> Result<Preset, String> {
        let done = self.commit_quiet(before, after, by, what, why)?;
        self.note_change(&if why.is_empty() { format!("Changed {}", done.name) } else { format!("{}: {why}", done.name) });
        Ok(done)
    }

    /// The same, without a copy of how I work: for what goes back through here many times.
    pub(super) fn commit_quiet(&self, before: &Preset, mut after: Preset, by: &str, what: &str, why: &str) -> Result<Preset, String> {
        self.ensure_baseline(before);
        after.version = before.version + 1;
        after.edited = true;
        self.write_preset(&after)?;
        self.log_preset(&after, by, what, why);
        Ok(after)
    }

    /// Every entry, newest first, with what each version added and took away.
    pub fn preset_history(&self, slug: &str) -> Vec<HistoryEntry> {
        let mut prev: Option<Vec<PresetRule>> = None;
        let mut out: Vec<HistoryEntry> = Vec::new();
        for (seq, path) in self.history_files(slug) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Some(p) = parse_preset(slug, &text) else { continue };
            let (header, _) = split(&text);
            let what = header_value(&header, "what").unwrap_or("version").to_string();
            let (mut added, mut removed) = (Vec::new(), Vec::new());
            if carries_rules(&what) {
                if let Some(before) = &prev {
                    added = p.rules.iter().filter(|r| !before.contains(r)).copied().collect();
                    removed = before.iter().filter(|r| !p.rules.contains(r)).copied().collect();
                }
                prev = Some(p.rules.clone());
            }
            out.push(HistoryEntry {
                seq,
                at: header_value(&header, "at").and_then(|v| v.parse().ok()).unwrap_or(0),
                by: header_value(&header, "by").unwrap_or("you").to_string(),
                what,
                why: header_value(&header, "why").unwrap_or("").to_string(),
                version: p.version,
                rules: p.rules,
                added,
                removed,
            });
        }
        out.reverse();
        out
    }

    /// What it was made as: the first version in its history, or what ships.
    /// `None` when there is nothing to go back to.
    pub fn original_rules(&self, slug: &str) -> Option<Vec<PresetRule>> {
        let first = self.preset_history(slug).into_iter().rev().find(|e| carries_rules(&e.what)).map(|e| e.rules);
        first.or_else(|| SHIPPED.iter().find(|s| s.slug == slug).map(|s| shipped_preset(s).rules))
    }

    /// Go back to the original or to an earlier version. It is written as a new
    /// version, so going back can itself be gone back from.
    pub fn go_back(&self, slug: &str, to: Back) -> Result<Preset, String> {
        let _guard = self.lock.lock().unwrap();
        let before = self.preset(slug).ok_or("I can't find that style.")?;
        let (rules, why) = match to {
            Back::Original => (self.original_rules(slug).ok_or("It has only the one version.")?, "went back to the original".to_string()),
            Back::Entry(seq) => {
                let e = self
                    .preset_history(slug)
                    .into_iter()
                    .find(|e| e.seq == seq && carries_rules(&e.what))
                    .ok_or("I can't find that version.")?;
                (e.rules, format!("went back to version {}", e.version))
            }
        };
        if rules == before.rules {
            return Err("It already works that way.".into());
        }
        self.commit_as(&before, Preset { rules, ..before.clone() }, "you", "back", &why)
    }

    /// A preset that branches off `parent` because it works differently in one
    /// area, with exactly `rules`. Evolution makes these; they start unlocked, and
    /// the parent stays as it is.
    pub fn create_offspring(&self, parent: &str, name: &str, line: &str, rules: Vec<PresetRule>, why: &str) -> Result<Preset, String> {
        let from = self.preset(parent).ok_or("I can't find the preset it branches from.")?;
        let without: Vec<PresetRule> = from.rules.iter().filter(|r| !rules.contains(r)).copied().collect();
        let rules = rules;
        self.make_preset(NewPreset {
            name,
            line,
            from: Some(parent),
            add: &rules,
            without: &without,
            locked: false,
            how: "evolution",
            by: "evolution",
            why,
        })
    }

    /// How many changes I made to how I work on my own, across every preset: a version
    /// evolution wrote, or a preset it branched off. The mark grows a fine layer for each.
    pub fn evolved_versions(&self) -> usize {
        self.presets()
            .iter()
            .map(|p| self.preset_history(&p.slug).iter().filter(|e| e.by == "evolution" && matches!(e.what.as_str(), "version" | "original")).count())
            .sum()
    }

    /// "Bring back into Lean": the offspring's rules join its parent, and the
    /// offspring is set aside. Where it was the default, the parent is now.
    pub fn merge_offspring(&self, slug: &str) -> Result<Preset, String> {
        let child = self.preset(slug).ok_or("I can't find that style.")?;
        if child.from.is_empty() {
            return Err("It didn't branch off anything.".into());
        }
        let parent = {
            let _guard = self.lock.lock().unwrap();
            let before = self.preset(&child.from).ok_or("The preset it branched from is gone.")?;
            let mut rules = before.rules.clone();
            for r in &child.rules {
                if !rules.contains(r) {
                    rules.push(*r);
                }
            }
            let why = format!("brought {} back", child.name);
            self.commit_preset(&before, Preset { rules, ..before.clone() }, "you", &why)?
        };
        for scope in self.defaults_using(slug) {
            let _ = self.set_default(&scope, Some(&parent.slug));
        }
        self.delete_preset(slug)?;
        Ok(parent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::ways::{Move, PresetEdit, Trigger};

    fn store() -> (MemoryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_ways_hist_{}", uuid::Uuid::new_v4().simple()));
        let s = MemoryStore::new(&dir).unwrap();
        s.ensure_ways();
        (s, dir)
    }

    fn rule(when: Trigger, mv: Move) -> PresetRule {
        PresetRule { when, mv }
    }

    #[test]
    fn a_change_starts_a_history_with_what_shipped_and_then_the_change() {
        let (s, dir) = store();
        assert!(s.preset_history("lean").is_empty(), "nothing to say before a change");
        let split = rule(Trigger::ManyChanges, Move::Split);
        s.set_preset_rule("lean", split, false).unwrap();
        let h = s.preset_history("lean");
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].what.as_str(), h[0].version, h[0].by.as_str()), ("version", 2, "you"));
        assert_eq!(h[0].removed, vec![split]);
        assert!(h[0].added.is_empty());
        assert_eq!((h[1].what.as_str(), h[1].version), ("original", 1));
        assert!(dir.join("memory/ways/history/lean/0001.md").exists(), "plain files");
        // The history is not a preset: the menu still lists five.
        assert_eq!(s.presets().len(), 5);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn going_back_to_the_original_is_a_new_version_and_can_be_gone_back_from() {
        let (s, dir) = store();
        let split = rule(Trigger::ManyChanges, Move::Split);
        s.set_preset_rule("lean", split, false).unwrap();
        let back = s.go_back("lean", Back::Original).unwrap();
        assert_eq!(back.version, 3);
        assert!(back.rules.contains(&split));
        assert!(s.go_back("lean", Back::Original).is_err(), "already there");
        // Undo: back to version 2, found by its place in the history.
        let v2 = s.preset_history("lean").into_iter().find(|e| e.version == 2).unwrap();
        let undone = s.go_back("lean", Back::Entry(v2.seq)).unwrap();
        assert_eq!(undone.version, 4);
        assert!(!undone.rules.contains(&split));
        let top = &s.preset_history("lean")[0];
        assert_eq!((top.what.as_str(), top.why.as_str()), ("back", "went back to version 2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_built_in_changed_before_there_was_a_history_still_goes_back_to_what_shipped() {
        let (s, dir) = store();
        // As Phase 4 left it: edited, version 3, no history.
        let mut p = s.preset("lean").unwrap();
        p.rules.truncate(1);
        p.version = 3;
        p.edited = true;
        s.write_preset(&p).unwrap();
        s.set_preset_rule("lean", rule(Trigger::Start, Move::Criteria), true).unwrap();
        let kinds: Vec<_> = s.preset_history("lean").iter().map(|e| e.what.clone()).collect();
        assert_eq!(kinds, vec!["version", "version", "original"]);
        assert_eq!(s.go_back("lean", Back::Original).unwrap().rules.len(), 4);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn locking_is_an_entry_of_its_own_and_not_a_version() {
        let (s, dir) = store();
        s.edit_preset("lean", PresetEdit { locked: Some(true), ..Default::default() }).unwrap();
        s.edit_preset("lean", PresetEdit { locked: Some(true), ..Default::default() }).unwrap();
        let h = s.preset_history("lean");
        assert_eq!(h.iter().filter(|e| e.what == "locked").count(), 1, "saying it twice is one entry");
        assert_eq!(s.preset("lean").unwrap().version, 1);
        s.edit_preset("lean", PresetEdit { locked: Some(false), ..Default::default() }).unwrap();
        assert_eq!(s.preset_history("lean")[0].what, "unlocked");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_custom_preset_remembers_how_it_began_and_what_it_was() {
        let (s, dir) = store();
        let p = s.create_preset("Mine", "x", Some("lean"), &[], true).unwrap();
        assert_eq!((p.how.as_str(), p.from.as_str()), ("preset", "lean"));
        assert!(s.go_back("mine", Back::Original).is_err(), "one version has nothing to go back to");
        s.set_preset_rule("mine", rule(Trigger::Start, Move::Criteria), true).unwrap();
        let back = s.go_back("mine", Back::Original).unwrap();
        assert_eq!(back.rules.len(), 4);
        let chat = s.create_preset_how("Kept", "", Some("lean"), &[], true, "chat").unwrap();
        assert_eq!(chat.lineage(Some("Lean")).as_deref().map(|l| l.starts_with("from the chat")), Some(true));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_offspring_branches_off_unlocked_and_the_parent_stays() {
        let (s, dir) = store();
        let split = rule(Trigger::ManyChanges, Move::Split);
        let all = s.preset("lean").unwrap().rules;
        let kid = s.create_offspring("lean", "Lean for code", "Without splitting", all, "skipped in code").unwrap();
        assert!(!kid.locked && !kid.built_in);
        assert_eq!((kid.from.as_str(), kid.how.as_str(), kid.glyph.as_str()), ("lean", "evolution", "lean"));
        assert_eq!(kid.rules.len(), 4, "it takes the parent's rules along");
        assert_eq!(s.preset("lean").unwrap().version, 1, "the parent is unchanged");
        assert_eq!(s.preset_history(&kid.slug)[0].by, "evolution");
        // One that leaves a rule out.
        let slim = s.create_offspring("lean", "Lean, no split", "", vec![rule(Trigger::Start, Move::OneAtATime)], "").unwrap();
        assert_eq!(slim.rules, vec![rule(Trigger::Start, Move::OneAtATime)]);
        assert!(!slim.rules.contains(&split));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bringing_an_offspring_back_joins_the_parent_and_moves_its_defaults() {
        let (s, dir) = store();
        let kid = s
            .create_offspring("lean", "Lean for code", "", vec![rule(Trigger::Start, Move::OneAtATime), rule(Trigger::AfterItem, Move::CheckIn)], "")
            .unwrap();
        s.set_default("kind:code", Some(&kid.slug)).unwrap();
        let parent = s.merge_offspring(&kid.slug).unwrap();
        assert!(parent.rules.contains(&rule(Trigger::AfterItem, Move::CheckIn)));
        assert_eq!(parent.version, 2);
        assert!(s.preset(&kid.slug).is_none());
        assert_eq!(s.defaults(), vec![("kind:code".to_string(), "lean".to_string())]);
        assert!(s.merge_offspring("lean").is_err(), "a preset that branched off nothing cannot be brought back");
        let _ = std::fs::remove_dir_all(dir);
    }
}
