//! External agent apps (Claude Code, Codex, ACP agents) that Poiesis can hand
//! work to (`plans/CLI_ENGINES_PLAN.md`). They are delegation targets, not chat
//! engines: the conversation, memory and the final word stay with Poiesis.
//!
//! Phase 1 is here: the registry (a row per agent), finding and probing them,
//! the driver contract, the settings, and **Try it**. Nothing hands them work
//! yet; `delegate` learns about them in Phase 2.

pub mod acp;
pub mod approve;
pub mod catalog;
pub mod claude;
pub mod codex;
pub mod diag;
pub mod driver;
#[cfg(all(test, windows))]
mod e2e;
pub mod learn;
pub mod locate;
pub mod play;
pub mod probe;
pub mod process;
pub mod rules;
pub mod run;
pub mod settings;
pub mod tools;
pub mod tryit;

use std::collections::BTreeMap;

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};

use crate::db::engines::EngineRow;
use crate::db::{now_ms, Db};

use catalog::CatalogEntry;
use locate::{locate, SearchRoots};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineKind {
    ClaudeCode,
    Codex,
    Acp,
}

/// Who pays for what an agent does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaidBy {
    /// The user's subscription, through the app's own sign-in.
    Plan,
    /// An API key the user chose for it.
    ApiKey,
    /// The user's own provider account (OpenCode).
    Own,
    /// A model the agent offers for nothing (OpenCode's `-free` models).
    Free,
}

impl PaidBy {
    pub fn as_str(self) -> &'static str {
        match self {
            PaidBy::Plan => "plan",
            PaidBy::ApiKey => "api-key",
            PaidBy::Own => "own",
            PaidBy::Free => "free",
        }
    }

    pub fn parse(s: &str) -> Option<PaidBy> {
        match s {
            "plan" => Some(PaidBy::Plan),
            "api-key" => Some(PaidBy::ApiKey),
            "own" => Some(PaidBy::Own),
            "free" => Some(PaidBy::Free),
            _ => None,
        }
    }
}

/// Where the installed version sits against the versions that were recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tested {
    Yes,
    Newer,
    Older,
    Unknown,
}

/// How long a probe stays good before opening Settings runs it again.
const PROBE_MAX_AGE_MS: i64 = 60 * 60 * 1000;
/// `settings` key: the agents found since the user last opened the section.
const UNSEEN_KEY: &str = "engines.unseen";

fn unseen(db: &Db) -> Vec<String> {
    db.get_setting(UNSEEN_KEY)
        .ok()
        .flatten()
        .map(|s| s.split(',').filter(|x| !x.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

fn add_unseen(db: &Db, ids: &[String]) {
    let mut all = unseen(db);
    for id in ids {
        if !all.contains(id) {
            all.push(id.clone());
        }
    }
    let _ = db.set_setting(UNSEEN_KEY, &all.join(","));
}

/// The user opened the section: the badge goes away.
pub fn mark_seen(db: &Db) {
    let _ = db.set_setting(UNSEEN_KEY, "");
}

/// Search for one agent and probe it when that is due. Returns `true` when it
/// was found for the first time, which creates its row (switched off: finding
/// an agent is not consent to hand it work).
///
/// `max_age_ms` is how old the last probe may be before it is repeated; `0`
/// forces it.
pub async fn refresh(db: &Db, roots: &SearchRoots, id: &str, max_age_ms: i64) -> Result<bool, String> {
    let entry = catalog::find(id);
    let existing = db.get_engine(id).map_err(|e| e.to_string())?;
    let (binaries, user_program) = match (&entry, &existing) {
        (Some(e), row) => (e.binaries.clone(), row.as_ref().and_then(|r| r.program.clone())),
        (None, Some(r)) => (Vec::new(), r.program.clone()),
        (None, None) => return Err("I don't know that agent.".into()),
    };
    let found = locate(&binaries, user_program.as_deref().map(std::path::Path::new), roots);

    let mut row = match (existing, &found) {
        (Some(r), _) => r,
        (None, None) => return Ok(false),
        (None, Some(_)) => {
            let e = entry.ok_or("I don't know that agent.")?;
            let r = EngineRow::new(&e.id, e.kind.as_str(), &e.name);
            db.insert_engine_if_absent(&r).map_err(|e| e.to_string())?;
            r
        }
    };
    let newly_found = row.found_path.is_none() && row.last_probe_at.is_none() && found.is_some();
    let found_path = found.as_ref().map(|f| f.path.to_string_lossy().to_string());

    let due = max_age_ms == 0
        || row.found_path != found_path
        || row.last_probe_at.is_none_or(|t| now_ms() - t > max_age_ms.max(0));
    match (&found, due) {
        (None, _) => {
            // Gone (or never there). The row and its choices stay; the version
            // and sign-in it had are no longer facts.
            db.save_engine_probe(id, None, row.version.as_deref(), None, row.paid_by.as_deref(), Some("I can't find it any more."))
                .map_err(|e| e.to_string())?;
        }
        (Some(f), true) => {
            let configured: BTreeMap<String, String> = serde_json::from_str(&row.env_json).unwrap_or_default();
            let env = process::child_env(entry, &configured, None);
            let p = probe::probe(entry, &f.path, &env).await;
            row.found_path = found_path.clone();
            db.save_engine_probe(id, found_path.as_deref(), p.version.as_deref(), p.signed_in, p.paid_by.map(|b| b.as_str()), p.error.as_deref())
                .map_err(|e| e.to_string())?;
        }
        (Some(_), false) => {}
    }
    Ok(newly_found)
}

/// Search for every known agent and probe those that are due. Agents found for
/// the first time are remembered for the badge on the Settings section.
pub async fn detect_all(db: &Db, max_age_ms: i64) -> Result<Vec<String>, String> {
    let roots = SearchRoots::from_system();
    let mut ids: Vec<String> = catalog::all().iter().map(|e| e.id.clone()).collect();
    for r in db.list_engines().map_err(|e| e.to_string())? {
        if !ids.contains(&r.id) {
            ids.push(r.id);
        }
    }
    let results = join_all(ids.iter().map(|id| refresh(db, &roots, id, max_age_ms))).await;
    let new: Vec<String> = ids
        .iter()
        .zip(results)
        .filter_map(|(id, r)| matches!(r, Ok(true)).then(|| id.clone()))
        .collect();
    if !new.is_empty() {
        add_unseen(db, &new);
    }
    Ok(new)
}

/// Opening Settings or regaining focus: cheap file checks, and a probe only
/// where something changed or the last one is old.
pub async fn detect_if_due(db: &Db) -> Result<Vec<String>, String> {
    detect_all(db, PROBE_MAX_AGE_MS).await
}

/// Everything the Settings section shows about one agent. No secret is in it:
/// `has_key` says only whether one is saved.
#[derive(Debug, Serialize)]
pub struct EngineInfo {
    pub id: String,
    pub kind: EngineKind,
    pub name: String,
    /// In the bundled catalog (as opposed to added by hand).
    pub builtin: bool,
    pub found: bool,
    /// An npm `.cmd` wrapper.
    pub shim: bool,
    /// What the user set; `None` = auto-detect.
    pub program: Option<String>,
    pub found_path: Option<String>,
    pub version: Option<String>,
    pub tested: Tested,
    pub signed_in: Option<bool>,
    pub paid_by: Option<PaidBy>,
    pub last_error: Option<String>,
    pub last_probe_at: Option<i64>,
    pub enabled: bool,
    pub payment: String,
    pub trust_ceiling: String,
    pub headless: bool,
    pub memory_tools: bool,
    /// `CAP-14`: *Let it use my connected apps*, for an agent that can be given tools.
    pub app_tools: bool,
    pub model: Option<String>,
    pub extra_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub args: Vec<String>,
    pub has_key: bool,
    pub supports_api_key: bool,
    pub can_pick_model: bool,
    pub install_url: Option<String>,
    /// Found for the first time since the section was last opened.
    pub is_new: bool,
    /// `RTE-1`: `on_request` | `suggest` | `auto`.
    pub use_mode: String,
    /// `RTE-3`.
    pub guards: rules::Guards,
    /// `RTE-2`: the user's rules, with the labels the lead cites.
    pub rules: Vec<RuleView>,
    /// Poiesis can hand this kind of agent a job today. Found, configurable and
    /// testable agents whose driver is not wired for jobs yet say so.
    pub delegate_ready: bool,
    /// This kind of agent can be given Poiesis's own tools (`MCPS-2`). The switch
    /// *Let it use my memory* is shown only for those.
    pub memory_ready: bool,
    /// `CLI-10`: how its recent jobs went.
    pub diagnostics: diag::Summary,
}

/// A rule and the label the lead cites it by (`U2`).
#[derive(Debug, Serialize)]
pub struct RuleView {
    #[serde(flatten)]
    pub rule: crate::db::engines::EngineRule,
    pub label: String,
}

fn info(row: &EngineRow, unseen: &[String], rule_rows: &[crate::db::engines::EngineRule]) -> EngineInfo {
    let entry: Option<&CatalogEntry> = catalog::find(&row.id);
    let kind = EngineKind::parse(&row.kind).unwrap_or(EngineKind::Acp);
    EngineInfo {
        use_mode: row.use_mode.clone(),
        guards: rules::Guards::resolve(&row.guards_json, &row.kind),
        rules: rules::labels(rule_rows)
            .into_iter()
            .map(|(label, r)| RuleView { rule: r.clone(), label })
            .collect(),
        delegate_ready: driver::delegate_ready(kind, &row.id),
        memory_ready: driver::memory_ready(kind, &row.id),
        diagnostics: diag::summary(&row.id),
        id: row.id.clone(),
        kind,
        name: row.display_name.clone(),
        builtin: entry.is_some(),
        found: row.found_path.is_some(),
        shim: row.found_path.as_deref().is_some_and(|p| locate::is_shim(std::path::Path::new(p))),
        program: row.program.clone(),
        found_path: row.found_path.clone(),
        version: row.version.clone(),
        tested: match (entry, &row.version) {
            (Some(e), Some(v)) => probe::tested(e, v),
            _ => Tested::Unknown,
        },
        signed_in: row.signed_in,
        paid_by: row.paid_by.as_deref().and_then(PaidBy::parse),
        last_error: row.last_error.clone(),
        last_probe_at: row.last_probe_at,
        enabled: row.enabled,
        payment: row.payment.clone(),
        trust_ceiling: row.trust_ceiling.clone(),
        headless: row.headless,
        memory_tools: row.memory_tools,
        app_tools: row.app_tools,
        model: row.model.clone(),
        extra_args: serde_json::from_str(&row.extra_args).unwrap_or_default(),
        env: serde_json::from_str(&row.env_json).unwrap_or_default(),
        args: serde_json::from_str(&row.args_json).unwrap_or_default(),
        has_key: crate::secrets::has_secret(crate::secrets::SERVICE_ENGINES, &row.id),
        supports_api_key: entry.is_some_and(|e| e.api_key_env.is_some()),
        can_pick_model: matches!(kind, EngineKind::ClaudeCode | EngineKind::Codex | EngineKind::Acp),
        install_url: entry.map(|e| e.install_url.clone()),
        is_new: unseen.contains(&row.id),
    }
}

/// Every known agent: those with a row, then catalog agents never found (shown
/// as "not found", so the user can point at one).
pub fn list_infos(db: &Db) -> Result<Vec<EngineInfo>, String> {
    // The default rules are written the first time an agent is looked at.
    for r in db.list_engines().map_err(|e| e.to_string())? {
        rules::ensure_seeded(db, &r)?;
    }
    let rows = db.list_engines().map_err(|e| e.to_string())?;
    let unseen = unseen(db);
    let mut out: Vec<EngineInfo> = rows
        .iter()
        .map(|r| info(r, &unseen, &db.list_engine_rules(&r.id).unwrap_or_default()))
        .collect();
    for e in catalog::all() {
        if !rows.iter().any(|r| r.id == e.id) {
            out.push(info(&EngineRow::new(&e.id, e.kind.as_str(), &e.name), &unseen, &[]));
        }
    }
    Ok(out)
}

/// The row for `id`, creating it (switched off) for a catalog agent that has
/// none yet. Used when the user points at a program for an agent that was
/// never found.
pub fn ensure_row(db: &Db, id: &str) -> Result<EngineRow, String> {
    if let Some(r) = db.get_engine(id).map_err(|e| e.to_string())? {
        return Ok(r);
    }
    let e = catalog::find(id).ok_or("I don't know that agent.")?;
    let row = EngineRow::new(&e.id, e.kind.as_str(), &e.name);
    db.insert_engine_if_absent(&row).map_err(|e| e.to_string())?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_has_every_catalog_agent_even_when_none_is_installed() {
        let db = Db::open_in_memory().unwrap();
        let infos = list_infos(&db).unwrap();
        assert_eq!(infos.len(), catalog::all().len());
        assert!(infos.iter().all(|i| !i.found && !i.enabled), "nothing is found, nothing is on");
        let claude = infos.iter().find(|i| i.id == "claude-code").unwrap();
        assert!(claude.supports_api_key && claude.can_pick_model);
        let opencode = infos.iter().find(|i| i.id == "acp:opencode").unwrap();
        assert!(!opencode.supports_api_key && opencode.can_pick_model, "OpenCode lists its models in a new session");
    }

    #[test]
    fn the_unseen_badge_is_set_by_a_find_and_cleared_by_opening_the_section() {
        let db = Db::open_in_memory().unwrap();
        add_unseen(&db, &["codex".to_string()]);
        add_unseen(&db, &["codex".to_string(), "claude-code".to_string()]);
        assert_eq!(unseen(&db), vec!["codex", "claude-code"]);
        assert!(list_infos(&db).unwrap().iter().any(|i| i.id == "codex" && i.is_new));
        mark_seen(&db);
        assert!(list_infos(&db).unwrap().iter().all(|i| !i.is_new));
    }

    /// `CLI-T-12`: a first find makes a switched-off row; an uninstalled agent
    /// keeps its row and its choices; reinstalling brings it back. The stand-in
    /// agent is a `.cmd`, so this runs on Windows only.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_first_find_makes_a_disabled_row_and_a_gone_agent_keeps_its_choices() {
        let tmp = tempfile::tempdir().unwrap();
        let roots = SearchRoots { path_dirs: vec![tmp.path().to_path_buf()], ..Default::default() };
        let db = Db::open_in_memory().unwrap();
        assert!(!refresh(&db, &roots, "claude-code", 0).await.unwrap(), "nothing there yet");
        assert!(db.get_engine("claude-code").unwrap().is_none());

        // A stand-in that answers --version. A .cmd, so it runs on any Windows.
        let shim = tmp.path().join("claude.cmd");
        std::fs::write(&shim, "@echo off\r\necho 2.1.301 (Claude Code)\r\n").unwrap();
        assert!(refresh(&db, &roots, "claude-code", 0).await.unwrap(), "a first find");
        let row = db.get_engine("claude-code").unwrap().unwrap();
        assert!(!row.enabled, "found is not consent");
        assert_eq!(row.version.as_deref(), Some("2.1.301"));
        assert!(row.found_path.is_some());
        assert!(!refresh(&db, &roots, "claude-code", 0).await.unwrap(), "only the first find is news");

        // The user turns it on and picks a model; then it disappears.
        let mut r = db.get_engine("claude-code").unwrap().unwrap();
        r.enabled = true;
        r.model = Some("opus".into());
        db.save_engine_settings(&r).unwrap();
        std::fs::remove_file(&shim).unwrap();
        refresh(&db, &roots, "claude-code", 0).await.unwrap();
        let gone = db.get_engine("claude-code").unwrap().unwrap();
        assert!(gone.found_path.is_none());
        assert!(gone.enabled && gone.model.as_deref() == Some("opus"), "choices are kept");
        assert!(list_infos(&db).unwrap().iter().any(|i| i.id == "claude-code" && !i.found));
    }
}
