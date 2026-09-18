//! Project commands (`PRJ-1`/`PRJ-3`): the working directory as a first-class
//! thing, and the sessions that happened in it.
//!
//! There is no picker here. Choosing a folder is `files::pick_folder_cmd`,
//! which already validates the choice and records the read grant; a second
//! dialog path would be a second place for that check to be forgotten.

use tauri::State;

use crate::commands::files::{app_data_dir, DialogGrants};
use crate::db::{project_name_for, Conversation, Db, Project};
use crate::permissions::{canonicalize_lenient, refuse_as_working_folder, Trust};
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

#[tauri::command]
pub fn list_projects_cmd(db: State<'_, Db>, include_archived: Option<bool>) -> Cmd<Vec<Project>> {
    db.list_projects(include_archived.unwrap_or(false)).map_err(err)
}

/// `PRJ-3` explicit create. Both arguments are optional (`PRJ-1a`): most
/// projects are not about a directory, and the ones that are get their name
/// from the folder for free.
///
/// A folder that is already a project returns that project, so "New project"
/// on something you already have open lands you in it rather than failing.
#[tauri::command]
pub fn create_project_cmd(
    db: State<'_, Db>,
    root_path: Option<String>,
    name: Option<String>,
) -> Cmd<Project> {
    let root = root_path
        .filter(|p| !p.trim().is_empty())
        .map(|p| canonicalize_lenient(std::path::Path::new(&p)).to_string_lossy().to_string());
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .or_else(|| root.as_deref().map(project_name_for))
        .unwrap_or_else(|| "New project".to_string());
    db.create_project(&name, root.as_deref()).map_err(err)
}

/// `PRJ-7`: the instructions every session in this project carries.
#[tauri::command]
pub fn set_project_instructions_cmd(
    db: State<'_, Db>,
    id: String,
    instructions: Option<String>,
) -> Cmd<()> {
    db.set_project_instructions(&id, instructions.as_deref()).map_err(err)
}

/// `PRJ-3a`: give a project a working folder, or with `None` take it away.
/// Removing it leaves the project and every one of its sessions in place —
/// the folder was a property, and dropping a property is not leaving.
///
/// Returns the project as it now stands. Fails rather than silently stealing
/// a folder another project owns; the frontend moves the conversation there
/// instead, which is what `set_conversation_folder_cmd` already does.
#[tauri::command]
pub fn set_project_root_cmd(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    id: String,
    root_path: Option<String>,
) -> Cmd<Project> {
    let root = match root_path.filter(|p| !p.trim().is_empty()) {
        Some(p) => {
            let path = canonicalize_lenient(std::path::Path::new(&p));
            // The same check the per-chat attach does. A project is not a way
            // around it.
            if let Some(reason) = refuse_as_working_folder(&path, app_data_dir(&app).as_deref()) {
                return Err(PoiesisError::Message(format!(
                    "Poiesis can't work in {} — {reason}.",
                    path.display()
                )));
            }
            grants.remember(&path);
            Some(path.to_string_lossy().to_string())
        }
        None => None,
    };
    if !db.set_project_root(&id, root.as_deref()).map_err(err)? {
        return Err(PoiesisError::Message(
            "Another project already works in that folder. Open that project instead.".into(),
        ));
    }
    db.get_project(&id)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("that project no longer exists".into()))
}

#[tauri::command]
pub fn rename_project_cmd(db: State<'_, Db>, id: String, name: String) -> Cmd<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PoiesisError::Message("a project needs a name".into()));
    }
    db.rename_project(&id, name).map_err(err)
}

/// Trust for the folder, granted once instead of once per chat.
#[tauri::command]
pub fn set_project_trust_cmd(db: State<'_, Db>, id: String, trust: String) -> Cmd<()> {
    // Round-trip through the enum so an unknown string can't land in the DB
    // and read back as something more permissive than intended — the same
    // reasoning as `set_conversation_trust_cmd`.
    db.set_project_trust(&id, Trust::parse(&trust).as_str()).map_err(err)
}

/// Archive hides the project and its sessions from the Rail. Nothing on disk
/// is touched, ever, which is why there is no delete beside this.
#[tauri::command]
pub fn set_project_archived_cmd(db: State<'_, Db>, id: String, archived: bool) -> Cmd<()> {
    db.set_project_archived(&id, archived).map_err(err)
}

/// `SHL-17`'s scope seam, filled by `PRJ-UI-2`: the open tab set, per project.
#[tauri::command]
pub fn set_project_tabs_cmd(db: State<'_, Db>, id: String, tabs_json: Option<String>) -> Cmd<()> {
    db.set_project_tabs(&id, tabs_json.as_deref()).map_err(err)
}

/// Put a conversation in a project, or with `None` take it out of one.
/// Leaving touches neither the project nor its other sessions (`PRJ-3`).
#[tauri::command]
pub fn set_conversation_project_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    project_id: Option<String>,
) -> Cmd<()> {
    db.set_conversation_project(&conversation_id, project_id.as_deref())
        .map_err(err)
}

/// The sessions in one project, newest first. The Rail lists these in place of
/// the flat conversation list when a project row is expanded (`PRJ-UI-1`).
#[tauri::command]
pub fn list_project_conversations_cmd(db: State<'_, Db>, project_id: String) -> Cmd<Vec<Conversation>> {
    Ok(db
        .list_conversations()
        .map_err(err)?
        .into_iter()
        .filter(|c| c.project_id.as_deref() == Some(project_id.as_str()))
        .collect())
}

// ---- the coding half (`CODING_PLAN`) ----

/// `COD-UI-1`: what the project header shows.
#[derive(serde::Serialize)]
pub struct ProjectCardView {
    pub card: crate::agent::project::ProjectCard,
    /// The policy in force: the project's own, or the Settings default.
    pub policy: crate::agent::coderun::Policy,
    /// Whether that policy is the project's own choice.
    pub policy_is_own: bool,
    pub allow: crate::agent::coderun::Allow,
    /// Whether `Run project tasks` is switched on in Settings at all.
    pub tasks_enabled: bool,
    pub card_built_at: Option<i64>,
}

fn project_or_err(db: &Db, id: &str) -> Cmd<Project> {
    db.get_project(id)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("that project no longer exists".into()))
}

/// `COD-1`/`COD-UI-1`: the card, detected now when `refresh` or when there is
/// none yet. `None` for a project with no folder.
#[tauri::command]
pub fn project_card_cmd(db: State<'_, Db>, id: String, refresh: Option<bool>) -> Cmd<Option<ProjectCardView>> {
    use crate::agent::{coderun, project};
    let mut p = project_or_err(&db, &id)?;
    let Some(root) = p.root_path.clone() else { return Ok(None) };
    if refresh.unwrap_or(false) {
        project::refresh(&db, &p.id, std::path::Path::new(&root));
        p = project_or_err(&db, &id)?;
    }
    let Some(card) = project::card_for(&db, &p) else { return Ok(None) };
    let p = project_or_err(&db, &id)?;
    Ok(Some(ProjectCardView {
        card,
        policy: coderun::project_policy(&db, &p),
        policy_is_own: coderun::Policy::parse(&p.exec_policy).is_some(),
        allow: coderun::allow_of(&p),
        tasks_enabled: crate::agent::toolsets::Toolset::CodeRun.is_enabled(&db),
        card_built_at: p.card_built_at,
    }))
}

/// `COD-7`: "off" | "ask" | "allow", or "inherit" for the Settings default.
#[tauri::command]
pub fn set_project_exec_policy_cmd(db: State<'_, Db>, id: String, policy: String) -> Cmd<()> {
    let policy = match policy.as_str() {
        "off" | "ask" | "allow" | "inherit" => policy,
        other => return Err(PoiesisError::Message(format!("unknown policy '{other}'"))),
    };
    db.set_project_exec_policy(&id, &policy).map_err(err)
}

/// `COD-UI-1`: a task's "always allow in this project", from the header.
#[tauri::command]
pub fn set_project_task_allowed_cmd(db: State<'_, Db>, id: String, task: String, allowed: bool) -> Cmd<()> {
    let p = project_or_err(&db, &id)?;
    let mut allow = crate::agent::coderun::allow_of(&p);
    allow.tasks.retain(|t| t != &task);
    if allowed {
        allow.tasks.push(task);
    }
    crate::agent::coderun::save_allow(&db, &id, &allow);
    Ok(())
}

/// `COD-8`: the project's opt-in to free-form commands, and forgetting a
/// remembered `command argv[0]` pair.
#[tauri::command]
pub fn set_project_commands_cmd(
    db: State<'_, Db>,
    id: String,
    run_command: bool,
    forget: Option<String>,
) -> Cmd<()> {
    let p = project_or_err(&db, &id)?;
    let mut allow = crate::agent::coderun::allow_of(&p);
    allow.run_command = run_command;
    if let Some(key) = forget {
        allow.commands.retain(|c| c != &key);
    }
    crate::agent::coderun::save_allow(&db, &id, &allow);
    Ok(())
}

/// `PRJ-UI-3`: the conversation's change set, since the last "Keep all".
#[tauri::command]
pub fn conversation_changes_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<crate::agent::changes::ChangeSet> {
    let since = crate::agent::changes::view_since(&db, &conversation_id);
    Ok(crate::agent::changes::change_set(&db, &conversation_id, since))
}

/// `PRJ-UI-3`: put files back. `path` for one file, `None` for all of them.
#[tauri::command]
pub fn undo_changes_cmd(db: State<'_, Db>, conversation_id: String, path: Option<String>) -> Cmd<()> {
    use crate::agent::changes;
    let set = changes::change_set(&db, &conversation_id, changes::view_since(&db, &conversation_id));
    // Newest file first, so a file moved and then edited unwinds in order.
    let mut files: Vec<&changes::FileChange> =
        set.files.iter().filter(|f| path.as_deref().map_or(true, |p| p == f.path)).collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.last_at));
    for f in files {
        changes::undo_file(&db, f).map_err(PoiesisError::Message)?;
        let _ = db.log_activity(Some(&conversation_id), "file", &format!("undid my changes to {}", f.path));
    }
    Ok(())
}

/// `PRJ-UI-3`: "Keep all" — the changes stay, and leave the review list.
#[tauri::command]
pub fn keep_changes_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    crate::agent::changes::keep_all(&db, &conversation_id, now);
    Ok(())
}

/// `COD-UI-5`: the Canvas Run button. A user action, so it needs no prompt.
#[tauri::command]
pub async fn run_code_artifact_cmd(db: State<'_, Db>, id: String) -> Cmd<crate::agent::artifacts::CodeRun> {
    let artifact = db
        .get_artifact(&id)
        .map_err(err)?
        .ok_or_else(|| PoiesisError::Message("that artifact no longer exists".into()))?;
    let run = crate::agent::artifacts::run_code(&artifact).await.map_err(PoiesisError::Message)?;
    let _ = db.log_activity(
        artifact.conversation_id.as_deref(),
        "artifact",
        &format!("ran {}: {}", artifact.title, run.outcome),
    );
    Ok(run)
}
