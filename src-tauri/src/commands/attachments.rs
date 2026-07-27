//! Multimodal input commands (Phase 5): read an image as a data URI for vision
//! requests (CHT-5) and extract text from a PDF (CHT-8).
//!
//! These read the user's real files, so they answer to the same scope rules as
//! the agent's tools — see `files::assert_ui_readable`. Otherwise they would be
//! a way to read any path on the machine straight past the consent system.

use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tauri::State;

use crate::commands::files::{assert_ui_readable_raw, DialogGrants};
use crate::db::Db;
use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

/// Attachments are inlined into a request, so an enormous one silently blows up
/// the turn. Refuse loudly instead.
const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;

fn check_size(path: &Path) -> Result<(), NexusError> {
    let len = std::fs::metadata(path).map_err(|e| NexusError::Message(e.to_string()))?.len();
    if len > MAX_ATTACHMENT_BYTES {
        return Err(NexusError::Message(format!(
            "{} is too large to attach ({} MB).",
            path.display(),
            len / (1024 * 1024)
        )));
    }
    Ok(())
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// Read an image file and return an OpenAI-compatible `data:` URI to inline into
/// a vision request's `image_url` content part.
#[tauri::command]
pub async fn read_image_data_uri_cmd(
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    conversation_id: Option<String>,
    path: String,
) -> Cmd<String> {
    let p = crate::permissions::canonicalize_lenient(Path::new(&path));
    assert_ui_readable_raw(&db, &grants, conversation_id.as_deref(), &p, Some(&path))?;
    check_size(&p)?;

    tauri::async_runtime::spawn_blocking(move || {
        let bytes = std::fs::read(&p).map_err(|e| NexusError::Message(e.to_string()))?;
        let mime = mime_for(&p);
        Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
    })
    .await
    .map_err(|e| NexusError::Message(e.to_string()))?
}

/// Save a Canvas artifact to a user-chosen path (CHT-6 download). `dest` comes
/// from the native save dialog, which is the consent; we record it so later
/// reads of the same file are allowed too.
#[tauri::command]
pub async fn save_artifact_cmd(
    grants: State<'_, DialogGrants>,
    dest: String,
    kind: String,
    content: String,
) -> Cmd<()> {
    grants.remember(Path::new(&dest));
    tauri::async_runtime::spawn_blocking(move || {
        if kind == "image" {
            std::fs::copy(&content, &dest).map_err(|e| NexusError::Message(e.to_string()))?;
        } else {
            std::fs::write(&dest, content).map_err(|e| NexusError::Message(e.to_string()))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| NexusError::Message(e.to_string()))?
}

/// Extract text from a text-based PDF (CHT-8). Scanned/image-only PDFs return
/// little or no text — surfaced to the user as a clear note (OCR is out of v1
/// scope). Page-image rendering for vision models is a documented follow-up
/// (would reintroduce a binary dependency; see §7.7-style trade-off).
#[tauri::command]
pub async fn extract_pdf_text_cmd(
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    conversation_id: Option<String>,
    path: String,
) -> Cmd<String> {
    let p = crate::permissions::canonicalize_lenient(Path::new(&path));
    assert_ui_readable_raw(&db, &grants, conversation_id.as_deref(), &p, Some(&path))?;
    check_size(&p)?;
    let path = p.to_string_lossy().to_string();

    tauri::async_runtime::spawn_blocking(move || {
        pdf_extract::extract_text(&path)
            .map_err(|e| NexusError::Message(format!("couldn't read that PDF: {e}")))
    })
    .await
    .map_err(|e| NexusError::Message(e.to_string()))?
}
