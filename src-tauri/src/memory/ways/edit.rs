//! `WRK-16`/`WRK-17`: changing ways of working and presets, and choosing which
//! preset a chat, project, persona or kind of task starts with.
//!
//! Everything here writes the same plain files `ensure_ways` and `rules_for`
//! read, so a change made in the Self view is exactly what the next run holds.
//! Nothing here can write a rule the loop cannot carry out: a trigger and a move
//! are the fixed enums, and a scope is parsed, never trusted.

use std::path::PathBuf;

use super::{
    parse_preset, parse_way, render_preset, render_way, sentence, shipped_preset, Move, Preset, PresetRule, Scope, Status, Trial,
    Trigger, Way, ALWAYS, AD_HOC, SHIPPED,
};
use crate::memory::{one_line, slugify, today, MemoryStore};

/// How long a preset's name may be. It is drawn in a chip and a menu row.
const NAME_CAP: usize = 40;
/// The longest one-line description.
const LINE_CAP: usize = 120;
/// The file that says which preset each place starts with.
const DEFAULTS_FILE: &str = "defaults.md";

/// What a person can change in one way of working. `None` leaves it as it is.
#[derive(Debug, Clone, Default)]
pub struct WayEdit {
    pub when: Option<Trigger>,
    pub mv: Option<Move>,
    pub scope: Option<Scope>,
    pub status: Option<Status>,
}

/// What a person can change in a preset. `None` leaves it as it is.
#[derive(Debug, Clone, Default)]
pub struct PresetEdit {
    pub name: Option<String>,
    pub line: Option<String>,
    pub locked: Option<bool>,
}

/// Everything `make_preset` needs, so there is one way to make a preset.
pub(super) struct NewPreset<'a> {
    pub name: &'a str,
    pub line: &'a str,
    pub from: Option<&'a str>,
    pub add: &'a [PresetRule],
    /// Rules of `from` it does not take along.
    pub without: &'a [PresetRule],
    pub locked: bool,
    pub how: &'a str,
    pub by: &'a str,
    pub why: &'a str,
}

/// One preset's file set aside by a delete, as the tab lists it.
#[derive(Debug, Clone)]
pub struct DeletedPreset {
    pub file: String,
    pub name: String,
    /// When it was set aside, in ms.
    pub at: i64,
}

/// A place a preset can be the starting point of.
fn valid_default_scope(scope: &str) -> bool {
    match scope.split_once(':') {
        None => scope == "global",
        Some((kind, id)) => matches!(kind, "project" | "persona" | "kind") && !id.trim().is_empty(),
    }
}

/// A slug that is safe to put in a path: what `slugify` makes, nothing else.
fn safe_slug(slug: &str) -> bool {
    !slug.is_empty() && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The base rules, which keep their words: the loop looks them up by pair.
fn is_base_rule(way: &Way) -> bool {
    way.origin == "shipped" && ALWAYS.iter().any(|&(slug, ..)| slug == way.slug)
}

impl MemoryStore {
    // ---- ways ----

    pub fn way(&self, slug: &str) -> Option<Way> {
        if !safe_slug(slug) {
            return None;
        }
        let path = self.ways_dir().join(format!("{slug}.md"));
        parse_way(slug, &std::fs::read_to_string(path).ok()?)
    }

    fn write_way(&self, way: &Way) -> Result<(), String> {
        std::fs::create_dir_all(self.ways_dir()).map_err(|e| e.to_string())?;
        std::fs::write(self.ways_dir().join(format!("{}.md", way.slug)), render_way(way)).map_err(|e| e.to_string())
    }

    /// A way of your own: it starts as learned, with you as its origin.
    pub fn add_way(&self, when: Trigger, mv: Move, scope: Scope) -> Result<Way, String> {
        self.add_way_from(when, mv, scope, Status::Learned, "you", Vec::new())
    }

    /// A way that came from somewhere else, such as evolution: `Learned` or `Trying`
    /// (with its span), what made it, and the conversations behind it.
    pub fn add_way_from(
        &self,
        when: Trigger,
        mv: Move,
        scope: Scope,
        status: Status,
        origin: &str,
        evidence: Vec<String>,
    ) -> Result<Way, String> {
        let _guard = self.lock.lock().unwrap();
        let taken = self.ways();
        if taken.iter().any(|w| w.when == when && w.mv == mv && w.scope == scope && w.status != Status::Retired) {
            return Err("That rule is already there.".into());
        }
        let base = format!("{}-{}", when.slug(), mv.slug());
        let mut slug = base.clone();
        let mut n = 2;
        while self.ways_dir().join(format!("{slug}.md")).exists() {
            slug = format!("{base}-{n}");
            n += 1;
        }
        let way = Way {
            slug,
            when,
            mv,
            scope,
            status,
            origin: origin.to_string(),
            since: today(),
            evidence,
            trial: (status == Status::Trying).then(Trial::starting_now),
        };
        self.write_way(&way)?;
        self.note_change(&format!("Added: {}", way.sentence()));
        Ok(way)
    }

    /// Settle a way that was being tried out, or give it more time (once asked, the
    /// span runs from now for as long again).
    pub fn extend_trial(&self, slug: &str) -> Result<Way, String> {
        let _guard = self.lock.lock().unwrap();
        let mut way = self.way(slug).ok_or("I can't find that rule.")?;
        if way.status != Status::Trying {
            return Err("That way is not being tried out.".into());
        }
        way.trial = Some(Trial { asked: true, ..way.trial.unwrap_or_else(Trial::starting_now) });
        self.write_way(&way)?;
        Ok(way)
    }

    /// Change one way. The base rules can be switched off and back on, and
    /// narrowed, but keep their words.
    pub fn update_way(&self, slug: &str, edit: WayEdit) -> Result<Way, String> {
        let _guard = self.lock.lock().unwrap();
        let mut way = self.way(slug).ok_or("I can't find that rule.")?;
        if is_base_rule(&way) && (edit.when.is_some_and(|w| w != way.when) || edit.mv.is_some_and(|m| m != way.mv)) {
            return Err("The base rules keep their words. You can switch one off.".into());
        }
        if let Some(when) = edit.when {
            way.when = when;
        }
        if let Some(mv) = edit.mv {
            way.mv = mv;
        }
        if let Some(scope) = edit.scope {
            way.scope = scope;
        }
        if let Some(status) = edit.status {
            way.status = match status {
                // Bringing a base rule back makes it a base rule again, not a learned one.
                Status::Learned | Status::Always if is_base_rule(&way) => Status::Always,
                Status::Always => Status::Learned,
                other => other,
            };
            if is_base_rule(&way) && way.status == Status::Trying {
                return Err("A base rule is either on or off.".into());
            }
            // Trying out has a span that begins when it is chosen, and ends with it.
            way.trial = match (way.status, way.trial) {
                (Status::Trying, None) => Some(Trial::starting_now()),
                (Status::Trying, kept) => kept,
                _ => None,
            };
        }
        // Two rules with one meaning would fire once and read twice.
        let same = |w: &Way| w.slug != way.slug && w.when == way.when && w.mv == way.mv && w.scope == way.scope;
        if way.status != Status::Retired && self.ways().iter().any(|w| w.status != Status::Retired && same(w)) {
            return Err("That rule is already there.".into());
        }
        self.write_way(&way)?;
        let verb = match way.status {
            Status::Retired => "Switched off",
            Status::Trying => "Trying",
            _ => "Changed",
        };
        self.note_change(&format!("{verb}: {}", way.sentence()));
        Ok(way)
    }

    // ---- presets ----

    pub(super) fn write_preset(&self, p: &Preset) -> Result<(), String> {
        std::fs::create_dir_all(self.presets_dir()).map_err(|e| e.to_string())?;
        std::fs::write(self.presets_dir().join(format!("{}.md", p.slug)), render_preset(p)).map_err(|e| e.to_string())
    }

    /// What a built-in preset ships with: the rules "Back to original" will one day
    /// bring back, and what a person who switched one off can switch on again.
    pub fn shipped_rules(&self, slug: &str) -> Vec<PresetRule> {
        SHIPPED.iter().find(|s| s.slug == slug).map(|s| shipped_preset(s).rules).unwrap_or_default()
    }

    /// A new preset of your own. It starts from `from` (its rules and its glyph)
    /// or from nothing, with `add` on top, and starts locked unless asked.
    pub fn create_preset(
        &self,
        name: &str,
        line: &str,
        from: Option<&str>,
        add: &[PresetRule],
        locked: bool,
    ) -> Result<Preset, String> {
        let how = if from.is_some_and(|f| !f.trim().is_empty()) { "preset" } else { "" };
        self.create_preset_how(name, line, from, add, locked, how)
    }

    /// The same, saying how it came to be: `preset`, or `chat` for what a run kept.
    pub fn create_preset_how(
        &self,
        name: &str,
        line: &str,
        from: Option<&str>,
        add: &[PresetRule],
        locked: bool,
        how: &str,
    ) -> Result<Preset, String> {
        self.make_preset(NewPreset { name, line, from, add, without: &[], locked, how, by: "you", why: "" })
    }

    /// The one place a preset is made, so a name is never taken twice and every
    /// preset's history starts with how it began.
    pub(super) fn make_preset(&self, new: NewPreset<'_>) -> Result<Preset, String> {
        let name = one_line(new.name.trim());
        if name.is_empty() {
            return Err("Give it a name.".into());
        }
        if name.chars().count() > NAME_CAP {
            return Err(format!("Keep the name under {NAME_CAP} letters."));
        }
        let line: String = one_line(new.line.trim()).chars().take(LINE_CAP).collect();
        let _guard = self.lock.lock().unwrap();
        let all = self.presets();
        if all.iter().any(|p| p.name.to_lowercase() == name.to_lowercase()) {
            return Err(format!("There is already a style called \u{201c}{name}\u{201d}."));
        }
        let parent = match new.from.map(str::trim).filter(|f| !f.is_empty()) {
            Some(f) => Some(all.iter().find(|p| p.slug == f).cloned().ok_or("I can't find the preset to start from.")?),
            None => None,
        };
        let base = slugify(&name).unwrap_or_else(|_| "preset".into());
        let mut slug = base.clone();
        let mut n = 2;
        while SHIPPED.iter().any(|s| s.slug == slug) || self.presets_dir().join(format!("{slug}.md")).exists() {
            slug = format!("{base}-{n}");
            n += 1;
        }
        let mut rules: Vec<PresetRule> =
            parent.as_ref().map(|p| p.rules.iter().filter(|r| !new.without.contains(r)).copied().collect()).unwrap_or_default();
        for r in new.add {
            if !rules.contains(r) {
                rules.push(*r);
            }
        }
        let preset = Preset {
            slug,
            name,
            line,
            glyph: parent.as_ref().map(|p| p.glyph.clone()).unwrap_or_else(|| AD_HOC.to_string()),
            built_in: false,
            locked: new.locked,
            version: 1,
            edited: true,
            rules,
            from: parent.as_ref().map(|p| p.slug.clone()).unwrap_or_default(),
            born: today(),
            how: new.how.to_string(),
        };
        self.write_preset(&preset)?;
        self.log_preset(&preset, new.by, "original", new.why);
        self.note_change(&format!("Made the style “{}”", preset.name));
        Ok(preset)
    }

    /// Rename it, change its line, lock or unlock it. A built-in keeps its name and line.
    pub fn edit_preset(&self, slug: &str, edit: PresetEdit) -> Result<Preset, String> {
        let _guard = self.lock.lock().unwrap();
        let before = self.preset(slug).ok_or("I can't find that style.")?;
        let mut p = before.clone();
        if p.slug == AD_HOC && edit.locked.is_some() {
            return Err("Ad hoc has nothing to evolve.".into());
        }
        let renamed = edit.name.is_some() || edit.line.is_some();
        if p.built_in && renamed {
            return Err("A built-in style keeps its name. Make your own from it to change it.".into());
        }
        let mut changed = false;
        if let Some(name) = edit.name {
            let name = one_line(name.trim());
            if name.is_empty() {
                return Err("Give it a name.".into());
            }
            if name.chars().count() > NAME_CAP {
                return Err(format!("Keep the name under {NAME_CAP} letters."));
            }
            if self.presets().iter().any(|o| o.slug != p.slug && o.name.to_lowercase() == name.to_lowercase()) {
                return Err(format!("There is already a style called \u{201c}{name}\u{201d}."));
            }
            changed |= p.name != name;
            p.name = name;
        }
        if let Some(line) = edit.line {
            let line: String = one_line(line.trim()).chars().take(LINE_CAP).collect();
            changed |= p.line != line;
            p.line = line;
        }
        let lock_changed = edit.locked.is_some_and(|l| l != before.locked);
        if let Some(locked) = edit.locked {
            p.locked = locked;
        }
        if changed {
            return self.commit_preset(&before, p, "you", "renamed it");
        }
        self.write_preset(&p)?;
        if lock_changed {
            self.ensure_baseline(&before);
            self.log_preset(&p, "you", if p.locked { "locked" } else { "unlocked" }, "");
            self.note_change(&format!("{} {}", if p.locked { "Locked" } else { "Unlocked" }, p.name));
        }
        Ok(p)
    }

    /// Switch one rule of a preset on or off. That is a change to the preset
    /// itself: a new version, and a built-in's file is the person's from then on.
    pub fn set_preset_rule(&self, slug: &str, rule: PresetRule, on: bool) -> Result<Preset, String> {
        self.set_preset_rule_by(slug, rule, on, "you", "")
    }

    /// The same, by whoever made the change (`you` or `evolution`) and why.
    pub fn set_preset_rule_by(&self, slug: &str, rule: PresetRule, on: bool, by: &str, why: &str) -> Result<Preset, String> {
        let _guard = self.lock.lock().unwrap();
        let before = self.preset(slug).ok_or("I can't find that style.")?;
        if before.slug == AD_HOC {
            return Err("Ad hoc has no rules of its own. Make a style of your own to add some.".into());
        }
        let has = before.rules.contains(&rule);
        if on == has {
            return Ok(before);
        }
        let mut p = before.clone();
        if on {
            p.rules.push(rule);
        } else {
            p.rules.retain(|r| *r != rule);
        }
        let why = if why.is_empty() {
            format!("{} {}", if on { "added" } else { "took away" }, sentence(rule.when, rule.mv))
        } else {
            why.to_string()
        };
        self.commit_preset(&before, p, by, &why)
    }

    /// Set a custom preset aside. A built-in cannot be deleted. Returns the file
    /// name `restore_preset` takes back.
    pub fn delete_preset(&self, slug: &str) -> Result<String, String> {
        let _guard = self.lock.lock().unwrap();
        let p = self.preset(slug).ok_or("I can't find that style.")?;
        if p.built_in {
            return Err("A built-in style can't be deleted.".into());
        }
        let trash = self.dir().join(".trash");
        std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
        let file = format!("{}-preset-{}.md", chrono::Utc::now().timestamp_millis(), p.slug);
        std::fs::rename(self.presets_dir().join(format!("{}.md", p.slug)), trash.join(&file)).map_err(|e| e.to_string())?;
        self.note_change(&format!("Deleted {}", p.name));
        Ok(file)
    }

    /// Every custom preset set aside, newest first.
    pub fn deleted_presets(&self) -> Vec<DeletedPreset> {
        let Ok(entries) = std::fs::read_dir(self.dir().join(".trash")) else { return Vec::new() };
        let mut out: Vec<DeletedPreset> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().to_string();
                let (at, slug) = preset_trash_name(&file)?;
                let preset = parse_preset(&slug, &std::fs::read_to_string(e.path()).ok()?)?;
                Some(DeletedPreset { file, name: preset.name, at })
            })
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at));
        out
    }

    /// Take a deleted preset back. Never over a preset that has the same file name
    /// now, and never under a name another preset has taken since.
    pub fn restore_preset(&self, file: &str) -> Result<Preset, String> {
        let _guard = self.lock.lock().unwrap();
        let (_, slug) = preset_trash_name(file).ok_or("That is not a deleted style.")?;
        let src: PathBuf = self.dir().join(".trash").join(file);
        let preset = parse_preset(&slug, &std::fs::read_to_string(&src).map_err(|e| e.to_string())?)
            .ok_or("I can't read that file any more.")?;
        let dest = self.presets_dir().join(format!("{slug}.md"));
        if dest.exists() || self.presets().iter().any(|p| p.name.to_lowercase() == preset.name.to_lowercase()) {
            return Err(format!(
                "There is a style called \u{201c}{}\u{201d} again. Rename or delete it first.",
                preset.name
            ));
        }
        std::fs::create_dir_all(self.presets_dir()).map_err(|e| e.to_string())?;
        std::fs::rename(&src, dest).map_err(|e| e.to_string())?;
        self.note_change(&format!("Brought back {}", preset.name));
        Ok(preset)
    }

    // ---- defaults ----

    /// Which preset each place starts with, as `(scope, preset slug)`.
    pub fn defaults(&self) -> Vec<(String, String)> {
        let text = std::fs::read_to_string(self.ways_dir().join(DEFAULTS_FILE)).unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let (scope, slug) = l.split_once("->")?;
                let (scope, slug) = (scope.trim(), slug.trim());
                (valid_default_scope(scope) && !slug.is_empty()).then(|| (scope.to_string(), slug.to_string()))
            })
            .collect()
    }

    fn write_defaults(&self, list: &[(String, String)]) -> Result<(), String> {
        std::fs::create_dir_all(self.ways_dir()).map_err(|e| e.to_string())?;
        let mut out = String::from("---\nname: Where each way of working starts\n---\n");
        for (scope, slug) in list {
            out.push_str(&format!("{scope} -> {slug}\n"));
        }
        std::fs::write(self.ways_dir().join(DEFAULTS_FILE), out).map_err(|e| e.to_string())
    }

    /// Say which preset a place starts with, or (`None`) take its default away.
    /// Ad hoc is a choice like the others: it lets a project opt out of the global one.
    pub fn set_default(&self, scope: &str, preset: Option<&str>) -> Result<(), String> {
        let scope = scope.trim();
        if !valid_default_scope(scope) {
            return Err("I don't know where that is.".into());
        }
        let _guard = self.lock.lock().unwrap();
        let mut list: Vec<(String, String)> = self.defaults().into_iter().filter(|(s, _)| s != scope).collect();
        if let Some(typed) = preset.map(str::trim).filter(|p| !p.is_empty()) {
            let found = self.find_preset(typed).ok_or_else(|| format!("I don't have a style called \u{201c}{typed}\u{201d}."))?;
            list.push((scope.to_string(), found.slug));
        }
        self.write_defaults(&list)?;
        self.note_change("Changed where chats start");
        Ok(())
    }

    /// The preset a place starts with when nobody chose one for the chat. The most
    /// particular place wins: the kind of task, then the persona, then the project,
    /// then everywhere. A default whose preset is gone is no default.
    pub fn default_for(&self, ctx: &super::ScopeCtx<'_>) -> Option<Preset> {
        let list = self.defaults();
        let places = [
            ctx.kind.map(|k| format!("kind:{k}")),
            ctx.persona.map(|p| format!("persona:{p}")),
            ctx.project.map(|p| format!("project:{p}")),
            Some("global".to_string()),
        ];
        places
            .into_iter()
            .flatten()
            .find_map(|scope| list.iter().find(|(s, _)| *s == scope).and_then(|(_, slug)| self.preset(slug)))
    }

    /// Where a preset is the default, as `(scope, preset)` pairs.
    pub fn defaults_using(&self, slug: &str) -> Vec<String> {
        self.defaults().into_iter().filter(|(_, s)| s == slug).map(|(scope, _)| scope).collect()
    }

    /// Forget every default that names a preset that is gone.
    pub fn drop_defaults_for(&self, slug: &str) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        let kept: Vec<(String, String)> = self.defaults().into_iter().filter(|(_, s)| s != slug).collect();
        self.write_defaults(&kept)
    }

    // ---- the story ----

    /// The sentence at the top of the tab, in the first person. What a person
    /// can say truthfully from the files alone: how many times the organization
    /// was changed and since when. What evolution did lately comes with `WRK-19`.
    pub fn ways_narrative(&self) -> String {
        let mut changes = 0usize;
        let mut since: Option<String> = None;
        let mut note = |date: &str| {
            if date.len() >= 7 && since.as_deref().is_none_or(|s| date < s) {
                since = Some(date.to_string());
            }
        };
        for w in self.ways() {
            let own = w.origin != "shipped";
            let switched_off = w.origin == "shipped" && w.status == Status::Retired;
            if own || switched_off {
                changes += 1;
                note(&w.since);
            }
        }
        for p in self.presets() {
            if p.version > 1 || (!p.built_in) {
                changes += (p.version as usize).saturating_sub(1).max(usize::from(!p.built_in));
                let modified = std::fs::metadata(self.presets_dir().join(format!("{}.md", p.slug)))
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).format("%Y-%m-%d").to_string());
                if let Some(d) = modified {
                    note(&d);
                }
            }
        }
        if changes == 0 {
            return "I work the way I came. Nothing has changed yet.".into();
        }
        let times = if changes == 1 { "once".to_string() } else { format!("{changes} times") };
        match since.as_deref().and_then(month_name) {
            Some(m) => format!("I've changed how I work {times} since {m}."),
            None => format!("I've changed how I work {times}."),
        }
    }
}

fn month_name(date: &str) -> Option<&'static str> {
    const MONTHS: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
        "November", "December",
    ];
    let m: usize = date.get(5..7)?.parse().ok()?;
    MONTHS.get(m.checked_sub(1)?).copied()
}

/// `<ms>-preset-<slug>.md` → `(ms, slug)`.
fn preset_trash_name(file: &str) -> Option<(i64, String)> {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return None;
    }
    let stem = file.strip_suffix(".md")?;
    let (ms, rest) = stem.split_once("-preset-")?;
    let slug = rest.to_string();
    safe_slug(&slug).then_some(())?;
    Some((ms.parse().ok()?, slug))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::ways::ScopeCtx;

    fn store() -> (MemoryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_ways_edit_{}", uuid::Uuid::new_v4().simple()));
        let s = MemoryStore::new(&dir).unwrap();
        s.ensure_ways();
        (s, dir)
    }

    fn rule(when: Trigger, mv: Move) -> PresetRule {
        PresetRule { when, mv }
    }

    #[test]
    fn a_way_of_your_own_is_added_once_and_reads_back() {
        let (s, dir) = store();
        let w = s.add_way(Trigger::BeforeAnswer, Move::SecondLook, Scope::Project("nexus".into())).unwrap();
        assert_eq!((w.status, w.origin.as_str()), (Status::Learned, "you"));
        assert!(s.ways().iter().any(|x| x.slug == w.slug));
        assert!(s.add_way(Trigger::BeforeAnswer, Move::SecondLook, Scope::Project("nexus".into())).is_err());
        // The same pair somewhere else is another rule.
        assert!(s.add_way(Trigger::BeforeAnswer, Move::SecondLook, Scope::Everywhere).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_base_rule_can_be_switched_off_and_back_but_keeps_its_words() {
        let (s, dir) = store();
        let off = s.update_way("stop-the-line", WayEdit { status: Some(Status::Retired), ..Default::default() }).unwrap();
        assert_eq!(off.status, Status::Retired);
        // Off means off: the built-in fallback does not bring it back.
        assert!(s.rules_for(AD_HOC, &ScopeCtx::default()).iter().all(|r| r.mv != Move::StopTheLine));
        let on = s.update_way("stop-the-line", WayEdit { status: Some(Status::Learned), ..Default::default() }).unwrap();
        assert_eq!(on.status, Status::Always, "back as a base rule, not a learned one");
        assert!(s.rules_for(AD_HOC, &ScopeCtx::default()).iter().any(|r| r.mv == Move::StopTheLine));
        assert!(s.update_way("stop-the-line", WayEdit { mv: Some(Move::Split), ..Default::default() }).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn retiring_and_bringing_back_a_learned_way_keeps_it() {
        let (s, dir) = store();
        let w = s.add_way(Trigger::Start, Move::OneAtATime, Scope::Everywhere).unwrap();
        let gone = s.update_way(&w.slug, WayEdit { status: Some(Status::Retired), ..Default::default() }).unwrap();
        assert_eq!(gone.status, Status::Retired);
        assert!(s.rules_for(AD_HOC, &ScopeCtx::default()).iter().all(|r| r.id != w.slug));
        let back = s.update_way(&w.slug, WayEdit { status: Some(Status::Learned), ..Default::default() }).unwrap();
        assert_eq!(back.status, Status::Learned);
        // A path-like slug never reaches the disk.
        assert!(s.update_way("../x", WayEdit::default()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_preset_made_from_another_has_its_rules_and_its_glyph_and_starts_locked() {
        let (s, dir) = store();
        let p = s
            .create_preset("Careful with mail", "I ask before anything leaves the house", Some("lean"), &[rule(Trigger::RiskyAction, Move::Advice)], true)
            .unwrap();
        assert_eq!(p.slug, "careful-with-mail");
        assert_eq!(p.glyph, "lean");
        assert!(p.locked && !p.built_in);
        assert_eq!(p.rules.len(), 5, "Lean's four and the one added");
        assert_eq!(s.preset("careful-with-mail"), Some(p));
        assert!(dir.join("memory/ways/presets/careful-with-mail.md").exists(), "a plain file you own");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_name_that_is_taken_is_refused_whatever_its_case() {
        let (s, dir) = store();
        assert!(s.create_preset("LEAN", "", None, &[], true).is_err());
        assert!(s.create_preset("  ", "", None, &[], true).is_err());
        let a = s.create_preset("Mine", "", None, &[], true).unwrap();
        assert!(s.edit_preset(&a.slug, PresetEdit { name: Some("lean".into()), ..Default::default() }).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn switching_a_rule_changes_the_preset_and_makes_a_built_in_the_persons() {
        let (s, dir) = store();
        let before = s.preset("lean").unwrap();
        let split = rule(Trigger::ManyChanges, Move::Split);
        let off = s.set_preset_rule("lean", split, false).unwrap();
        assert_eq!((off.version, off.edited), (before.version + 1, true));
        assert!(!off.rules.contains(&split));
        // A release does not put it back, because the file is theirs now.
        s.ensure_ways();
        assert!(!s.preset("lean").unwrap().rules.contains(&split));
        assert!(s.shipped_rules("lean").contains(&split), "what was switched off can be seen against the original");
        let on = s.set_preset_rule("lean", split, true).unwrap();
        assert!(on.rules.contains(&split));
        assert!(s.set_preset_rule(AD_HOC, split, true).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn locking_a_built_in_is_not_a_change_to_it() {
        let (s, dir) = store();
        let p = s.edit_preset("lean", PresetEdit { locked: Some(true), ..Default::default() }).unwrap();
        assert!(p.locked && !p.edited && p.version == 1);
        assert!(s.edit_preset("lean", PresetEdit { name: Some("Slim".into()), ..Default::default() }).is_err());
        assert!(s.edit_preset(AD_HOC, PresetEdit { locked: Some(true), ..Default::default() }).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_deleted_preset_is_set_aside_and_can_come_back() {
        let (s, dir) = store();
        let p = s.create_preset("Mine", "mine", Some("swarm"), &[], false).unwrap();
        assert!(s.delete_preset("lean").is_err(), "a built-in stays");
        let file = s.delete_preset(&p.slug).unwrap();
        assert!(s.preset("mine").is_none());
        let kept = s.deleted_presets();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "Mine");
        let back = s.restore_preset(&file).unwrap();
        assert_eq!(back, p);
        assert!(s.deleted_presets().is_empty());
        assert!(s.restore_preset("../../x.md").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_deleted_preset_does_not_come_back_over_one_that_has_its_name() {
        let (s, dir) = store();
        let p = s.create_preset("Mine", "", None, &[], true).unwrap();
        let file = s.delete_preset(&p.slug).unwrap();
        s.create_preset("Mine", "newer", None, &[], true).unwrap();
        assert!(s.restore_preset(&file).is_err());
        assert_eq!(s.preset("mine").unwrap().line, "newer", "the newer one is untouched");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_most_particular_default_wins_and_a_gone_preset_is_no_default() {
        let (s, dir) = store();
        s.set_default("global", Some("lean")).unwrap();
        s.set_default("project:nexus", Some("swarm")).unwrap();
        s.set_default("persona:assistant", Some("Mission Command")).unwrap();
        s.set_default("kind:mail", Some("command-and-control")).unwrap();
        let slug = |ctx: ScopeCtx<'_>| s.default_for(&ctx).map(|p| p.slug);
        assert_eq!(slug(ScopeCtx::default()), Some("lean".into()));
        assert_eq!(slug(ScopeCtx { project: Some("nexus"), ..Default::default() }), Some("swarm".into()));
        assert_eq!(
            slug(ScopeCtx { project: Some("nexus"), persona: Some("assistant"), ..Default::default() }),
            Some("mission-command".into())
        );
        assert_eq!(
            slug(ScopeCtx { project: Some("nexus"), persona: Some("assistant"), kind: Some("mail") }),
            Some("command-and-control".into())
        );
        // Ad hoc is a choice: a project can say no to the global one.
        s.set_default("project:nexus", Some("ad hoc")).unwrap();
        assert_eq!(slug(ScopeCtx { project: Some("nexus"), ..Default::default() }), Some("ad-hoc".into()));
        s.set_default("project:nexus", None).unwrap();
        assert_eq!(slug(ScopeCtx { project: Some("nexus"), ..Default::default() }), Some("lean".into()));
        assert!(s.set_default("nowhere", Some("lean")).is_err());
        assert!(s.set_default("global", Some("no such")).is_err());
        let own = s.create_preset("Mine", "", None, &[], true).unwrap();
        s.set_default("project:x", Some(&own.slug)).unwrap();
        assert_eq!(s.defaults_using("mine"), vec!["project:x".to_string()]);
        s.delete_preset("mine").unwrap();
        assert_eq!(slug(ScopeCtx { project: Some("x"), ..Default::default() }), Some("lean".into()));
        s.drop_defaults_for("mine").unwrap();
        assert!(s.defaults_using("mine").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_story_counts_what_changed_and_says_nothing_when_nothing_did() {
        let (s, dir) = store();
        assert_eq!(s.ways_narrative(), "I work the way I came. Nothing has changed yet.");
        s.add_way(Trigger::Start, Move::OneAtATime, Scope::Everywhere).unwrap();
        s.set_preset_rule("lean", rule(Trigger::ManyChanges, Move::Split), false).unwrap();
        let story = s.ways_narrative();
        assert!(story.starts_with("I've changed how I work 2 times"), "{story}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
