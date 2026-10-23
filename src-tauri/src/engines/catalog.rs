//! The agent apps Poiesis knows how to drive (`CLI-2`): `shared/engines.json`,
//! bundled with the app. An agent that is not in it is never found
//! automatically; the user adds it by hand.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::EngineKind;

static MANIFEST: &str = include_str!("../../../shared/engines.json");

/// How to read one agent's own sign-in status command.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusCmd {
    pub args: Vec<String>,
    /// Which stream carries the answer. Codex prints it on stderr.
    pub reads: String,
    /// `claude-json` | `codex-text`.
    pub format: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub kind: EngineKind,
    pub name: String,
    pub binaries: Vec<String>,
    pub install_url: String,
    /// What **Sign in** runs in a terminal. The agent's own flow, never ours (TRM-2).
    pub login_args: Vec<String>,
    /// The argv an ACP agent starts its protocol with.
    #[serde(default)]
    pub acp_args: Vec<String>,
    #[serde(default)]
    pub status: Option<StatusCmd>,
    /// `Some("own")` for an agent that has no plan of its own (OpenCode).
    #[serde(default)]
    pub paid_by: Option<String>,
    /// The variable an API key is passed in. Absent = the agent has no API-key
    /// mode we have recorded, so none is offered.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Variables set so that the agent asks before it edits or runs anything.
    /// OpenCode never asks by default (`CLI-6`).
    #[serde(default)]
    pub ask_everything_env: BTreeMap<String, String>,
    /// The same variables as they are set where the user lets agents edit freely:
    /// edits go through, commands still ask. Empty = no such setting.
    #[serde(default)]
    pub auto_edits_env: BTreeMap<String, String>,
    /// More variables to take out of the child env, besides the common keys.
    #[serde(default)]
    pub strip_env_prefixes: Vec<String>,
    pub tested_from: String,
    pub tested_below: String,
}

#[derive(Debug, Deserialize)]
struct File {
    engines: Vec<CatalogEntry>,
}

pub fn all() -> &'static [CatalogEntry] {
    static ALL: OnceLock<Vec<CatalogEntry>> = OnceLock::new();
    ALL.get_or_init(|| {
        serde_json::from_str::<File>(MANIFEST).expect("shared/engines.json is not valid").engines
    })
}

pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    all().iter().find(|e| e.id == id)
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::ClaudeCode => "claude-code",
            EngineKind::Codex => "codex",
            EngineKind::Acp => "acp",
        }
    }

    pub fn parse(s: &str) -> Option<EngineKind> {
        match s {
            "claude-code" => Some(EngineKind::ClaudeCode),
            "codex" => Some(EngineKind::Codex),
            "acp" => Some(EngineKind::Acp),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_catalog_parses_and_is_consistent() {
        let all = all();
        assert!(all.len() >= 3);
        let mut ids: Vec<&str> = all.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "ids are unique");
        for e in all {
            assert!(!e.binaries.is_empty(), "{} names a binary", e.id);
            assert!(e.install_url.starts_with("https://"), "{}", e.id);
            assert!(!e.login_args.is_empty(), "{} has a sign-in flow", e.id);
            if e.kind == EngineKind::Acp {
                assert!(!e.acp_args.is_empty(), "{} says how to start ACP", e.id);
            }
            // Parseable on both ends, so `Tested` never has to guess.
            assert!(super::super::probe::parse_version(&e.tested_from).is_some());
            assert!(super::super::probe::parse_version(&e.tested_below).is_some());
        }
    }

    /// OpenCode's `auth export` prints stored secrets. Nothing in the catalog
    /// may ever run it.
    #[test]
    fn no_catalog_command_can_print_secrets() {
        for e in all() {
            let mut cmds = vec![e.login_args.clone(), e.acp_args.clone()];
            if let Some(s) = &e.status {
                cmds.push(s.args.clone());
            }
            for c in cmds {
                assert!(!c.iter().any(|a| a == "export"), "{}: {c:?}", e.id);
            }
        }
    }
}
