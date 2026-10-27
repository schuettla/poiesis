//! Commands for external agent apps (`CLI-2`, `CLI-12`, `CLI-13`): what
//! Settings → External agents reads and changes. A mutating command returns the
//! whole fresh list, so the screen never has to guess what changed.
//!
//! The API key is accepted here and stored in the credential store. It is never
//! returned: the list says only `has_key`.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::Db;
use crate::engines::driver::ModelChoice;
use crate::engines::settings::{apply_patch, validate_args, EnginePatch};
use crate::engines::tryit::TryResult;
use crate::engines::{self, catalog, EngineInfo};
use crate::runtime::proxy::CancelFlag;
use crate::secrets;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn msg(e: impl ToString) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// What is known right now, without searching or probing.
#[tauri::command]
pub fn engines_list_cmd(db: State<'_, Db>) -> Cmd<Vec<EngineInfo>> {
    engines::list_infos(&db).map_err(msg)
}

/// Search for the agents and probe those that are due. `force` probes every
/// one (the **Check again** button).
#[tauri::command]
pub async fn engines_detect_cmd(db: State<'_, Db>, force: bool) -> Cmd<Vec<EngineInfo>> {
    if force {
        engines::detect_all(&db, 0).await.map_err(msg)?;
    } else {
        engines::detect_if_due(&db).await.map_err(msg)?;
    }
    engines::list_infos(&db).map_err(msg)
}

/// The user opened the section: the "found something new" badge clears.
#[tauri::command]
pub fn engines_mark_seen_cmd(db: State<'_, Db>) -> Cmd<()> {
    engines::mark_seen(&db);
    Ok(())
}

/// Probe one agent now (after **Sign in** finished, or a path changed).
#[tauri::command]
pub async fn engines_probe_cmd(db: State<'_, Db>, id: String) -> Cmd<Vec<EngineInfo>> {
    let roots = engines::locate::SearchRoots::from_system();
    engines::refresh(&db, &roots, &id, 0).await.map_err(msg)?;
    engines::list_infos(&db).map_err(msg)
}

/// Change any of the editable settings. All checks happen here.
#[tauri::command]
pub async fn engines_update_cmd(db: State<'_, Db>, id: String, patch: EnginePatch) -> Cmd<Vec<EngineInfo>> {
    let mut row = engines::ensure_row(&db, &id).map_err(msg)?;
    // A key must be in the vault before the engine can be set to use one.
    if patch.payment.as_deref() == Some("api-key") && !secrets::has_secret(secrets::SERVICE_ENGINES, &id) {
        return Err(msg("Paste a key first."));
    }
    let program_before = row.program.clone();
    apply_patch(&mut row, &patch, catalog::find(&id)).map_err(msg)?;
    db.save_engine_settings(&row).map_err(msg)?;
    if row.program != program_before {
        // A new path: look at it now, so the row shows the truth.
        let roots = engines::locate::SearchRoots::from_system();
        engines::refresh(&db, &roots, &id, 0).await.map_err(msg)?;
    }
    engines::list_infos(&db).map_err(msg)
}

/// Add an agent by hand. It must speak the Agent Client Protocol.
#[tauri::command]
pub async fn engines_add_acp_cmd(
    db: State<'_, Db>,
    display_name: String,
    program: String,
    args: Vec<String>,
) -> Cmd<Vec<EngineInfo>> {
    let name = display_name.trim();
    if name.is_empty() {
        return Err(msg("Give it a name."));
    }
    let id = format!("acp:custom-{}", uuid::Uuid::new_v4().simple());
    let mut row = crate::db::engines::EngineRow::new(&id, "acp", name);
    let patch = EnginePatch { program: Some(program), args: Some(args), display_name: Some(name.to_string()), ..Default::default() };
    apply_patch(&mut row, &patch, None).map_err(msg)?;
    if row.program.is_none() {
        return Err(msg("Give the full path to the app."));
    }
    if serde_json::from_str::<Vec<String>>(&row.args_json).unwrap_or_default().is_empty() {
        row.args_json = serde_json::to_string(&validate_args(&["acp".to_string()]).map_err(msg)?).unwrap_or_else(|_| "[\"acp\"]".into());
    }
    db.insert_engine_if_absent(&row).map_err(msg)?;
    let roots = engines::locate::SearchRoots::from_system();
    engines::refresh(&db, &roots, &id, 0).await.map_err(msg)?;
    engines::list_infos(&db).map_err(msg)
}

/// Forget an agent that was added by hand, and its key.
#[tauri::command]
pub fn engines_remove_cmd(db: State<'_, Db>, id: String) -> Cmd<Vec<EngineInfo>> {
    if catalog::find(&id).is_some() {
        return Err(msg("Built-in agents can be switched off, not removed."));
    }
    db.delete_engine(&id).map_err(msg)?;
    let _ = secrets::delete_secret(secrets::SERVICE_ENGINES, &id);
    engines::list_infos(&db).map_err(msg)
}

/// Save an API key for an agent. Write-only: nothing returns it.
#[tauri::command]
pub fn engines_set_key_cmd(db: State<'_, Db>, id: String, key: String) -> Cmd<Vec<EngineInfo>> {
    let entry = catalog::find(&id).ok_or_else(|| msg("I don't know an API-key mode for this app."))?;
    if entry.api_key_env.is_none() {
        return Err(msg("I don't know an API-key mode for this app yet."));
    }
    let key = key.trim();
    if key.is_empty() || key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(msg("That doesn't look like a key."));
    }
    engines::ensure_row(&db, &id).map_err(msg)?;
    secrets::set_secret(secrets::SERVICE_ENGINES, &id, key).map_err(msg)?;
    engines::list_infos(&db).map_err(msg)
}

/// Remove the saved key. An agent set to pay with it goes back to its own sign-in.
#[tauri::command]
pub fn engines_clear_key_cmd(db: State<'_, Db>, id: String) -> Cmd<Vec<EngineInfo>> {
    secrets::delete_secret(secrets::SERVICE_ENGINES, &id).map_err(msg)?;
    if let Some(mut row) = db.get_engine(&id).map_err(msg)? {
        if row.payment == "api-key" {
            row.payment = "plan".into();
            db.save_engine_settings(&row).map_err(msg)?;
        }
    }
    engines::list_infos(&db).map_err(msg)
}

/// Open a terminal running the agent's own sign-in (`TRM-2`). Poiesis never
/// sees the password or the tokens: the app talks to its own login page.
#[tauri::command]
pub fn engines_open_login_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    let row = db.get_engine(&id).map_err(msg)?.ok_or_else(|| msg("I don't know that agent."))?;
    let entry = catalog::find(&id).ok_or_else(|| msg("Sign in to this app in its own terminal."))?;
    let program = row.program.or(row.found_path).ok_or_else(|| msg("I can't find the app."))?;
    open_terminal(&entry.name, &program, &entry.login_args).map_err(msg)
}

#[cfg(windows)]
fn open_terminal(title: &str, program: &str, args: &[String]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let bad = |s: &str| s.chars().any(|c| matches!(c, '"' | '%' | '^' | '&' | '|' | '<' | '>' | '!' | '\n' | '\r'));
    if bad(program) || args.iter().any(|a| bad(a)) || bad(title) {
        return Err("The path has a character I can't pass on safely.".into());
    }
    let mut line = format!("/d /c start \"Sign in to {title}\" cmd.exe /d /k \"\"{program}\"");
    for a in args {
        line.push(' ');
        line.push_str(a);
    }
    line.push('"');
    let mut cmd = std::process::Command::new("cmd.exe");
    // The parent is hidden; `start` gives the terminal its own window.
    cmd.raw_arg(line).creation_flags(0x0800_0000);
    cmd.spawn().map(|_| ()).map_err(|e| format!("I couldn't open a terminal: {e}"))
}

#[cfg(not(windows))]
fn open_terminal(_title: &str, _program: &str, _args: &[String]) -> Result<(), String> {
    Err("Opening a terminal is only built for Windows.".into())
}

/// **Try it** (`CLI-13`).
#[tauri::command]
pub async fn engines_test_cmd(db: State<'_, Db>, id: String) -> Cmd<TryResult> {
    engines::tryit::run(&db, &id, CancelFlag::new()).await.map_err(msg)
}

/// The models to offer for an agent. Empty = no picker.
#[tauri::command]
pub async fn engines_models_cmd(db: State<'_, Db>, id: String) -> Cmd<Vec<ModelChoice>> {
    engines::tryit::models(&db, &id).await.map_err(msg)
}

// ---- `RTE-2`: the user's rules for when to hand work over ----

/// What a rule edit may change. `None` leaves a field alone.
#[derive(serde::Deserialize, Default)]
pub struct RulePatch {
    pub text: Option<String>,
    /// `use` | `avoid`: "Move to Don't use".
    pub kind: Option<String>,
    pub enabled: Option<bool>,
    /// "Always for this", switched off again from the ⚡ mark.
    pub auto: Option<bool>,
}

fn row_of(db: &Db, id: &str) -> Cmd<crate::db::engines::EngineRow> {
    db.get_engine(id).map_err(msg)?.ok_or_else(|| msg("I don't know that agent."))
}

#[tauri::command]
pub fn engines_rule_add_cmd(db: State<'_, Db>, engine_id: String, kind: String, text: String) -> Cmd<Vec<EngineInfo>> {
    let row = row_of(&db, &engine_id)?;
    engines::rules::ensure_seeded(&db, &row).map_err(msg)?;
    if !matches!(kind.as_str(), "use" | "avoid") {
        return Err(msg("A rule says when to use it or when not to."));
    }
    if db.list_engine_rules(&engine_id).map_err(msg)?.len() >= engines::rules::MAX_RULES {
        return Err(msg("That is as many rules as I can keep in mind. Delete one first."));
    }
    let text = engines::rules::clean_rule_text(&text).map_err(msg)?;
    db.insert_engine_rule(&crate::db::engines::EngineRule {
        id: String::new(),
        engine_id,
        kind,
        text,
        enabled: true,
        auto: false,
        position: 0,
        source: "user".into(),
        hits: 0,
        overrides: 0,
    })
    .map_err(msg)?;
    engines::list_infos(&db).map_err(msg)
}

#[tauri::command]
pub fn engines_rule_update_cmd(db: State<'_, Db>, rule_id: String, patch: RulePatch) -> Cmd<Vec<EngineInfo>> {
    let mut rule = db.get_engine_rule(&rule_id).map_err(msg)?.ok_or_else(|| msg("That rule is gone."))?;
    if let Some(t) = &patch.text {
        rule.text = engines::rules::clean_rule_text(t).map_err(msg)?;
        // The user's words now, even if they started as a default.
        if rule.source == "default" {
            rule.source = "user".into();
        }
    }
    if let Some(k) = &patch.kind {
        if !matches!(k.as_str(), "use" | "avoid") {
            return Err(msg("A rule says when to use it or when not to."));
        }
        rule.kind = k.clone();
        // A rule that says "don't" cannot also be a standing yes.
        if k == "avoid" {
            rule.auto = false;
        }
    }
    if let Some(e) = patch.enabled {
        rule.enabled = e;
    }
    if let Some(a) = patch.auto {
        rule.auto = a && rule.kind == "use";
    }
    db.save_engine_rule(&rule).map_err(msg)?;
    engines::list_infos(&db).map_err(msg)
}

#[tauri::command]
pub fn engines_rule_delete_cmd(db: State<'_, Db>, rule_id: String) -> Cmd<Vec<EngineInfo>> {
    db.delete_engine_rule(&rule_id).map_err(msg)?;
    engines::list_infos(&db).map_err(msg)
}

/// Put the rules in the order the screen shows. Labels renumber from it.
#[tauri::command]
pub fn engines_rules_reorder_cmd(db: State<'_, Db>, engine_id: String, ids: Vec<String>) -> Cmd<Vec<EngineInfo>> {
    db.reorder_engine_rules(&engine_id, &ids).map_err(msg)?;
    engines::list_infos(&db).map_err(msg)
}

/// **Reset to defaults**: the shipped rules again, counts cleared.
#[tauri::command]
pub fn engines_rules_reset_cmd(db: State<'_, Db>, engine_id: String) -> Cmd<Vec<EngineInfo>> {
    let row = row_of(&db, &engine_id)?;
    engines::rules::reset(&db, &row).map_err(msg)?;
    engines::list_infos(&db).map_err(msg)
}

/// `RTE-5`: keep agent apps out of this chat, or let them back in.
#[tauri::command]
pub fn set_conversation_engines_off_cmd(db: State<'_, Db>, id: String, off: bool) -> Cmd<()> {
    db.set_conversation_engines_off(&id, off).map_err(msg)
}

/// `RTE-UI-3`: "This shouldn't have gone to Claude Code". Stops the child if it
/// still runs and counts a no against the rule it cited, which is the signal
/// that rule is wrong.
#[tauri::command]
pub async fn engines_undo_handover_cmd(
    db: State<'_, Db>,
    mgr: State<'_, crate::runtime::RuntimeManager>,
    fleet: State<'_, crate::agent::fleet::Fleet>,
    run_id: String,
) -> Cmd<bool> {
    let Some(run) = db.get_subagent_run(&run_id).map_err(msg)? else { return Ok(false) };
    let (Some(engine_id), Some(label)) = (run.engine_id.clone(), run.because.clone()) else { return Ok(false) };
    if fleet.get(&run_id).is_some() {
        fleet.cancel_tree(&run_id);
    }
    let rules = db.list_engine_rules(&engine_id).map_err(msg)?;
    if let Some((_, rule)) = engines::rules::labels(&rules).into_iter().find(|(l, _)| *l == label) {
        db.bump_engine_rule(&rule.id, false).map_err(msg)?;
        // `RTE-6`: the third no in 30 days raises a proposal about this rule.
        let due = engines::learn::record_override(&db, rule, &run.task);
        engines::learn::follow_up(&db, &mgr.client, None, due).await;
    }
    Ok(true)
}

/// `USE-UI-10`: how a rule fared when a quick decision matched it before the lead
/// ran. `rule` is `<agent id>:<label>`, as the rule list knows it.
#[derive(serde::Serialize)]
pub struct PrematchCount {
    pub rule: String,
    /// The user said *Hand it over* or *Always for this*.
    pub matched: u32,
    /// The user said *Do it yourself*.
    pub declined: u32,
}

#[tauri::command]
pub fn engines_prematch_counts_cmd(db: State<'_, Db>) -> Cmd<Vec<PrematchCount>> {
    use crate::agent::decide::handover::{Label, CARD_SOURCE};
    use crate::agent::decide::Purpose;
    let rows = db.outcome_counts(Purpose::HandoverRoute.name(), CARD_SOURCE).map_err(msg)?;
    let mut out: Vec<PrematchCount> = Vec::new();
    for (rule, label, n) in rows {
        let at = match out.iter().position(|c| c.rule == rule) {
            Some(at) => at,
            None => {
                out.push(PrematchCount { rule, matched: 0, declined: 0 });
                out.len() - 1
            }
        };
        let n = n.max(0) as u32;
        if label == Label::Right.name() {
            out[at].matched += n;
        } else if label == Label::Wrong.name() {
            out[at].declined += n;
        }
    }
    Ok(out)
}

/// `CLI-UI-5` (**Continue there**): open a terminal in the folder the job ran in,
/// running the agent's own `--resume <session>`. Returns the command, so the
/// screen can copy it whatever happens to the terminal.
#[derive(serde::Serialize)]
pub struct ContinueResult {
    /// What to type, with the program's full path when it is not on the PATH.
    pub command: String,
    pub opened: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub fn engines_continue_cmd(db: State<'_, Db>, run_id: String) -> Cmd<ContinueResult> {
    let run = db.get_subagent_run(&run_id).map_err(msg)?.ok_or_else(|| msg("I don't know that job."))?;
    let (Some(engine_id), Some(session)) = (run.engine_id.clone(), run.engine_session.clone()) else {
        return Err(msg("That job has no session to continue."));
    };
    let row = row_of(&db, &engine_id)?;
    // The flag each app's own command line takes to pick a session up.
    let flag = match (row.kind.as_str(), row.id.as_str()) {
        ("claude-code", _) => "--resume",
        ("acp", "acp:opencode") => "--session",
        _ => return Err(msg("I only know how to continue Claude Code and OpenCode sessions so far.")),
    };
    if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(msg("That session id looks wrong."));
    }
    let program = row.program.or(row.found_path).ok_or_else(|| msg("I can't find the app."))?;
    let parent = db.get_conversation(&run.parent_conversation_id).map_err(msg)?;
    let (folder, _) = db.conversation_folder(&run.parent_conversation_id).map_err(msg)?;
    let folder = folder.or(parent.and_then(|c| c.folder_path)).ok_or_else(|| msg("The folder it worked in is gone."))?;
    let shown = if program.contains(' ') { format!("\"{program}\" {flag} {session}") } else { format!("{program} {flag} {session}") };
    // The command is handed back whatever happens to the terminal, so it can be
    // copied and pasted into one the user opens themselves.
    let opened = open_terminal_in(&row.display_name, &folder, &program, &[flag.to_string(), session]);
    Ok(ContinueResult { command: shown, opened: opened.is_ok(), error: opened.err() })
}

#[cfg(windows)]
fn open_terminal_in(title: &str, folder: &str, program: &str, args: &[String]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let bad = |s: &str| s.chars().any(|c| matches!(c, '"' | '%' | '^' | '&' | '|' | '<' | '>' | '!' | '\n' | '\r'));
    if bad(program) || bad(folder) || bad(title) || args.iter().any(|a| bad(a)) {
        return Err("The path has a character I can't pass on safely.".into());
    }
    let mut line = format!("/d /c start \"{title}\" /D \"{folder}\" cmd.exe /d /k \"\"{program}\"");
    for a in args {
        line.push(' ');
        line.push_str(a);
    }
    line.push('"');
    let mut cmd = std::process::Command::new("cmd.exe");
    cmd.raw_arg(line).creation_flags(0x0800_0000);
    cmd.spawn().map(|_| ()).map_err(|e| format!("I couldn't open a terminal: {e}"))
}

#[cfg(not(windows))]
fn open_terminal_in(_title: &str, _folder: &str, _program: &str, _args: &[String]) -> Result<(), String> {
    Err("Opening a terminal is only built for Windows.".into())
}

/// `RTE-7` (**Try a request**): what would the lead do with this? One cheap call
/// on the model the user has picked; the mode and guards are applied here.
#[tauri::command]
pub async fn engines_route_preview_cmd(
    mgr: State<'_, crate::runtime::RuntimeManager>,
    db: State<'_, Db>,
    text: String,
    target: Option<crate::commands::agent::ChatTarget>,
) -> Cmd<engines::rules::Preview> {
    let request = text.trim();
    if request.is_empty() {
        return Err(msg("Type a request to try."));
    }
    let candidates = engines::rules::candidates(&db);
    if candidates.is_empty() {
        return Ok(engines::rules::Preview::default());
    }
    // `DEC-6`: the quick decision first, as a choice among the user's own rules.
    // Guards stay exactly where they were: `preview_from_option` runs them. A
    // wavering or unsure answer, or no decision engine, asks the model below.
    {
        use crate::agent::decide::sites::{self, Route};
        use crate::agent::decide::{ask, Opts, Purpose};
        let options = engines::rules::route_options(&candidates);
        if !options.is_empty() {
            let site = sites::route(request, options, &engines::rules::route_avoid_lines(&candidates));
            if let Ok(verdict) = ask(&site.as_ask(), Purpose::HandoverRoute, Opts::default()).await {
                let routed = sites::route_from(&verdict);
                let act = if matches!(routed, Route::Unsure) {
                    crate::agent::decide::control::Act::Below
                } else {
                    crate::agent::decide::control::Act::Go
                };
                crate::agent::decide::settle(&verdict, Purpose::HandoverRoute, act, Opts::default());
                let quick_ms = Some(verdict.ms);
                match routed {
                    Route::Agent(option) => {
                        if let Some(p) = engines::rules::preview_from_option(&candidates, &option, &engines::rules::Place::assumed()) {
                            return Ok(engines::rules::Preview { quick_ms, ..p });
                        }
                    }
                    Route::Nobody => return Ok(engines::rules::Preview { quick_ms, ..Default::default() }),
                    Route::Unsure => {}
                }
            }
        }
    }
    let setup = crate::commands::agent::resolve_turn(&mgr, &db, &target.unwrap_or_default()).await.map_err(msg)?;
    let msgs = vec![serde_json::json!({ "role": "user", "content": engines::rules::preview_prompt(&candidates, request) })];
    let outcome = crate::cloud::drive_turn(
        &mgr.client,
        &setup.endpoint,
        &msgs,
        &[],
        0.0,
        crate::cloud::Effort::Off,
        &CancelFlag::new(),
        |_| {},
    )
    .await
    .map_err(|e| msg(format!("I couldn't ask the model: {e}")))?;
    let crate::runtime::proxy::TurnOutcome::Final { content, .. } = outcome else {
        return Ok(engines::rules::Preview::default());
    };
    Ok(engines::rules::read_preview(&candidates, &content, &engines::rules::Place::assumed()))
}

/// `CLI-2`: look for installed agents once at start, in the background. Never
/// blocks boot. Tells the window when something new turned up.
pub fn spawn_detect_on_start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let db = app.state::<Db>();
        if db.get_setting("engines.detect_on_start").ok().flatten().as_deref() == Some("false") {
            return;
        }
        if let Ok(found) = engines::detect_all(&db, 0).await {
            if !found.is_empty() {
                let _ = app.emit("poiesis-engines-found", found);
            }
        }
    });
}
