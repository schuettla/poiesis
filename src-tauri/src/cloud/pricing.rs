//! `OBS-2`/`OBS-3`: what a cloud model costs, and how much it can hold.
//!
//! Deliberately a small constant table rather than a fetched catalogue. A
//! catalogue is a network call, a cache, a staleness problem and a failure
//! mode, for a number that only decorates a panel. A table is none of those —
//! at the price of going out of date, which is why every reader treats a miss
//! as "unknown" and shows tokens instead of inventing a figure.
//!
//! Matching is by prefix on the model id, longest first, so
//! `claude-sonnet-4-5-20250929` and `anthropic/claude-sonnet-4-5` both land on
//! the same row.

/// USD per million tokens, and the model's context window in tokens.
#[derive(Debug, Clone, Copy)]
pub struct ModelFacts {
    pub prompt_per_mtok: f64,
    pub output_per_mtok: f64,
    pub context_window: usize,
}

/// Published list prices, as of 2026-09. A model that is not here is not
/// guessed at: `price_of` returns `None` and every caller falls back to showing
/// tokens.
const TABLE: &[(&str, ModelFacts)] = &[
    // Anthropic
    ("claude-opus-4", ModelFacts { prompt_per_mtok: 15.0, output_per_mtok: 75.0, context_window: 200_000 }),
    ("claude-sonnet-4", ModelFacts { prompt_per_mtok: 3.0, output_per_mtok: 15.0, context_window: 200_000 }),
    ("claude-haiku-4", ModelFacts { prompt_per_mtok: 1.0, output_per_mtok: 5.0, context_window: 200_000 }),
    ("claude-3-5-haiku", ModelFacts { prompt_per_mtok: 0.8, output_per_mtok: 4.0, context_window: 200_000 }),
    ("claude-3-5-sonnet", ModelFacts { prompt_per_mtok: 3.0, output_per_mtok: 15.0, context_window: 200_000 }),
    // OpenAI
    ("gpt-4o-mini", ModelFacts { prompt_per_mtok: 0.15, output_per_mtok: 0.6, context_window: 128_000 }),
    ("gpt-4o", ModelFacts { prompt_per_mtok: 2.5, output_per_mtok: 10.0, context_window: 128_000 }),
    ("gpt-4.1-mini", ModelFacts { prompt_per_mtok: 0.4, output_per_mtok: 1.6, context_window: 1_000_000 }),
    ("gpt-4.1", ModelFacts { prompt_per_mtok: 2.0, output_per_mtok: 8.0, context_window: 1_000_000 }),
];

/// The facts for a model id, matched on the longest prefix that fits. `None`
/// means we do not know — never "it is free".
pub fn facts_for(model: &str) -> Option<ModelFacts> {
    // The id may be namespaced (`anthropic/claude-sonnet-4-5`), so match on any
    // segment boundary rather than only the start of the string.
    let needle = model.rsplit('/').next().unwrap_or(model).to_lowercase();
    TABLE
        .iter()
        .filter(|(prefix, _)| needle.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, facts)| *facts)
}

/// What a run's tokens cost in USD, or `None` when the model is not in the
/// table. A local run has no price at all and never reaches here.
pub fn cost_usd(model: &str, prompt_tokens: u64, output_tokens: u64) -> Option<f64> {
    let facts = facts_for(model)?;
    Some(
        (prompt_tokens as f64 * facts.prompt_per_mtok
            + output_tokens as f64 * facts.output_per_mtok)
            / 1_000_000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_namespaced_id_matches_the_same_row_as_a_bare_one() {
        let a = facts_for("anthropic/claude-sonnet-4-5-20250929").unwrap();
        let b = facts_for("claude-sonnet-4-5").unwrap();
        assert_eq!(a.prompt_per_mtok, b.prompt_per_mtok);
    }

    /// The longest prefix wins, or every `gpt-4o-mini` would be billed as a
    /// `gpt-4o` and read sixteen times too expensive.
    #[test]
    fn the_more_specific_row_wins() {
        assert_eq!(facts_for("gpt-4o-mini-2024-07-18").unwrap().prompt_per_mtok, 0.15);
        assert_eq!(facts_for("gpt-4o-2024-11-20").unwrap().prompt_per_mtok, 2.5);
    }

    /// A model we have never heard of must read as unknown, never as free.
    #[test]
    fn an_unknown_model_has_no_price_rather_than_a_zero_one() {
        assert!(facts_for("some-new-model").is_none());
        assert!(cost_usd("some-new-model", 1000, 1000).is_none());
    }

    #[test]
    fn cost_is_per_million_tokens() {
        let usd = cost_usd("claude-sonnet-4-5", 1_000_000, 0).unwrap();
        assert!((usd - 3.0).abs() < 1e-9, "got {usd}");
    }
}
