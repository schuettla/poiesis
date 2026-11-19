//! `TEAM-8`: teams, as plain files next to the presets.
//!
//! A team is a set of members (a persona or a plain helper, with a job) and the
//! lines that say who may hand work to whom, who reviews whom, who may ask whom,
//! who reports to you and whom you may talk to. A team is not a style: it can work
//! Lean or Mission Command, so it is its own file, but it gets its versions, lock
//! and rings from the same code the presets use (`history.rs`, `rings.rs`).
//!
//! ```text
//! memory/ways/
//! ├─ teams/shop-team.md           the team as it is now
//! └─ history/team-shop-team/      its versions: 0001.md, 0002.md …
//! ```
//!
//! Reading is forgiving and never widens: a line that cannot be read adds no line
//! and a member that cannot be read adds no member, and the reader says what it
//! skipped (`TeamRead::problems`). A field it cannot read takes its safe default.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::edit::{safe_slug, LINE_CAP, NAME_CAP};
use super::{header_value, split, yes, MemoryStore};
use crate::memory::{one_line, slugify};

pub const TEAMS_DIR: &str = "teams";

/// Words a role cannot be: they mean someone else in a line.
const RESERVED: [&str; 3] = ["coordinator", "you", "all"];
const COORDINATOR: &str = "coordinator";
const YOU: &str = "you";

macro_rules! words {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: [$name; [$($name::$variant),+].len()] = [$($name::$variant),+];

            pub fn slug(self) -> &'static str {
                match self { $($name::$variant => $word),+ }
            }

            pub fn parse(s: &str) -> Option<$name> {
                let s = s.trim().to_ascii_lowercase();
                $name::ALL.into_iter().find(|v| v.slug() == s)
            }
        }
    };
}

words! {
    /// What a team started from. The lines are the truth; this only says where it began.
    Shape { Hub => "hub", Pipeline => "pipeline", Review => "review", Open => "open" }
}
words! {
    /// When the coordinator tells you how it goes.
    Report { EachItem => "each-item", End => "end", Problems => "problems" }
}
words! {
    /// Where members work: each in its own copy, or in the folder with parts of their own.
    Folder { Copies => "copies", Owners => "owners" }
}
words! {
    /// What a line lets someone do (a fixed list, like triggers and moves).
    LineKind {
        HandsOver => "hands-over",
        Reviews => "reviews",
        Asks => "asks",
        Reports => "reports",
        Talks => "talks",
    }
}

/// One member: a role name that is unique in the team, who plays it, and what for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub role: String,
    /// `persona:<id>` or `helper`.
    pub persona: String,
    pub job: String,
    /// Paths that are this member's alone. Only used when the folder is `owners`.
    #[serde(default)]
    pub owns: Vec<String>,
}

/// `from kind to`. Both ends are a role, `coordinator` or `you`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub kind: LineKind,
    pub from: String,
    pub to: String,
}

impl Line {
    pub fn new(kind: LineKind, from: &str, to: &str) -> Line {
        Line { kind, from: from.to_string(), to: to.to_string() }
    }

    /// "verifier reviews api", as the file has it.
    pub fn text(&self) -> String {
        format!("{} {} {}", self.from, self.kind.slug(), self.to)
    }

    /// Does the line make sense? `you` only talks (`you talks A`) or is reported to,
    /// and nobody has a line to themselves.
    fn fits(&self) -> bool {
        let (from_you, to_you) = (self.from == YOU, self.to == YOU);
        self.from != self.to
            && match self.kind {
                LineKind::Talks => from_you && !to_you,
                LineKind::Reports => !from_you,
                _ => !from_you && !to_you,
            }
    }

    fn read(item: &str, roles: &[&str]) -> Result<Line, &'static str> {
        let w: Vec<&str> = item.split_whitespace().collect();
        let [from, kind, to] = w[..] else { return Err("a line is three words: who, what, whom") };
        let kind = LineKind::parse(kind).ok_or("I don't know that kind of line")?;
        let known = |who: &str| who == COORDINATOR || who == YOU || roles.contains(&who);
        if !known(from) || !known(to) {
            return Err("nobody here has that name");
        }
        let line = Line::new(kind, from, to);
        line.fits().then_some(line).ok_or("that line does not make sense")
    }
}

/// The two lines every team has, and nobody can take away.
fn fixed_lines() -> [Line; 2] {
    [Line::new(LineKind::Reports, COORDINATOR, YOU), Line::new(LineKind::Talks, YOU, COORDINATOR)]
}

/// What stops a team: how many work at once, what it may spend, how long, how many
/// times two members go back and forth, and whether it asks you first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub at_once: u32,
    /// In dollars, what the price table and Settings, Usage count in. 0 means no spend cap.
    pub budget: f64,
    pub minutes: u32,
    pub rounds: u32,
    pub ask_cost: bool,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { at_once: 3, budget: 2.0, minutes: 90, rounds: 3, ask_cost: true }
    }
}

impl Limits {
    fn clamped(self) -> Limits {
        let budget = if self.budget.is_finite() && self.budget >= 0.0 { self.budget } else { Limits::default().budget };
        Limits { at_once: self.at_once.clamp(1, 5), budget, minutes: self.minutes.max(1), rounds: self.rounds.max(1), ..self }
    }

    fn render(&self) -> String {
        format!(
            "{{ at_once: {}, budget: {:.2}, minutes: {}, rounds: {}, ask_cost: {} }}",
            self.at_once,
            self.budget,
            self.minutes,
            self.rounds,
            if self.ask_cost { "yes" } else { "no" }
        )
    }

    /// `{ at_once: 3, budget: 2.00, … }`. A part it cannot read keeps its default, and is said.
    fn read(v: &str, problems: &mut Vec<String>) -> Limits {
        let mut out = Limits::default();
        let Some(inner) = v.trim().strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
            problems.push(format!("I can't read the limits “{v}”, so they are the usual ones."));
            return out;
        };
        for part in inner.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part.split_once(':').map_or((part, ""), |(k, v)| (k.trim(), v.trim()));
            let ok = match key {
                "at_once" => value.parse().map(|n| out.at_once = n).is_ok(),
                "budget" => value.parse().map(|n| out.budget = n).is_ok(),
                "minutes" => value.parse().map(|n| out.minutes = n).is_ok(),
                "rounds" => value.parse().map(|n| out.rounds = n).is_ok(),
                "ask_cost" => matches!(value, "yes" | "no") && {
                    out.ask_cost = value == "yes";
                    true
                },
                _ => true, // from a newer version: left alone
            };
            if !ok {
                problems.push(format!("I can't read the limit “{part}”, so it is the usual one."));
            }
        }
        out.clamped()
    }
}

/// A team, as its file says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
    /// The file stem. Empty for a team that is not saved yet.
    pub slug: String,
    pub name: String,
    pub line: String,
    /// `poiesis` or `persona:<id>`.
    pub coordinator: String,
    /// How the coordinator works (a style slug; empty: the usual order decides).
    pub style: String,
    /// One style for all members.
    pub member_style: String,
    pub shape: Shape,
    pub report: Report,
    pub limits: Limits,
    pub folder: Folder,
    /// A team the person made starts locked: evolution skips it.
    pub locked: bool,
    pub version: u32,
    pub members: Vec<Member>,
    /// Includes the two fixed lines.
    pub lines: Vec<Line>,
}

/// A team read from its file, and what the reader had to skip.
#[derive(Debug, Clone, Serialize)]
pub struct TeamRead {
    pub team: Team,
    /// Plain sentences about every part of the file that was left out or replaced.
    pub problems: Vec<String>,
}

fn valid_role(role: &str) -> bool {
    !role.is_empty()
        && role.chars().count() <= 24
        && role.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !RESERVED.contains(&role)
}

fn valid_persona(who: &str) -> bool {
    who == "helper" || who.strip_prefix("persona:").is_some_and(|id| !id.is_empty() && !id.contains(char::is_whitespace))
}

fn valid_coordinator(who: &str) -> bool {
    who == "poiesis" || (who.starts_with("persona:") && valid_persona(who))
}

impl Member {
    fn read(item: &str) -> Result<Member, &'static str> {
        let (role, rest) = item.split_once(':').ok_or("a member is a role, a colon and who plays it")?;
        let role = role.trim();
        if !valid_role(role) {
            return Err("that is not a role name I can use");
        }
        let mut parts = rest.split(" \u{b7} ").map(str::trim);
        let who = parts.next().unwrap_or("");
        if !valid_persona(who) {
            return Err("I can't tell who plays it");
        }
        let (mut job, mut owns) = (String::new(), Vec::new());
        for part in parts {
            match part.strip_prefix("owns:") {
                Some(paths) => owns = paths.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
                None if job.is_empty() => job = part.to_string(),
                None => {}
            }
        }
        Ok(Member { role: role.to_string(), persona: who.to_string(), job, owns })
    }

    fn render(&self) -> String {
        let mut out = format!("- {}: {}", self.role, self.persona);
        if !self.job.is_empty() {
            out.push_str(&format!(" \u{b7} {}", self.job));
        }
        if !self.owns.is_empty() {
            out.push_str(&format!(" \u{b7} owns: {}", self.owns.join(", ")));
        }
        out
    }
}

impl Team {
    /// A team that is not made yet: what "New team" starts from. The one place the
    /// usual limits and the starting shape (a Hub, decision 13) are said for the screen.
    pub fn blank() -> Team {
        Team {
            slug: String::new(),
            name: String::new(),
            line: String::new(),
            coordinator: "poiesis".into(),
            style: String::new(),
            member_style: String::new(),
            shape: Shape::Hub,
            report: Report::EachItem,
            limits: Limits::default(),
            folder: Folder::Copies,
            locked: false,
            version: 0,
            members: Vec::new(),
            lines: shape_lines(Shape::Hub, &[]),
        }
    }

    /// The persona that coordinates, or `None` when Poiesis does.
    pub fn coordinator_persona(&self) -> Option<&str> {
        self.coordinator.strip_prefix("persona:")
    }

    /// Every line, with the two fixed ones last when the file did not place them.
    fn all_lines(&self) -> Vec<Line> {
        let mut out = self.lines.clone();
        for fixed in fixed_lines() {
            if !out.contains(&fixed) {
                out.push(fixed);
            }
        }
        out
    }

    /// This team with everything about the work taken from `other`: members, lines,
    /// coordinator, styles, limits, folder. What it is called, its lock and its
    /// version stay (a version brought back is a new version).
    pub(super) fn with_content_of(&self, other: &Team) -> Team {
        Team {
            slug: self.slug.clone(),
            name: self.name.clone(),
            line: self.line.clone(),
            locked: self.locked,
            version: self.version,
            ..other.clone()
        }
    }

    /// What a version says about the work, one short sentence each, for what it
    /// added and took away (`history.rs`): the settings, the members, the lines.
    pub(super) fn facts(&self) -> Vec<String> {
        const SETTINGS: [&str; 7] = ["coordinator:", "style:", "member_style:", "shape:", "report:", "limits:", "folder:"];
        render_team(self)
            .lines()
            .filter(|l| l.starts_with("- ") || SETTINGS.iter().any(|s| l.starts_with(s)))
            .map(|l| l.trim_start_matches("- ").to_string())
            .collect()
    }

    /// What a person may save: names and paths made safe for the file, every line
    /// one that fits, the fixed lines there. `Err` says what to fix.
    pub(super) fn cleaned(mut self) -> Result<Team, String> {
        self.name = one_line(self.name.trim());
        if self.name.is_empty() {
            return Err("Give it a name.".into());
        }
        if self.name.chars().count() > NAME_CAP {
            return Err(format!("Keep the name under {NAME_CAP} letters."));
        }
        self.line = one_line(self.line.trim()).chars().take(LINE_CAP).collect();
        if !valid_coordinator(&self.coordinator) {
            return Err("I don't know who coordinates.".into());
        }
        self.style = one_line(&self.style);
        self.member_style = one_line(&self.member_style);
        let mut members: Vec<Member> = Vec::new();
        for mut m in std::mem::take(&mut self.members) {
            m.role = m.role.trim().to_string();
            m.job = one_line(&m.job).replace('\u{b7}', "-");
            m.owns = m.owns.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
            if !valid_role(&m.role) {
                return Err(format!("“{}” is not a role name I can use. Use small letters, digits and dashes.", m.role));
            }
            if !valid_persona(&m.persona) {
                return Err(format!("I can't tell who plays {}.", m.role));
            }
            if m.job.starts_with("owns:") {
                return Err(format!("The job of {} can't start with \u{201c}owns:\u{201d}.", m.role));
            }
            if m.owns.iter().any(|p| p.contains([',', '\u{b7}', '\r', '\n'])) {
                return Err(format!("A path of {} can't have a comma in it.", m.role));
            }
            if members.iter().any(|o| o.role == m.role) {
                return Err(format!("Two members are called {}.", m.role));
            }
            members.push(m);
        }
        self.members = members;
        let roles: Vec<&str> = self.members.iter().map(|m| m.role.as_str()).collect();
        let mut lines: Vec<Line> = Vec::new();
        for l in &self.lines {
            if Line::read(&l.text(), &roles).is_err() {
                return Err(format!("The line \u{201c}{}\u{201d} does not fit this team.", l.text()));
            }
            if !lines.contains(l) {
                lines.push(l.clone());
            }
        }
        self.lines = lines;
        self.lines = self.all_lines();
        self.limits = self.limits.clone().clamped();
        Ok(self)
    }
}

/// The lines a team has when it starts from a shape, for the members as they are.
/// The two fixed lines are last.
///
/// - Hub: the coordinator hands over to every member.
/// - Pipeline: to the first, each to the next, the last reports to the coordinator.
/// - Review: the coordinator hands over to the workers and the last member, the
///   reviewer, reviews each of them. One member alone has nobody to review: it is a Hub.
/// - Open: everyone may hand over to and ask everyone, and you talk with everyone.
pub fn shape_lines(shape: Shape, members: &[Member]) -> Vec<Line> {
    use LineKind::{Asks, HandsOver, Reports, Reviews, Talks};
    let roles: Vec<&str> = members.iter().map(|m| m.role.as_str()).collect();
    let mut out: Vec<Line> = Vec::new();
    match shape {
        Shape::Hub => out.extend(roles.iter().map(|r| Line::new(HandsOver, COORDINATOR, r))),
        Shape::Pipeline => {
            if let (Some(first), Some(last)) = (roles.first(), roles.last()) {
                out.push(Line::new(HandsOver, COORDINATOR, first));
                out.extend(roles.windows(2).map(|w| Line::new(HandsOver, w[0], w[1])));
                out.push(Line::new(Reports, last, COORDINATOR));
            }
        }
        Shape::Review => match roles.split_last() {
            Some((reviewer, workers)) if !workers.is_empty() => {
                out.extend(workers.iter().map(|r| Line::new(HandsOver, COORDINATOR, r)));
                out.extend(workers.iter().map(|r| Line::new(Reviews, reviewer, r)));
            }
            _ => out.extend(roles.iter().map(|r| Line::new(HandsOver, COORDINATOR, r))),
        },
        Shape::Open => {
            let everyone: Vec<&str> = std::iter::once(COORDINATOR).chain(roles.iter().copied()).collect();
            for kind in [HandsOver, Asks] {
                for from in &everyone {
                    for to in everyone.iter().filter(|to| to != &from) {
                        out.push(Line::new(kind, from, to));
                    }
                }
            }
            out.extend(roles.iter().map(|r| Line::new(Talks, YOU, r)));
        }
    }
    for fixed in fixed_lines() {
        if !out.contains(&fixed) {
            out.push(fixed);
        }
    }
    out
}

fn read_word<T: Copy>(
    header: &[(String, String)],
    key: &str,
    parse: fn(&str) -> Option<T>,
    default: (T, &str),
    problems: &mut Vec<String>,
) -> T {
    match header_value(header, key).filter(|v| !v.is_empty()) {
        None => default.0,
        Some(v) => parse(v).unwrap_or_else(|| {
            problems.push(format!("I can't read \u{201c}{v}\u{201d} as the {key}, so it is {}.", default.1));
            default.0
        }),
    }
}

/// Read a team file. `None` when it has no name: it is not a team this version can
/// use, and is left where it is. Anything else that cannot be read is skipped or
/// replaced by a safe default, and said in the second part.
pub(super) fn parse_team(slug: &str, text: &str) -> Option<(Team, Vec<String>)> {
    let (header, body) = split(text);
    let name = header_value(&header, "name").filter(|n| !n.is_empty())?.to_string();
    let mut problems: Vec<String> = Vec::new();

    let coordinator = match header_value(&header, "coordinator").filter(|v| !v.is_empty()) {
        None => "poiesis".to_string(),
        Some(v) if valid_coordinator(v) => v.to_string(),
        Some(v) => {
            problems.push(format!("I can't tell who \u{201c}{v}\u{201d} is, so Poiesis coordinates."));
            "poiesis".to_string()
        }
    };
    let shape = read_word(&header, "shape", Shape::parse, (Shape::Hub, "hub"), &mut problems);
    let report = read_word(&header, "report", Report::parse, (Report::EachItem, "each-item"), &mut problems);
    let folder = read_word(&header, "folder", Folder::parse, (Folder::Copies, "copies"), &mut problems);
    let limits = header_value(&header, "limits").map_or_else(Limits::default, |v| Limits::read(v, &mut problems));

    let mut members: Vec<Member> = Vec::new();
    let mut raw_lines: Vec<&str> = Vec::new();
    let mut section = "";
    for l in body.lines().map(str::trim) {
        if let Some(h) = l.strip_prefix("##") {
            section = match h.trim().to_ascii_lowercase().as_str() {
                "members" => "members",
                "lines" => "lines",
                _ => "",
            };
            continue;
        }
        let Some(item) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).map(str::trim) else { continue };
        match section {
            "members" => match Member::read(item) {
                Ok(m) if members.iter().any(|o| o.role == m.role) => {
                    problems.push(format!("I skipped the member \u{201c}{item}\u{201d}: {} is there already.", m.role));
                }
                Ok(m) => members.push(m),
                Err(why) => problems.push(format!("I skipped the member \u{201c}{item}\u{201d}: {why}.")),
            },
            "lines" => raw_lines.push(item),
            _ => {}
        }
    }
    let roles: Vec<&str> = members.iter().map(|m| m.role.as_str()).collect();
    let mut lines: Vec<Line> = Vec::new();
    for item in raw_lines {
        match Line::read(item, &roles) {
            Ok(l) if lines.contains(&l) => {}
            Ok(l) => lines.push(l),
            Err(why) => problems.push(format!("I skipped the line \u{201c}{item}\u{201d}: {why}.")),
        }
    }

    let mut team = Team {
        slug: slug.to_string(),
        name,
        line: header_value(&header, "line").unwrap_or("").to_string(),
        coordinator,
        style: header_value(&header, "style").unwrap_or("").to_string(),
        member_style: header_value(&header, "member_style").unwrap_or("").to_string(),
        shape,
        report,
        limits,
        folder,
        locked: yes(header_value(&header, "locked")),
        version: header_value(&header, "version").and_then(|v| v.parse().ok()).unwrap_or(1),
        members,
        lines,
    };
    team.lines = team.all_lines();
    Some((team, problems))
}

pub(super) fn render_team(t: &Team) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", one_line(&t.name)));
    out.push_str(&format!("line: {}\n", one_line(&t.line)));
    out.push_str(&format!("coordinator: {}\n", t.coordinator));
    out.push_str(&format!("style: {}\n", one_line(&t.style)));
    out.push_str(&format!("member_style: {}\n", one_line(&t.member_style)));
    out.push_str(&format!("shape: {}\n", t.shape.slug()));
    out.push_str(&format!("report: {}\n", t.report.slug()));
    out.push_str(&format!("limits: {}\n", t.limits.render()));
    out.push_str(&format!("folder: {}\n", t.folder.slug()));
    out.push_str(&format!("locked: {}\n", if t.locked { "yes" } else { "no" }));
    out.push_str(&format!("version: {}\n", t.version));
    out.push_str("---\n## Members\n");
    for m in &t.members {
        out.push_str(&m.render());
        out.push('\n');
    }
    out.push_str("\n## Lines\n");
    for l in t.all_lines() {
        out.push_str(&format!("- {}\n", l.text()));
    }
    out
}

impl MemoryStore {
    pub(super) fn teams_dir(&self) -> PathBuf {
        self.ways_dir().join(TEAMS_DIR)
    }

    /// Every team, by name, with what could not be read in each. A file that is not
    /// a team this version can use is left alone: it may be from a newer one.
    pub fn teams(&self) -> Vec<TeamRead> {
        let mut out: Vec<TeamRead> = std::fs::read_dir(self.teams_dir())
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
            .filter_map(|p| {
                let slug = p.file_stem()?.to_string_lossy().to_string();
                let (team, problems) = parse_team(&slug, &std::fs::read_to_string(&p).ok()?)?;
                Some(TeamRead { team, problems })
            })
            .collect();
        out.sort_by(|a, b| a.team.name.to_lowercase().cmp(&b.team.name.to_lowercase()));
        out
    }

    pub fn read_team(&self, slug: &str) -> Option<TeamRead> {
        if !safe_slug(slug) {
            return None;
        }
        let text = std::fs::read_to_string(self.teams_dir().join(format!("{slug}.md"))).ok()?;
        let (team, problems) = parse_team(slug, &text)?;
        Some(TeamRead { team, problems })
    }

    pub fn team(&self, slug: &str) -> Option<Team> {
        self.read_team(slug).map(|r| r.team)
    }

    /// A team as a person types it: its slug, or its name in any case.
    pub fn find_team(&self, typed: &str) -> Option<Team> {
        let t = typed.trim().to_lowercase();
        let slugged = slugify(&t).ok();
        self.teams().into_iter().map(|r| r.team).find(|x| x.slug == t || x.name.to_lowercase() == t || Some(&x.slug) == slugged.as_ref())
    }

    pub(super) fn write_team(&self, t: &Team) -> Result<(), String> {
        std::fs::create_dir_all(self.teams_dir()).map_err(|e| e.to_string())?;
        std::fs::write(self.teams_dir().join(format!("{}.md", t.slug)), render_team(t)).map_err(|e| e.to_string())
    }

    /// Save a team. Without a slug it is a new team: it starts locked, as a style of
    /// your own does, at version 1. With one it is a change: a new version when the
    /// work changed (members, lines, settings, name), and an entry of its own when
    /// only the lock did. `by` is `you` or `evolution`.
    pub fn save_team(&self, team: Team, by: &str, why: &str) -> Result<Team, String> {
        let wants_locked = team.locked;
        let team = team.cleaned()?;
        let _guard = self.lock.lock().unwrap();
        if self.teams().iter().any(|o| o.team.slug != team.slug && o.team.name.to_lowercase() == team.name.to_lowercase()) {
            return Err(format!("There is already a team called \u{201c}{}\u{201d}.", team.name));
        }
        if team.slug.is_empty() {
            return self.make_team(team);
        }
        let before = self.team(&team.slug).ok_or("I can't find that team.")?;
        let after = Team { version: before.version, locked: before.locked, ..team };
        let done = if after == before {
            before
        } else {
            self.commit_team(&before, after, by, "version", if why.is_empty() { "changed it" } else { why })?
        };
        if wants_locked == done.locked {
            return Ok(done);
        }
        let locked = self.set_team_locked(&done, wants_locked)?;
        self.note_change(&format!("{} {}", if locked.locked { "Locked" } else { "Unlocked" }, locked.name));
        Ok(locked)
    }

    fn make_team(&self, mut team: Team) -> Result<Team, String> {
        let base = slugify(&team.name).unwrap_or_else(|_| "team".into());
        let mut slug = base.clone();
        let mut n = 2;
        // `history/team-<slug>` must not be a preset's history folder.
        while self.teams_dir().join(format!("{slug}.md")).exists() || self.preset(&format!("team-{slug}")).is_some() {
            slug = format!("{base}-{n}");
            n += 1;
        }
        team.slug = slug;
        team.version = 1;
        team.locked = true;
        self.write_team(&team)?;
        self.log_team(&team, "you", "original", "");
        self.note_change(&format!("Made the team \u{201c}{}\u{201d}", team.name));
        Ok(team)
    }

    /// Lock or unlock: an entry of its own in the history, not a version. Callers
    /// hold the lock and say what happened (`note_change`).
    pub(super) fn set_team_locked(&self, before: &Team, locked: bool) -> Result<Team, String> {
        self.ensure_team_baseline(before);
        let team = Team { locked, ..before.clone() };
        self.write_team(&team)?;
        self.log_team(&team, "you", if locked { "locked" } else { "unlocked" }, "");
        Ok(team)
    }

    /// A persona is going away: where it played a member, the place becomes a helper
    /// with the same job, and where it coordinated, Poiesis coordinates. Each change is a
    /// version of its team. Returns the names of the teams that changed. Stops at the
    /// first team that cannot be saved, so the caller can leave the persona where it is.
    pub fn release_persona_from_teams(&self, persona_id: &str, name: &str) -> Result<Vec<String>, String> {
        let who = format!("persona:{persona_id}");
        let mut changed = Vec::new();
        for TeamRead { mut team, .. } in self.teams() {
            let places = team.members.iter().filter(|m| m.persona == who).count();
            let leads = team.coordinator == who;
            if places == 0 && !leads {
                continue;
            }
            for m in team.members.iter_mut().filter(|m| m.persona == who) {
                m.persona = "helper".into();
            }
            if leads {
                team.coordinator = "poiesis".into();
            }
            let why = format!("{name} was deleted, so the place went to a helper");
            changed.push(team.name.clone());
            self.save_team(team, "you", &why)?;
        }
        Ok(changed)
    }

    /// Set a team aside. Its history stays. Returns the file name in the trash.
    pub fn delete_team(&self, slug: &str) -> Result<String, String> {
        let _guard = self.lock.lock().unwrap();
        let t = self.team(slug).ok_or("I can't find that team.")?;
        let trash = self.dir().join(".trash");
        std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
        let file = format!("{}-team-{}.md", chrono::Utc::now().timestamp_millis(), t.slug);
        std::fs::rename(self.teams_dir().join(format!("{}.md", t.slug)), trash.join(&file)).map_err(|e| e.to_string())?;
        self.note_change(&format!("Deleted {}", t.name));
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::ways::{Back, Move, PresetRule, Trigger};

    /// The file of Part I.4, without its explanatory comments.
    const SAMPLE: &str = "---\nname: Shop team\nline: Builds and checks web shop features\ncoordinator: persona:7f3c\nstyle: lean\nmember_style: lean\nshape: hub\nreport: each-item\nlimits: { at_once: 3, budget: 2.00, minutes: 90, rounds: 3, ask_cost: yes }\nfolder: copies\nlocked: yes\nversion: 4\n---\n## Members\n- api: persona:a19b \u{b7} Builds the API \u{b7} owns: src/api/\n- ui: persona:c02e \u{b7} Builds the screens \u{b7} owns: src/ui/\n- verifier: persona:5d77 \u{b7} Checks every change\n- docs: helper \u{b7} Keeps the README current\n\n## Lines\n- coordinator hands-over api\n- coordinator hands-over ui\n- coordinator hands-over docs\n- verifier reviews api\n- verifier reviews ui\n- coordinator reports you\n- you talks coordinator\n";

    fn store() -> (MemoryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_ways_teams_{}", uuid::Uuid::new_v4().simple()));
        let s = MemoryStore::new(&dir).unwrap();
        s.ensure_ways();
        (s, dir)
    }

    fn member(role: &str, who: &str) -> Member {
        Member { role: role.into(), persona: who.into(), job: String::new(), owns: Vec::new() }
    }

    /// A team that is not saved yet.
    fn draft(name: &str) -> Team {
        let members = vec![member("api", "persona:a"), member("verifier", "helper")];
        Team {
            slug: String::new(),
            name: name.into(),
            line: String::new(),
            coordinator: "poiesis".into(),
            style: String::new(),
            member_style: String::new(),
            shape: Shape::Review,
            report: Report::EachItem,
            limits: Limits::default(),
            folder: Folder::Copies,
            locked: false,
            version: 0,
            lines: shape_lines(Shape::Review, &members),
            members,
        }
    }

    #[test]
    fn the_team_of_the_plan_reads_and_writes_back_exactly() {
        let (team, problems) = parse_team("shop-team", SAMPLE).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!((team.name.as_str(), team.version, team.locked), ("Shop team", 4, true));
        assert_eq!(team.coordinator_persona(), Some("7f3c"));
        assert_eq!(team.members.len(), 4);
        assert_eq!(team.members[0].owns, vec!["src/api/"]);
        assert_eq!(team.members[3].persona, "helper");
        assert_eq!(team.members[2].job, "Checks every change");
        assert_eq!(team.limits, Limits::default());
        assert_eq!(team.lines.len(), 7);
        assert_eq!(render_team(&team), SAMPLE, "stable to the byte");
        assert_eq!(parse_team("shop-team", &render_team(&team)).unwrap().0, team);
    }

    #[test]
    fn a_line_that_cannot_be_read_adds_no_line_and_is_reported() {
        let text = SAMPLE.replace(
            "- coordinator hands-over docs\n",
            "- coordinator hands-over docs\n- api hands-over ghost\n- coordinator hands-over\n- api flies ui\n- you hands-over api\n- api talks coordinator\n- ui asks ui\n- api reports you\n",
        );
        let (team, problems) = parse_team("shop-team", &text).unwrap();
        let (clean, _) = parse_team("shop-team", SAMPLE).unwrap();
        // Only the one readable extra line (`api reports you`) got in; none of the six others did.
        let mut expected = clean.lines.clone();
        expected.insert(3, Line::new(LineKind::Reports, "api", "you"));
        assert_eq!(team.lines, expected);
        assert_eq!(problems.len(), 6, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("api hands-over ghost") && p.contains("nobody here has that name")));
        assert!(problems.iter().any(|p| p.contains("api flies ui") && p.contains("I don't know that kind of line")));
        assert!(problems.iter().any(|p| p.contains("you hands-over api")));
    }

    #[test]
    fn a_member_that_cannot_be_read_is_left_out_and_so_are_the_lines_that_need_it() {
        let text = SAMPLE
            .replace("- docs: helper", "- docs: someone")
            .replace("- ui: persona:c02e", "- Not A Role: persona:c02e")
            .replace("- api: persona:a19b", "- verifier: persona:a19b");
        let (team, problems) = parse_team("shop-team", &text).unwrap();
        let roles: Vec<&str> = team.members.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["verifier"], "the first verifier stays, the second is a duplicate");
        for l in &team.lines {
            assert!(
                ["verifier", "coordinator", "you"].contains(&l.from.as_str()) && ["verifier", "coordinator", "you"].contains(&l.to.as_str()),
                "{}",
                l.text()
            );
        }
        assert!(problems.iter().any(|p| p.contains("I can't tell who plays it")));
        assert!(problems.iter().any(|p| p.contains("not a role name")));
        assert!(problems.iter().any(|p| p.contains("verifier is there already")));
    }

    #[test]
    fn a_field_that_cannot_be_read_takes_its_safe_default_and_is_reported() {
        let text = SAMPLE
            .replace("shape: hub", "shape: ring")
            .replace("folder: copies", "folder: shared")
            .replace("coordinator: persona:7f3c", "coordinator: Mira")
            .replace("at_once: 3, budget: 2.00", "at_once: lots, budget: -1");
        let (team, problems) = parse_team("shop-team", &text).unwrap();
        assert_eq!((team.shape, team.folder, team.report), (Shape::Hub, Folder::Copies, Report::EachItem));
        assert_eq!(team.coordinator, "poiesis");
        assert_eq!((team.limits.at_once, team.limits.budget), (3, 2.0), "{:?}", team.limits);
        assert_eq!(problems.len(), 4, "{problems:?}");
        // No name, no team.
        assert!(parse_team("x", "---\nline: nothing\n---\n").is_none());
    }

    #[test]
    fn the_two_fixed_lines_are_always_there() {
        let bare = "---\nname: Bare\n---\n## Members\n- api: helper\n\n## Lines\n- coordinator hands-over api\n";
        let (team, problems) = parse_team("bare", bare).unwrap();
        assert!(problems.is_empty());
        assert_eq!(&team.lines[1..], &fixed_lines());
        assert!(render_team(&team).contains("- coordinator reports you\n- you talks coordinator\n"));

        // A file that placed them first keeps its order, and one written without them gets them.
        let mut stripped = team.clone();
        stripped.lines.clear();
        assert!(render_team(&stripped).contains("- you talks coordinator"));
        let (s, dir) = store();
        let mut t = draft("Fixed");
        t.lines.clear();
        let saved = s.save_team(t, "you", "").unwrap();
        assert_eq!(saved.lines, fixed_lines());
        let mut t = saved.clone();
        t.lines.retain(|l| l.kind != LineKind::Talks && l.kind != LineKind::Reports);
        let again = s.save_team(t, "you", "").unwrap();
        assert!(again.lines.contains(&Line::new(LineKind::Talks, "you", "coordinator")));
        assert!(again.lines.contains(&Line::new(LineKind::Reports, "coordinator", "you")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_shapes_give_the_lines_the_plan_says() {
        use LineKind::*;
        let m = [member("a", "helper"), member("b", "helper"), member("c", "helper")];
        let l = |k, f: &str, t: &str| Line::new(k, f, t);
        let [rep, talk] = fixed_lines();

        assert_eq!(
            shape_lines(Shape::Hub, &m),
            vec![l(HandsOver, "coordinator", "a"), l(HandsOver, "coordinator", "b"), l(HandsOver, "coordinator", "c"), rep.clone(), talk.clone()]
        );
        assert_eq!(
            shape_lines(Shape::Pipeline, &m),
            vec![l(HandsOver, "coordinator", "a"), l(HandsOver, "a", "b"), l(HandsOver, "b", "c"), l(Reports, "c", "coordinator"), rep.clone(), talk.clone()]
        );
        assert_eq!(
            shape_lines(Shape::Review, &m),
            vec![l(HandsOver, "coordinator", "a"), l(HandsOver, "coordinator", "b"), l(Reviews, "c", "a"), l(Reviews, "c", "b"), rep.clone(), talk.clone()]
        );
        assert_eq!(shape_lines(Shape::Review, &m[..1]), shape_lines(Shape::Hub, &m[..1]), "one member has nobody to review");

        let open = shape_lines(Shape::Open, &m[..2]);
        // Three people (coordinator, a, b): six hand-overs, six questions, you talk with both, the fixed two.
        assert_eq!(open.len(), 16, "{open:?}");
        assert!(open.contains(&l(Asks, "a", "coordinator")) && open.contains(&l(HandsOver, "b", "a")) && open.contains(&l(Talks, "you", "b")));
        assert!(!open.iter().any(|x| x.from == x.to));

        assert!(shape_lines(Shape::Pipeline, &[]).len() == 2, "no members: only the fixed lines");
        // Every shape's lines are lines the reader accepts.
        for shape in Shape::ALL {
            for line in shape_lines(shape, &m) {
                assert!(Line::read(&line.text(), &["a", "b", "c"]).is_ok(), "{}", line.text());
            }
        }
    }

    #[test]
    fn a_new_team_starts_locked_and_a_change_is_a_version_in_the_teams_own_history() {
        let (s, dir) = store();
        let t = s.save_team(draft("Shop team"), "you", "").unwrap();
        assert_eq!((t.slug.as_str(), t.version, t.locked), ("shop-team", 1, true));
        assert!(dir.join("memory/ways/teams/shop-team.md").exists(), "a plain file");
        assert_eq!(s.teams().len(), 1);
        assert_eq!(s.find_team("SHOP team").map(|t| t.slug), Some("shop-team".to_string()));

        let mut more = t.clone();
        more.members.push(member("docs", "helper"));
        more.lines.insert(0, Line::new(LineKind::HandsOver, "coordinator", "docs"));
        let v2 = s.save_team(more, "you", "").unwrap();
        assert_eq!(v2.version, 2);
        assert!(v2.locked, "changing the work does not change the lock");
        assert_eq!(s.save_team(v2.clone(), "you", "").unwrap().version, 2, "saving the same thing is not a version");

        let h = s.team_history("shop-team");
        assert_eq!(h.iter().map(|e| (e.what.as_str(), e.version)).collect::<Vec<_>>(), vec![("version", 2), ("original", 1)]);
        assert_eq!(h[0].by, "you");
        assert_eq!(h[0].why, "changed it");
        assert!(h[0].added.contains(&"docs: helper".to_string()) && h[0].added.contains(&"coordinator hands-over docs".to_string()), "{:?}", h[0].added);
        assert!(h[0].removed.is_empty());
        assert!(dir.join("memory/ways/history/team-shop-team/0001.md").exists());
        assert!(dir.join("memory/ways/history/team-shop-team/0002.md").exists());

        // Lock and unlock are entries of their own.
        let open = s.save_team(Team { locked: false, ..v2.clone() }, "you", "").unwrap();
        assert_eq!((open.locked, open.version), (false, 2));
        assert_eq!(s.team_history("shop-team")[0].what, "unlocked");
        s.save_team(Team { locked: true, ..open }, "you", "").unwrap();
        assert_eq!(s.team_history("shop-team")[0].what, "locked");

        // Styles are not teams, and their history is where it was.
        assert_eq!(s.presets().len(), 5);
        assert!(s.preset_history("shop-team").is_empty());
        s.set_preset_rule("lean", PresetRule { when: Trigger::ManyChanges, mv: Move::Split }, false).unwrap();
        assert!(dir.join("memory/ways/history/lean/0001.md").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn going_back_is_a_new_version_and_can_be_gone_back_from() {
        let (s, dir) = store();
        let t = s.save_team(draft("Shop team"), "you", "").unwrap();
        assert!(s.go_back_team("shop-team", Back::Original).is_err(), "the original is what it is");
        let mut changed = t.clone();
        changed.report = Report::End;
        changed.limits.rounds = 5;
        changed.members.retain(|m| m.role != "api");
        changed.lines.retain(|l| l.from != "api" && l.to != "api");
        changed.name = "Shop crew".into();
        s.save_team(changed, "you", "").unwrap();

        let back = s.go_back_team("shop-team", Back::Original).unwrap();
        assert_eq!(back.version, 3);
        assert_eq!(back.name, "Shop crew", "a name is not part of the work");
        assert_eq!((back.report, back.limits.rounds, back.members.len()), (Report::EachItem, 3, 2));
        assert!(s.go_back_team("shop-team", Back::Original).is_err(), "already there");
        let v2 = s.team_history("shop-team").into_iter().find(|e| e.version == 2).unwrap();
        let undone = s.go_back_team("shop-team", Back::Entry(v2.seq)).unwrap();
        assert_eq!((undone.version, undone.report, undone.members.len()), (4, Report::End, 1));
        let top = &s.team_history("shop-team")[0];
        assert_eq!((top.what.as_str(), top.why.as_str()), ("back", "went back to version 2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_a_person_saves_is_checked_and_a_name_is_taken_once() {
        let (s, dir) = store();
        let bad = |f: &dyn Fn(&mut Team)| {
            let mut t = draft("X");
            f(&mut t);
            s.save_team(t, "you", "")
        };
        assert!(bad(&|t| t.name = "  ".into()).is_err());
        assert!(bad(&|t| t.members.push(member("api", "helper"))).unwrap_err().contains("Two members"));
        assert!(bad(&|t| t.members[0].role = "coordinator".into()).is_err(), "a reserved word is no role");
        assert!(bad(&|t| t.members[0].persona = "somebody".into()).is_err());
        assert!(bad(&|t| t.coordinator = "Mira".into()).is_err());
        assert!(bad(&|t| t.lines.push(Line::new(LineKind::HandsOver, "api", "ghost"))).unwrap_err().contains("does not fit"));
        assert!(bad(&|t| t.lines.push(Line::new(LineKind::Talks, "api", "verifier"))).is_err());
        assert!(bad(&|t| t.members[0].owns = vec!["a,b".into()]).is_err());
        assert!(s.teams().is_empty(), "nothing was written");

        s.save_team(draft("Shop team"), "you", "").unwrap();
        assert!(s.save_team(draft("shop TEAM"), "you", "").unwrap_err().contains("already a team"));
        let mut ghost = draft("Ghost");
        ghost.slug = "no-such".into();
        assert!(s.save_team(ghost, "you", "").is_err());
        // Limits are kept in range.
        let mut loose = draft("Loose");
        loose.limits = Limits { at_once: 99, budget: f64::NAN, minutes: 0, rounds: 0, ask_cost: false };
        let saved = s.save_team(loose, "you", "").unwrap();
        assert_eq!((saved.limits.at_once, saved.limits.budget, saved.limits.minutes, saved.limits.rounds), (5, 2.0, 1, 1));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_style_and_a_team_never_share_a_history_folder() {
        let (s, dir) = store();
        let style = s.create_preset("Team shop", "", None, &[], true).unwrap();
        assert_eq!(style.slug, "team-shop");
        let t = s.save_team(draft("Shop"), "you", "").unwrap();
        assert_eq!(t.slug, "shop-2", "history/team-shop belongs to the style");
        let other = s.create_preset("Team shop two", "", None, &[], true).unwrap();
        assert_eq!(other.slug, "team-shop-two");
        let t2 = s.save_team(draft("Two"), "you", "").unwrap();
        let clash = s.create_preset("Team two", "", None, &[], true).unwrap();
        assert_eq!((t2.slug.as_str(), clash.slug.as_str()), ("two", "team-two-2"), "history/team-two belongs to the team");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_blank_team_is_a_hub_with_the_usual_limits_and_can_be_saved_once_named() {
        let (s, dir) = store();
        let blank = Team::blank();
        assert_eq!((blank.shape, blank.coordinator.as_str(), blank.members.len()), (Shape::Hub, "poiesis", 0));
        assert_eq!(blank.limits, Limits::default());
        assert_eq!(blank.lines, fixed_lines());
        assert!(s.save_team(blank.clone(), "you", "").is_err(), "a team needs a name");
        let made = s.save_team(Team { name: "Desk".into(), ..blank }, "you", "").unwrap();
        assert_eq!((made.slug.as_str(), made.locked), ("desk", true));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_deleted_persona_leaves_a_helper_with_the_same_job_in_its_teams() {
        let (s, dir) = store();
        let mut t = draft("Shop team");
        t.coordinator = "persona:mira".into();
        t.members = vec![member("api", "persona:mira"), member("docs", "persona:kai")];
        t.members[0].job = "Builds the API".into();
        t.lines = shape_lines(Shape::Hub, &t.members);
        s.save_team(t, "you", "").unwrap();
        s.save_team(draft("Other"), "you", "").unwrap();

        let changed = s.release_persona_from_teams("mira", "Mira").unwrap();
        assert_eq!(changed, vec!["Shop team".to_string()]);
        let after = s.team("shop-team").unwrap();
        assert_eq!((after.members[0].persona.as_str(), after.members[0].job.as_str()), ("helper", "Builds the API"));
        assert_eq!(after.members[1].persona, "persona:kai", "somebody else's place stays");
        assert_eq!(after.coordinator, "poiesis");
        assert_eq!(after.version, 2);
        assert!(s.team_history("shop-team")[0].why.contains("Mira was deleted"));
        assert!(s.release_persona_from_teams("mira", "Mira").unwrap().is_empty(), "nothing left to change");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_deleted_team_is_set_aside_and_its_file_is_gone() {
        let (s, dir) = store();
        let t = s.save_team(draft("Shop team"), "you", "").unwrap();
        let file = s.delete_team(&t.slug).unwrap();
        assert!(s.team(&t.slug).is_none());
        assert!(dir.join("memory/.trash").join(file).exists());
        assert!(s.delete_team(&t.slug).is_err());
        assert!(s.read_team("../x").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
