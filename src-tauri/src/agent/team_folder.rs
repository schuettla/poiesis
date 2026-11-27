//! `TEAM-13`: the folder a team works in (Part I.7).
//!
//! Two ways, chosen by the team's `folder`:
//!
//! - **`copies`:** each member works in a copy of the project folder (`branch.rs`, as "try two
//!   ways" does), one per member chat, so a hand-over that goes on in the same chat goes on in
//!   the same copy. When a member's item counts as done (its reviewer kept it, or it has no
//!   reviewer and finished) the coordinator's loop writes the member's changes into the main
//!   folder. The copy remembers what the folder held when it was made, so a file that another
//!   member (or the person) changed meanwhile is told apart from one only this member touched:
//!   that one is never overwritten. It becomes an item for the coordinator, in the plan when
//!   there is one and in the report either way.
//! - **`owners`:** members work in the main folder. A write, move or delete outside the
//!   member's `owns:` places, or inside someone else's, is refused by the file tools.
//!
//! Where it does not apply (a team with no folder, the coordinator, a run that is not a team's)
//! nothing here does anything.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::agent::branch::{self, Base, Changed};
use crate::agent::ledger::Ledger;
use crate::agent::team;
use crate::db::Db;
use crate::memory::ways::{Folder, Team};
use crate::memory::MemoryStore;

/// How much of a conflicting file's copy the coordinator reads.
const PATCH_CLIP: usize = 6_000;

// ---- copies ----

/// Where a member chat's copy is.
pub fn copy_dir(data_dir: &Path, chat: &str) -> PathBuf {
    branch::workdir(data_dir, chat).join("copy")
}

fn base_file(data_dir: &Path, chat: &str) -> PathBuf {
    branch::workdir(data_dir, chat).join("base.json")
}

fn load_base(data_dir: &Path, chat: &str) -> Option<Base> {
    serde_json::from_str(&std::fs::read_to_string(base_file(data_dir, chat)).ok()?).ok()
}

fn save_base(data_dir: &Path, chat: &str, base: &Base) {
    if let Ok(json) = serde_json::to_string(base) {
        let _ = std::fs::write(base_file(data_dir, chat), json);
    }
}

/// The copy this member chat works in: the one it has, or a new one of `main`. `Err` says why
/// there is none (a folder too big to copy), and the member works in the main folder.
pub fn make_copy(data_dir: &Path, main: &Path, chat: &str) -> Result<PathBuf, String> {
    if !branch::clean_id(chat) {
        return Err("The member's chat has no safe name for a copy.".into());
    }
    let dir = copy_dir(data_dir, chat);
    if dir.is_dir() && load_base(data_dir, chat).is_some() {
        return Ok(dir);
    }
    let _ = std::fs::remove_dir_all(branch::workdir(data_dir, chat));
    branch::copy_folder(main, &dir)?;
    save_base(data_dir, chat, &branch::base_of(&dir));
    Ok(dir)
}

/// The copy `role` works in for this session, when it has one: where a reviewer reads the work.
pub fn copy_of(db: &Db, data_dir: &Path, session: &str, role: &str) -> Option<PathBuf> {
    let run = db.list_subagent_runs(session).ok()?.into_iter().rev().find(|r| r.member.as_deref() == Some(role))?;
    Some(copy_dir(data_dir, &run.child_conversation_id)).filter(|d| d.is_dir())
}

/// Is `folder` this chat's own copy? Nothing written there reaches the person's folder until the loop
/// brings it in, so a run nobody watches works in it without asking (`TEAM-19`); what is brought in
/// waits for the person instead (`merge_done` with `headless`).
pub fn is_own_copy(data_dir: &Path, chat: &str, folder: Option<&Path>) -> bool {
    let Some(folder) = folder else { return false };
    let copy = copy_dir(data_dir, chat);
    branch::clean_id(chat) && copy.is_dir() && crate::permissions::canonicalize_lenient(&copy) == folder
}

// ---- conflicts as items ----

/// Two members, or a member and the folder, changed the same files: what the coordinator is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// For the plan: "api and ui both changed `src/app.ts`".
    pub item: String,
    /// The same, with what the member's copy has, for the report.
    pub text: String,
    /// Something for the coordinator to decide, so an item on its plan. A note that work waits for
    /// the person is only said in the report.
    pub on_plan: bool,
}

fn conflicts() -> &'static Mutex<Vec<(String, Conflict)>> {
    static C: OnceLock<Mutex<Vec<(String, Conflict)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

/// What the loop owes the coordinator since the last call, for the session.
pub fn take_conflicts(session: &str) -> Vec<Conflict> {
    let Ok(mut all) = conflicts().lock() else { return Vec::new() };
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *all).into_iter().partition(|(s, _)| s == session);
    *all = rest;
    mine.into_iter().map(|(_, c)| c).collect()
}

/// Who last wrote each file of the main folder by a merge, per session: so a conflict can say
/// "api and ui" and not only "api and the main folder".
fn written() -> &'static Mutex<HashMap<String, HashMap<String, String>>> {
    static W: OnceLock<Mutex<HashMap<String, HashMap<String, String>>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(HashMap::new()))
}

fn written_by(session: &str, rel: &str) -> Option<String> {
    written().lock().ok()?.get(session)?.get(rel).cloned()
}

fn merged_runs() -> &'static Mutex<HashSet<String>> {
    static M: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashSet::new()))
}

/// What a merge of one copy did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Merged {
    pub written: usize,
    pub conflicts: Vec<Conflict>,
}

/// Write what `role`'s copy changed into `main`, except the files the folder changed too.
/// `None` when the chat has no copy.
///
/// `headless`: nobody is there to say yes to a change in the folder, so nothing is written. The clean
/// changes wait for the person as one entry (`filesystem::leave_bring_in`) that carries the files'
/// contents, and the coordinator is told that they wait. They count as brought in for this copy either
/// way: the copy may be gone by the time the person answers.
#[allow(clippy::too_many_arguments)]
fn merge_copy(db: &Db, data_dir: &Path, session: &str, ledger: &Ledger, main: &Path, chat: &str, role: &str, headless: bool) -> Option<Merged> {
    let copy = copy_dir(data_dir, chat);
    let mut base = load_base(data_dir, chat).filter(|_| copy.is_dir())?;
    let changes = branch::changes_since(&base, &copy);
    if changes.is_empty() {
        return Some(Merged::default());
    }
    let split = branch::split(&base, main, &copy, changes);
    let mut notes = Vec::new();
    let written_now = if headless {
        notes.extend(leave_for_the_person(db, session, &copy, role, &base, &split.clean));
        0
    } else {
        let n = branch::apply(db, data_dir, session, ledger, main, &copy, &split.clean);
        if let Ok(mut all) = written().lock() {
            let by = all.entry(session.to_string()).or_default();
            for c in &split.clean {
                by.insert(c.rel.clone(), role.to_string());
            }
        }
        n
    };
    let done: Vec<Changed> = split.clean.iter().chain(&split.same).cloned().collect();
    branch::advance(&mut base, &copy, &done);
    save_base(data_dir, chat, &base);

    // One item for each other party: "api and ui both changed `a`, `b`".
    let mut by_other: Vec<(String, Vec<Changed>)> = Vec::new();
    for c in split.conflicts {
        let other = written_by(session, &c.rel).filter(|o| o != role).unwrap_or_else(|| "the main folder".to_string());
        match by_other.iter_mut().find(|(o, _)| *o == other) {
            Some((_, files)) => files.push(c),
            None => by_other.push((other, vec![c])),
        }
    }
    let conflicts = by_other
        .into_iter()
        .map(|(other, files)| {
            let names = files.iter().map(|c| format!("`{}`", c.rel)).collect::<Vec<_>>().join(", ");
            let item = format!("{role} and {other} both changed {names}");
            let patch = branch::patch(main, &copy, &files);
            let patch: String = patch.chars().take(PATCH_CLIP).collect();
            let text = format!(
                "{item}. Nothing was overwritten: {role}'s version of {} is not in the folder. What {role}'s copy has, against the folder now:\n{patch}\n\nDecide which version stays, or put the two together.",
                if files.len() == 1 { "it" } else { "them" }
            );
            Conflict { item, text, on_plan: true }
        })
        .collect::<Vec<_>>();
    Some(Merged { written: written_now, conflicts: notes.into_iter().chain(conflicts).collect() })
}

/// The largest file, and the most in all, one entry carries for the person to bring in.
const BRING_IN_FILE_CAP: u64 = 4 * 1024 * 1024;
const BRING_IN_TOTAL_CAP: u64 = 16 * 1024 * 1024;

/// `headless`: leave `role`'s clean changes for the person as one entry, and say so to the coordinator.
/// A file too large to carry is left out, and that is said too.
fn leave_for_the_person(db: &Db, session: &str, copy: &Path, role: &str, base: &Base, clean: &[Changed]) -> Option<Conflict> {
    if clean.is_empty() {
        return None;
    }
    let mut files = Vec::new();
    let mut left_out = Vec::new();
    let mut total = 0u64;
    for c in clean {
        // The folder holds what the copy started from (`split` said so); the person's yes checks
        // that it still does.
        let expect = base.get(&c.rel).cloned();
        let bytes = match c.kind {
            branch::Kind::Deleted => None,
            _ => match std::fs::metadata(copy.join(&c.rel)) {
                Ok(m) if m.len() <= BRING_IN_FILE_CAP && total + m.len() <= BRING_IN_TOTAL_CAP => {
                    total += m.len();
                    std::fs::read(copy.join(&c.rel)).ok()
                }
                _ => None,
            },
        };
        if c.kind != branch::Kind::Deleted && bytes.is_none() {
            left_out.push(format!("`{}`", c.rel));
            continue;
        }
        files.push(crate::agent::filesystem::BringIn { rel: c.rel.clone(), bytes, expect });
    }
    let mut said = Vec::new();
    if !files.is_empty() {
        let names = files.iter().map(|f| format!("`{}`", f.rel)).collect::<Vec<_>>().join(", ");
        said.push(match crate::agent::filesystem::leave_bring_in(db, session, role, &files) {
            Ok(()) => format!(
                "{role}'s changes to {names} wait for the person to approve: nobody is here, so they are not in the folder yet. Say in your answer that they wait."
            ),
            Err(why) => format!("{role}'s changes to {names} were not brought in: {why}"),
        });
    }
    if !left_out.is_empty() {
        let one = left_out.len() == 1;
        said.push(format!(
            "{} from {role} {} too large to leave for the person, so {} not brought in.",
            left_out.join(", "),
            if one { "is" } else { "are" },
            if one { "it was" } else { "they were" }
        ));
    }
    Some(Conflict { item: format!("{role}'s changes wait for the person"), text: said.join(" "), on_plan: false })
}

/// Does the item of `rows[at]` count as done? It finished, and its reviewer kept it or it has
/// no reviewer.
fn counts_done(team: &Team, rows: &[crate::db::SubagentRun], at: usize) -> bool {
    let run = &rows[at];
    let Some(role) = run.member.as_deref() else { return false };
    if run.ended_at.is_none() || run.stop_reason.as_deref() != Some("completed") {
        return false;
    }
    match team::reviewer_of(team, role) {
        None => true,
        Some(_) => matches!(team::standing(team, rows, at), Some((team::Standing::Kept(_), _))),
    }
}

/// Is the work of `rows[at]` there to be brought in? It is the latest run of its chat (the one
/// that stands for it), its copy is there, and its item counts as done.
fn ready(data_dir: &Path, team: &Team, rows: &[crate::db::SubagentRun], at: usize) -> bool {
    let run = &rows[at];
    run.member.is_some()
        && !rows[at + 1..].iter().any(|x| x.child_conversation_id == run.child_conversation_id)
        && copy_dir(data_dir, &run.child_conversation_id).is_dir()
        && counts_done(team, rows, at)
}

/// `PAR-5`: the folder `merge_done` would write into now, when it has something to bring in.
pub fn to_merge(db: &Db, data_dir: &Path, session: &str, team: &Team) -> Option<PathBuf> {
    if team.folder != Folder::Copies {
        return None;
    }
    let main = db.conversation_folder(session).ok().and_then(|(f, _)| f).map(PathBuf::from)?;
    let rows = db.list_subagent_runs(session).unwrap_or_default();
    let due = (0..rows.len()).any(|at| {
        ready(data_dir, team, &rows, at) && merged_runs().lock().is_ok_and(|s| !s.contains(&rows[at].id))
    });
    due.then_some(main)
}

/// Bring in the work of every member chat whose latest item counts as done and is not merged
/// yet. What cannot be brought in is queued for the coordinator (`take_conflicts`). Returns how
/// many files were written. Does nothing for a team that is not on `copies`. In a run nobody watches
/// (`headless`) nothing is written: the work waits for the person (`merge_copy`).
pub fn merge_done(db: &Db, data_dir: &Path, session: &str, ledger: &Ledger, team: &Team, headless: bool) -> usize {
    if team.folder != Folder::Copies {
        return 0;
    }
    let Some(main) = db.conversation_folder(session).ok().and_then(|(f, _)| f).map(PathBuf::from) else { return 0 };
    let rows = db.list_subagent_runs(session).unwrap_or_default();
    let mut total = 0;
    for at in 0..rows.len() {
        let run = &rows[at];
        let Some(role) = run.member.as_deref() else { continue };
        if !ready(data_dir, team, &rows, at) {
            continue;
        }
        if !merged_runs().lock().map(|mut s| s.insert(run.id.clone())).unwrap_or(false) {
            continue;
        }
        if let Some(merged) = merge_copy(db, data_dir, session, ledger, &main, &run.child_conversation_id, role, headless) {
            total += merged.written;
            if let Ok(mut all) = conflicts().lock() {
                all.extend(merged.conflicts.into_iter().map(|c| (session.to_string(), c)));
            }
        }
    }
    total
}

/// A new task begins: what the old task's members did is brought in if it was done, and their
/// copies are deleted the way "try two ways" deletes its own. Work that was not done (a member
/// that did not finish, a review that sent it back) stays out of the folder; its chat still shows
/// what it changed.
pub fn sweep(db: &Db, mem: &MemoryStore, data_dir: &Path, session: &str, turn: Option<&str>) {
    let Some(team) = team::of_session(db, mem, session) else { return };
    if team.folder != Folder::Copies {
        return;
    }
    let b = team::boundary(db, session, turn);
    let rows = db.list_subagent_runs(session).unwrap_or_default();
    let old: Vec<&crate::db::SubagentRun> =
        rows.iter().filter(|r| r.member.is_some() && r.ended_at.is_some() && !team::in_task(r, &b)).collect();
    if old.is_empty() {
        return;
    }
    // The person started this task, so the old task's work is brought in as in a watched run.
    merge_done(db, data_dir, session, &Ledger::default(), &team, false);
    for run in old {
        if branch::clean_id(&run.child_conversation_id) {
            let _ = std::fs::remove_dir_all(branch::workdir(data_dir, &run.child_conversation_id));
        }
    }
}

// ---- owners ----

fn norm(s: &str) -> String {
    s.trim().replace('\\', "/").trim_start_matches("./").trim_matches('/').to_lowercase()
}

/// Is `rel` (as `norm`) the owned place or inside it?
fn within(rel: &str, owned: &str) -> bool {
    !owned.is_empty() && (rel == owned || rel.strip_prefix(owned).is_some_and(|r| r.starts_with('/')))
}

/// Why a change to `paths` is refused to the member whose chat this is, or `None` when it may
/// go ahead. `folder` is the chat's own working folder. Only a team member's chat is ever
/// refused: the coordinator and every other run are not touched.
pub fn refusal(db: &Db, mem: &MemoryStore, chat: &str, folder: Option<&Path>, paths: &[&Path]) -> Option<String> {
    let session = db.get_conversation(chat).ok().flatten()?.parent_conversation_id?;
    let team = team::of_session(db, mem, &session)?;
    let role = db
        .list_subagent_runs(&session)
        .ok()?
        .into_iter()
        .rev()
        .find(|r| r.child_conversation_id == chat && r.member.is_some())?
        .member?;
    let me = team.members.iter().find(|m| m.role == role)?;
    let folder = folder?;

    match team.folder {
        Folder::Copies => {
            // The member's own copy is its folder. Writing into the main folder instead would
            // be the very overwrite the copy is there to prevent.
            let main = db.conversation_folder(&session).ok().and_then(|(f, _)| f)?;
            let main = crate::permissions::canonicalize_lenient(Path::new(&main));
            if main == folder {
                return None;
            }
            paths.iter().find(|p| p.starts_with(&main) && !p.starts_with(folder)).map(|p| {
                let rel = p.strip_prefix(&main).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                format!("{rel} is in the main folder. {role} works in its own copy in this team, and the coordinator brings the changes over.")
            })
        }
        Folder::Owners => {
            for path in paths {
                let Ok(rel) = path.strip_prefix(folder) else { continue };
                let rel = norm(&rel.to_string_lossy());
                if rel.is_empty() {
                    continue;
                }
                if me.owns.iter().any(|o| within(&rel, &norm(o))) {
                    continue;
                }
                // Somebody else's place, named the way the team file names it.
                if let Some((owned, owner)) = team
                    .members
                    .iter()
                    .filter(|m| m.role != role)
                    .find_map(|m| m.owns.iter().find(|o| within(&rel, &norm(o))).map(|o| (o.trim().to_string(), m.role.clone())))
                {
                    return Some(format!("{owned} belongs to {owner} in this team."));
                }
                // Nobody's place. A member with places of its own keeps to them; one with none
                // may work wherever nobody else owns.
                if !me.owns.is_empty() {
                    return Some(format!(
                        "{} is outside what {role} owns in this team: {}.",
                        rel_display(path, folder),
                        me.owns.join(", ")
                    ));
                }
            }
            None
        }
    }
}

fn rel_display(path: &Path, folder: &Path) -> String {
    path.strip_prefix(folder).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::team::Standing;
    use crate::memory::ways::{Limits, Line, LineKind, Member};

    fn scratch(name: &str) -> PathBuf {
        let dir = crate::permissions::canonicalize_lenient(&std::env::temp_dir())
            .join(format!("poiesis_teamfolder_{name}_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn project() -> PathBuf {
        let root = scratch("project");
        std::fs::create_dir_all(root.join("src/ui")).unwrap();
        std::fs::create_dir_all(root.join("src/api")).unwrap();
        std::fs::write(root.join("src/app.ts"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(root.join("src/ui/button.ts"), "button\n").unwrap();
        std::fs::write(root.join("src/api/routes.ts"), "routes\n").unwrap();
        std::fs::write(root.join("README.md"), "readme\n").unwrap();
        root
    }

    fn team(folder: Folder, reviewed: bool) -> Team {
        let mut t = Team::blank();
        t.slug = "shop-team".into();
        t.name = "Shop team".into();
        t.folder = folder;
        t.limits = Limits { at_once: 2, ..Limits::default() };
        let member = |role: &str, owns: &[&str]| Member {
            role: role.into(),
            persona: "helper".into(),
            job: String::new(),
            owns: owns.iter().map(|s| s.to_string()).collect(),
        };
        t.members = vec![member("api", &["src/api/"]), member("ui", &["src/ui/"]), member("docs", &[]), member("verifier", &[])];
        t.lines = vec![
            Line::new(LineKind::HandsOver, "coordinator", "api"),
            Line::new(LineKind::HandsOver, "coordinator", "ui"),
            Line::new(LineKind::HandsOver, "coordinator", "docs"),
        ];
        if reviewed {
            t.lines.push(Line::new(LineKind::Reviews, "verifier", "api"));
        }
        t
    }

    fn memory() -> MemoryStore {
        let dir = std::env::temp_dir().join(format!("poiesis_teamfolder_mem_{}", uuid::Uuid::new_v4().simple()));
        let mem = MemoryStore::new(&dir).unwrap();
        mem.ensure_ways();
        mem
    }

    /// A session a team leads, with the project as its folder.
    fn session(db: &Db, mem: &MemoryStore, team: Team, main: &Path) -> (String, Team) {
        let team = mem.save_team(Team { slug: String::new(), ..team }, "you", "").unwrap();
        let session = db.create_conversation("s", None, false).unwrap().id;
        crate::agent::leader::set_leader(db, mem, &session, Some(&crate::agent::leader::Leader::Team(team.slug.clone()))).unwrap();
        db.set_conversation_folder(&session, Some(&main.to_string_lossy())).unwrap();
        (session, team)
    }

    /// A finished member run with a chat of its own working in `folder`.
    fn member_run(db: &Db, session: &str, role: &str, task: &str, folder: &Path, done: Option<&str>) -> (String, String) {
        let chat = db.create_conversation(role, None, false).unwrap().id;
        db.set_conversation_parent(&chat, session).unwrap();
        db.set_conversation_folder(&chat, Some(&folder.to_string_lossy())).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        db.create_subagent_run(&id, session, Some("turn-1"), &chat, role, task).unwrap();
        db.set_subagent_member(&id, role, Some(1)).unwrap();
        if let Some(stop) = done {
            db.finish_subagent_run(&id, "done", stop, "done it", 1).unwrap();
        }
        (id, chat)
    }

    #[test]
    fn a_copy_is_made_once_per_chat_and_keeps_its_work() {
        let (main, data) = (project(), scratch("data"));
        let first = make_copy(&data, &main, "chat-1").unwrap();
        std::fs::write(first.join("src/app.ts"), "changed\n").unwrap();
        // The chat goes on: the same copy, with what it did.
        let again = make_copy(&data, &main, "chat-1").unwrap();
        assert_eq!(first, again);
        assert_eq!(std::fs::read_to_string(again.join("src/app.ts")).unwrap(), "changed\n");
        // A chat nobody can put in a path is refused.
        assert!(make_copy(&data, &main, "../x").is_err());
    }

    #[test]
    fn a_clean_change_is_written_to_the_folder_and_can_be_undone() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, team) = session(&db, &mem, team(Folder::Copies, false), &main);
        let (_, chat) = member_run(&db, &session, "ui", "Do the button", &main, Some("completed"));
        let copy = make_copy(&data, &main, &chat).unwrap();
        std::fs::write(copy.join("src/ui/button.ts"), "button v2\n").unwrap();
        std::fs::write(copy.join("src/ui/new.ts"), "new\n").unwrap();

        let ledger = Ledger::default();
        assert_eq!(merge_done(&db, &data, &session, &ledger, &team, false), 2);
        assert_eq!(std::fs::read_to_string(main.join("src/ui/button.ts")).unwrap(), "button v2\n");
        assert_eq!(std::fs::read_to_string(main.join("src/ui/new.ts")).unwrap(), "new\n");
        assert!(take_conflicts(&session).is_empty());
        // Merging again does nothing, and the files are the coordinator's changes to undo.
        assert_eq!(merge_done(&db, &data, &session, &ledger, &team, false), 0);
        let set = crate::agent::changes::change_set(&db, &session, 0);
        assert_eq!(set.files.len(), 2);
    }

    #[test]
    fn a_member_in_a_run_nobody_watches_works_in_its_copy_and_its_work_waits_for_the_person() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, team) = session(&db, &mem, team(Folder::Copies, false), &main);
        let (_, chat) = member_run(&db, &session, "ui", "Do the button", &main, Some("completed"));
        let copy = make_copy(&data, &main, &chat).unwrap();
        // Its own copy is its own: the file tools let it write there without asking.
        let canon = crate::permissions::canonicalize_lenient(&copy);
        assert!(is_own_copy(&data, &chat, Some(&canon)));
        assert!(!is_own_copy(&data, &chat, Some(&main)), "the main folder is not its copy");
        assert!(!is_own_copy(&data, &session, Some(&canon)), "nor is it anyone else's");
        std::fs::write(copy.join("src/ui/button.ts"), "button v2\n").unwrap();
        std::fs::write(copy.join("src/ui/new.ts"), "new\n").unwrap();
        std::fs::remove_file(copy.join("README.md")).unwrap();

        // Nobody watches: nothing is written into the folder, one entry waits, and the coordinator is told.
        assert_eq!(merge_done(&db, &data, &session, &Ledger::default(), &team, true), 0);
        assert_eq!(std::fs::read_to_string(main.join("src/ui/button.ts")).unwrap(), "button\n");
        assert!(!main.join("src/ui/new.ts").exists() && main.join("README.md").exists());
        let waiting = db.list_waiting().unwrap();
        assert_eq!(waiting.len(), 1, "one entry for the member's work");
        assert_eq!((waiting[0].author.as_deref(), waiting[0].session_id.as_deref()), (Some("ui"), Some(session.as_str())));
        assert!(waiting[0].rationale.starts_with("Bring in its changes to "), "{}", waiting[0].rationale);
        let told = take_conflicts(&session);
        assert_eq!(told.len(), 1);
        assert!(!told[0].on_plan && told[0].text.contains("wait for the person"), "{:?}", told[0]);

        // The copy goes with the next task; the entry still carries the work.
        std::fs::remove_dir_all(branch::workdir(&data, &chat)).unwrap();
        // Meanwhile the person changed the README themselves: that one is not deleted under them.
        std::fs::write(main.join("README.md"), "mine now\n").unwrap();
        let said = crate::commands::waiting::answer(&db, &data, &waiting[0].id, true, None).unwrap();
        assert!(said.contains("Brought in 2 files") && said.contains("README.md changed in the folder since"), "{said}");
        assert_eq!(std::fs::read_to_string(main.join("src/ui/button.ts")).unwrap(), "button v2\n");
        assert_eq!(std::fs::read_to_string(main.join("src/ui/new.ts")).unwrap(), "new\n");
        assert_eq!(std::fs::read_to_string(main.join("README.md")).unwrap(), "mine now\n");
        assert!(db.list_waiting().unwrap().is_empty());
    }

    #[test]
    fn two_members_changing_the_same_file_make_an_item_and_nothing_is_overwritten() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, _) = session(&db, &mem, team(Folder::Copies, false), &main);
        let (_, api_chat) = member_run(&db, &session, "api", "Do the api", &main, Some("completed"));
        let (_, ui_chat) = member_run(&db, &session, "ui", "Do the ui", &main, Some("completed"));
        let api = make_copy(&data, &main, &api_chat).unwrap();
        let ui = make_copy(&data, &main, &ui_chat).unwrap();
        std::fs::write(api.join("src/app.ts"), "one\nAPI\nthree\n").unwrap();
        std::fs::write(api.join("src/api/routes.ts"), "routes v2\n").unwrap();
        std::fs::write(ui.join("src/app.ts"), "one\nUI\nthree\n").unwrap();
        std::fs::write(ui.join("src/ui/button.ts"), "button v2\n").unwrap();

        // api's item is done first, then ui's.
        let ledger = Ledger::default();
        let rows = db.list_subagent_runs(&session).unwrap();
        assert_eq!(rows.len(), 2);
        merge_copy(&db, &data, &session, &ledger, &main, &api_chat, "api", false).unwrap();
        assert!(take_conflicts(&session).is_empty());
        let merged = merge_copy(&db, &data, &session, &ledger, &main, &ui_chat, "ui", false).unwrap();

        // The clean file went in; the shared one is api's, not overwritten.
        assert_eq!(merged.written, 1);
        assert_eq!(std::fs::read_to_string(main.join("src/ui/button.ts")).unwrap(), "button v2\n");
        assert_eq!(std::fs::read_to_string(main.join("src/app.ts")).unwrap(), "one\nAPI\nthree\n");
        assert_eq!(merged.conflicts.len(), 1);
        assert_eq!(merged.conflicts[0].item, "ui and api both changed `src/app.ts`");
        assert!(merged.conflicts[0].text.contains("Nothing was overwritten") && merged.conflicts[0].text.contains("UI"), "{}", merged.conflicts[0].text);
    }

    #[test]
    fn merge_done_queues_the_conflict_for_the_coordinator_once() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, team) = session(&db, &mem, team(Folder::Copies, false), &main);
        let (_, a) = member_run(&db, &session, "api", "api", &main, Some("completed"));
        let (_, u) = member_run(&db, &session, "ui", "ui", &main, Some("completed"));
        let (api, ui) = (make_copy(&data, &main, &a).unwrap(), make_copy(&data, &main, &u).unwrap());
        std::fs::write(api.join("src/app.ts"), "A\n").unwrap();
        std::fs::write(ui.join("src/app.ts"), "U\n").unwrap();

        let ledger = Ledger::default();
        merge_done(&db, &data, &session, &ledger, &team, false);
        let items = take_conflicts(&session);
        assert_eq!(items.len(), 1, "{items:?}");
        assert!(items[0].item == "ui and api both changed `src/app.ts`", "{}", items[0].item);
        merge_done(&db, &data, &session, &ledger, &team, false);
        assert!(take_conflicts(&session).is_empty(), "said once");
        assert_eq!(std::fs::read_to_string(main.join("src/app.ts")).unwrap(), "A\n");
    }

    #[test]
    fn a_change_the_person_made_to_the_folder_is_a_conflict_with_the_main_folder() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, team) = session(&db, &mem, team(Folder::Copies, false), &main);
        let (_, chat) = member_run(&db, &session, "docs", "docs", &main, Some("completed"));
        let copy = make_copy(&data, &main, &chat).unwrap();
        std::fs::write(copy.join("README.md"), "from docs\n").unwrap();
        std::fs::write(main.join("README.md"), "from the person\n").unwrap();
        merge_done(&db, &data, &session, &Ledger::default(), &team, false);
        let items = take_conflicts(&session);
        assert_eq!(items[0].item, "docs and the main folder both changed `README.md`");
        assert_eq!(std::fs::read_to_string(main.join("README.md")).unwrap(), "from the person\n");
    }

    #[test]
    fn a_member_whose_item_is_not_done_is_not_merged() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, team) = session(&db, &mem, team(Folder::Copies, true), &main);
        // Finished, but its reviewer has not looked: owed, so not done.
        let (id, chat) = member_run(&db, &session, "api", "Do the api", &main, Some("completed"));
        let copy = make_copy(&data, &main, &chat).unwrap();
        std::fs::write(copy.join("src/api/routes.ts"), "routes v2\n").unwrap();
        let rows = db.list_subagent_runs(&session).unwrap();
        assert!(matches!(team::standing(&team, &rows, 0), Some((Standing::Owed, _))));
        assert_eq!(merge_done(&db, &data, &session, &Ledger::default(), &team, false), 0);
        assert_eq!(std::fs::read_to_string(main.join("src/api/routes.ts")).unwrap(), "routes\n");

        // The reviewer keeps it: now the item is done and the change goes in.
        let (rid, _) = member_run(&db, &session, "verifier", &format!("Review item 1 by api: Do the api\n\nreport"), &main, None);
        db.set_subagent_member(&rid, "verifier", Some(1)).unwrap();
        db.finish_subagent_run(&rid, "done", "completed", "VERDICT: CHECKED\nREASON: fine", 1).unwrap();
        let rows = db.list_subagent_runs(&session).unwrap();
        assert!(matches!(team::standing(&team, &rows, 0), Some((Standing::Kept(_), _))), "{id}");
        assert_eq!(merge_done(&db, &data, &session, &Ledger::default(), &team, false), 1);
        assert_eq!(std::fs::read_to_string(main.join("src/api/routes.ts")).unwrap(), "routes v2\n");
    }

    #[test]
    fn a_member_that_did_not_finish_is_not_merged() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, team) = session(&db, &mem, team(Folder::Copies, false), &main);
        let (_, chat) = member_run(&db, &session, "docs", "docs", &main, Some("stopped"));
        let copy = make_copy(&data, &main, &chat).unwrap();
        std::fs::write(copy.join("README.md"), "half done\n").unwrap();
        assert_eq!(merge_done(&db, &data, &session, &Ledger::default(), &team, false), 0);
        assert_eq!(std::fs::read_to_string(main.join("README.md")).unwrap(), "readme\n");
    }

    #[test]
    fn owners_mode_does_not_merge_anything() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, team) = session(&db, &mem, team(Folder::Owners, false), &main);
        member_run(&db, &session, "ui", "ui", &main, Some("completed"));
        assert_eq!(merge_done(&db, &data, &session, &Ledger::default(), &team, false), 0);
    }

    #[test]
    fn a_new_task_brings_in_done_work_and_deletes_the_copies() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, _) = session(&db, &mem, team(Folder::Copies, false), &main);
        let (_, chat) = member_run(&db, &session, "docs", "docs", &main, Some("completed"));
        let copy = make_copy(&data, &main, &chat).unwrap();
        std::fs::write(copy.join("README.md"), "docs v2\n").unwrap();
        // The next turn is another task: the old run is not in it.
        sweep(&db, &mem, &data, &session, Some("turn-2"));
        assert_eq!(std::fs::read_to_string(main.join("README.md")).unwrap(), "docs v2\n");
        assert!(!branch::workdir(&data, &chat).exists());
    }

    #[test]
    fn the_review_of_an_item_is_known_by_whose_work_it_is() {
        assert_eq!(team::reviewed("Review item 3 by api: Do it\n\nreport"), Some("api"));
        assert_eq!(team::reviewed("Review the work of ui: Do it"), Some("ui"));
        assert_eq!(team::reviewed("Sent back by verifier on item 3: x"), None);
        assert_eq!(team::reviewed("Build the screen"), None);
    }

    /// The reviewer looks at the member's copy, where the work is.
    #[test]
    fn a_reviewer_finds_the_copy_of_the_member_it_reviews() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, _) = session(&db, &mem, team(Folder::Copies, true), &main);
        let (_, chat) = member_run(&db, &session, "api", "api", &main, Some("completed"));
        assert_eq!(copy_of(&db, &data, &session, "api"), None, "no copy yet");
        let copy = make_copy(&data, &main, &chat).unwrap();
        assert_eq!(copy_of(&db, &data, &session, "api"), Some(copy));
        assert_eq!(copy_of(&db, &data, &session, "ui"), None);
    }

    // ---- owners ----

    fn owners_run(role: &str, extra: impl FnOnce(&mut Team)) -> (Db, MemoryStore, PathBuf, String) {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let main = project();
        let mut t = team(Folder::Owners, false);
        extra(&mut t);
        let (session, _) = session(&db, &mem, t, &main);
        let (_, chat) = member_run(&db, &session, role, "task", &main, None);
        (db, mem, main, chat)
    }

    #[test]
    fn a_write_in_someone_elses_place_is_refused_in_the_teams_words() {
        let (db, mem, main, chat) = owners_run("api", |_| {});
        let ui = main.join("src/ui/button.ts");
        assert_eq!(refusal(&db, &mem, &chat, Some(&main), &[&ui]).unwrap(), "src/ui/ belongs to ui in this team.");
        // Its own place is fine, and so is a new file in it.
        assert_eq!(refusal(&db, &mem, &chat, Some(&main), &[&main.join("src/api/routes.ts")]), None);
        assert_eq!(refusal(&db, &mem, &chat, Some(&main), &[&main.join("src/api/new/deep.ts")]), None);
        // A place that merely starts the same way is not inside it.
        let lookalike = main.join("src/ui-kit/x.ts");
        assert!(refusal(&db, &mem, &chat, Some(&main), &[&lookalike]).unwrap().contains("outside what api owns"));
    }

    #[test]
    fn a_move_is_refused_when_either_end_is_not_the_members() {
        let (db, mem, main, chat) = owners_run("api", |_| {});
        let from = main.join("src/api/routes.ts");
        let to = main.join("src/ui/routes.ts");
        assert_eq!(refusal(&db, &mem, &chat, Some(&main), &[&from, &to]).unwrap(), "src/ui/ belongs to ui in this team.");
    }

    #[test]
    fn a_place_nobody_owns_is_refused_to_a_member_with_places_and_free_to_one_without() {
        let (db, mem, main, chat) = owners_run("api", |_| {});
        let shared = main.join("src/app.ts");
        assert_eq!(
            refusal(&db, &mem, &chat, Some(&main), &[&shared]).unwrap(),
            "src/app.ts is outside what api owns in this team: src/api/."
        );
        // docs owns nothing: it may write where nobody else does, but not in ui's place.
        let (db, mem, main, chat) = owners_run("docs", |_| {});
        assert_eq!(refusal(&db, &mem, &chat, Some(&main), &[&main.join("README.md")]), None);
        assert_eq!(refusal(&db, &mem, &chat, Some(&main), &[&main.join("src/api/routes.ts")]).unwrap(), "src/api/ belongs to api in this team.");
    }

    #[test]
    fn the_coordinator_and_other_runs_are_never_refused() {
        let (db, mem, main, _chat) = owners_run("api", |_| {});
        // The session itself is the coordinator's chat.
        let session = db.list_conversations().unwrap().into_iter().find(|c| c.title == "s").unwrap().id;
        assert_eq!(refusal(&db, &mem, &session, Some(&main), &[&main.join("src/ui/button.ts")]), None);
        // A chat that is nobody's member.
        let other = db.create_conversation("other", None, false).unwrap().id;
        assert_eq!(refusal(&db, &mem, &other, Some(&main), &[&main.join("src/ui/button.ts")]), None);
    }

    #[test]
    fn a_member_in_a_copy_may_not_write_in_the_main_folder_but_may_in_its_copy() {
        let db = Db::open_in_memory().unwrap();
        let mem = memory();
        let (main, data) = (project(), scratch("data"));
        let (session, _) = session(&db, &mem, team(Folder::Copies, false), &main);
        let (_, chat) = member_run(&db, &session, "ui", "ui", &main, None);
        let copy = make_copy(&data, &main, &chat).unwrap();
        db.set_conversation_copy(&chat, &copy.to_string_lossy()).unwrap();
        assert!(refusal(&db, &mem, &chat, Some(&copy), &[&main.join("src/app.ts")]).unwrap().contains("works in its own copy"));
        assert_eq!(refusal(&db, &mem, &chat, Some(&copy), &[&copy.join("src/app.ts")]), None);
    }
}
