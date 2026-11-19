//! Delegation commands (`SUB-9`): read back what a turn handed out, and take
//! hold of a child that is still working.
//!
//! Reading a child's *transcript* needs nothing new — a child is a
//! conversation, so `list_messages_cmd` and the artifact commands already serve
//! the Agents tab.

use tauri::State;

use crate::agent::fleet::{Fleet, Steer, SteerSource};
use crate::agent::subagents::{agent_types, AgentType};
use crate::db::{Db, Post, SubagentRun};
use crate::PoiesisError;

#[tauri::command]
pub fn list_subagent_runs_cmd(
    db: State<'_, Db>,
    conversation_id: String,
) -> Result<Vec<SubagentRun>, PoiesisError> {
    db.list_subagent_runs(&conversation_id)
        .map_err(|e| PoiesisError::Message(e.to_string()))
}

/// `TEAM-15`: what passed between the people of the team that leads this session, oldest first: the
/// Coordination view's Board and the line widths on its chart read this.
#[tauri::command]
pub fn list_posts_cmd(db: State<'_, Db>, conversation_id: String) -> Result<Vec<Post>, PoiesisError> {
    db.list_posts(&conversation_id).map_err(|e| PoiesisError::Message(e.to_string()))
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

/// `CLI-UI-6`: what `/delegate` can name in this chat, with the reason one cannot
/// be used here.
#[tauri::command]
pub fn list_delegate_targets_cmd(db: State<'_, Db>, conversation_id: Option<String>) -> Vec<crate::agent::subagents::Target> {
    crate::agent::subagents::delegate_targets(&db, conversation_id.as_deref())
}

/// `TEAM-18`: after a reply in a session a team leads, would one of the other personas add something the
/// person needs? The rules come first (lines, the turn rule), so a chat where nobody may chime in, or one
/// that has had its one unasked reply, never spends a call. `None` is nobody, and is also what any
/// failure is: this fails closed. The window then starts the reply as a turn with `addressed` and
/// `unasked` (no user row), the text being `nudge`.
#[derive(serde::Serialize)]
pub struct NextVoice {
    pub role: String,
    pub nudge: String,
}

#[tauri::command]
pub async fn team_next_voice_cmd(
    mgr: State<'_, crate::runtime::RuntimeManager>,
    db: State<'_, Db>,
    memory: State<'_, crate::memory::MemoryStore>,
    conversation_id: String,
    target: crate::commands::agent::ChatTarget,
) -> Result<Option<NextVoice>, PoiesisError> {
    let asked = crate::agent::team_voices::next_voice(&db, &memory, &conversation_id, |messages| async {
        let setup = crate::commands::agent::resolve_turn(&mgr, &db, &target).await.ok()?;
        let cancel = crate::runtime::proxy::CancelFlag::new();
        crate::agent::aside::ask(&mgr.client, &setup.endpoint, &cancel, messages).await.ok().map(|(text, _)| text)
    })
    .await;
    Ok(asked.map(|(who, nudge)| NextVoice { role: who.role, nudge: nudge.to_string() }))
}
