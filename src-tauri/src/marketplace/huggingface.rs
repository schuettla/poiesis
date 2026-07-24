//! Hugging Face source (MKT-1): search for GGUF models and list a repo's GGUF
//! files with real sizes and parsed quantization labels.

use serde::{Deserialize, Serialize};

use super::catalog::CatalogModel;

const HF_API: &str = "https://huggingface.co/api";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfModelSummary {
    pub id: String,
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct HfTreeEntry {
    path: String,
    #[serde(default)]
    size: u64,
}

/// Parse a quantization label (e.g. "Q4_K_M", "Q8_0", "F16") out of a GGUF
/// filename, falling back to the whole stem.
fn parse_quant(filename: &str) -> String {
    let upper = filename.to_ascii_uppercase();
    for token in upper.split(['.', '-', '_']) {
        if (token.starts_with('Q') && token.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
            || token == "F16"
            || token == "F32"
            || token == "BF16"
        {
            // Reconstruct multi-part labels like Q4_K_M from the original name.
            if let Some(idx) = upper.find(token) {
                let tail = &filename[idx..];
                let label: String = tail
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                return label.trim_end_matches(".gguf").to_string();
            }
        }
    }
    filename.trim_end_matches(".gguf").to_string()
}

/// Search Hugging Face for GGUF models, most-downloaded first (MKT-1).
pub async fn search_models(
    client: &reqwest::Client,
    query: &str,
    limit: u32,
) -> Result<Vec<HfModelSummary>, reqwest::Error> {
    let url = format!(
        "{HF_API}/models?search={}&filter=gguf&sort=downloads&direction=-1&limit={}",
        urlencoding(query),
        limit
    );
    client.get(&url).send().await?.error_for_status()?.json().await
}

/// List the GGUF files in a repo as catalog entries with real sizes (MKT-2).
pub async fn list_gguf_files(
    client: &reqwest::Client,
    repo: &str,
) -> Result<Vec<CatalogModel>, reqwest::Error> {
    let url = format!("{HF_API}/models/{repo}/tree/main?recursive=true");
    let entries: Vec<HfTreeEntry> = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let short_name = repo.rsplit('/').next().unwrap_or(repo).replace("-GGUF", "");
    let models = entries
        .into_iter()
        .filter(|e| e.path.to_ascii_lowercase().ends_with(".gguf"))
        .map(|e| {
            let filename = e.path.rsplit('/').next().unwrap_or(&e.path).to_string();
            let quant = parse_quant(&filename);
            CatalogModel {
                id: format!("hf:{repo}:{quant}"),
                name: format!("{short_name} · {quant}"),
                description: format!("From {repo}"),
                quant,
                size_mb: e.size / (1024 * 1024),
                vision: filename.to_ascii_lowercase().contains("vision")
                    || filename.to_ascii_lowercase().contains("-vl-"),
                url: format!("https://huggingface.co/{repo}/resolve/main/{}?download=true", e.path),
                source: "huggingface".into(),
                license: None,
            }
        })
        .collect();
    Ok(models)
}

/// Minimal percent-encoding for the search query.
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            other => format!("%{:02X}", other as u32),
        })
        .collect()
}
