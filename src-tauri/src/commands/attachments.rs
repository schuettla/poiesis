//! Multimodal input commands (Phase 5): read an image as a data URI for vision
//! requests (CHT-5) and extract text from a PDF (CHT-8).

use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

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
pub async fn read_image_data_uri_cmd(path: String) -> Cmd<String> {
    let p = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = std::fs::read(&p).map_err(|e| NexusError::Message(e.to_string()))?;
        let mime = mime_for(Path::new(&p));
        Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
    })
    .await
    .map_err(|e| NexusError::Message(e.to_string()))?
}

/// Extract text from a text-based PDF (CHT-8). Scanned/image-only PDFs return
/// little or no text — surfaced to the user as a clear note (OCR is out of v1
/// scope). Page-image rendering for vision models is a documented follow-up
/// (would reintroduce a binary dependency; see §7.7-style trade-off).
#[tauri::command]
pub async fn extract_pdf_text_cmd(path: String) -> Cmd<String> {
    tauri::async_runtime::spawn_blocking(move || {
        pdf_extract::extract_text(&path)
            .map_err(|e| NexusError::Message(format!("couldn't read that PDF: {e}")))
    })
    .await
    .map_err(|e| NexusError::Message(e.to_string()))?
}
