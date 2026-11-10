//! `WRK-21`: every change to how I work leaves a copy of how I worked after it, so a
//! person can go back to a day, and so the Self view can show the organization growing.
//!
//! ```text
//! memory/.snapshots/ways/<ms>/    the rule files, defaults.md and the presets as they were
//!    label.txt                    "Added: When I am about to call an item done, I get a second look."
//! ```
//!
//! The history of each preset (`history/`) is not copied: it is the record, and going
//! back adds to it. Going back writes presets through their own history, as new
//! versions, and puts the rule files and the defaults back as they were. What was
//! made after that day (a preset of your own) stays, and a deleted preset stays in the
//! trash. The set as it is now is copied first, so going back can be undone.

use std::path::{Path, PathBuf};

use super::{parse_preset, MemoryStore, Preset};

/// How many copies are kept. Older ones are let go.
const KEEP: usize = 60;

/// One point to go back to.
#[derive(Debug, Clone)]
pub struct Moment {
    /// The folder name, in ms.
    pub id: String,
    pub at: i64,
    pub label: String,
}

fn md_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
        .collect()
}

impl MemoryStore {
    fn rings_dir(&self) -> PathBuf {
        self.dir().join(".snapshots").join("ways")
    }

    /// Copy how I work now, and say what just happened. Best effort: a change that
    /// could not be copied still happened.
    pub(super) fn note_change(&self, label: &str) {
        let root = self.rings_dir();
        let mut ms = chrono::Utc::now().timestamp_millis();
        while root.join(ms.to_string()).exists() {
            ms += 1;
        }
        let dest = root.join(ms.to_string());
        if std::fs::create_dir_all(dest.join("presets")).is_err() {
            return;
        }
        for f in md_files(&self.ways_dir()) {
            if let Some(name) = f.file_name() {
                let _ = std::fs::copy(&f, dest.join(name));
            }
        }
        for f in md_files(&self.presets_dir()) {
            if let Some(name) = f.file_name() {
                let _ = std::fs::copy(&f, dest.join("presets").join(name));
            }
        }
        let _ = std::fs::write(dest.join("label.txt"), label.replace(['\r', '\n'], " "));
        let mut all = self.moments();
        if all.len() > KEEP {
            for old in all.drain(KEEP..) {
                let _ = std::fs::remove_dir_all(root.join(old.id));
            }
        }
    }

    /// The first copy: how I came. Only when there is none yet.
    pub(super) fn note_start(&self) {
        if !self.rings_dir().exists() {
            self.note_change("The way I came");
        }
    }

    /// Every point to go back to, newest first.
    pub fn moments(&self) -> Vec<Moment> {
        let mut out: Vec<Moment> = std::fs::read_dir(self.rings_dir())
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().to_string();
                let at: i64 = id.parse().ok()?;
                let label = std::fs::read_to_string(e.path().join("label.txt")).unwrap_or_default();
                Some(Moment { id, at, label })
            })
            .collect();
        out.sort_by_key(|m| std::cmp::Reverse(m.at));
        out
    }

    /// Go back to how I worked at `moment`. `date` is how the person reads it, for
    /// the history lines.
    pub fn go_back_to(&self, moment: &str, date: &str) -> Result<(), String> {
        if moment.is_empty() || !moment.chars().all(|c| c.is_ascii_digit()) {
            return Err("I can't find that day.".into());
        }
        let src = self.rings_dir().join(moment);
        if !src.is_dir() {
            return Err("I can't find that day.".into());
        }
        // The set as it is now, first, so this can be undone.
        self.note_change(&format!("Before I went back to {date}"));
        {
            let _guard = self.lock.lock().unwrap();
            for f in md_files(&self.ways_dir()) {
                let _ = std::fs::remove_file(f);
            }
            for f in md_files(&src) {
                if let Some(name) = f.file_name() {
                    std::fs::copy(&f, self.ways_dir().join(name)).map_err(|e| e.to_string())?;
                }
            }
        }
        // Presets go back through their own history, as new versions.
        let why = format!("went back to how I worked on {date}");
        for file in md_files(&src.join("presets")) {
            let Some(slug) = file.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
            let Some(then) = std::fs::read_to_string(&file).ok().and_then(|t| parse_preset(&slug, &t)) else { continue };
            let Some(now) = self.preset(&slug) else { continue };
            let _guard = self.lock.lock().unwrap();
            if now.rules != then.rules {
                self.commit_quiet(&now, Preset { rules: then.rules.clone(), ..now.clone() }, "you", "back", &why)?;
            }
            if now.locked != then.locked {
                let mut p = self.preset(&slug).unwrap_or(now);
                p.locked = then.locked;
                self.write_preset(&p)?;
                self.log_preset(&p, "you", if p.locked { "locked" } else { "unlocked" }, "");
            }
        }
        self.note_change(&format!("Went back to how I worked on {date}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Move, PresetRule, Scope, Status, Trigger, WayEdit};
    use super::*;

    fn store() -> (MemoryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_ways_rings_{}", uuid::Uuid::new_v4().simple()));
        let s = MemoryStore::new(&dir).unwrap();
        s.ensure_ways();
        (s, dir)
    }

    #[test]
    fn the_first_moment_is_how_i_came_and_each_change_adds_one() {
        let (s, dir) = store();
        assert_eq!(s.moments().iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), vec!["The way I came"]);
        let w = s.add_way(Trigger::BeforeDone, Move::SecondLook, Scope::Kind("mail".into())).unwrap();
        s.update_way(&w.slug, WayEdit { status: Some(Status::Retired), ..Default::default() }).unwrap();
        s.set_default("global", Some("lean")).unwrap();
        let labels: Vec<String> = s.moments().into_iter().map(|m| m.label).collect();
        assert_eq!(labels.len(), 4);
        assert!(labels[2].starts_with("Added:") && labels[1].starts_with("Switched off:"), "{labels:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn going_back_restores_the_ways_the_defaults_and_the_presets_and_can_be_undone() {
        let (s, dir) = store();
        let start = s.moments().last().unwrap().id.clone();
        let w = s.add_way(Trigger::BeforeDone, Move::SecondLook, Scope::Everywhere).unwrap();
        s.set_default("global", Some("lean")).unwrap();
        let split = PresetRule { when: Trigger::ManyChanges, mv: Move::Split };
        s.set_preset_rule("lean", split, false).unwrap();
        let mine = s.create_preset("Mine", "", None, &[], true).unwrap();

        s.go_back_to(&start, "Nov 7").unwrap();
        assert!(s.way(&w.slug).is_none(), "the way did not exist then");
        assert!(s.defaults().is_empty());
        let lean = s.preset("lean").unwrap();
        assert!(lean.rules.contains(&split), "the rule is back");
        assert_eq!(lean.version, 3, "as a new version, so the history still says what happened");
        assert_eq!(s.preset_history("lean")[0].why, "went back to how I worked on Nov 7");
        assert!(s.preset(&mine.slug).is_some(), "what was made later stays");
        assert_eq!(s.moments()[0].label, "Went back to how I worked on Nov 7");

        // The copy taken before going back is the way to undo it.
        let before = s.moments().into_iter().find(|m| m.label.starts_with("Before I went back")).unwrap();
        s.go_back_to(&before.id, "Nov 7").unwrap();
        assert!(s.way(&w.slug).is_some());
        assert_eq!(s.defaults(), vec![("global".to_string(), "lean".to_string())]);
        assert!(!s.preset("lean").unwrap().rules.contains(&split));
        assert!(s.go_back_to("../x", "Nov 7").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_the_latest_copies_are_kept() {
        let (s, dir) = store();
        for _ in 0..70 {
            s.note_change("x");
        }
        assert_eq!(s.moments().len(), KEEP);
        let _ = std::fs::remove_dir_all(dir);
    }
}
