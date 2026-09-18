//! Delegation commands (`SUB-9`): read back what a turn handed out, and take
//! hold of a child that is still working.
//!
//! Reading a child's *transcript* needs nothing new — a child is a
//! conversation, so `list_messages_cmd` and the artifact commands already serve
//! the Agents tab.

use tauri::State;

use crate::agent::fleet::{Fleet, Steer, SteerSource};
use crate::agent::subagents::{agent_types, AgentType};
use crate::db::{Db, SubagentRun};
use crate::PoiesisError;

#[tauri::command]
pub fn list_subagent_runs_cmd(
    db: State<'_, Db>,
    conversation_id: String,
) -> Result<Vec<SubagentRun>, PoiesisError> {
    db.list_subagent_runs(&conversation_id)
        .map_err(|e| PoiesisError::Message(e.to_string()))
}

#[tauri::command]
pub fn get_subagent_run_cmd(db: State<'_, Db>, id: String) -> Result<Option<SubagentRun>, PoiesisError> {
    db.get_subagent_run(&id)
        .map_err(|e| PoiesisError::Message(e.to_string()))
}

/// `SUB-7`: stop one child and everything it started, keeping what it has.
/// The child's loop returns its partial text on cancel, so the lead is handed a
/// labelled partial rather than nothing.
///
/// Returns false when the run already finished — not an error, just a race
/// between the click and the last step.
#[tauri::command]
pub fn stop_run_cmd(fleet: State<'_, Fleet>, run_id: String) -> bool {
    if fleet.get(&run_id).is_none() {
        return false;
    }
    fleet.cancel_tree(&run_id);
    true
}

/// `SUB-6`: say something to a child that is already working. Same inbox the
/// composer uses for the main run, marked as coming from the lead's side so the
/// child can tell a redirection from its original brief.
#[tauri::command]
pub fn steer_subagent_cmd(fleet: State<'_, Fleet>, run_id: String, text: String) -> bool {
    let Some(run) = fleet.get(&run_id) else {
        return false;
    };
    run.steer(Steer { text, from: SteerSource::Lead });
    true
}

/// The agent types available to delegate to (`SUB-3`): the built-in general
/// agent plus every persona the user marked delegatable.
#[tauri::command]
pub fn list_agent_types_cmd(db: State<'_, Db>) -> Vec<AgentType> {
    agent_types(&db)
}
