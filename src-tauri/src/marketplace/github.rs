//! GitHub releases source (MKT-1, "both" sources): list GGUF assets from a
//! given `owner/repo`'s releases so models distributed via GitHub (rather than
//! Hugging Face) surface in the same unified catalog.

use serde::Deserialize;

use super::catalog::CatalogModel;

const GITHUB_API: &str = "https://api.github.com/repos";
const USER_AGENT: &str = concat!("ProjectNexus/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

fn parse_quant(name: &str) -> String {
    let upper = name.to_ascii_uppercase();
    for q in ["Q2_K", "Q3_K_M", "Q4_K_M", "Q4_K_S", "Q5_K_M", "Q6_K", "Q8_0", "F16"] {
        if upper.contains(q) {
            return q.to_string();
        }
    }
    "GGUF".to_string()
}

/// List GGUF assets across a repo's releases as catalog entries (MKT-1).
pub async fn list_release_models(
    client: &reqwest::Client,
    owner_repo: &str,
) -> Result<Vec<CatalogModel>, reqwest::Error> {
    let url = format!("{GITHUB_API}/{owner_repo}/releases?per_page=10");
    let releases: Vec<GhRelease> = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let short = owner_repo.rsplit('/').next().unwrap_or(owner_repo).to_string();
    let mut models = Vec::new();
    for rel in releases {
        for asset in rel.assets {
            if !asset.name.to_ascii_lowercase().ends_with(".gguf") {
                continue;
            }
            let quant = parse_quant(&asset.name);
            models.push(CatalogModel {
                id: format!("gh:{owner_repo}:{}:{quant}", rel.tag_name),
                name: format!("{short} {} · {quant}", rel.tag_name),
                description: format!("From github.com/{owner_repo}"),
                quant,
                size_mb: asset.size / (1024 * 1024),
                vision: asset.name.to_ascii_lowercase().contains("vision"),
                url: asset.browser_download_url,
                source: "github".into(),
                license: None,
            });
        }
    }
    Ok(models)
}
