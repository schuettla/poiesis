//! `OBS-2`: what runs have cost, for Settings → Usage.
//!
//! The table itself is written by the agent loop when a run ends. This is only
//! the read side, plus the one thing the database cannot do: turn tokens into
//! money, which needs the price table and therefore lives here.

use serde::Serialize;
use tauri::State;

use crate::cloud::pricing;
use crate::db::{Db, UsageBucket, UsageSummary};
use crate::PoiesisError;

/// A bucket with money attached where we know the price.
#[derive(Debug, Serialize)]
pub struct PricedBucket {
    #[serde(flatten)]
    pub bucket: UsageBucket,
    /// `None` means the price is unknown, which is never the same as free. The
    /// UI shows the token count in that case.
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PricedUsage {
    pub total: PricedBucket,
    pub by_day: Vec<PricedBucket>,
    pub by_model: Vec<PricedBucket>,
    pub by_conversation: Vec<PricedBucket>,
    /// True when at least one bucket has no price, so the UI can say why the
    /// total is a floor rather than a figure.
    pub some_prices_unknown: bool,
}

/// Only a model bucket can be priced: a day or a conversation may mix models,
/// and adding a known price to an unknown one would read as a total when it is
/// a floor. Those keep `None` and show tokens.
fn price(bucket: UsageBucket, by_model: bool) -> PricedBucket {
    let cost_usd = if by_model && bucket.provenance != "local" {
        pricing::cost_usd(&bucket.key, bucket.prompt_tokens, bucket.output_tokens)
    } else {
        None
    };
    PricedBucket { bucket, cost_usd }
}

#[tauri::command]
pub fn usage_summary_cmd(db: State<'_, Db>, days: Option<i64>) -> Result<PricedUsage, PoiesisError> {
    let days = days.unwrap_or(30).clamp(1, 365);
    let since = crate::db::now_ms() - days * 86_400_000;
    let UsageSummary { total, by_day, by_model, by_conversation } = db
        .usage_summary(since)
        .map_err(|e| PoiesisError::Message(e.to_string()))?;

    let by_model: Vec<PricedBucket> = by_model.into_iter().map(|b| price(b, true)).collect();
    let some_prices_unknown = by_model
        .iter()
        .any(|b| b.cost_usd.is_none() && b.bucket.provenance != "local");
    // The total is the sum of the model rows we could price. Saying so is the
    // point: a run on a model we have no price for is real spend that this
    // number does not contain.
    let priced_total: f64 = by_model.iter().filter_map(|b| b.cost_usd).sum();

    Ok(PricedUsage {
        total: PricedBucket {
            bucket: total,
            cost_usd: (priced_total > 0.0).then_some(priced_total),
        },
        by_day: by_day.into_iter().map(|b| price(b, false)).collect(),
        by_model,
        by_conversation: by_conversation.into_iter().map(|b| price(b, false)).collect(),
        some_prices_unknown,
    })
}
