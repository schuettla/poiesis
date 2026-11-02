//! `TIER-3`: what a model can carry.
//!
//! A small model gets what it can carry; a big model gets everything. Every turn
//! runs at one of three tiers, decided here from the model's facts and nowhere
//! else. The frontend receives the tier with each model and never works it out,
//! so the rule cannot drift between the two sides.

use serde::{Deserialize, Serialize};

use super::toolsets::Toolset;
use crate::db::Db;

/// Below this many tokens a model cannot hold the tool specs and still have room
/// for a conversation.
pub const CHAT_BELOW: usize = 8192;
/// From this many tokens a model gets everything. Also the cap on a local
/// model's window (`runtime::window::WINDOW_CAP`): a bigger window adds memory
/// cost and no features.
pub const FULL_FROM: usize = 32768;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    /// Every tool, skills, planning, delegation, fast-slow narrowing, effort.
    #[default]
    Full,
    /// A short list of tools and nothing else.
    Basic,
    /// No tools at all.
    Chat,
}

impl ModelTier {
    pub fn parse(raw: &str) -> Option<ModelTier> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "full" => Some(ModelTier::Full),
            "basic" => Some(ModelTier::Basic),
            "chat" => Some(ModelTier::Chat),
            _ => None,
        }
    }

    /// Does this turn get the full stack?
    pub fn is_full(self) -> bool {
        self == ModelTier::Full
    }
}

/// The tier for a model.
///
/// | Tier | Rule (when no override) |
/// |---|---|
/// | Chat | the model says it cannot use tools, or `window < 8192` |
/// | Basic | `8192 <= window < 32768` |
/// | Full | `window >= 32768`, or the window is unknown |
///
/// An unknown window counts as Full. That keeps every cloud model whose window we
/// do not know exactly as it is today.
pub fn tier_for(tools: bool, window: Option<usize>, override_: Option<ModelTier>) -> ModelTier {
    if let Some(tier) = override_ {
        return tier;
    }
    if !tools {
        return ModelTier::Chat;
    }
    match window {
        Some(w) if w < CHAT_BELOW => ModelTier::Chat,
        Some(w) if w < FULL_FROM => ModelTier::Basic,
        _ => ModelTier::Full,
    }
}

/// `TIER-4`: whether a turn gets tools at all. The one rule for a typed turn, a
/// resume and a scheduled run: the user's own switch, unless the model is Chat.
/// Everything that depends on tools (toolsets, loop tools, the skills block, tool
/// and memory guidance, narrowing, wrap-up) already follows this one value.
pub fn tools_on(asked: bool, tier: ModelTier) -> bool {
    asked && tier != ModelTier::Chat
}

/// `TIER-5`: the toolsets Basic keeps. A first guess; Erich's manual pass
/// confirms or changes it.
///
/// Left out: skills, subagents, browser, mail, system, code_exec, code_run,
/// artifacts, image_gen, present, indexing and decide. Connected apps and MCP
/// servers are left out where the registry is built.
pub const BASIC_TOOLSETS: [Toolset; 4] =
    [Toolset::FileSystem, Toolset::WebSearch, Toolset::Memory, Toolset::Recall];

/// The settings key holding the user's tier for one model (`full` | `basic` |
/// `chat`). Unset, or empty, means Auto. `model_id` is the id the frontend uses.
pub fn tier_key(model_id: &str) -> String {
    format!("models.tier.{model_id}")
}

/// The settings key holding the user's window for one local model, in tokens.
pub fn ctx_key(model_id: &str) -> String {
    format!("models.ctx.{model_id}")
}

pub fn tier_override(db: &Db, model_id: &str) -> Option<ModelTier> {
    db.get_setting(&tier_key(model_id)).ok().flatten().as_deref().and_then(ModelTier::parse)
}

pub fn ctx_override(db: &Db, model_id: &str) -> Option<u32> {
    db.get_setting(&ctx_key(model_id))
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| *n > 0)
}

/// The id the frontend gives a cloud model: `cloud:<provider>:<model>`.
pub fn cloud_model_id(provider: &str, model: &str) -> String {
    format!("cloud:{provider}:{model}")
}

/// The id the frontend gives a model on a connected server:
/// `endpoint:<endpoint id>:<model>`.
pub fn endpoint_model_id(endpoint_id: &str, model: &str) -> String {
    format!("endpoint:{endpoint_id}:{model}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_row_of_the_table() {
        // Chat: no tool support, whatever the window.
        assert_eq!(tier_for(false, Some(200_000), None), ModelTier::Chat);
        assert_eq!(tier_for(false, None, None), ModelTier::Chat);
        // Chat: a small window.
        assert_eq!(tier_for(true, Some(4096), None), ModelTier::Chat);
        assert_eq!(tier_for(true, Some(8191), None), ModelTier::Chat);
        // Basic: 8192 up to 32767.
        assert_eq!(tier_for(true, Some(8192), None), ModelTier::Basic);
        assert_eq!(tier_for(true, Some(32767), None), ModelTier::Basic);
        // Full: 32768 and up.
        assert_eq!(tier_for(true, Some(32768), None), ModelTier::Full);
        assert_eq!(tier_for(true, Some(1_000_000), None), ModelTier::Full);
    }

    #[test]
    fn an_unknown_window_is_full() {
        assert_eq!(tier_for(true, None, None), ModelTier::Full);
    }

    #[test]
    fn the_override_beats_the_rule() {
        assert_eq!(tier_for(false, Some(2048), Some(ModelTier::Full)), ModelTier::Full);
        assert_eq!(tier_for(true, Some(200_000), Some(ModelTier::Chat)), ModelTier::Chat);
        assert_eq!(tier_for(true, None, Some(ModelTier::Basic)), ModelTier::Basic);
    }

    #[test]
    fn only_chat_loses_the_tools_and_never_gains_them() {
        assert!(tools_on(true, ModelTier::Full));
        assert!(tools_on(true, ModelTier::Basic));
        assert!(!tools_on(true, ModelTier::Chat));
        for tier in [ModelTier::Full, ModelTier::Basic, ModelTier::Chat] {
            assert!(!tools_on(false, tier), "the user's own switch still wins");
        }
    }

    #[test]
    fn the_words_round_trip() {
        for tier in [ModelTier::Full, ModelTier::Basic, ModelTier::Chat] {
            let word = serde_json::to_string(&tier).unwrap();
            assert_eq!(ModelTier::parse(word.trim_matches('"')), Some(tier));
        }
        assert_eq!(ModelTier::parse(" Basic "), Some(ModelTier::Basic));
        assert_eq!(ModelTier::parse(""), None, "an empty setting is Auto");
        assert_eq!(ModelTier::parse("auto"), None);
    }

    #[test]
    fn the_keys_match_what_the_window_writes() {
        assert_eq!(tier_key("cloud:openrouter:a/b"), "models.tier.cloud:openrouter:a/b");
        assert_eq!(ctx_key("abc"), "models.ctx.abc");
        assert_eq!(cloud_model_id("openrouter", "a/b"), "cloud:openrouter:a/b");
        assert_eq!(endpoint_model_id("e1", "llama3"), "endpoint:e1:llama3");
    }

    #[test]
    fn basic_keeps_four_and_none_of_the_reaching_ones() {
        let ids: Vec<&str> = BASIC_TOOLSETS.iter().map(|t| t.id()).collect();
        assert_eq!(ids, ["filesystem", "web_search", "memory", "recall"]);
    }
}
