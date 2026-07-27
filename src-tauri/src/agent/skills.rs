//! The built-in skills framework (TOOL-6, §7.5). A `Skill` advertises OpenAI
//! tool specs, claims the tool names it handles, and executes a call — sharing a
//! single dispatch table with MCP connectors. The File System skill is the first
//! citizen; Web Search and Code Execution slot in as further variants.
//!
//! Each skill is independently enable/disable-able (persisted in `settings`), so
//! a small model isn't tempted to over-call a capability the user hasn't asked
//! for. Network/exec skills default **off**; the File System skill defaults on to
//! preserve the v1 behavior.

use serde::Serialize;

use crate::db::Db;
use crate::memory::MemoryStore;
use crate::permissions::PermissionManager;

use super::run::AgentEventSink;
use super::{
    artifacts, codeexec, filesystem, imagegen, memory_skill, present, recall, recipes, websearch,
};

/// Everything a skill might need to run a call. Built-in skills receive this so
/// new skills can reach the HTTP client, database, permission manager, and the
/// event sink without changing the dispatch signature.
pub struct SkillContext<'a> {
    pub client: &'a reqwest::Client,
    pub db: &'a Db,
    pub perms: &'a PermissionManager,
    pub sink: &'a AgentEventSink,
    pub conversation_id: &'a str,
    /// The assistant message this turn is producing, so a block can anchor to it.
    pub assistant_message_id: Option<&'a str>,
    /// App-data directory for persistent skill output (e.g. generated images).
    pub data_dir: &'a std::path::Path,
    /// This tool call's id, so a skill can attach richer data to its timeline step.
    pub call_id: &'a str,
    /// The durable self on disk (facts, lessons, recipes, SOUL.md).
    pub memory: &'a MemoryStore,
}

/// A built-in skill. Adding a capability means adding a variant here and a
/// backing module — the four match arms below keep dispatch exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    FileSystem,
    WebSearch,
    CodeExec,
    Artifacts,
    ImageGen,
    Present,
    Recall,
    Memory,
    Recipes,
}

/// A skill's metadata for the Settings surface (id, label, blurb, current state).
#[derive(Debug, Clone, Serialize)]
pub struct SkillInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    /// True if turning this on sends data off the device (web search) or runs
    /// code — the UI shows a heads-up.
    pub sensitive: bool,
}

impl Skill {
    /// Every built-in skill, in display order.
    pub const ALL: [Skill; 9] = [
        Skill::FileSystem,
        Skill::WebSearch,
        Skill::CodeExec,
        Skill::Artifacts,
        Skill::ImageGen,
        Skill::Present,
        Skill::Recall,
        Skill::Memory,
        Skill::Recipes,
    ];

    /// Stable id used for settings keys and the frontend.
    pub fn id(self) -> &'static str {
        match self {
            Skill::FileSystem => "filesystem",
            Skill::WebSearch => "web_search",
            Skill::CodeExec => "code_exec",
            Skill::Artifacts => "artifacts",
            Skill::ImageGen => "image_gen",
            Skill::Present => "present",
            Skill::Recall => "recall",
            Skill::Memory => "memory",
            Skill::Recipes => "recipes",
        }
    }

    pub fn from_id(id: &str) -> Option<Skill> {
        Skill::ALL.into_iter().find(|s| s.id() == id)
    }

    fn label(self) -> &'static str {
        match self {
            Skill::FileSystem => "File access",
            Skill::WebSearch => "Web search",
            Skill::CodeExec => "Code execution",
            Skill::Artifacts => "Artifacts",
            Skill::ImageGen => "Image generation",
            Skill::Present => "Workspace UI",
            Skill::Recall => "Recall",
            Skill::Memory => "Memory",
            Skill::Recipes => "Recipes",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Skill::FileSystem => {
                "Read and write files in folders you allow. Every access asks first."
            }
            Skill::WebSearch => {
                "Look things up on the web. Your query leaves your device to reach the search engine."
            }
            Skill::CodeExec => {
                "Run small Python or Node snippets in a throwaway sandbox with strict time and memory limits."
            }
            Skill::Artifacts => {
                "Let the assistant render web pages, graphics, documents, or code in the Canvas panel."
            }
            Skill::ImageGen => {
                "Generate images on your device from a text prompt. Set it up in Models → Image."
            }
            Skill::Present => {
                "Let the assistant compose a live, interactive interface for the task in the Workspace view — plus inline chat blocks."
            }
            Skill::Recall => {
                "Search your past conversations and saved memories. Stays on your device."
            }
            Skill::Memory => {
                "Remember durable facts about you across conversations — stored as markdown files on this device."
            }
            Skill::Recipes => {
                "Let Poiesis keep and reuse step-by-step procedures it developed with you — stored as markdown on this device."
            }
        }
    }

    /// Off-device or code-running skills are flagged so the UI can warn.
    fn sensitive(self) -> bool {
        matches!(self, Skill::WebSearch | Skill::CodeExec)
    }

    /// Default state when the user hasn't chosen. File System + Artifacts are
    /// benign and default on; network/exec skills default off for safety.
    fn default_enabled(self) -> bool {
        matches!(
            self,
            Skill::FileSystem
                | Skill::Artifacts
                | Skill::Present
                | Skill::Recall
                | Skill::Memory
                | Skill::Recipes
        )
    }

    fn setting_key(self) -> String {
        format!("skill.{}.enabled", self.id())
    }

    /// Is this skill currently enabled (user choice, else the default)?
    pub fn is_enabled(self, db: &Db) -> bool {
        match db.get_setting(&self.setting_key()).ok().flatten() {
            Some(v) => v == "true" || v == "1",
            None => self.default_enabled(),
        }
    }

    pub fn set_enabled(self, db: &Db, on: bool) {
        let _ = db.set_setting(&self.setting_key(), if on { "true" } else { "false" });
    }

    pub fn info(self, db: &Db) -> SkillInfo {
        SkillInfo {
            id: self.id().to_string(),
            label: self.label().to_string(),
            description: self.description().to_string(),
            enabled: self.is_enabled(db),
            sensitive: self.sensitive(),
        }
    }

    /// The OpenAI tool schemas this skill advertises.
    pub fn tool_specs(self) -> Vec<serde_json::Value> {
        let v = match self {
            Skill::FileSystem => filesystem::tool_specs(),
            Skill::WebSearch => websearch::tool_specs(),
            Skill::CodeExec => codeexec::tool_specs(),
            Skill::Artifacts => artifacts::tool_specs(),
            Skill::ImageGen => imagegen::tool_specs(),
            Skill::Present => present::tool_specs(),
            Skill::Recall => recall::tool_specs(),
            Skill::Memory => memory_skill::tool_specs(),
            Skill::Recipes => recipes::tool_specs(),
        };
        v.as_array().cloned().unwrap_or_default()
    }

    /// Does this skill own the given tool name?
    pub fn handles(self, name: &str) -> bool {
        match self {
            Skill::FileSystem => filesystem::handles(name),
            Skill::WebSearch => websearch::handles(name),
            Skill::CodeExec => codeexec::handles(name),
            Skill::Artifacts => artifacts::handles(name),
            Skill::ImageGen => imagegen::handles(name),
            Skill::Present => present::handles(name),
            Skill::Recall => recall::handles(name),
            Skill::Memory => memory_skill::handles(name),
            Skill::Recipes => recipes::handles(name),
        }
    }

    /// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
    pub fn describe(self, name: &str, args: &serde_json::Value) -> (String, String) {
        match self {
            Skill::FileSystem => filesystem::describe(name, args),
            Skill::WebSearch => websearch::describe(name, args),
            Skill::CodeExec => codeexec::describe(name, args),
            Skill::Artifacts => artifacts::describe(name, args),
            Skill::ImageGen => imagegen::describe(name, args),
            Skill::Present => present::describe(name, args),
            Skill::Recall => recall::describe(name, args),
            Skill::Memory => memory_skill::describe(name, args),
            Skill::Recipes => recipes::describe(name, args),
        }
    }

    /// Execute a call this skill handles. Returns the text fed back to the model.
    pub async fn execute(
        self,
        ctx: &SkillContext<'_>,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        match self {
            Skill::FileSystem => filesystem::execute(ctx, name, args).await,
            Skill::WebSearch => websearch::execute(ctx, name, args).await,
            Skill::CodeExec => codeexec::execute(ctx, name, args).await,
            Skill::Artifacts => artifacts::execute(ctx, name, args).await,
            Skill::ImageGen => imagegen::execute(ctx, name, args).await,
            Skill::Present => present::execute(ctx, name, args).await,
            Skill::Recall => recall::execute(ctx, name, args).await,
            Skill::Memory => memory_skill::execute(ctx, name, args).await,
            Skill::Recipes => recipes::execute(ctx, name, args).await,
        }
    }
}

/// The enabled built-in skills, in display order.
pub fn enabled(db: &Db) -> Vec<Skill> {
    Skill::ALL.into_iter().filter(|s| s.is_enabled(db)).collect()
}

/// Metadata for all skills (for the Settings surface).
pub fn all_info(db: &Db) -> Vec<SkillInfo> {
    Skill::ALL.into_iter().map(|s| s.info(db)).collect()
}
