//! `TEAM-8`: teams as files (`memory/ways/teams/`): list, read, save, delete, their
//! history, and the lines a shape starts with. Running a team is `TEAM-10`.

use serde::Serialize;
use tauri::State;

use crate::db::Db;
use crate::memory::ways::{shape_lines, Back, Line, Member, Shape, Team, TeamRead};
use crate::memory::MemoryStore;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// Every team, by name, with what could not be read in each.
#[tauri::command]
pub fn teams_list_cmd(mem: State<'_, MemoryStore>) -> Vec<TeamRead> {
    mem.teams()
}

#[tauri::command]
pub fn team_get_cmd(mem: State<'_, MemoryStore>, slug: String) -> Option<TeamRead> {
    mem.read_team(&slug)
}

/// A team that is not made yet, for "New team": a Hub with the usual limits. The screen
/// takes its starting values from here and says none of them itself.
#[tauri::command]
pub fn team_blank_cmd() -> Team {
    Team::blank()
}

/// `TEAM-22`: the New team "Keep these helpers as a team" starts from, made of what this session
/// did. `None` when it used fewer than two helpers, or a team already leads it.
#[tauri::command]
pub fn team_from_session_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, conversation_id: String) -> Option<Team> {
    crate::agent::team::keep_as_team(&db, &mem, &conversation_id)
}

/// `TEAM-23`: the person tried to speak to a member (`/ask kai`) they have no line to. Said once for
/// each chat and member; after three chats Poiesis offers to draw the line. Quiet when it is not
/// a signal, and never an error: it must not get in the way of the message.
#[tauri::command]
pub fn team_asked_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, conversation_id: String, role: String) -> bool {
    crate::agent::team::note_asked_without_line(&db, &mem, &conversation_id, &role)
}

/// Save a team. A team without a slug is new: it starts locked. Returns it as saved
/// (slug, version, lines made whole).
#[tauri::command]
pub fn team_save_cmd(mem: State<'_, MemoryStore>, team: Team) -> Cmd<Team> {
    mem.save_team(team, "you", "").map_err(err)
}

/// Set a team aside. Where chats started with it, they start with Poiesis again, and
/// the chats it led go back to Poiesis.
#[tauri::command]
pub fn team_delete_cmd(db: State<'_, Db>, mem: State<'_, MemoryStore>, slug: String) -> Cmd<()> {
    mem.delete_team(&slug).map_err(err)?;
    db.release_team_chats(&slug).map_err(err)?;
    mem.drop_default_leaders_for(&format!("team:{slug}")).map_err(err)
}

/// One entry of a team's history, as the dialog draws it.
#[derive(Debug, Clone, Serialize)]
pub struct TeamHistoryView {
    pub seq: u32,
    pub at: i64,
    /// `you`, `evolution` or `original`.
    pub by: String,
    /// `original`, `version`, `back`, `locked`, `unlocked`.
    pub what: String,
    pub why: String,
    pub version: u32,
    /// What this version has that the one before did not, in the words of the file.
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// The team as it was then, so "Compare with now" has something to compare.
    pub team: Team,
    /// The latest version that carries the team's work: what "now" is.
    pub current: bool,
}

#[tauri::command]
pub fn team_history_cmd(mem: State<'_, MemoryStore>, slug: String) -> Vec<TeamHistoryView> {
    let list = mem.team_history(&slug);
    let carries = |what: &str| matches!(what, "original" | "version" | "back");
    let current = list.iter().find(|e| carries(&e.what)).map(|e| e.seq);
    list.into_iter()
        .map(|e| TeamHistoryView {
            current: Some(e.seq) == current,
            seq: e.seq,
            at: e.at,
            by: e.by,
            what: e.what,
            why: e.why,
            version: e.version,
            added: e.added,
            removed: e.removed,
            team: e.team,
        })
        .collect()
}

/// Go back to the original (`to` absent) or to the version at that place in the history.
#[tauri::command]
pub fn team_back_cmd(mem: State<'_, MemoryStore>, slug: String, to: Option<u32>) -> Cmd<Team> {
    mem.go_back_team(&slug, to.map_or(Back::Original, Back::Entry)).map_err(err)
}

/// The lines a team has when it starts from `shape`, for `members` as they are (the
/// two fixed lines last). "Start from" in the Teams tab asks before it uses them.
#[tauri::command]
pub fn team_shape_lines_cmd(shape: Shape, members: Vec<Member>) -> Vec<Line> {
    shape_lines(shape, &members)
}
