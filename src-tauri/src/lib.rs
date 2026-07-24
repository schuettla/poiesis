//! Project Nexus — Tauri application backend.
//!
//! The backend is organized into subsystems that map to the implementation plan
//! (`docs/IMPLEMENTATION_PLAN.md`). Each is grown in its own phase; this module
//! wires them into the Tauri runtime and exposes the IPC command surface.

mod agent;
mod cloud;
mod commands;
mod db;
mod marketplace;
mod mcp;
mod permissions;
mod runtime;
mod secrets;
mod telemetry;

use tauri::Manager;

use db::Db;
use permissions::PermissionManager;
use runtime::RuntimeManager;

/// Application-wide error type surfaced to the frontend as a string.
#[derive(Debug, thiserror::Error)]
pub enum NexusError {
    #[error("{0}")]
    Message(String),
}

impl serde::Serialize for NexusError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type NexusResult<T> = Result<T, NexusError>;

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
            let db = Db::open(&base_dir.join("nexus.db")).expect("failed to open database");
            // Content-free, opt-in only (no-ops unless the user enabled it).
            telemetry::record(&db, "app_open");
            app.manage(db);

            app.manage(RuntimeManager::new(base_dir));
            app.manage(PermissionManager::new());
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
            commands::agent::list_skills_cmd,
            commands::agent::set_skill_enabled_cmd,
            commands::imagegen::image_setup_status_cmd,
            commands::imagegen::setup_image_generation_cmd,
            commands::imagegen::install_image_engine_cmd,
            commands::imagegen::image_catalog_cmd,
            commands::imagegen::list_image_models_cmd,
            commands::imagegen::download_image_model_cmd,
            commands::imagegen::generate_image_cmd,
            commands::imagegen::set_default_image_model_cmd,
            commands::imagegen::delete_image_model_cmd,
            commands::permissions::list_permissions_cmd,
            commands::permissions::add_permission_cmd,
            commands::permissions::revoke_permission_cmd,
            commands::permissions::list_activity_cmd,
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
            commands::attachments::read_image_data_uri_cmd,
            commands::attachments::extract_pdf_text_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Nexus application")
        .run(|app_handle, event| {
            // Lifecycle safety (§7.4): terminate the engine on exit so no orphan
            // process holds VRAM.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(mgr) = app_handle.try_state::<RuntimeManager>() {
                    tauri::async_runtime::block_on(mgr.stop());
                }
            }
        });
}
