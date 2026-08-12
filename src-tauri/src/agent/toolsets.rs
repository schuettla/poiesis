//! The built-in toolsets framework (TOOL-6, §7.5). A `Toolset` advertises OpenAI
//! tool specs, claims the tool names it handles, and executes a call — sharing a
//! single dispatch table with MCP connectors. The File System toolset is the first
//! citizen; Web Search and Code Execution slot in as further variants.
//!
//! Each toolset is independently enable/disable-able (persisted in `settings`), so
//! a small model isn't tempted to over-call a capability the user hasn't asked
//! for. Network/exec toolsets default **off**; the File System toolset defaults on to
//! preserve the v1 behavior.
//!
//! `TSET-1`: this was `Skill`/`skills.rs`. The name now belongs to Agent Skills
//! (`SKL`, a prompt-level capability pack with a `SKILL.md`), which are a
//! different thing entirely — these are *tool groups*. Nothing in this file
//! knows about Agent Skills.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::db::Db;
use crate::memory::MemoryStore;
use crate::permissions::PermissionManager;
use crate::runtime::{EmbedManager, RerankManager, RuntimeManager};

use super::run::AgentEventSink;
use super::{
    artifacts, browser, codeexec, filesystem, imagegen, mail, memory_skill, present, recall,
    retrieval, screen, skillpack, websearch,
};

/// Everything a toolset might need to run a call. Built-in toolsets receive this so
/// new toolsets can reach the HTTP client, database, permission manager, and the
/// event sink without changing the dispatch signature.
pub struct ToolContext<'a> {
    pub client: &'a reqwest::Client,
    /// The **local** engine, for a toolset's own small side call (e.g. the Memory
    /// toolset's one-shot scope classification, `SCP-1`) — never for anything
    /// that competes with the turn itself. Deliberately not this turn's
    /// endpoint: a call the user did not ask for must not be billed to their
    /// cloud provider or shipped off the machine just because the turn happens
    /// to be running there. `None` when no local model is loaded, in which case
    /// the toolset does without rather than falling back to the cloud.
    pub local_endpoint: Option<&'a crate::cloud::ChatEndpoint>,
    pub db: &'a Db,
    /// The engine registry + shared HTTP client, and the local embedding
    /// engine — `RET`'s `search_folder` needs both to embed a query the same
    /// way `IDX` embedded the folder it's searching. `None` of the built-in
    /// toolsets before `RET` needed either, so this is new.
    pub mgr: &'a RuntimeManager,
    pub embed_mgr: &'a EmbedManager,
    /// The local reranking engine (`RRK`), for `search_folder`'s selective
    /// re-read of the top candidates (`RRK-4`). `None` of the built-in toolsets
    /// before `RRK` needed it.
    pub rerank_mgr: &'a RerankManager,
    pub perms: &'a PermissionManager,
    pub sink: &'a AgentEventSink,
    pub conversation_id: &'a str,
    /// The assistant message this turn is producing, so a block can anchor to it.
    pub assistant_message_id: Option<&'a str>,
    /// App-data directory for persistent toolset output (e.g. generated images).
    pub data_dir: &'a std::path::Path,
    /// This tool call's id, so a toolset can attach richer data to its timeline step.
    pub call_id: &'a str,
    /// The durable self on disk (facts, lessons, SOUL.md).
    pub memory: &'a MemoryStore,
    /// True for an unattended run (SCH-3): a headless caller has no one to look
    /// at a render, so toolsets must skip emitting one rather than waste the work.
    pub headless: bool,
    /// Whether this call has already emitted a render (RND-3). One context is
    /// built per tool call, so this enforces "one render per tool call" here
    /// rather than trusting every toolset to remember it.
    pub rendered: AtomicBool,
    /// What the timeline's result line should say for this step, when the
    /// generic "— N lines" summary would be worse than useless. `RET-UI-2` is
    /// the case that forced it: a weak retrieval has to reach the *user* as
    /// "I'm not sure they answer this", not only reach the model as a warning
    /// buried in the tool text. Left `None`, `run.rs` falls back to
    /// `summarize()` exactly as before.
    pub step_note: Mutex<Option<String>>,
    /// `SKL-3`: directories, beyond the working folder, that `read_file`/
    /// `search_files` may read from without a permission prompt this run —
    /// populated with a skill's own folder the moment the `skill` tool loads
    /// it, so a bundled `references/`/`assets/` file is reachable for the
    /// rest of the run. Shared (not per-call) because a skill activated by
    /// one tool call must stay readable for every later call in the same run.
    pub extra_read_roots: &'a Mutex<Vec<std::path::PathBuf>>,
    /// `SKL-2`: skills already loaded in this run. A skill's body is static and
    /// the earlier tool result is still in the transcript, so loading one twice
    /// buys nothing and costs its full length again — 534 lines, in the run that
    /// prompted this. Shared across the run (not per-call) for the same reason
    /// `extra_read_roots` is.
    pub loaded_skills: &'a Mutex<Vec<String>>,
    /// `BRW-1`: the live per-conversation browser sessions. `None` when the
    /// caller (e.g. `EVL`'s dispatched harness) doesn't wire one up — the
    /// Browser toolset reports itself unavailable rather than panicking.
    pub browser_pool: Option<&'a super::browser::BrowserPool>,
}

/// Set this call's timeline result line (see `ToolContext::step_note`). Last
/// writer wins — a toolset sets it once, at the point it knows the outcome.
pub fn set_step_note(ctx: &ToolContext<'_>, note: impl Into<String>) {
    *ctx.step_note.lock().unwrap() = Some(note.into());
}

/// `TRU-2`: scan `text`, wrap it as outside content, and tell the UI so the
/// step's `◇ from outside` chip can render (`TRU-UI-1`) — every intake site
/// that hands the model text it didn't write itself goes through this rather
/// than reimplementing scan+wrap+log+emit at each call site. Returns the
/// wrapped text to substitute into the tool's own output; `label` is
/// user-facing provenance ("page at example.com", "file README.md").
///
/// Risk `>= 2` also lands one `activity_log` row (`TRU-3`) — the emit to the
/// UI happens regardless of risk, since the marking is about provenance, not
/// an alarm (Part I §4.2).
pub fn mark_untrusted(ctx: &ToolContext<'_>, label: &str, text: &str) -> String {
    let scan = super::untrusted::scan(text);
    if scan.risk >= 2 {
        let _ = ctx.db.log_activity(
            Some(ctx.conversation_id),
            "untrusted",
            &format!("risk {} in {label}: {}", scan.risk, scan.flags.join(", ")),
        );
    }
    ctx.sink.emit(super::AgentEvent::Untrusted {
        id: ctx.call_id.to_string(),
        label: label.to_string(),
        risk: scan.risk,
        flags: scan.flags.clone(),
        text: text.to_string(),
    });
    super::untrusted::wrap(label, text, &scan)
}

/// Cap on a tool-emitted render's serialized payload (RND-3). A render this
/// large is almost certainly a bug (a tool dumping raw file contents into a
/// block, say) rather than something worth showing inline.
const RENDER_CAP_BYTES: usize = 64 * 1024;

/// Why a render didn't happen (RND-3). Separated from the effectful path so
/// the guard rails can be tested without a database or an event sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderSkip {
    /// Unattended run — nobody is looking (SCH-3).
    Headless,
    /// Payload past `RENDER_CAP_BYTES`; almost always a tool dumping raw file
    /// contents into a block.
    TooLarge,
    /// This tool call already rendered once.
    AlreadyRendered,
}

/// Apply RND-3's guard rails. `Ok(json)` means go ahead and persist.
fn check_render(headless: bool, already_rendered: bool, payload_len: usize) -> Result<(), RenderSkip> {
    if headless {
        return Err(RenderSkip::Headless);
    }
    if already_rendered {
        return Err(RenderSkip::AlreadyRendered);
    }
    if payload_len > RENDER_CAP_BYTES {
        return Err(RenderSkip::TooLarge);
    }
    Ok(())
}

/// Let a tool render its own structured result directly, instead of only
/// returning text for the model to describe (RND-1/2). Reuses exactly the
/// block persistence + event path `present.rs` already writes from the
/// backend — this just makes it reachable from any toolset, with the guard
/// rails RND-3 asks for: one render per tool call, a size cap, and a skip in
/// headless runs.
///
/// Returns the new block's id, or `None` if the render was skipped (headless,
/// oversized, already rendered, or a save error — logged, never surfaced as a
/// tool failure).
pub fn render_block(
    ctx: &ToolContext<'_>,
    kind: &str,
    title: &str,
    data: &serde_json::Value,
) -> Option<String> {
    let data_json = serde_json::to_string(data).ok()?;
    let already = ctx.rendered.load(Ordering::Relaxed);
    if let Err(skip) = check_render(ctx.headless, already, data_json.len()) {
        if skip != RenderSkip::Headless {
            eprintln!("render_block: skipped render for '{title}' ({skip:?})");
        }
        return None;
    }
    // Claim the one render this call gets before doing the work, so two
    // concurrent attempts can't both pass the check above.
    if ctx.rendered.swap(true, Ordering::Relaxed) {
        return None;
    }
    match ctx
        .db
        .add_block(ctx.conversation_id, ctx.assistant_message_id, kind, title, &data_json)
    {
        Ok(block) => {
            ctx.sink.block(&block.id, ctx.assistant_message_id, kind, title, data);
            Some(block.id)
        }
        Err(e) => {
            eprintln!("render_block: couldn't save render for '{title}': {e}");
            None
        }
    }
}

/// A built-in toolset. Adding a capability means adding a variant here and a
/// backing module — the four match arms below keep dispatch exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toolset {
    FileSystem,
    WebSearch,
    CodeExec,
    Artifacts,
    ImageGen,
    Present,
    Recall,
    Memory,
    Indexing,
    /// Read/search/send email over IMAP/SMTP (`MAIL-2`). Default off, like
    /// Web Search and Code Execution — it reaches outside the device and can
    /// send on the user's behalf.
    Mail,
    /// The `skill`/`propose_skill` tools (`SKL-2`) — reading and proposing
    /// Agent Skills. Not sensitive itself (it never leaves the device); the
    /// content a skill teaches is what `TRU-2` marks.
    Skills,
    /// Drives the user's installed Chrome/Edge over CDP (`BRW`). Default off
    /// — it reaches the open web on the model's behalf.
    Browser,
    /// Screenshot + launch-an-app (`SYS-1`). Default off — a screenshot can
    /// contain anything, and launching reaches outside the app's own sandbox.
    System,
}

/// A toolset's metadata for the Settings surface (id, label, blurb, current state).
#[derive(Debug, Clone, Serialize)]
pub struct ToolsetInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    /// True if turning this on sends data off the device (web search) or runs
    /// code — the UI shows a heads-up.
    pub sensitive: bool,
}

impl Toolset {
    /// Every built-in toolset, in display order.
    pub const ALL: [Toolset; 13] = [
        Toolset::FileSystem,
        Toolset::WebSearch,
        Toolset::CodeExec,
        Toolset::Artifacts,
        Toolset::ImageGen,
        Toolset::Present,
        Toolset::Recall,
        Toolset::Memory,
        Toolset::Indexing,
        Toolset::Mail,
        Toolset::Skills,
        Toolset::Browser,
        Toolset::System,
    ];

    /// Stable id used for settings keys and the frontend.
    pub fn id(self) -> &'static str {
        match self {
            Toolset::FileSystem => "filesystem",
            Toolset::WebSearch => "web_search",
            Toolset::CodeExec => "code_exec",
            Toolset::Artifacts => "artifacts",
            Toolset::ImageGen => "image_gen",
            Toolset::Present => "present",
            Toolset::Recall => "recall",
            Toolset::Memory => "memory",
            Toolset::Indexing => "indexing",
            Toolset::Mail => "mail",
            Toolset::Skills => "skills",
            Toolset::Browser => "browser",
            Toolset::System => "system",
        }
    }

    pub fn from_id(id: &str) -> Option<Toolset> {
        Toolset::ALL.into_iter().find(|s| s.id() == id)
    }

    fn label(self) -> &'static str {
        match self {
            Toolset::FileSystem => "File access",
            Toolset::WebSearch => "Web search",
            Toolset::CodeExec => "Code execution",
            Toolset::Artifacts => "Artifacts",
            Toolset::ImageGen => "Image generation",
            Toolset::Present => "Workspace UI",
            Toolset::Recall => "Recall",
            Toolset::Memory => "Memory",
            Toolset::Indexing => "Folder reading",
            Toolset::Mail => "Mail",
            Toolset::Skills => "Skills",
            Toolset::Browser => "Browser",
            Toolset::System => "Screen & apps",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Toolset::FileSystem => {
                "Read and write files in folders you allow. Every access asks first."
            }
            Toolset::WebSearch => {
                "Look things up on the web. Your query leaves your device to reach the search engine."
            }
            Toolset::CodeExec => {
                "Run small Python or Node snippets in a throwaway sandbox with strict time and memory limits."
            }
            Toolset::Artifacts => {
                "Let the assistant render web pages, graphics, documents, or code in the Canvas panel."
            }
            Toolset::ImageGen => {
                "Generate images on your device from a text prompt. Set it up in Models → Image."
            }
            Toolset::Present => {
                "Let the assistant compose a live, interactive interface for the task in the Workspace view — plus inline chat blocks."
            }
            Toolset::Recall => {
                "Search your past conversations and saved memories. Stays on your device."
            }
            Toolset::Memory => {
                "Remember durable facts about you across conversations — stored as markdown files on this device."
            }
            Toolset::Indexing => {
                "Read the files in an attached folder ahead of time so it can find things in them by meaning, not just by name. Turning this on doesn't grant any new folder — it only lets an already-attached one be read this way."
            }
            Toolset::Mail => {
                "Read, search, and send email over an account you connect. Sending always asks first. Set accounts up in Settings → Mail."
            }
            Toolset::Skills => {
                "Read and reuse Agent Skills — step-by-step procedures, its own or ones you already use with another agent. Reading a skill is free; keeping a new one still asks first."
            }
            Toolset::Browser => {
                "Open pages and click around in your installed Chrome or Edge. The first visit to a new site asks first."
            }
            Toolset::System => {
                "Take a screenshot or launch an app on this machine. Both ask first, unless you say otherwise."
            }
        }
    }

    /// Off-device or code-running toolsets are flagged so the UI can warn.
    fn sensitive(self) -> bool {
        matches!(
            self,
            Toolset::WebSearch | Toolset::CodeExec | Toolset::Mail | Toolset::Browser | Toolset::System
        )
    }

    /// Default state when the user hasn't chosen. File System + Artifacts are
    /// benign and default on; network/exec toolsets default off for safety.
    ///
    /// `Indexing` defaults **on** per `SMP-4b`, which amends `IDX-8`: attaching
    /// a folder is itself the consent to read it, so there is no second toggle
    /// to hunt for in Settings before the folder can be read. It grants no new
    /// folder on its own — with nothing attached it can do nothing at all — and
    /// the toggle stays in Settings → Tools for switching it off globally.
    fn default_enabled(self) -> bool {
        matches!(
            self,
            Toolset::FileSystem
                | Toolset::Artifacts
                | Toolset::Present
                | Toolset::Recall
                | Toolset::Memory
                | Toolset::Indexing
                | Toolset::Skills
        )
    }

    /// `TSET-3`: was `skill.<name>.enabled` — migrated to `toolset.<name>.enabled`
    /// by the v9 schema migration (`db::Db::migrate`) on first run after upgrade.
    fn setting_key(self) -> String {
        format!("toolset.{}.enabled", self.id())
    }

    /// Is this toolset currently enabled (user choice, else the default)?
    pub fn is_enabled(self, db: &Db) -> bool {
        match db.get_setting(&self.setting_key()).ok().flatten() {
            Some(v) => v == "true" || v == "1",
            None => self.default_enabled(),
        }
    }

    pub fn set_enabled(self, db: &Db, on: bool) {
        let _ = db.set_setting(&self.setting_key(), if on { "true" } else { "false" });
    }

    pub fn info(self, db: &Db) -> ToolsetInfo {
        ToolsetInfo {
            id: self.id().to_string(),
            label: self.label().to_string(),
            description: self.description().to_string(),
            enabled: self.is_enabled(db),
            sensitive: self.sensitive(),
        }
    }

    /// The OpenAI tool schemas this toolset advertises.
    pub fn tool_specs(self) -> Vec<serde_json::Value> {
        let v = match self {
            Toolset::FileSystem => filesystem::tool_specs(),
            Toolset::WebSearch => websearch::tool_specs(),
            Toolset::CodeExec => codeexec::tool_specs(),
            Toolset::Artifacts => artifacts::tool_specs(),
            Toolset::ImageGen => imagegen::tool_specs(),
            Toolset::Present => present::tool_specs(),
            Toolset::Recall => recall::tool_specs(),
            Toolset::Memory => memory_skill::tool_specs(),
            // Indexing gates both halves of folder reading: the "Read it"
            // button in the Workbench builds the index (IDX-UI-1, not a tool
            // call), and `search_folder` here is the tool that searches it
            // (RET-1). Off means neither is offered.
            Toolset::Indexing => retrieval::tool_specs(),
            Toolset::Mail => mail::tool_specs(),
            Toolset::Skills => skillpack::tool_specs(),
            Toolset::Browser => browser::tool_specs(),
            Toolset::System => screen::tool_specs(),
        };
        v.as_array().cloned().unwrap_or_default()
    }

    /// Does this toolset own the given tool name?
    pub fn handles(self, name: &str) -> bool {
        match self {
            Toolset::FileSystem => filesystem::handles(name),
            Toolset::WebSearch => websearch::handles(name),
            Toolset::CodeExec => codeexec::handles(name),
            Toolset::Artifacts => artifacts::handles(name),
            Toolset::ImageGen => imagegen::handles(name),
            Toolset::Present => present::handles(name),
            Toolset::Recall => recall::handles(name),
            Toolset::Memory => memory_skill::handles(name),
            Toolset::Indexing => retrieval::handles(name),
            Toolset::Mail => mail::handles(name),
            Toolset::Skills => skillpack::handles(name),
            Toolset::Browser => browser::handles(name),
            Toolset::System => screen::handles(name),
        }
    }

    /// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
    pub fn describe(self, name: &str, args: &serde_json::Value) -> (String, String) {
        match self {
            Toolset::FileSystem => filesystem::describe(name, args),
            Toolset::WebSearch => websearch::describe(name, args),
            Toolset::CodeExec => codeexec::describe(name, args),
            Toolset::Artifacts => artifacts::describe(name, args),
            Toolset::ImageGen => imagegen::describe(name, args),
            Toolset::Present => present::describe(name, args),
            Toolset::Recall => recall::describe(name, args),
            Toolset::Memory => memory_skill::describe(name, args),
            Toolset::Indexing => retrieval::describe(name, args),
            Toolset::Mail => mail::describe(name, args),
            Toolset::Skills => skillpack::describe(name, args),
            Toolset::Browser => browser::describe(name, args),
            Toolset::System => screen::describe(name, args),
        }
    }

    /// Execute a call this toolset handles. Returns the text fed back to the model.
    pub async fn execute(
        self,
        ctx: &ToolContext<'_>,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        match self {
            Toolset::FileSystem => filesystem::execute(ctx, name, args).await,
            Toolset::WebSearch => websearch::execute(ctx, name, args).await,
            Toolset::CodeExec => codeexec::execute(ctx, name, args).await,
            Toolset::Artifacts => artifacts::execute(ctx, name, args).await,
            Toolset::ImageGen => imagegen::execute(ctx, name, args).await,
            Toolset::Present => present::execute(ctx, name, args).await,
            Toolset::Recall => recall::execute(ctx, name, args).await,
            Toolset::Memory => memory_skill::execute(ctx, name, args).await,
            Toolset::Indexing => retrieval::execute(ctx, name, args).await,
            Toolset::Mail => mail::execute(ctx, name, args).await,
            Toolset::Skills => skillpack::execute(ctx, name, args).await,
            Toolset::Browser => browser::execute(ctx, name, args).await,
            Toolset::System => screen::execute(ctx, name, args).await,
        }
    }
}

/// The enabled built-in toolsets, in display order.
pub fn enabled(db: &Db) -> Vec<Toolset> {
    Toolset::ALL.into_iter().filter(|s| s.is_enabled(db)).collect()
}

/// `PER-2`: a persona's allowlist (`tools_json`, a JSON array of toolset ids)
/// intersected with the global toggles above — a persona can narrow its own
/// tools but never re-enable one the user switched off in Settings. `None`
/// (the `tools_json` `NULL` default, `PER-1`) means "every enabled toolset",
/// which is exactly `enabled()` and so leaves every existing persona's
/// behaviour untouched.
pub fn enabled_for_persona(db: &Db, tools_json: Option<&str>) -> Vec<Toolset> {
    let global = enabled(db);
    let Some(allow) = tools_json.and_then(|j| serde_json::from_str::<Vec<String>>(j).ok()) else {
        return global;
    };
    global
        .into_iter()
        .filter(|s| allow.iter().any(|id| id == s.id()))
        .collect()
}

/// Metadata for all toolsets (for the Settings surface).
pub fn all_info(db: &Db) -> Vec<ToolsetInfo> {
    Toolset::ALL.into_iter().map(|s| s.info(db)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn a_headless_run_renders_nothing() {
        assert_eq!(check_render(true, false, 10), Err(RenderSkip::Headless));
    }

    #[test]
    fn one_render_per_tool_call() {
        assert_eq!(check_render(false, false, 10), Ok(()));
        assert_eq!(check_render(false, true, 10), Err(RenderSkip::AlreadyRendered));
    }

    #[test]
    fn an_oversized_payload_is_dropped_not_truncated() {
        assert_eq!(check_render(false, false, RENDER_CAP_BYTES), Ok(()));
        assert_eq!(
            check_render(false, false, RENDER_CAP_BYTES + 1),
            Err(RenderSkip::TooLarge)
        );
    }

    /// SMP-4b: attaching a folder is the consent to read it, so folder reading
    /// must not sit behind a toggle the user has to find first. Off by default
    /// meant the very first "Read it" failed with "turn it on in Settings".
    #[test]
    fn folder_reading_is_on_without_the_user_finding_a_toggle() {
        let db = Db::open_in_memory().unwrap();
        assert!(Toolset::Indexing.is_enabled(&db));
        // Still switchable off globally, which is the half SMP-4b keeps.
        Toolset::Indexing.set_enabled(&db, false);
        assert!(!Toolset::Indexing.is_enabled(&db));
    }

    /// `PER-2`: a persona's allowlist can drop a toolset, but never bring back one
    /// switched off globally — the intersection, not just the allowlist, governs.
    #[test]
    fn a_persona_cannot_re_enable_a_globally_disabled_toolset() {
        let db = Db::open_in_memory().unwrap();
        Toolset::WebSearch.set_enabled(&db, false);
        let allow = serde_json::json!(["filesystem", "web_search"]).to_string();
        let effective = enabled_for_persona(&db, Some(&allow));
        assert!(effective.contains(&Toolset::FileSystem));
        assert!(!effective.contains(&Toolset::WebSearch), "global off must win");
    }

    /// `PER-1`: `NULL` `tools_json` means every enabled toolset — a persona that
    /// never touches the tool list behaves exactly as it did before `PER`.
    #[test]
    fn no_allowlist_means_every_enabled_toolset() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(enabled_for_persona(&db, None), enabled(&db));
    }

    /// RND-UI-1: a block the *backend* authored anchors to the in-flight
    /// assistant message and is still anchored after a reload — nothing in the
    /// persistence path assumes the model emitted it.
    #[test]
    fn a_backend_authored_block_stays_anchored_across_a_reload() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("Retrieval", None, false).unwrap();
        let msg = db
            .append_message(
                &conv.id,
                &crate::db::NewMessage {
                    role: "assistant".into(),
                    content: String::new(),
                    model_name: None,
                    model_provenance: None,
                    steps_json: None,
                    attachments: Vec::new(),
                },
            )
            .unwrap();

        let block = db
            .add_block(&conv.id, Some(&msg.id), "collection", "3 files", r#"{"items":[]}"#)
            .unwrap();

        // A reload is exactly this call — the Workspace re-reads blocks by
        // conversation and places each one against its message_id.
        let reloaded = db.list_blocks(&conv.id).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].id, block.id);
        assert_eq!(
            reloaded[0].message_id.as_deref(),
            Some(msg.id.as_str()),
            "the block must stay anchored to the assistant message it belongs to"
        );
        assert_eq!(reloaded[0].kind, "collection");
    }
}
