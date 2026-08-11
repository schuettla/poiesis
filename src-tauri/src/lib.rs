//! Project Poiesis — Tauri application backend.
//!
//! The backend is organized into subsystems that map to the implementation plan
//! (`docs/IMPLEMENTATION_PLAN.md`). Each is grown in its own phase; this module
//! wires them into the Tauri runtime and exposes the IPC command surface.

// `pub` so the `tests/eval` integration harness (EVL) can drive a real agent
// run the same way `commands/agent.rs` does, from outside the crate.
pub mod agent;
pub mod autonomy;
pub mod cloud;
mod commands;
pub mod db;
mod marketplace;
mod mcp;
pub mod media;
pub mod memory;
pub mod permissions;
pub mod runtime;
mod secrets;
mod telemetry;

use tauri::{Emitter, Manager};

use db::Db;
use permissions::PermissionManager;
use runtime::{EmbedManager, RerankManager, RuntimeManager};

/// Application-wide error type surfaced to the frontend as a string.
#[derive(Debug, thiserror::Error)]
pub enum PoiesisError {
    #[error("{0}")]
    Message(String),
}

impl serde::Serialize for PoiesisError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type PoiesisResult<T> = Result<T, PoiesisError>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // All local state (runtimes, models, db) lives under the app-data dir.
            let base_dir = app
                .path()
                .app_data_dir()
                .expect("could not resolve app-data directory");
            std::fs::create_dir_all(&base_dir).ok();

            // Open the SQLite database (conversations, settings, library, …).
            let db = Db::open(&base_dir.join("poiesis.db")).expect("failed to open database");
            // Content-free, opt-in only (no-ops unless the user enabled it).
            telemetry::record(&db, "app_open");

            // The durable self: markdown files the user owns, alongside models/.
            let memory =
                memory::MemoryStore::new(&base_dir).expect("failed to open the memory folder");
            // The FTS index is derived state — rebuild it from disk at startup so
            // hand-edits made outside the app are searchable.
            memory.sync_fts(&db);
            // Set aside anything a hand-edit left unreadable, and say so in the
            // activity log rather than letting it disappear quietly (HEAL-3).
            memory.quarantine_scan(&db);
            // `SKL-5`: procedures saved before Agent Skills existed become
            // skills, once. Runs before anything reads the skills folder, and
            // moves the originals to `.trash/` rather than deleting them.
            memory::recipe_legacy::migrate(&memory, &db, &base_dir.join("skills"));
            // Drop file-undo snapshots past their retention window, so the trash
            // can't grow without bound behind the user's back.
            agent::trash::prune(&db, &base_dir);
            // BRW-1: each conversation's browsing keeps its own Chrome
            // profile — tens of megabytes — so long-dead ones are let go.
            agent::browser::prune_profiles(&base_dir);
            agent::browser::prune_screenshots(&base_dir);
            // FIX-1: fail→fix pairs hold content (arguments, error text), so
            // they're pruned on a much shorter horizon than tool_stats.
            let _ = db.prune_tool_fixes(30);
            // TTL-2: let short-lived facts go at startup, not only overnight.
            let swept = memory.sweep_expired(&db);
            if !swept.is_empty() {
                let _ = db.log_activity(
                    None,
                    "memory",
                    &format!("let {} expired notes go", swept.len()),
                );
                let _ = app.emit("poiesis-expiry-swept", serde_json::json!({ "count": swept.len() }));
            }
            // GLD-1: seed the built-in golden cases on first run, merging by id
            // so a user's own additions are never overwritten.
            agent::golden::seed_builtin_cases(&base_dir);
            app.manage(db);
            app.manage(memory);

            // Paths the user picks in a native dialog become readable for the
            // session — a dialog is consent, but only for what was picked.
            // Poiesis's own output (generated images, exports) is readable too.
            let grants = commands::files::DialogGrants::new();
            grants.allow_app_data(&base_dir);
            app.manage(grants);

            app.manage(RuntimeManager::new(base_dir));
            app.manage(EmbedManager::new());
            runtime::embedserver::spawn_idle_stop(app.handle().clone());
            app.manage(RerankManager::new());
            runtime::rerankserver::spawn_idle_stop(app.handle().clone());
            app.manage(PermissionManager::new());
            app.manage(agent::index::IndexManager::new());
            app.manage(commands::scheduler::SchedulerState::new());
            commands::scheduler::spawn_ticker(app.handle().clone());
            // BRW-1: one browser session per conversation, closed on idle.
            app.manage(agent::browser::BrowserPool::new());
            agent::browser::spawn_idle_sweep(app.handle().clone());
            // `JOB-1`: media generation runs in the background and announces
            // itself app-wide. Also sweeps jobs a crash left mid-flight, so a
            // placeholder can't spin forever across a restart. Must come after
            // `app.manage(db)` — the sweep reads it.
            media::jobs::init(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::runtime::detect_hardware_cmd,
            commands::runtime::recommend_runtime_cmd,
            commands::runtime::runtime_status_cmd,
            commands::runtime::ensure_runtime_cmd,
            commands::runtime::load_model_cmd,
            commands::runtime::stop_engine_cmd,
            commands::runtime::runtime_overview_cmd,
            commands::runtime::set_backend_override_cmd,
            commands::runtime::start_engine_cmd,
            commands::runtime::check_runtime_update_cmd,
            commands::runtime::chat_cmd,
            commands::runtime::stop_chat_cmd,
            commands::runtime::get_context_budget_cmd,
            commands::conversations::compact_conversation_cmd,
            commands::memory::get_memory_context_cmd,
            commands::memory::recall_for_cmd,
            commands::memory::context_manifest_cmd,
            commands::memory::list_memory_facts_cmd,
            commands::memory::update_memory_fact_cmd,
            commands::memory::set_fact_scope_cmd,
            commands::memory::forget_memory_fact_cmd,
            commands::memory::restore_memory_fact_cmd,
            commands::memory::set_soul_cmd,
            commands::memory::get_profile_cmd,
            commands::memory::rebuild_profile_cmd,
            commands::memory::edit_profile_cmd,
            commands::memory::undo_profile_rebuild_cmd,
            commands::memory::open_memory_dir_cmd,
            commands::memory::export_memory_zip_cmd,
            commands::memory::list_change_proposals_cmd,
            commands::memory::update_change_proposal_text_cmd,
            commands::memory::resolve_change_proposal_cmd,
            commands::memory::consolidate_memory_cmd,
            commands::memory::get_pending_consolidation_cmd,
            commands::memory::apply_consolidation_cmd,
            commands::reflect::reflect_conversation_cmd,
            commands::reflect::list_lessons_cmd,
            commands::reflect::forget_lesson_cmd,
            commands::organism::get_vitality_cmd,
            commands::organism::get_tool_health_cmd,
            commands::organism::restore_quarantined_cmd,
            commands::organism::delete_quarantined_cmd,
            commands::organism::set_surface_cmd,
            commands::organism::get_golden_status_cmd,
            commands::organism::check_golden_cmd,
            commands::conversations::list_conversations_cmd,
            commands::conversations::create_conversation_cmd,
            commands::conversations::rename_conversation_cmd,
            commands::conversations::set_conversation_workspace_cmd,
            commands::conversations::delete_conversation_cmd,
            commands::conversations::list_messages_cmd,
            commands::conversations::append_message_cmd,
            commands::conversations::finalize_message_cmd,
            commands::conversations::search_conversations_cmd,
            commands::conversations::list_artifacts_cmd,
            commands::conversations::list_all_artifacts_cmd,
            commands::conversations::list_blocks_cmd,
            commands::conversations::update_block_state_cmd,
            commands::conversations::get_session_state_cmd,
            commands::conversations::set_session_state_cmd,
            commands::conversations::get_setting_cmd,
            commands::conversations::set_setting_cmd,
            commands::personas::list_personas_cmd,
            commands::personas::create_persona_cmd,
            commands::personas::update_persona_cmd,
            commands::personas::delete_persona_cmd,
            commands::personas::set_default_persona_cmd,
            commands::personas::set_conversation_persona_cmd,
            commands::models::recommended_catalog_cmd,
            commands::models::search_huggingface_cmd,
            commands::models::list_repo_files_cmd,
            commands::models::list_github_models_cmd,
            commands::models::download_model_cmd,
            commands::models::add_local_model_cmd,
            commands::models::list_models_cmd,
            commands::models::delete_model_cmd,
            commands::models::set_default_model_cmd,
            commands::agent::agent_chat_cmd,
            commands::agent::resolve_permission_cmd,
            commands::agent::list_toolsets_cmd,
            commands::agent::get_tool_stats_cmd,
            commands::agent::set_toolset_enabled_cmd,
            commands::imagegen::image_setup_status_cmd,
            commands::imagegen::setup_image_generation_cmd,
            commands::imagegen::install_image_engine_cmd,
            commands::imagegen::image_catalog_cmd,
            commands::imagegen::list_image_models_cmd,
            commands::imagegen::download_image_model_cmd,
            commands::imagegen::generate_image_cmd,
            commands::imagegen::set_default_image_model_cmd,
            commands::imagegen::delete_image_model_cmd,
            commands::media::list_media_models_cmd,
            commands::media::generate_media_cmd,
            commands::media::cancel_media_job_cmd,
            commands::media::list_running_media_jobs_cmd,
            commands::media::media_spend_cmd,
            commands::embedgen::embed_setup_status_cmd,
            commands::embedgen::install_embed_engine_cmd,
            commands::embedgen::remove_embed_engine_cmd,
            commands::embedgen::embed_catalog_cmd,
            commands::embedgen::list_embed_models_cmd,
            commands::embedgen::download_embed_model_cmd,
            commands::embedgen::set_default_embed_model_cmd,
            commands::embedgen::delete_embed_model_cmd,
            commands::rerankgen::rerank_setup_status_cmd,
            commands::rerankgen::install_rerank_engine_cmd,
            commands::rerankgen::remove_rerank_engine_cmd,
            commands::rerankgen::set_rerank_enabled_cmd,
            commands::rerankgen::rerank_catalog_cmd,
            commands::rerankgen::list_rerank_models_cmd,
            commands::rerankgen::download_rerank_model_cmd,
            commands::rerankgen::set_default_rerank_model_cmd,
            commands::rerankgen::delete_rerank_model_cmd,
            commands::index::index_status_cmd,
            commands::index::should_auto_index_cmd,
            commands::index::build_index_cmd,
            commands::index::cancel_index_cmd,
            commands::index::forget_index_cmd,
            commands::index::list_index_roots_cmd,
            commands::permissions::list_permissions_cmd,
            commands::permissions::add_permission_cmd,
            commands::permissions::revoke_permission_cmd,
            commands::permissions::list_activity_cmd,
            commands::browser::browser_state_cmd,
            commands::browser::stop_browser_cmd,
            commands::browser::forget_browser_session_cmd,
            commands::browser::list_capability_grants_cmd,
            commands::browser::revoke_capability_grant_cmd,
            commands::connectors::add_connector_cmd,
            commands::connectors::list_connectors_cmd,
            commands::connectors::test_connector_cmd,
            commands::connectors::set_connector_enabled_cmd,
            commands::connectors::delete_connector_cmd,
            commands::connectors::export_connectors_cmd,
            commands::connectors::import_connectors_cmd,
            commands::cloud::list_providers_cmd,
            commands::cloud::set_provider_key_cmd,
            commands::cloud::clear_provider_key_cmd,
            commands::cloud::list_cloud_models_cmd,
            commands::files::pick_folder_cmd,
            commands::files::pick_files_cmd,
            commands::files::pick_zip_file_cmd,
            commands::files::set_conversation_folder_cmd,
            commands::files::set_conversation_trust_cmd,
            commands::files::read_dir_tree_cmd,
            commands::files::read_text_file_cmd,
            commands::files::open_path_cmd,
            commands::files::reveal_path_cmd,
            commands::files::save_artifact_to_folder_cmd,
            commands::files::list_trash_cmd,
            commands::files::undo_file_op_cmd,
            commands::files::find_duplicates_cmd,
            commands::files::trash_file_cmd,
            commands::attachments::read_image_data_uri_cmd,
            commands::attachments::extract_pdf_text_cmd,
            commands::attachments::save_artifact_cmd,
            commands::scheduler::list_scheduler_jobs_cmd,
            commands::scheduler::create_scheduler_job_cmd,
            commands::scheduler::update_scheduler_job_cmd,
            commands::scheduler::delete_scheduler_job_cmd,
            commands::scheduler::run_scheduler_job_now_cmd,
            commands::scheduler::scheduler_status_cmd,
            commands::scheduler::stop_scheduler_job_cmd,
            commands::scheduler::get_scheduler_digest_cmd,
            commands::scheduler::mark_digest_read_cmd,
            commands::mail::add_mail_account_cmd,
            commands::mail::list_mail_accounts_cmd,
            commands::mail::test_mail_account_cmd,
            commands::mail::set_mail_account_enabled_cmd,
            commands::mail::delete_mail_account_cmd,
            commands::skills::list_skills_cmd,
            commands::skills::set_skill_enabled_cmd,
            commands::skills::create_skill_cmd,
            commands::skills::update_skill_cmd,
            commands::skills::install_skill_cmd,
            commands::skills::install_skill_zip_cmd,
            commands::skills::forget_skill_cmd,
            commands::skills::skill_surface_cmd,
            commands::skills::skill_body_cmd,
            commands::skills::scan_skill_text_cmd,
            commands::skills::personal_skills_dir_cmd,
            commands::skills::discoverable_skill_imports_cmd,
            commands::skills::import_skills_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Poiesis application")
        .run(|app_handle, event| {
            // Lifecycle safety (§7.4): terminate the engine on exit so no orphan
            // process holds VRAM.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(mgr) = app_handle.try_state::<RuntimeManager>() {
                    tauri::async_runtime::block_on(mgr.stop());
                }
                if let Some(mgr) = app_handle.try_state::<EmbedManager>() {
                    tauri::async_runtime::block_on(mgr.stop());
                }
                if let Some(mgr) = app_handle.try_state::<RerankManager>() {
                    tauri::async_runtime::block_on(mgr.stop());
                }
            }
        });
}
