//! `WRK-1`/`WRK-3`: ways of working, as plain files like facts and lessons.
//!
//! A way of working is one sentence: **when** something happens, **I do**
//! something. Both halves come from fixed lists (`Trigger`, `Move`), so nothing
//! can be written that the loop cannot do, and every rule stays readable,
//! switchable and undoable. A preset is a named bundle of such rules.
//!
//! ```text
//! memory/
//! └─ ways/
//!    ├─ stop-the-line.md      an Always rule
//!    ├─ ask-on-repeat.md
//!    └─ presets/
//!       ├─ ad-hoc.md          a preset: name, one line, and its rules
//!       └─ lean.md
//! ```
//!
//! The person owns these files, like the rest of `memory/`. A built-in preset's
//! file is rewritten when a new Poiesis ships a newer original, but only while it
//! still says `edited: no`: the first time a person (or, later, evolution)
//! changes it, the file is theirs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{one_line, today, MemoryStore};

mod edit;
mod history;
mod rings;
pub use edit::{DeletedPreset, PresetEdit, WayEdit};
pub use history::{Back, HistoryEntry};
pub use rings::Moment;


pub const WAYS_DIR: &str = "ways";
pub const PRESETS_DIR: &str = "presets";
/// The preset a chat has when nobody chose one.
pub const AD_HOC: &str = "ad-hoc";

/// Something the loop can observe. The wording is what the person reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    Start,
    Unclear,
    AfterItem,
    BeforeDone,
    BeforeAnswer,
    CheckFails,
    SameProblemTwice,
    ItemFailedTwice,
    HeadFull,
    ManyChanges,
    RiskyAction,
    SmallModel,
}

impl Trigger {
    pub const ALL: [Trigger; 12] = [
        Trigger::Start,
        Trigger::Unclear,
        Trigger::AfterItem,
        Trigger::BeforeDone,
        Trigger::BeforeAnswer,
        Trigger::CheckFails,
        Trigger::SameProblemTwice,
        Trigger::ItemFailedTwice,
        Trigger::HeadFull,
        Trigger::ManyChanges,
        Trigger::RiskyAction,
        Trigger::SmallModel,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Trigger::Start => "start",
            Trigger::Unclear => "unclear",
            Trigger::AfterItem => "after-item",
            Trigger::BeforeDone => "before-done",
            Trigger::BeforeAnswer => "before-answer",
            Trigger::CheckFails => "check-fails",
            Trigger::SameProblemTwice => "same-problem-twice",
            Trigger::ItemFailedTwice => "item-failed-twice",
            Trigger::HeadFull => "head-full",
            Trigger::ManyChanges => "many-changes",
            Trigger::RiskyAction => "risky-action",
            Trigger::SmallModel => "small-model",
        }
    }

    pub fn parse(s: &str) -> Option<Trigger> {
        let s = s.trim().to_ascii_lowercase();
        Trigger::ALL.into_iter().find(|t| t.slug() == s)
    }

    /// Finishes "When …".
    pub fn words(self) -> &'static str {
        match self {
            Trigger::Start => "I start",
            Trigger::Unclear => "it's unclear what you want",
            Trigger::AfterItem => "I finish an item",
            Trigger::BeforeDone => "I'm about to call an item done",
            Trigger::BeforeAnswer => "I'm about to answer",
            Trigger::CheckFails => "a check fails",
            Trigger::SameProblemTwice => "the same problem comes twice",
            Trigger::ItemFailedTwice => "an item has failed twice",
            Trigger::HeadFull => "my head is getting full",
            Trigger::ManyChanges => "I've changed many files",
            Trigger::RiskyAction => "I'm about to delete, send or change something in an app",
            Trigger::SmallModel => "I run on a small model",
        }
    }
}

/// What the loop can do about a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Move {
    Criteria,
    LookFirst,
    OneAtATime,
    Split,
    SecondLook,
    StopTheLine,
    TwoWays,
    FreshStart,
    Advice,
    CheckIn,
    HandOff,
    Parallel,
}

impl Move {
    pub const ALL: [Move; 12] = [
        Move::Criteria,
        Move::LookFirst,
        Move::OneAtATime,
        Move::Split,
        Move::SecondLook,
        Move::StopTheLine,
        Move::TwoWays,
        Move::FreshStart,
        Move::Advice,
        Move::CheckIn,
        Move::HandOff,
        Move::Parallel,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Move::Criteria => "criteria",
            Move::LookFirst => "look-first",
            Move::OneAtATime => "one-at-a-time",
            Move::Split => "split",
            Move::SecondLook => "second-look",
            Move::StopTheLine => "stop-the-line",
            Move::TwoWays => "two-ways",
            Move::FreshStart => "fresh-start",
            Move::Advice => "advice",
            Move::CheckIn => "check-in",
            Move::HandOff => "hand-off",
            Move::Parallel => "parallel",
        }
    }

    pub fn parse(s: &str) -> Option<Move> {
        let s = s.trim().to_ascii_lowercase();
        Move::ALL.into_iter().find(|m| m.slug() == s)
    }

    /// Finishes "When …, ". Starts with "I" except for the one that is the
    /// helpers' own doing.
    pub fn words(self) -> &'static str {
        match self {
            Move::Criteria => "I agree with you what done means",
            Move::LookFirst => "I look around first and change nothing",
            Move::OneAtATime => "I finish one thing before the next",
            Move::Split => "I split it into smaller pieces",
            Move::SecondLook => "I get a second look",
            Move::StopTheLine => "I fix that before anything else",
            Move::TwoWays => "I try two ways and keep the better one",
            Move::FreshStart => "I start fresh with a handover note",
            Move::Advice => "I ask for advice first",
            Move::CheckIn => "I check in with you",
            Move::HandOff => "I hand it to the helper that's best at it",
            Move::Parallel => "helpers work on several items at once",
        }
    }
}

/// Where a rule applies. `Everywhere` unless the person (or evolution) narrowed it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Everywhere,
    Project(String),
    Persona(String),
    /// A kind of task, as `agent::selfmodel` names them.
    Kind(String),
}

/// What a run knows about where it is, for narrowing.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScopeCtx<'a> {
    pub project: Option<&'a str>,
    pub persona: Option<&'a str>,
    pub kind: Option<&'a str>,
}

impl Scope {
    pub fn parse(s: &str) -> Scope {
        let s = s.trim();
        match s.split_once(':') {
            Some(("project", id)) if !id.trim().is_empty() => Scope::Project(id.trim().to_string()),
            Some(("persona", id)) if !id.trim().is_empty() => Scope::Persona(id.trim().to_string()),
            Some(("kind", k)) if !k.trim().is_empty() => Scope::Kind(k.trim().to_string()),
            // An unreadable scope widens to everywhere, like every other field
            // here: a typo must not switch a rule off without anyone saying so.
            _ => Scope::Everywhere,
        }
    }

    pub fn as_text(&self) -> String {
        match self {
            Scope::Everywhere => "everywhere".into(),
            Scope::Project(id) => format!("project:{id}"),
            Scope::Persona(id) => format!("persona:{id}"),
            Scope::Kind(k) => format!("kind:{k}"),
        }
    }

    pub fn applies(&self, ctx: &ScopeCtx<'_>) -> bool {
        match self {
            Scope::Everywhere => true,
            Scope::Project(id) => ctx.project == Some(id.as_str()),
            Scope::Persona(id) => ctx.persona == Some(id.as_str()),
            Scope::Kind(k) => ctx.kind == Some(k.as_str()),
        }
    }
}

/// How a way of working stands (`I.2`). `Retired` is kept, not deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Always,
    Learned,
    Trying,
    Retired,
}

impl Status {
    pub fn slug(self) -> &'static str {
        match self {
            Status::Always => "always",
            Status::Learned => "learned",
            Status::Trying => "trying",
            Status::Retired => "retired",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        match s.trim().to_ascii_lowercase().as_str() {
            "always" => Some(Status::Always),
            "learned" => Some(Status::Learned),
            "trying" => Some(Status::Trying),
            "retired" => Some(Status::Retired),
            _ => None,
        }
    }
}

/// One rule in its own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Way {
    /// The file stem.
    pub slug: String,
    pub when: Trigger,
    pub mv: Move,
    pub scope: Scope,
    pub status: Status,
    /// Who made it: `shipped`, `you`, or what proposed it.
    pub origin: String,
    /// YYYY-MM-DD.
    pub since: String,
    /// The conversations behind it, oldest first.
    pub evidence: Vec<String>,
    /// While it is being tried out: from when, for how many tasks and days.
    pub trial: Option<Trial>,
}

/// `Trying out` (`IV.4`): the next `tasks` tasks of its kind, at most `days` days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trial {
    /// When it began, ms.
    pub from: i64,
    pub tasks: u32,
    pub days: u32,
    /// The person was asked once what to do with a trial that ran out of days.
    pub asked: bool,
}

impl Trial {
    pub const TASKS: u32 = 5;
    pub const DAYS: u32 = 30;

    pub fn starting_now() -> Trial {
        Trial { from: chrono::Utc::now().timestamp_millis(), tasks: Trial::TASKS, days: Trial::DAYS, asked: false }
    }
}

impl Way {
    /// "When a check fails, I fix that before anything else."
    pub fn sentence(&self) -> String {
        sentence(self.when, self.mv)
    }
}

/// The rule as a person reads it.
pub fn sentence(when: Trigger, mv: Move) -> String {
    format!("When {}, {}.", when.words(), mv.words())
}

/// One rule inside a preset. Its identity is the pair: a preset says *which*
/// moves fire on *which* triggers, once each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetRule {
    pub when: Trigger,
    pub mv: Move,
}

/// A named bundle of rules, in its own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub slug: String,
    pub name: String,
    /// The second line: what it means here, in a sentence.
    pub line: String,
    /// Which glyph draws it (`IX.1`): a built-in preset's own slug, or the
    /// parent's for one made from it.
    pub glyph: String,
    pub built_in: bool,
    /// `🔒 Locked`: evolution skips it.
    pub locked: bool,
    pub version: u32,
    /// A person or evolution changed a built-in's file. It is theirs from then on.
    pub edited: bool,
    pub rules: Vec<PresetRule>,
    /// The preset it was made from (a slug), when it came from one.
    pub from: String,
    /// When it was made, `YYYY-MM-DD`. Empty for what ships.
    pub born: String,
    /// How it came to be: `preset`, `chat`, `evolution`; empty from nothing or shipped.
    pub how: String,
}

impl Preset {
    /// "from Lean, Nov 7", "from the chat on Nov 7": where it comes from, for the tab.
    /// `parent` is the name of `from`, when that preset is still there.
    pub fn lineage(&self, parent: Option<&str>) -> Option<String> {
        let date = short_date(&self.born);
        match (self.how.as_str(), parent) {
            ("chat", _) => Some(format!("from the chat{}", date.map(|d| format!(" on {d}")).unwrap_or_default())),
            ("preset" | "evolution", Some(p)) => {
                Some(format!("from {p}{}", date.map(|d| format!(", {d}")).unwrap_or_default()))
            }
            ("evolution", None) => Some("from evolution".into()),
            _ => None,
        }
    }
}

/// `2026-11-07` as "Nov 7".
pub fn short_date(date: &str) -> Option<String> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let m: usize = date.get(5..7)?.parse().ok()?;
    let d: u32 = date.get(8..10)?.parse().ok()?;
    Some(format!("{} {d}", MONTHS.get(m.checked_sub(1)?)?))
}

/// What one run enforces from a rule, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// A way's slug, or `<preset>:<trigger>-<move>` for a preset's.
    pub id: String,
    pub when: Trigger,
    pub mv: Move,
    pub text: String,
    /// `always` | `learned` | `trying` | `preset`.
    pub from: String,
    /// When it was adopted, `YYYY-MM-DD`, for what was learned; empty otherwise.
    pub since: String,
}

// ---- what ships ----

struct Shipped {
    slug: &'static str,
    name: &'static str,
    line: &'static str,
    rules: &'static [(Trigger, Move)],
}

/// The built-in presets, as this version of Poiesis ships them.
///
/// Only rules the loop can carry out are listed. The rest of each preset
/// arrives with the step that builds its move (`ORG_LOOP_PLAN` Part X), and
/// `edited: no` is what lets that arrive in a built-in's file without touching
/// one a person has changed.
const SHIPPED: &[Shipped] = &[
    Shipped { slug: AD_HOC, name: "Ad hoc", line: "Just answer", rules: &[] },
    Shipped {
        slug: "lean",
        name: "Lean",
        line: "One thing at a time, stop the line when something breaks",
        rules: &[
            (Trigger::Start, Move::OneAtATime),
            (Trigger::ManyChanges, Move::Split),
            (Trigger::BeforeDone, Move::SecondLook),
            (Trigger::HeadFull, Move::FreshStart),
        ],
    },
    Shipped {
        slug: "command-and-control",
        name: "Command & Control",
        line: "You decide every step",
        rules: &[
            (Trigger::Start, Move::Criteria),
            (Trigger::Unclear, Move::LookFirst),
            (Trigger::Start, Move::OneAtATime),
            (Trigger::AfterItem, Move::CheckIn),
            (Trigger::BeforeDone, Move::SecondLook),
        ],
    },
    Shipped {
        slug: "mission-command",
        name: "Mission Command",
        line: "You set the goal, I find the way",
        rules: &[
            (Trigger::Start, Move::Criteria),
            (Trigger::Unclear, Move::LookFirst),
            (Trigger::BeforeAnswer, Move::SecondLook),
            (Trigger::ItemFailedTwice, Move::TwoWays),
            (Trigger::HeadFull, Move::FreshStart),
            (Trigger::RiskyAction, Move::Advice),
        ],
    },
    Shipped {
        slug: "swarm",
        name: "Swarm",
        line: "Many helpers, no boss",
        rules: &[
            (Trigger::Start, Move::Parallel),
            (Trigger::Start, Move::HandOff),
            (Trigger::BeforeDone, Move::SecondLook),
            (Trigger::HeadFull, Move::FreshStart),
        ],
    },
];

/// The base rules: they hold in every preset, and only the person can switch
/// one off.
const ALWAYS: &[(&str, Trigger, Move)] = &[
    ("stop-the-line", Trigger::CheckFails, Move::StopTheLine),
    ("ask-on-repeat", Trigger::SameProblemTwice, Move::CheckIn),
];

// ---- files ----

fn header_value<'a>(header: &'a [(String, String)], key: &str) -> Option<&'a str> {
    header.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Split `---` frontmatter from the rest. Forgiving, like `parse_entry`: CRLF
/// tolerated, unknown keys kept and ignored, no header means all body.
fn split(text: &str) -> (Vec<(String, String)>, String) {
    let text = text.replace("\r\n", "\n");
    if let Some(rest) = text.strip_prefix("---\n") {
        let lines: Vec<&str> = rest.split('\n').collect();
        if let Some(close) = lines.iter().position(|l| *l == "---") {
            let header = lines[..close]
                .iter()
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect();
            return (header, lines[close + 1..].join("\n"));
        }
    }
    (Vec::new(), text)
}

fn yes(v: Option<&str>) -> bool {
    matches!(v.map(|v| v.to_ascii_lowercase()).as_deref(), Some("yes" | "true" | "on"))
}

fn parse_way(slug: &str, text: &str) -> Option<Way> {
    let (header, _) = split(text);
    // A rule that names a trigger or a move this version does not know cannot be
    // carried out, so it is not a rule: skipped, never guessed at.
    let when = Trigger::parse(header_value(&header, "when")?)?;
    let mv = Move::parse(header_value(&header, "move")?)?;
    Some(Way {
        slug: slug.to_string(),
        when,
        mv,
        scope: header_value(&header, "scope").map(Scope::parse).unwrap_or_default(),
        status: header_value(&header, "status").and_then(Status::parse).unwrap_or(Status::Learned),
        origin: header_value(&header, "origin").unwrap_or("you").to_string(),
        since: header_value(&header, "since").unwrap_or("").to_string(),
        evidence: header_value(&header, "evidence")
            .map(|v| v.split(',').map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect())
            .unwrap_or_default(),
        trial: header_value(&header, "trial_from").and_then(|v| v.parse().ok()).map(|from| Trial {
            from,
            tasks: header_value(&header, "trial_tasks").and_then(|v| v.parse().ok()).unwrap_or(Trial::TASKS),
            days: header_value(&header, "trial_days").and_then(|v| v.parse().ok()).unwrap_or(Trial::DAYS),
            asked: yes(header_value(&header, "trial_asked")),
        }),
    })
}

fn render_way(w: &Way) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("when: {}\n", w.when.slug()));
    out.push_str(&format!("move: {}\n", w.mv.slug()));
    out.push_str(&format!("scope: {}\n", one_line(&w.scope.as_text())));
    out.push_str(&format!("status: {}\n", w.status.slug()));
    out.push_str(&format!("origin: {}\n", one_line(&w.origin)));
    out.push_str(&format!("since: {}\n", one_line(&w.since)));
    if !w.evidence.is_empty() {
        out.push_str(&format!(
            "evidence: {}\n",
            w.evidence.iter().map(|e| one_line(e).replace(',', "")).collect::<Vec<_>>().join(", ")
        ));
    }
    if let (Some(t), Status::Trying) = (&w.trial, w.status) {
        out.push_str(&format!("trial_from: {}\ntrial_tasks: {}\ntrial_days: {}\n", t.from, t.tasks, t.days));
        if t.asked {
            out.push_str("trial_asked: yes\n");
        }
    }
    out.push_str("---\n");
    out.push_str(&w.sentence());
    out.push('\n');
    out
}

fn parse_preset(slug: &str, text: &str) -> Option<Preset> {
    let (header, body) = split(text);
    let name = header_value(&header, "name").filter(|n| !n.is_empty())?.to_string();
    let rules = body
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*']).trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (when, mv) = l.split_once("->")?;
            Some(PresetRule { when: Trigger::parse(when)?, mv: Move::parse(mv)? })
        })
        .collect();
    let built_in = header_value(&header, "origin") == Some("built-in");
    Some(Preset {
        slug: slug.to_string(),
        name,
        line: header_value(&header, "description").unwrap_or("").to_string(),
        glyph: header_value(&header, "glyph")
            .map(str::to_string)
            .unwrap_or_else(|| if built_in { slug.to_string() } else { AD_HOC.to_string() }),
        built_in,
        locked: header_value(&header, "lock") == Some("locked"),
        version: header_value(&header, "version").and_then(|v| v.parse().ok()).unwrap_or(1),
        edited: yes(header_value(&header, "edited")),
        rules,
        from: header_value(&header, "from").unwrap_or("").to_string(),
        born: header_value(&header, "born").unwrap_or("").to_string(),
        how: header_value(&header, "how").unwrap_or("").to_string(),
    })
}

fn render_preset(p: &Preset) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", one_line(&p.name)));
    out.push_str(&format!("description: {}\n", one_line(&p.line)));
    out.push_str(&format!("glyph: {}\n", one_line(&p.glyph)));
    out.push_str(&format!("origin: {}\n", if p.built_in { "built-in" } else { "you" }));
    out.push_str(&format!("lock: {}\n", if p.locked { "locked" } else { "evolves" }));
    out.push_str(&format!("version: {}\n", p.version));
    out.push_str(&format!("edited: {}\n", if p.edited { "yes" } else { "no" }));
    for (key, value) in [("from", &p.from), ("born", &p.born), ("how", &p.how)] {
        if !value.is_empty() {
            out.push_str(&format!("{key}: {}\n", one_line(value)));
        }
    }
    out.push_str("---\n");
    for r in &p.rules {
        out.push_str(&format!("{} -> {}\n", r.when.slug(), r.mv.slug()));
    }
    out
}

fn shipped_preset(s: &Shipped) -> Preset {
    Preset {
        slug: s.slug.to_string(),
        name: s.name.to_string(),
        line: s.line.to_string(),
        glyph: s.slug.to_string(),
        built_in: true,
        locked: false,
        version: 1,
        edited: false,
        rules: s.rules.iter().map(|&(when, mv)| PresetRule { when, mv }).collect(),
        from: String::new(),
        born: String::new(),
        how: String::new(),
    }
}

impl MemoryStore {
    fn ways_dir(&self) -> PathBuf {
        self.dir().join(WAYS_DIR)
    }

    fn presets_dir(&self) -> PathBuf {
        self.ways_dir().join(PRESETS_DIR)
    }

    /// Put what ships in place: the Always rules where they are missing, the
    /// built-in presets where they are missing or still the original. Never
    /// overwrites a file a person has made theirs. Best effort: a read-only
    /// folder leaves the loop on the built-in definitions (`rules_for`).
    pub fn ensure_ways(&self) {
        self.write_shipped();
        self.note_start();
    }

    fn write_shipped(&self) {
        let _guard = self.lock.lock().unwrap();
        let _ = std::fs::create_dir_all(self.presets_dir());
        for &(slug, when, mv) in ALWAYS {
            let path = self.ways_dir().join(format!("{slug}.md"));
            if !path.exists() {
                let way = Way {
                    slug: slug.to_string(),
                    when,
                    mv,
                    scope: Scope::Everywhere,
                    status: Status::Always,
                    origin: "shipped".into(),
                    since: today(),
                    evidence: Vec::new(),
                    trial: None,
                };
                let _ = std::fs::write(path, render_way(&way));
            }
        }
        for s in SHIPPED {
            let path = self.presets_dir().join(format!("{}.md", s.slug));
            let current = std::fs::read_to_string(&path).ok().and_then(|t| parse_preset(s.slug, &t));
            let theirs = current.as_ref().is_some_and(|p| p.edited || !p.built_in);
            if theirs {
                continue;
            }
            let shipped = shipped_preset(s);
            // `locked` is the person's switch even on an untouched original.
            let wanted = Preset { locked: current.as_ref().is_some_and(|p| p.locked), ..shipped };
            if current.as_ref() != Some(&wanted) {
                let _ = std::fs::write(&path, render_preset(&wanted));
            }
        }
    }

    /// Every rule file, Always first, then by name. A file this version cannot
    /// read as a rule is left alone, not moved: it may be from a newer one.
    pub fn ways(&self) -> Vec<Way> {
        let Ok(entries) = std::fs::read_dir(self.ways_dir()) else { return Vec::new() };
        let mut out: Vec<Way> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
            .filter_map(|p| {
                let slug = p.file_stem()?.to_string_lossy().to_string();
                parse_way(&slug, &std::fs::read_to_string(&p).ok()?)
            })
            .collect();
        out.sort_by(|a, b| {
            (a.status != Status::Always).cmp(&(b.status != Status::Always)).then(a.slug.cmp(&b.slug))
        });
        out
    }

    /// The built-in presets in shipped order, then the person's own by name.
    pub fn presets(&self) -> Vec<Preset> {
        let mut out: Vec<Preset> = Vec::new();
        for s in SHIPPED {
            let path = self.presets_dir().join(format!("{}.md", s.slug));
            let found = std::fs::read_to_string(&path).ok().and_then(|t| parse_preset(s.slug, &t));
            // A missing or unreadable file reads as the original, so the menu is
            // never empty and Ad hoc always exists.
            out.push(found.unwrap_or_else(|| shipped_preset(s)));
        }
        let mut own: Vec<Preset> = std::fs::read_dir(self.presets_dir())
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
            .filter_map(|p| {
                let slug = p.file_stem()?.to_string_lossy().to_string();
                if SHIPPED.iter().any(|s| s.slug == slug) {
                    return None;
                }
                parse_preset(&slug, &std::fs::read_to_string(&p).ok()?)
            })
            .collect();
        own.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out.extend(own);
        out
    }

    pub fn preset(&self, slug: &str) -> Option<Preset> {
        self.presets().into_iter().find(|p| p.slug == slug)
    }

    /// A preset as a person types it: its slug, or its name in any case
    /// (`/style mission command`).
    pub fn find_preset(&self, typed: &str) -> Option<Preset> {
        let t = typed.trim().to_lowercase();
        let slugged = super::slugify(&t).ok();
        self.presets()
            .into_iter()
            .find(|p| p.slug == t || p.name.to_lowercase() == t || Some(&p.slug) == slugged.as_ref())
    }

    /// What one run enforces: the Always rules, the learned and trying ones in
    /// scope, and the chat's preset on top. Learned rules apply whatever the
    /// preset is (`I.3`). The same trigger and move from two places fire once.
    pub fn rules_for(&self, preset: &str, ctx: &ScopeCtx<'_>) -> Vec<Rule> {
        let mut out: Vec<Rule> = Vec::new();
        let mut push = |rule: Rule| {
            if !out.iter().any(|r| r.when == rule.when && r.mv == rule.mv) {
                out.push(rule);
            }
        };
        let ways = self.ways();
        // A fresh install, or a folder that could not be written, still has the
        // base rules: they are part of the loop, not only of the folder.
        for way in ways.iter().filter(|w| w.status == Status::Always) {
            if way.scope.applies(ctx) {
                push(Rule {
                    id: way.slug.clone(),
                    when: way.when,
                    mv: way.mv,
                    text: way.sentence(),
                    from: "always".into(),
                    since: way.since.clone(),
                });
            }
        }
        // Only where there is no file for it at all: a base rule the person
        // switched off has a file (retired) and stays off.
        for &(slug, when, mv) in ALWAYS {
            if ways.iter().any(|w| w.slug == slug) {
                continue;
            }
            push(Rule { id: slug.into(), when, mv, text: sentence(when, mv), from: "always".into(), since: String::new() });
        }
        for way in ways.iter().filter(|w| matches!(w.status, Status::Learned | Status::Trying)) {
            if way.scope.applies(ctx) {
                push(Rule {
                    id: way.slug.clone(),
                    when: way.when,
                    mv: way.mv,
                    text: way.sentence(),
                    from: way.status.slug().into(),
                    since: way.since.clone(),
                });
            }
        }
        if let Some(p) = self.preset(preset) {
            for r in &p.rules {
                push(Rule {
                    id: format!("{}:{}-{}", p.slug, r.when.slug(), r.mv.slug()),
                    when: r.when,
                    mv: r.mv,
                    text: sentence(r.when, r.mv),
                    from: "preset".into(),
                    since: String::new(),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (MemoryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_ways_{}", uuid::Uuid::new_v4().simple()));
        (MemoryStore::new(&dir).unwrap(), dir)
    }

    #[test]
    fn every_trigger_and_move_round_trips_through_its_slug() {
        for t in Trigger::ALL {
            assert_eq!(Trigger::parse(t.slug()), Some(t));
        }
        for m in Move::ALL {
            assert_eq!(Move::parse(m.slug()), Some(m));
        }
        assert_eq!(Trigger::parse("whenever"), None);
    }

    #[test]
    fn a_preset_is_found_by_slug_or_by_name_in_any_case() {
        let (store, dir) = store();
        store.ensure_ways();
        for typed in ["lean", "Lean", " LEAN "] {
            assert_eq!(store.find_preset(typed).map(|p| p.slug), Some("lean".to_string()), "{typed}");
        }
        assert_eq!(store.find_preset("ad hoc").map(|p| p.slug), Some("ad-hoc".to_string()));
        assert!(store.find_preset("no such way").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_rule_reads_as_one_sentence() {
        assert_eq!(
            sentence(Trigger::CheckFails, Move::StopTheLine),
            "When a check fails, I fix that before anything else."
        );
        assert_eq!(
            sentence(Trigger::Start, Move::OneAtATime),
            "When I start, I finish one thing before the next."
        );
    }

    #[test]
    fn a_fresh_folder_gets_the_always_rules_and_the_five_presets() {
        let (store, dir) = store();
        store.ensure_ways();
        let ways = store.ways();
        assert_eq!(ways.len(), 2);
        assert!(ways.iter().all(|w| w.status == Status::Always && w.origin == "shipped"));
        let presets = store.presets();
        assert_eq!(
            presets.iter().map(|p| p.slug.as_str()).collect::<Vec<_>>(),
            vec!["ad-hoc", "lean", "command-and-control", "mission-command", "swarm"]
        );
        assert!(presets[0].rules.is_empty(), "Ad hoc only uses the Always rules");
        assert!(dir.join("memory/ways/presets/lean.md").exists(), "plain files, owned by the person");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_the_person_edited_is_not_rewritten_but_an_untouched_original_follows_the_release() {
        let (store, dir) = store();
        store.ensure_ways();
        let path = dir.join("memory/ways/presets/lean.md");

        // Untouched: an older original is brought up to date.
        std::fs::write(&path, "---\nname: Lean\ndescription: old\norigin: built-in\nedited: no\n---\n").unwrap();
        store.ensure_ways();
        let lean = store.preset("lean").unwrap();
        assert_eq!(lean.rules.len(), 4);
        assert_eq!(lean.line, "One thing at a time, stop the line when something breaks");

        // Theirs: left exactly as written.
        std::fs::write(
            &path,
            "---\nname: Lean\ndescription: mine\norigin: built-in\nedited: yes\n---\nstart -> one-at-a-time\n",
        )
        .unwrap();
        store.ensure_ways();
        let lean = store.preset("lean").unwrap();
        assert_eq!(lean.line, "mine");
        assert_eq!(lean.rules.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_lock_survives_a_refresh_of_an_untouched_original() {
        let (store, dir) = store();
        store.ensure_ways();
        let path = dir.join("memory/ways/presets/lean.md");
        let locked = Preset { locked: true, ..store.preset("lean").unwrap() };
        std::fs::write(&path, render_preset(&locked)).unwrap();
        store.ensure_ways();
        assert!(store.preset("lean").unwrap().locked);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_rule_with_a_trigger_this_version_does_not_know_is_not_a_rule() {
        let (store, dir) = store();
        store.ensure_ways();
        std::fs::write(
            dir.join("memory/ways/odd.md"),
            "---\nwhen: full-moon\nmove: split\nstatus: learned\n---\nWhen it is a full moon, I split.\n",
        )
        .unwrap();
        assert!(store.ways().iter().all(|w| w.slug != "odd"));
        assert!(dir.join("memory/ways/odd.md").exists(), "left alone, not moved: it may be from a newer version");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rules_are_the_always_ones_then_what_was_learned_in_scope_then_the_preset() {
        let (store, dir) = store();
        store.ensure_ways();
        let learned = Way {
            slug: "split-in-nexus".into(),
            when: Trigger::ManyChanges,
            mv: Move::Split,
            scope: Scope::Project("nexus".into()),
            status: Status::Learned,
            origin: "you".into(),
            since: "2026-11-07".into(),
            evidence: vec![],
            trial: None,
        };
        std::fs::write(dir.join("memory/ways/split-in-nexus.md"), render_way(&learned)).unwrap();

        let adhoc = store.rules_for(AD_HOC, &ScopeCtx::default());
        assert_eq!(adhoc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["ask-on-repeat", "stop-the-line"]);

        // Learned rules apply on top of every preset, but only in their scope.
        let elsewhere = store.rules_for("lean", &ScopeCtx::default());
        assert!(elsewhere.iter().all(|r| r.id != "split-in-nexus"));
        let in_nexus = store.rules_for("lean", &ScopeCtx { project: Some("nexus"), ..Default::default() });
        assert!(in_nexus.iter().any(|r| r.id == "split-in-nexus" && r.from == "learned"));
        // The same trigger and move from two places fire once: the learned one wins.
        assert_eq!(in_nexus.iter().filter(|r| r.mv == Move::Split).count(), 1);
        assert!(in_nexus.iter().any(|r| r.mv == Move::OneAtATime && r.from == "preset"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_retired_rule_is_kept_and_does_not_fire() {
        let (store, dir) = store();
        store.ensure_ways();
        let retired = Way {
            slug: "old".into(),
            when: Trigger::Start,
            mv: Move::OneAtATime,
            scope: Scope::Everywhere,
            status: Status::Retired,
            origin: "you".into(),
            since: "2026-10-28".into(),
            evidence: vec!["c1".into(), "c2".into()],
            trial: None,
        };
        std::fs::write(dir.join("memory/ways/old.md"), render_way(&retired)).unwrap();
        assert!(store.ways().iter().any(|w| w.slug == "old" && w.evidence == vec!["c1", "c2"]));
        assert!(store.rules_for(AD_HOC, &ScopeCtx::default()).iter().all(|r| r.id != "old"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_base_rules_hold_even_when_the_folder_could_not_be_written() {
        let (store, dir) = store();
        // No ensure_ways: nothing on disk.
        let rules = store.rules_for(AD_HOC, &ScopeCtx::default());
        assert!(rules.iter().any(|r| r.mv == Move::StopTheLine));
        assert!(rules.iter().any(|r| r.mv == Move::CheckIn));
        assert_eq!(store.presets().len(), 5, "the menu is never empty");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `WRK-12`/`WRK-14`: each preset is what the plan's table says it starts with,
    /// and `/style mission command` finds it by the words a person types.
    #[test]
    fn the_new_presets_start_with_what_the_plan_says() {
        let (store, dir) = store();
        store.ensure_ways();
        let has = |slug: &str, when: Trigger, mv: Move| store.preset(slug).unwrap().rules.iter().any(|r| r.when == when && r.mv == mv);
        assert!(has("command-and-control", Trigger::Start, Move::Criteria));
        assert!(has("command-and-control", Trigger::AfterItem, Move::CheckIn));
        assert!(has("command-and-control", Trigger::BeforeDone, Move::SecondLook));
        assert!(has("mission-command", Trigger::BeforeAnswer, Move::SecondLook));
        assert!(has("mission-command", Trigger::ItemFailedTwice, Move::TwoWays));
        assert!(has("mission-command", Trigger::RiskyAction, Move::Advice));
        assert!(!has("mission-command", Trigger::AfterItem, Move::CheckIn), "it checks in only on a repeated problem");
        assert!(has("swarm", Trigger::Start, Move::Parallel));
        assert!(has("swarm", Trigger::Start, Move::HandOff));
        for typed in ["mission command", "Mission Command", "command & control", "swarm"] {
            assert!(store.find_preset(typed).is_some(), "{typed}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_custom_preset_reads_back_with_its_own_glyph_and_lock() {
        let p = Preset {
            slug: "careful".into(),
            name: "Careful with mail".into(),
            line: "I ask before anything leaves the house".into(),
            glyph: "lean".into(),
            built_in: false,
            locked: true,
            version: 3,
            edited: true,
            rules: vec![PresetRule { when: Trigger::RiskyAction, mv: Move::Advice }],
            from: "lean".into(),
            born: "2026-11-07".into(),
            how: "evolution".into(),
        };
        assert_eq!(parse_preset("careful", &render_preset(&p)), Some(p.clone()));
        assert_eq!(p.lineage(Some("Lean")).as_deref(), Some("from Lean, Nov 7"));
        assert_eq!(Preset { how: "chat".into(), ..p.clone() }.lineage(None).as_deref(), Some("from the chat on Nov 7"));
        assert_eq!(Preset { how: String::new(), ..p }.lineage(Some("Lean")), None, "made from nothing has no line");
    }
}
