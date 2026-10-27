//! The decision model's setup and status (`DEC-3`, `DEC-UI-1`, `DEC-UI-2`): the
//! twin of `rerankgen.rs`. Reuses the shared `llama-server` binary; only the
//! model and launch arguments differ. No setting switches it on: an installed,
//! passing model is the switch, and with none installed every site keeps its
//! old path.

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::{Manager, State};

use crate::agent::decide::policy::{Escalation, Impact, Mode, ALL};
use crate::agent::decide::cloud::{self, CloudModel, CloudStatus, Probe};
use crate::agent::decide::{outcomes, seed, typing, DecideError};
use crate::db::decisions::{DecisionRow, DecisionUsageRow};
use crate::db::{Db, ModelEntry, NewModelEntry};
use crate::runtime::decideserver::{
    self, decide_catalog, DecideCatalogEntry, DecideManager, DecideStatus, Fits, PlacementSetting, PLACEMENT_KEY,
};
use crate::runtime::download::{download_with_resume, DownloadProgress};
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

const ROLE: &str = "decide";

/// `DEC-UI-2`'s status line: grey, amber, green or red, and what it ran on.
#[tauri::command]
pub async fn decide_status_cmd(decide: State<'_, DecideManager>, db: State<'_, Db>) -> Cmd<DecideStatus> {
    let name = db
        .default_model_by_role(ROLE)
        .map_err(err)?
        .filter(|m| PathBuf::from(&m.path).exists())
        .map(|m| m.name);
    Ok(decide.status(name))
}

/// One row of the read-only table under Runtime's "Advanced" (`DEC-UI-2`,
/// `CTL-1`).
#[derive(serde::Serialize)]
pub struct PolicyRow {
    pub purpose: &'static str,
    pub label: &'static str,
    /// Runs without the user asking in that moment, so it stays on this computer.
    pub background: bool,
    /// A person or a streaming turn is waiting: skipped, not waited for.
    pub skipped_if_not_ready: bool,
    pub deadline_s: u64,
    pub mode: Mode,
    pub impact: Impact,
    pub escalate: Escalation,
    /// The bar once a fit backs the numbers, and the one in force until then.
    pub act_at_calibrated: f32,
    pub act_at_uncalibrated: f32,
    /// What happens below the bar, in words.
    pub below: &'static str,
    /// The experiment that has to be on, if any.
    pub experiment: Option<&'static str>,
    /// `cloud` when the person allowed this purpose to use the cloud by name
    /// (decision 12); otherwise nothing.
    pub backend_override: Option<String>,
}

#[tauri::command]
pub fn decide_policy_cmd(db: State<'_, Db>) -> Vec<PolicyRow> {
    crate::agent::decide::policy::ALL
        .iter()
        .map(|p| {
            let policy = p.policy();
            PolicyRow {
                purpose: p.name(),
                label: p.label(),
                background: policy.background,
                skipped_if_not_ready: policy.needs_fast,
                deadline_s: policy.deadline.as_secs(),
                mode: policy.mode,
                impact: policy.impact,
                escalate: policy.escalate,
                act_at_calibrated: policy.act_at.calibrated,
                act_at_uncalibrated: policy.act_at.uncalibrated,
                below: p.below(),
                experiment: policy.experiment,
                backend_override: db
                    .get_setting(&crate::agent::decide::policy::override_key(*p))
                    .ok()
                    .flatten()
                    .filter(|v| v == "cloud"),
            }
        })
        .collect()
}

/// `CTL-UI-1`: a decision as the Activity screen shows it, with the labels that
/// were written for it and those read from what the user did.
#[derive(serde::Serialize)]
pub struct DecisionView {
    #[serde(flatten)]
    pub row: DecisionRow,
    pub label: &'static str,
    /// Runs without the user asking: the screen groups these by the hour.
    pub background: bool,
    /// The experiment the purpose belongs to, for the filter chips.
    pub experiment: Option<&'static str>,
    pub labels: Vec<LabelView>,
}

#[derive(serde::Serialize)]
pub struct LabelView {
    pub label: &'static str,
    pub value: Option<String>,
    pub source: String,
}

fn view(db: &Db, row: DecisionRow) -> DecisionView {
    let labels = outcomes::labels_for(db, &row)
        .into_iter()
        .map(|l| LabelView { label: l.kind.code(), value: l.value, source: l.source })
        .collect();
    let purpose = ALL.iter().find(|p| p.name() == row.purpose);
    DecisionView {
        label: purpose.map_or("", |p| p.label()),
        // "Folded by the hour" on the screen: the ones nobody waits for,
        // and the ones asked once for each of many items.
        background: purpose.is_some_and(|p| p.policy().background || p.is_bulk()),
        experiment: purpose.and_then(|p| p.policy().experiment),
        row,
        labels,
    }
}

#[tauri::command]
pub fn decisions_list_cmd(db: State<'_, Db>, limit: Option<i64>, purpose: Option<String>) -> Cmd<Vec<DecisionView>> {
    let rows = db.decisions_recent(limit.unwrap_or(200).clamp(1, 1000), purpose.as_deref()).map_err(err)?;
    Ok(rows.into_iter().map(|row| view(&db, row)).collect())
}

/// `SEE-2`: the receipt of one turn. Its own decisions and the ones made while it
/// ran that no run id was kept for. A decision nobody waits for (a mail triage, a
/// reflection) is about something else and is left out.
#[tauri::command]
pub fn run_decisions_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    run_id: String,
    from_ms: i64,
    to_ms: i64,
) -> Cmd<Vec<DecisionView>> {
    let rows = db.decisions_of_turn(&conversation_id, &run_id, from_ms, to_ms).map_err(err)?;
    Ok(rows
        .into_iter()
        .filter(|row| !ALL.iter().any(|p| p.name() == row.purpose && p.policy().background))
        .map(|row| view(&db, row))
        .collect())
}

/// `SEE-2`: "That was wrong" on a receipt row. For the tool menu `value` is the
/// unit the person says was needed. A second press from the same place is ignored.
#[tauri::command]
pub fn receipt_wrong_cmd(db: State<'_, Db>, decision_id: String, value: Option<String>) -> Cmd<bool> {
    let source = match &value {
        Some(v) => format!("receipt:{v}"),
        None => "receipt".to_string(),
    };
    db.add_outcome(&decision_id, "wrong", value.as_deref(), &source).map_err(err)
}

/// `DEC-UI-2`'s Start, from the strip's "Quick thinking is off" line. Returns once
/// the engine is up, or says why it is not.
#[tauri::command]
pub async fn decide_start_cmd(
    decide: State<'_, DecideManager>,
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
) -> Cmd<()> {
    let Some(local) = decideserver::resolve_local(&mgr, &db).await else {
        return Err(err("Install a decision model first."));
    };
    decide.ensure_started(&mgr.client, &local).await.map_err(err)?;
    Ok(())
}

/// `SEE-3`: what the composer has to say about a message while it is typed. At most one
/// hint, in the order `typing::first` gives. Asked of the backend the person chose: a cloud
/// decision model answers as well as a local one on the card, and a local engine that is
/// still cold is started for next time and says nothing now (`decide::ask_typing`). `None`
/// is the answer for everything else, and it is never an error: the composer shows nothing.
#[tauri::command]
pub async fn composer_hint_cmd(
    db: State<'_, Db>,
    fleet: State<'_, crate::agent::fleet::Fleet>,
    conversation_id: String,
    text: String,
    run_id: Option<String>,
) -> Cmd<Option<typing::Hint>> {
    if !typing::askable(&text) || !crate::experiments::is_on(&db, "fast-slow") {
        return Ok(None);
    }
    let previous = db
        .list_messages_window(&conversation_id, 8)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .find(|m| m.role == "assistant" && !m.content.trim().is_empty())
        .map(|m| m.content)
        .unwrap_or_default();
    // The same clipping the turn applies, so the state is the one the send would ask about.
    let (message, previous) = crate::agent::decide::tools::last_exchange(&[
        serde_json::json!({ "role": "assistant", "content": previous }),
        serde_json::json!({ "role": "user", "content": text.trim() }),
    ]);
    let live = run_id.as_deref().and_then(|id| fleet.get(id)).filter(|r| r.conversation_id == conversation_id);
    let reader = Typed {
        db: &db,
        conversation_id: &conversation_id,
        message,
        previous,
        task: live.as_ref().map(|r| crate::commands::agent::current_task(&db, &r.conversation_id)),
    };
    Ok(typing::first(&reader, live.is_some()).await)
}

/// The composer's three questions, asked of the quick decision model for one message.
struct Typed<'a> {
    db: &'a Db,
    conversation_id: &'a str,
    message: String,
    previous: String,
    /// What the working run is doing, when there is one.
    task: Option<String>,
}

impl typing::Reader for Typed<'_> {
    async fn stop(&self) -> Option<typing::Hint> {
        use crate::agent::decide::{ask_typing, loopctl, Opts, Purpose};
        let site = loopctl::steer_ask(self.task.as_deref().unwrap_or_default(), &self.message);
        let verdict = ask_typing(&site.as_ask(), Purpose::SteerClass, Opts::default()).await.ok()?;
        typing::stop_hint(&verdict)
    }

    async fn handover(&self) -> Option<typing::Hint> {
        use crate::agent::decide::{ask_typing, handover, Opts, Purpose};
        use crate::agent::toolsets::Toolset;
        use crate::engines::rules;
        if !Toolset::Subagents.is_enabled(self.db) {
            return None;
        }
        let route = rules::RouteCtx { conversation_id: self.conversation_id, headless: false, delegate_to: None };
        let (offered, _) = rules::offered(self.db, &route);
        let offered: Vec<_> =
            offered.into_iter().filter(|o| rules::within_limit(self.db, o, self.conversation_id).is_ok()).collect();
        let site = handover::ask_for(&self.message, &self.previous, &offered)?;
        let verdict = ask_typing(&site.as_ask(), Purpose::HandoverRoute, Opts::default()).await.ok()?;
        // The send reads this answer if the words are the same.
        typing::remember(self.conversation_id, Purpose::HandoverRoute, &site.as_ask(), &verdict);
        typing::handover_hint(&offered, &verdict)
    }

    /// A group the person switched off, asked about as the tool menu asks about the ones
    /// that are on.
    async fn web(&self) -> Option<typing::Hint> {
        use crate::agent::decide::{ask_typing, tools, Opts, Purpose};
        use crate::agent::toolsets::Toolset;
        let web = Toolset::WebSearch;
        if web.is_enabled(self.db) {
            return None;
        }
        let offers: Vec<(String, tools::Owner)> = web
            .tool_specs()
            .iter()
            .filter_map(|s| s.pointer("/function/name").and_then(|n| n.as_str()))
            .map(|name| (name.to_string(), tools::Owner::Builtin(web)))
            .collect();
        let units = tools::units(&offers, "", &[]);
        let ask = tools::ask_for(&self.message, &self.previous, &units)?;
        let verdict = ask_typing(&ask.as_ask(), Purpose::ToolSelect, Opts::default()).await.ok()?;
        typing::web_hint(&verdict)
    }
}

/// `SEE-3`: the person took a hint's action, or put it away. Either is a label on the
/// decision that said it; sending the message with the hint standing is not.
#[tauri::command]
pub fn composer_hint_label_cmd(db: State<'_, Db>, decision_id: String, kind: typing::Kind, taken: bool) -> Cmd<()> {
    // Held until now: a hint nobody acted on leaves nothing in the log.
    typing::log_shown(&db, &decision_id);
    db.set_outcome(&decision_id, typing::label(kind, taken), None, typing::SOURCE).map_err(err)
}

/// `USE-UI-5`: a person picks an answer for a row of a sorted list. Returns the
/// label it became (`right` when it is the model's own answer, `corrected` when
/// it is another).
#[tauri::command]
pub fn classify_correct_cmd(db: State<'_, Db>, decision_id: String, answer: String) -> Cmd<&'static str> {
    crate::agent::classify::correct(&db, &decision_id, &answer).map_err(err)
}

/// `USE-UI-2`: the user answered "Stop the run?". The answer is a label for the
/// decision that asked; stopping the run itself is the ordinary Stop.
#[tauri::command]
pub fn steer_answer_cmd(db: State<'_, Db>, decision_id: String, stopped: bool) {
    crate::agent::decide::loopctl::label_stop_prompt(&db, &decision_id, stopped);
}

/// `USE-UI-3`: the user reopened a ringed tick. The item goes back to open in the
/// stored plan and the decision that ticked it is labelled wrong. `false` when
/// there is nothing to reopen (no such message or item, or a tick the run gave
/// itself).
#[tauri::command]
pub fn reopen_plan_item_cmd(db: State<'_, Db>, message_id: String, index: usize) -> Cmd<bool> {
    let Some(json) = db.message_plan_json(&message_id).map_err(err)? else { return Ok(false) };
    let Ok(mut plan) = serde_json::from_str::<crate::agent::plan::Plan>(&json) else { return Ok(false) };
    let Some(decision) = plan.reopen(index) else { return Ok(false) };
    let json = serde_json::to_string(&plan).map_err(err)?;
    db.set_message_plan(&message_id, &json).map_err(err)?;
    crate::agent::decide::loopctl::label_reopened(&db, &decision);
    Ok(true)
}

/// `CTL-4`: what each purpose's fit says.
#[tauri::command]
pub fn decide_calibration_cmd(db: State<'_, Db>) -> Vec<seed::Report> {
    seed::reports(&db)
}

/// One row of the "Calibrate now" run.
#[derive(Clone, serde::Serialize)]
pub struct CalibrateProgress {
    pub purpose: &'static str,
    pub done: usize,
    pub total: usize,
}

/// `CTL-4`: run the seed sets through the engine and fit each purpose. Takes a
/// few minutes on a processor and seconds on a card.
#[tauri::command]
pub async fn calibrate_decide_cmd(
    app: tauri::AppHandle,
    on_progress: Channel<CalibrateProgress>,
) -> Cmd<Vec<seed::Report>> {
    let reports = seed::calibrate_now(&app, |purpose, done, total| {
        let _ = on_progress.send(CalibrateProgress { purpose: purpose.name(), done, total });
    })
    .await
    .map_err(|e| match e {
        DecideError::Unavailable => err("Install a decision model first."),
        other => err(other),
    })?;
    let _ = app.state::<Db>().log_activity(None, "decide", "Calibrated the decision model");
    Ok(reports)
}

/// `CTL-3`: something happened to a goal that the decision had judged. Returns
/// the label written, if any. The goal lives on the screen, so the screen says.
#[tauri::command]
pub fn record_goal_outcome_cmd(db: State<'_, Db>, conversation_id: String, event: String) -> Cmd<Option<String>> {
    let Some(event) = outcomes::GoalEvent::parse(&event) else {
        return Err(err("I don't know that goal event."));
    };
    Ok(outcomes::record_goal_event(&db, &conversation_id, event).map(|k| k.code().to_string()))
}

/// One catalog entry as the Models screen shows it.
#[derive(serde::Serialize)]
pub struct DecideModelRow {
    #[serde(flatten)]
    pub entry: DecideCatalogEntry,
    /// The library row when the file is installed.
    pub id: Option<String>,
    pub installed: bool,
    pub is_default: bool,
    /// Where it would run, beside what is loaded now.
    pub fit: Fits,
}

#[derive(serde::Serialize)]
pub struct DecideModels {
    pub models: Vec<DecideModelRow>,
    /// The chat model currently loaded, for "next to Qwen3.5-4B".
    pub chat_model: Option<String>,
    /// `auto` | `gpu` | `cpu`
    pub placement: String,
}

#[tauri::command]
pub async fn decide_models_cmd(mgr: State<'_, RuntimeManager>, db: State<'_, Db>) -> Cmd<DecideModels> {
    let installed = db.list_models_by_role(ROLE).map_err(err)?;
    let setting = PlacementSetting::parse(db.get_setting(PLACEMENT_KEY).ok().flatten().as_deref());
    let mut models = Vec::new();
    for entry in decide_catalog() {
        let row = installed
            .iter()
            .find(|m| PathBuf::from(&m.path).file_name().and_then(|n| n.to_str()) == Some(entry.filename.as_str()))
            .filter(|m| PathBuf::from(&m.path).exists());
        let loads = decideserver::loads_for(&mgr, &db, entry.size_mb).await;
        models.push(DecideModelRow {
            fit: decideserver::plan_fit(setting, &loads),
            id: row.map(|m| m.id.clone()),
            installed: row.is_some(),
            is_default: row.map(|m| m.is_default).unwrap_or(false),
            entry,
        });
    }
    Ok(DecideModels {
        models,
        chat_model: mgr.engine_model_name().await,
        placement: db
            .get_setting(PLACEMENT_KEY)
            .ok()
            .flatten()
            .filter(|p| p == "gpu" || p == "cpu")
            .unwrap_or_else(|| "auto".into()),
    })
}

/// A file that is there and the right size. A short file is a download that
/// stopped, and a different size is a file that is not the one in the catalog.
fn is_whole(path: &std::path::Path, bytes: u64) -> bool {
    std::fs::metadata(path).map(|m| m.len() == bytes).unwrap_or(false)
}

/// Download a catalog model and its vision projector, add it to the library,
/// and run the head check. A file that fails the check is removed again, with
/// the words the screen shows.
#[tauri::command]
pub async fn download_decide_model_cmd(
    mgr: State<'_, RuntimeManager>,
    decide: State<'_, DecideManager>,
    db: State<'_, Db>,
    name: String,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<ModelEntry> {
    let entry = decide_catalog()
        .into_iter()
        .find(|e| e.name == name)
        .ok_or_else(|| err("I don't know that decision model."))?;
    crate::commands::runtime::provision_active(&mgr, &db, &on_progress).await?;

    let dir = decideserver::models_dir(&mgr);
    let model = dir.join(&entry.filename);
    let mmproj = dir.join(&entry.mmproj_filename);
    for (url, dest, bytes, label) in [
        (&entry.url, &model, entry.bytes, "Downloading the decision model"),
        (&entry.mmproj_url, &mmproj, entry.mmproj_bytes, "Downloading its picture reader"),
    ] {
        if !is_whole(dest, bytes) {
            download_with_resume(&mgr.client, url, dest, label, |p| {
                let _ = on_progress.send(p);
            })
            .await
            .map_err(err)?;
        }
        if !is_whole(dest, bytes) {
            let _ = std::fs::remove_file(dest);
            return Err(err("The download is not the file I expected. Try again."));
        }
    }

    let local = decideserver::Local {
        server_binary: crate::commands::embedgen::engine_binary_path(&mgr, &db)
            .await?
            .ok_or_else(|| err("The engine is not installed yet."))?,
        model_path: model.clone(),
        model_name: entry.name.clone(),
        mmproj: Some(mmproj.clone()),
        gpu: decideserver::plan_fit(
            PlacementSetting::parse(db.get_setting(PLACEMENT_KEY).ok().flatten().as_deref()),
            &decideserver::loads_for(&mgr, &db, entry.size_mb).await,
        ) == Fits::Gpu,
    };
    let checked = decideserver::head_check(&decide, &mgr.client, &local).await;
    // The check started the engine on this file. Let go of it either way: it is
    // not necessarily the default, and a fresh start places it properly.
    decide.stop().await;
    if let Err(message) = checked {
        let _ = std::fs::remove_file(&model);
        let _ = std::fs::remove_file(&mmproj);
        return Err(err(message));
    }

    // Already in the library (a second download of the same file): reuse the row.
    if let Some(existing) = db
        .list_models_by_role(ROLE)
        .map_err(err)?
        .into_iter()
        .find(|m| PathBuf::from(&m.path) == model)
    {
        return Ok(existing);
    }
    let size_bytes = std::fs::metadata(&model).map(|m| m.len() as i64).ok();
    let added = db
        .add_model_with_role(
            &NewModelEntry {
                name: entry.name.clone(),
                path: model.to_string_lossy().to_string(),
                quant: Some("Q4_K_M".into()),
                size_bytes,
                vision: true,
            },
            ROLE,
        )
        .map_err(err)?;
    let _ = db.log_activity(None, "decide", &format!("Installed the decision model {}", entry.name));
    Ok(added)
}

/// Switch which model decides. The engine stops so the next start loads the
/// new one.
#[tauri::command]
pub async fn set_default_decide_model_cmd(decide: State<'_, DecideManager>, db: State<'_, Db>, id: String) -> Cmd<()> {
    let previous = db.default_model_by_role(ROLE).map_err(err)?.map(|m| m.id);
    if previous.as_deref() == Some(id.as_str()) {
        return Ok(());
    }
    db.set_default_model(&id).map_err(err)?;
    decide.stop().await;
    Ok(())
}

/// Remove a decision model and its projector.
#[tauri::command]
pub async fn delete_decide_model_cmd(decide: State<'_, DecideManager>, db: State<'_, Db>, id: String) -> Cmd<()> {
    let was_default = db
        .default_model_by_role(ROLE)
        .map_err(err)?
        .map(|m| m.id == id)
        .unwrap_or(false);
    if was_default {
        decide.stop().await;
    }
    if let Some(path) = db.delete_model(&id).map_err(err)? {
        let path = PathBuf::from(path);
        let projector = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(decideserver::catalog_entry_for)
            .map(|e| path.with_file_name(e.mmproj_filename));
        let _ = std::fs::remove_file(&path);
        if let Some(p) = projector {
            let _ = std::fs::remove_file(p);
        }
    }
    Ok(())
}

/// `DEC-UI-2`'s "Runs on": `auto` | `gpu` | `cpu`. The engine stops so the next
/// start is placed again.
#[tauri::command]
pub async fn set_decide_placement_cmd(decide: State<'_, DecideManager>, db: State<'_, Db>, placement: String) -> Cmd<()> {
    if !matches!(placement.as_str(), "auto" | "gpu" | "cpu") {
        return Err(err("Pick automatic, graphics card or processor."));
    }
    db.set_setting(PLACEMENT_KEY, &placement).map_err(err)?;
    decide.stop().await;
    Ok(())
}

// ---- the cloud (`DEC-5`, `DEC-5a`, `DEC-UI-1`, `DEC-UI-2`) ----

/// One OpenRouter decision model as the Models screen shows it.
#[derive(serde::Serialize)]
pub struct CloudModelRow {
    #[serde(flatten)]
    pub model: CloudModel,
    /// The model quick decisions use when the cloud is chosen.
    pub selected: bool,
    /// What the last Test found.
    pub probe: Option<Probe>,
}

#[derive(serde::Serialize)]
pub struct CloudCatalog {
    pub key_set: bool,
    pub models: Vec<CloudModelRow>,
    /// Epoch ms of the list shown. `None` when there is none.
    pub fetched_at: Option<i64>,
    /// Why the list could not be refreshed, if it could not.
    pub error: Option<String>,
    pub selected: String,
    /// `local` | `cloud`
    pub backend: String,
}

/// The list of OpenRouter's decision models (public, so it needs no key), from the
/// cache unless it is a day old or `refresh` is set.
#[tauri::command]
pub async fn decide_cloud_catalog_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    refresh: Option<bool>,
) -> Cmd<CloudCatalog> {
    let (cache, error) = cloud::load_catalog(&mgr.client, &db, refresh.unwrap_or(false)).await;
    let selected = cloud::selected_model(&db);
    let status = cloud::status(&db);
    let models = cache
        .as_ref()
        .map(|c| c.models.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|m| CloudModelRow {
            selected: m.slug == selected || m.id == selected,
            probe: cloud::saved_probe(&db, &m.slug),
            model: m.clone(),
        })
        .collect();
    Ok(CloudCatalog {
        key_set: status.key_set,
        models,
        fetched_at: cache.map(|c| c.fetched_at),
        error,
        selected,
        backend: status.backend,
    })
}

/// `DEC-5a`'s Test: a fixed request that must come back well formed, and for a
/// model that lists images, one picture. The result is kept per model.
#[tauri::command]
pub async fn decide_cloud_probe_cmd(mgr: State<'_, RuntimeManager>, db: State<'_, Db>, slug: String) -> Cmd<Probe> {
    let model = cloud::cached_catalog(&db)
        .and_then(|c| c.models.into_iter().find(|m| m.slug == slug || m.id == slug))
        .ok_or_else(|| err("I don't know that model. Refresh the list first."))?;
    let probe = cloud::probe(&mgr.client, &db, &model).await;
    cloud::save_probe(&db, &model.slug, &probe);
    Ok(probe)
}

/// Pick the model quick decisions use in the cloud. It does not switch the
/// backend; that is Runtime's choice. A model with no zero-retention provider is
/// refused here as well as greyed on the screen.
#[tauri::command]
pub fn decide_cloud_select_cmd(db: State<'_, Db>, slug: String) -> Cmd<()> {
    let model = cloud::cached_catalog(&db)
        .and_then(|c| c.models.into_iter().find(|m| m.slug == slug))
        .ok_or_else(|| err("I don't know that model. Refresh the list first."))?;
    if !model.zdr {
        return Err(err("No provider of that model keeps nothing, so I won't send questions to it."));
    }
    db.set_setting(cloud::MODEL_KEY, &model.slug).map_err(err)?;
    cloud::forget_faults();
    Ok(())
}

/// `DEC-UI-2`'s Backend: `local` | `cloud`. The cloud needs an OpenRouter key.
#[tauri::command]
pub fn set_decide_backend_cmd(db: State<'_, Db>, backend: String) -> Cmd<()> {
    match backend.as_str() {
        "local" => {}
        "cloud" => {
            if cloud::api_key(&db).is_none() {
                return Err(err("Add an OpenRouter key in Providers first."));
            }
        }
        _ => return Err(err("Pick this computer or the cloud.")),
    }
    db.set_setting(cloud::BACKEND_KEY, &backend).map_err(err)?;
    cloud::forget_faults();
    let _ = db.log_activity(
        None,
        "decide",
        if backend == "cloud" { "Quick decisions now use OpenRouter" } else { "Quick decisions now stay on this computer" },
    );
    Ok(())
}

/// `DEC-UI-2` Advanced: "Allow the cloud for this", for a purpose that runs in the
/// background and so stays on this computer unless the person says otherwise
/// (decision 12).
#[tauri::command]
pub fn set_decide_purpose_cloud_cmd(db: State<'_, Db>, purpose: String, allow: bool) -> Cmd<()> {
    let p = ALL.iter().find(|p| p.name() == purpose).ok_or_else(|| err("I don't know that decision."))?;
    if !p.policy().background {
        return Err(err("Only decisions that run on their own have this choice."));
    }
    if allow && cloud::api_key(&db).is_none() {
        return Err(err("Add an OpenRouter key in Providers first."));
    }
    db.set_setting(&crate::agent::decide::policy::override_key(*p), if allow { "cloud" } else { "" }).map_err(err)?;
    Ok(())
}

/// What Runtime shows about the cloud.
#[tauri::command]
pub fn decide_cloud_status_cmd(db: State<'_, Db>) -> CloudStatus {
    cloud::status(&db)
}

/// `CTL-UI-2`: the Usage screen's "Decisions" section.
#[tauri::command]
pub fn decide_usage_cmd(db: State<'_, Db>, days: i64) -> Cmd<Vec<DecisionUsageRow>> {
    let since = crate::db::now_ms() - days.clamp(1, 365) * 86_400_000;
    db.decision_usage_since(since).map_err(err)
}
