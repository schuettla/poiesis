//! Settings -> Experiments (`EXP-UI-1`): the registry with its state, the switch,
//! and "I have looked". The rules are in `crate::experiments`.

use std::path::PathBuf;

use tauri::State;

use crate::agent::decide::cloud;
use crate::db::Db;
use crate::experiments::{self, ExperimentView, Facts};
use crate::experiments_report::{self, Report};
use crate::runtime::decideserver::catalog_entry_for;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

/// What the requirements ask about this computer: is a decision model installed,
/// and does a vision projector sit beside it (the same test the engine uses). A
/// cloud decision model counts too once it is the chosen backend and has a key,
/// and its pictures count once a Test showed it reads them.
pub(crate) fn facts(db: &Db) -> Facts {
    let local = local_facts(db);
    let cloud_on = cloud::chosen(db);
    Facts {
        decision_model: local.decision_model || cloud_on,
        vision: local.vision || (cloud_on && cloud::sees_images(db, &cloud::selected_model(db))),
    }
}

fn local_facts(db: &Db) -> Facts {
    let model = db
        .default_model_by_role("decide")
        .ok()
        .flatten()
        .map(|m| PathBuf::from(m.path))
        .filter(|p| p.exists());
    let vision = model
        .as_ref()
        .and_then(|p| p.file_name().and_then(|n| n.to_str()).and_then(catalog_entry_for).map(|e| (p, e)))
        .is_some_and(|(p, e)| p.with_file_name(e.mmproj_filename).exists());
    Facts { decision_model: model.is_some(), vision }
}

#[tauri::command]
pub fn experiments_list_cmd(db: State<'_, Db>) -> Vec<ExperimentView> {
    experiments::list(&db, facts(&db))
}

/// Turning on needs the requirements (checked again here); turning off always works.
#[tauri::command]
pub fn set_experiment_cmd(db: State<'_, Db>, id: String, on: bool) -> Cmd<Vec<ExperimentView>> {
    experiments::set(&db, &id, on, facts(&db)).map_err(PoiesisError::Message)?;
    Ok(experiments::list(&db, facts(&db)))
}

/// `EXP-4`: what the experiment did in the last 30 days.
#[tauri::command]
pub fn experiment_report_cmd(db: State<'_, Db>, id: String) -> Report {
    experiments_report::report(&db, &id)
}

/// `EXP-4`: forget what the experiment recorded. The switch stays as it is. The
/// answer is the report after, so the card can draw it.
#[tauri::command]
pub fn experiment_reset_cmd(db: State<'_, Db>, id: String) -> Cmd<Report> {
    experiments_report::reset(&db, &id).map_err(PoiesisError::Message)?;
    Ok(experiments_report::report(&db, &id))
}

/// The screen was opened: the tab's dot goes away.
#[tauri::command]
pub fn experiments_seen_cmd(db: State<'_, Db>) -> Cmd<()> {
    experiments::mark_seen(&db).map_err(PoiesisError::Message)
}
