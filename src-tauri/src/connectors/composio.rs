//! `CAP`: Composio as a connector kind.
//!
//! Composio is one hosted MCP endpoint that fronts hundreds of app toolkits and
//! owns their sign-ins. Here it is a `connectors` row of transport `composio`
//! whose address comes from a live session and whose one secret, the project
//! key, lives in the OS keychain. Everything else Poiesis needs to know is a
//! non-secret identifier.
//!
//! The REST shapes below were checked against Composio's v3.1 reference
//! (`tool_router/session`, `.../link`, `connected_accounts`, `auth_configs`,
//! `toolkits`). Every parser is lenient about fields it does not use, because
//! Composio adds fields and a new one must not break a working install.
//!
//! The key goes in a header on requests to Composio and nowhere else: not to
//! the frontend, not into a prompt, a log, a child process or a stored row.
//!
//! There are two kinds of key and so two modes (`CAP-C1`). Composio **Connect**
//! (`ck_...`) is what Composio gives a person for their own AI client: one MCP
//! address, no REST. The developer **Project** key (`ak_...`) is the platform
//! API the first five phases were built on. Both show the model the same
//! meta-tools; they differ in how sign-ins are started and listed.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::db::{AppAccount, Connector, Db};
use crate::mcp::McpClient;
use crate::secrets::{self, SERVICE_COMPOSIO};

pub const TRANSPORT: &str = "composio";
const KEY_ACCOUNT: &str = "project";
const DEFAULT_API: &str = "https://backend.composio.dev/api/v3.1";
/// `CAP-C1`: Composio Connect's one address, spoken to as Streamable HTTP.
pub const CONNECT_MCP: &str = "https://connect.composio.dev/mcp";
/// The header a Connect key (`ck_...`) travels in. Never the URL.
pub const CONNECT_HEADER: &str = "x-consumer-api-key";

pub const SEARCH_TOOLS: &str = "COMPOSIO_SEARCH_TOOLS";
pub const GET_TOOL_SCHEMAS: &str = "COMPOSIO_GET_TOOL_SCHEMAS";
pub const MULTI_EXECUTE: &str = "COMPOSIO_MULTI_EXECUTE_TOOL";
pub const MANAGE_CONNECTIONS: &str = "COMPOSIO_MANAGE_CONNECTIONS";

/// `CAP-4`: the only Composio meta-tools the model is ever offered. An
/// allowlist, so a tool Composio adds tomorrow stays hidden until someone has
/// read what it does. The remote workbench and bash tools are not on it, and
/// `WAIT_FOR_CONNECTIONS` is not either: Poiesis watches for the sign-in itself.
pub const OFFERED: [&str; 4] = [SEARCH_TOOLS, GET_TOOL_SCHEMAS, MULTI_EXECUTE, MANAGE_CONNECTIONS];

/// `CAP-7`: how many usable sign-ins one app may have.
pub const MAX_ACCOUNTS_PER_APP: usize = 5;

const READ_TIMEOUT: Duration = Duration::from_secs(15);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY: u64 = 20 * 1024 * 1024;
const CATALOG_TTL: Duration = Duration::from_secs(600);
/// How many apps one catalog call asks for. A full page means there may be more.
pub const CATALOG_PAGE: usize = 500;

// ---- the key (CAP-2) ----

pub fn has_key() -> bool {
    // A test never sees the real keychain: it has a key only when it says so.
    #[cfg(test)]
    return tests_pretend::on();
    #[cfg(not(test))]
    secrets::has_secret(SERVICE_COMPOSIO, KEY_ACCOUNT)
}

/// Lets a test act as if a key were saved without touching the OS keychain. Per
/// thread, so tests running side by side do not see each other's.
#[cfg(test)]
pub mod tests_pretend {
    use std::cell::{Cell, RefCell};

    use super::Mode;

    thread_local! {
        static KEY: Cell<bool> = const { Cell::new(false) };
        static MODE: Cell<Mode> = const { Cell::new(Mode::Project) };
        static CONNECT_URL: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub fn on() -> bool {
        KEY.with(Cell::get)
    }

    pub fn key(present: bool) {
        KEY.with(|k| k.set(present));
    }

    /// Which kind of key the pretend key is. Project unless a test says otherwise.
    pub fn mode(mode: Mode) {
        MODE.with(|m| m.set(mode));
    }

    pub fn pretend_mode() -> Mode {
        MODE.with(Cell::get)
    }

    /// Point Connect mode at a stub MCP server.
    pub fn connect_url(url: Option<&str>) {
        CONNECT_URL.with(|c| *c.borrow_mut() = url.map(str::to_string));
    }

    pub fn pretend_connect_url() -> Option<String> {
        CONNECT_URL.with(|c| c.borrow().clone())
    }
}

/// `CAP-C1`: which of Composio's two products a key belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// `ck_...`: the person's own consumer key, spoken to over MCP only.
    Connect,
    /// `ak_...` / `uak_...`: the developer platform's REST API.
    Project,
}

/// Derived from the key every time and never stored on its own, so it cannot
/// drift from the key it describes.
pub fn mode_of(key: &str) -> Mode {
    if key.starts_with("ck_") {
        Mode::Connect
    } else {
        Mode::Project
    }
}

/// The mode of the saved key, or `None` when there is no key.
pub fn current_mode() -> Option<Mode> {
    #[cfg(test)]
    return tests_pretend::on().then(tests_pretend::pretend_mode);
    #[cfg(not(test))]
    read_key().map(|k| mode_of(&k))
}

/// Where Connect mode speaks. A debug build can point it at a local stub with
/// `POIESIS_COMPOSIO_CONNECT`; a release build cannot.
pub fn connect_url() -> String {
    #[cfg(test)]
    if let Some(u) = tests_pretend::pretend_connect_url() {
        return u;
    }
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("POIESIS_COMPOSIO_CONNECT") {
        if !v.trim().is_empty() {
            return v.trim().to_string();
        }
    }
    CONNECT_MCP.to_string()
}

pub fn read_key() -> Option<String> {
    secrets::get_secret(SERVICE_COMPOSIO, KEY_ACCOUNT).ok().flatten().filter(|k| !k.trim().is_empty())
}

pub fn store_key(key: &str) -> Result<(), String> {
    secrets::set_secret(SERVICE_COMPOSIO, KEY_ACCOUNT, &clean_key(key)).map_err(|e| e.to_string())
}

/// A pasted key without the spaces, line breaks or quotes that come along when
/// it is copied out of a dashboard or a `.env` line.
pub fn clean_key(key: &str) -> String {
    key.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`').trim().to_string()
}

/// Composio has several kinds of key and each travels in its own header. A
/// Connect key (`ck_...`) is `x-consumer-api-key`. A user key (`uak_...`) sent as
/// `x-api-key` is refused, so it is sent as the user key it is. Every other kind
/// of key, the project key above all, uses `x-api-key`.
pub fn auth_header(key: &str) -> &'static str {
    if key.starts_with("ck_") {
        CONNECT_HEADER
    } else if key.starts_with("uak_") {
        "x-user-api-key"
    } else {
        "x-api-key"
    }
}

/// Removing the key keeps the user id and the connector row, so pasting the key
/// again finds the same sign-ins. A sign-in link held in memory is dropped: it
/// belonged to that key.
pub fn remove_key() -> Result<(), String> {
    forget_all_links();
    if let Ok(mut shared) = shared_connect().try_lock() {
        *shared = None;
    }
    secrets::delete_secret(SERVICE_COMPOSIO, KEY_ACCOUNT).map_err(|e| e.to_string())
}

// ---- errors ----

#[derive(Debug)]
pub enum ApiError {
    Status { code: u16, message: String },
    Network,
    TooLarge,
    Unreadable,
    /// A link or address that is not on Composio's own domain.
    Untrusted,
}

impl ApiError {
    pub fn code(&self) -> Option<u16> {
        match self {
            ApiError::Status { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// In the person's words. Never carries the key or a raw response body.
    pub fn plain(&self) -> String {
        match self {
            ApiError::Status { code: 401 | 403, message } => {
                let why = if message.is_empty() { String::new() } else { format!(" It said: {message}") };
                format!(
                    "Composio didn't accept this key.{why} Use the project API key from composio.dev under \
                     Settings, Project Settings. If the key is scoped, it needs Session management write access."
                )
            }
            ApiError::Status { code: 404, .. } => "Composio couldn't find that.".to_string(),
            ApiError::Status { code: 429, .. } => "Composio is limiting how fast I can ask. Try again in a minute.".to_string(),
            ApiError::Status { code, message } if *code < 500 && !message.is_empty() => {
                format!("Composio said no: {message}")
            }
            ApiError::Status { .. } => "Composio had a problem on its side. Try again shortly.".to_string(),
            ApiError::Network => "I couldn't reach Composio.".to_string(),
            ApiError::TooLarge => "Composio's answer was too large to read.".to_string(),
            ApiError::Unreadable => "Composio's answer wasn't what I expected.".to_string(),
            ApiError::Untrusted => "Composio sent an address I don't trust, so I didn't use it.".to_string(),
        }
    }
}

// ---- the API (CAP-2) ----

/// The Composio REST API for one key. `base` is the v3.1 root.
pub struct Api<'a> {
    http: &'a reqwest::Client,
    key: &'a str,
    base: String,
}

/// Is `url` an address Poiesis will follow for Composio? Only `https` on
/// `composio.dev` or a subdomain of it. A debug build, and only a debug build,
/// also follows addresses on the host a `POIESIS_COMPOSIO_API` override names,
/// so a local stub can stand in for Composio in tests.
fn trusted(url: &str, stub_base: Option<&str>) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false };
    if let Some(base) = stub_base {
        if let Ok(b) = url::Url::parse(base) {
            if parsed.scheme() == b.scheme() && parsed.host_str() == b.host_str() && parsed.port() == b.port() {
                return true;
            }
        }
    }
    parsed.scheme() == "https"
        && parsed
            .host_str()
            .is_some_and(|h| h == "composio.dev" || h.ends_with(".composio.dev"))
}

impl<'a> Api<'a> {
    pub fn new(http: &'a reqwest::Client, key: &'a str) -> Self {
        #[cfg(debug_assertions)]
        if let Ok(v) = std::env::var("POIESIS_COMPOSIO_API") {
            if !v.trim().is_empty() {
                return Self { http, key, base: v.trim().trim_end_matches('/').to_string() };
            }
        }
        Self { http, key, base: DEFAULT_API.to_string() }
    }

    /// A stub standing in for Composio. Not compiled into release builds.
    #[cfg(any(test, debug_assertions))]
    pub fn at(http: &'a reqwest::Client, key: &'a str, base: &str) -> Self {
        Self { http, key, base: base.trim_end_matches('/').to_string() }
    }

    fn trusts(&self, url: &str) -> bool {
        #[cfg(any(test, debug_assertions))]
        let stub = (self.base != DEFAULT_API).then_some(self.base.as_str());
        #[cfg(not(any(test, debug_assertions)))]
        let stub: Option<&str> = None;
        trusted(url, stub)
    }

    async fn send(&self, method: reqwest::Method, url: &str, body: Option<&Value>) -> Result<Value, ApiError> {
        let timeout = if method == reqwest::Method::GET { READ_TIMEOUT } else { WRITE_TIMEOUT };
        let mut req = self.http.request(method, url).header(auth_header(self.key), self.key).timeout(timeout);
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|_| ApiError::Network)?;
        let status = resp.status();
        if resp.content_length().is_some_and(|n| n > MAX_BODY) {
            return Err(ApiError::TooLarge);
        }
        let bytes = resp.bytes().await.map_err(|_| ApiError::Network)?;
        if bytes.len() as u64 > MAX_BODY {
            return Err(ApiError::TooLarge);
        }
        let parsed: Option<Value> = serde_json::from_slice(&bytes).ok();
        if !status.is_success() {
            // Composio says why in `error.message` (or a bare `message`). It does
            // not echo the key, but the key is blanked out here regardless.
            let message = parsed
                .as_ref()
                .and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")))
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .replace(self.key, "[key]");
            return Err(ApiError::Status { code: status.as_u16(), message });
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        parsed.ok_or(ApiError::Unreadable)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// `POST tool_router/session`. Multi-account on, the person's own custom
    /// sign-in setups pinned, the code sandbox off, and the model-facing
    /// connection removal off: disconnecting is the person's act, in Settings.
    pub async fn create_session(
        &self,
        user_id: &str,
        custom_auth: &BTreeMap<String, String>,
    ) -> Result<Session, ApiError> {
        let mut body = json!({
            "user_id": user_id,
            "manage_connections": {
                "enable": true,
                "enable_wait_for_connections": false,
                "enable_connection_removal": false,
            },
            "multi_account": {
                "enable": true,
                "max_accounts_per_toolkit": MAX_ACCOUNTS_PER_APP,
                "require_explicit_selection": true,
            },
            "workbench": { "enable": false },
        });
        if !custom_auth.is_empty() {
            body["auth_configs"] = json!(custom_auth);
        }
        let v = self.send(reqwest::Method::POST, &self.url("/tool_router/session"), Some(&body)).await?;
        self.session_of(&v)
    }

    /// `GET tool_router/session/{id}`: `None` when it no longer exists.
    pub async fn get_session(&self, id: &str) -> Result<Option<(Session, Value)>, ApiError> {
        match self
            .send(reqwest::Method::GET, &self.url(&format!("/tool_router/session/{id}")), None)
            .await
        {
            Ok(v) => {
                let config = v.get("config").cloned().unwrap_or(Value::Null);
                Ok(Some((self.session_of(&v)?, config)))
            }
            Err(ApiError::Status { code: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn session_of(&self, v: &Value) -> Result<Session, ApiError> {
        let session_id = v.get("session_id").and_then(|s| s.as_str()).ok_or(ApiError::Unreadable)?;
        let mcp_url = v.pointer("/mcp/url").and_then(|s| s.as_str()).ok_or(ApiError::Unreadable)?;
        if !self.trusts(mcp_url) {
            return Err(ApiError::Untrusted);
        }
        Ok(Session { session_id: session_id.to_string(), mcp_url: mcp_url.to_string() })
    }

    /// `GET auth_configs?is_composio_managed=false`: the sign-in setups the
    /// person made in their own project, newest enabled one per app.
    pub async fn custom_auth_configs(&self) -> Result<BTreeMap<String, String>, ApiError> {
        let v = self
            .send(
                reqwest::Method::GET,
                &self.url("/auth_configs?is_composio_managed=false&limit=200"),
                None,
            )
            .await?;
        Ok(newest_enabled(&v))
    }

    /// `POST tool_router/session/{id}/link`: a sign-in address for one app,
    /// under the person's own label. The address is checked before it is
    /// returned; one that is not Composio's never leaves this function.
    pub async fn link(&self, session_id: &str, toolkit: &str, alias: &str) -> Result<LinkResult, ApiError> {
        let body = json!({ "toolkit": toolkit, "alias": alias });
        let v = self
            .send(
                reqwest::Method::POST,
                &self.url(&format!("/tool_router/session/{session_id}/link")),
                Some(&body),
            )
            .await?;
        let redirect_url = v.get("redirect_url").and_then(|s| s.as_str()).ok_or(ApiError::Unreadable)?;
        if !self.trusts(redirect_url) {
            return Err(ApiError::Untrusted);
        }
        let connected_account_id = v
            .get("connected_account_id")
            .and_then(|s| s.as_str())
            .ok_or(ApiError::Unreadable)?;
        Ok(LinkResult {
            redirect_url: redirect_url.to_string(),
            connected_account_id: connected_account_id.to_string(),
        })
    }

    /// Every sign-in Composio holds for `user_id`, all statuses.
    pub async fn accounts(&self, user_id: &str) -> Result<Vec<AccountRow>, ApiError> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..5 {
            let mut url = self.url(&format!("/connected_accounts?limit=100&user_ids={}", encode(user_id)));
            if let Some(c) = &cursor {
                url.push_str(&format!("&cursor={}", encode(c)));
            }
            let v = self.send(reqwest::Method::GET, &url, None).await?;
            out.extend(parse_accounts(&v));
            cursor = v.get("next_cursor").and_then(|c| c.as_str()).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }

    /// `DELETE connected_accounts/{id}?revoke_on_delete=true`.
    pub async fn delete_account(&self, id: &str) -> Result<(), ApiError> {
        self.send(
            reqwest::Method::DELETE,
            &self.url(&format!("/connected_accounts/{}?revoke_on_delete=true", encode(id))),
            None,
        )
        .await
        .map(|_| ())
    }

    /// The apps Composio offers, most used first (`CAP-8`).
    pub async fn toolkits(&self, search: Option<&str>) -> Result<Vec<ToolkitInfo>, ApiError> {
        let url = match search {
            Some(q) => self.url(&format!("/toolkits?limit=50&sort_by=usage&search={}", encode(q))),
            None => self.url(&format!("/toolkits?limit={CATALOG_PAGE}&sort_by=usage")),
        };
        let v = self.send(reqwest::Method::GET, &url, None).await?;
        Ok(parse_toolkits(&v))
    }

    /// A short, throwaway fingerprint of the key and endpoint for cache keys.
    /// Never the key itself.
    fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.key.as_bytes());
        h.update(self.base.as_bytes());
        h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
    }
}

/// Percent-encode one query value.
fn encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

// ---- shapes ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub session_id: String,
    pub mcp_url: String,
}

#[derive(Debug, Clone)]
pub struct LinkResult {
    pub redirect_url: String,
    pub connected_account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub id: String,
    pub app: String,
    pub alias: Option<String>,
    pub status: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolkitInfo {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    /// Composio can sign the person in without their own setup (`CAP-8`).
    #[serde(default = "yes")]
    pub managed: bool,
}

fn yes() -> bool {
    true
}

/// `{auth_config items}` -> `{toolkit: id}`: enabled ones only, newest per app.
pub fn newest_enabled(v: &Value) -> BTreeMap<String, String> {
    let mut best: BTreeMap<String, (String, String)> = BTreeMap::new();
    for item in v.get("items").and_then(|i| i.as_array()).into_iter().flatten() {
        let enabled = item
            .get("status")
            .and_then(|s| s.as_str())
            .map(|s| s.eq_ignore_ascii_case("ENABLED"))
            .unwrap_or(true);
        if !enabled {
            continue;
        }
        let (Some(slug), Some(id)) = (
            item.pointer("/toolkit/slug").and_then(|s| s.as_str()),
            item.get("id").and_then(|s| s.as_str()),
        ) else {
            continue;
        };
        let stamp = item
            .get("last_updated_at")
            .or_else(|| item.get("created_at"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let slug = slug.to_lowercase();
        if best.get(&slug).is_none_or(|(old, _)| stamp >= *old) {
            best.insert(slug, (stamp, id.to_string()));
        }
    }
    best.into_iter().map(|(slug, (_, id))| (slug, id)).collect()
}

pub fn parse_accounts(v: &Value) -> Vec<AccountRow> {
    v.get("items")
        .and_then(|i| i.as_array())
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some(AccountRow {
                id: a.get("id")?.as_str()?.to_string(),
                app: a.pointer("/toolkit/slug")?.as_str()?.to_lowercase(),
                alias: a.get("alias").and_then(|s| s.as_str()).map(str::to_string).filter(|s| !s.is_empty()),
                status: a.get("status").and_then(|s| s.as_str()).unwrap_or("UNKNOWN").to_uppercase(),
                user_id: a.get("user_id").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            })
        })
        .collect()
}

pub fn parse_toolkits(v: &Value) -> Vec<ToolkitInfo> {
    v.get("items")
        .and_then(|i| i.as_array())
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let slug = t.get("slug")?.as_str()?.to_lowercase();
            let name = t.get("name").and_then(|n| n.as_str()).unwrap_or(&slug).to_string();
            let managed = t
                .get("composio_managed_auth_schemes")
                .and_then(|a| a.as_array())
                .is_some_and(|a| !a.is_empty())
                || t.get("no_auth").and_then(|b| b.as_bool()).unwrap_or(false);
            Some(ToolkitInfo {
                slug,
                name,
                logo: t.pointer("/meta/logo").and_then(|l| l.as_str()).map(str::to_string),
                description: t.pointer("/meta/description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                categories: t
                    .pointer("/meta/categories")
                    .and_then(|c| c.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    .collect(),
                managed,
            })
        })
        .collect()
}

/// `CAP-8`: the small list shown when Composio cannot be reached or no key is
/// set yet. Read-only; connecting needs the key.
pub fn curated() -> Vec<ToolkitInfo> {
    curated_ref().to_vec()
}

/// The bundled list, read once: `app_label` runs for every line the person sees.
fn curated_ref() -> &'static [ToolkitInfo] {
    static LIST: OnceLock<Vec<ToolkitInfo>> = OnceLock::new();
    LIST.get_or_init(|| {
        #[derive(Deserialize)]
        struct File {
            apps: Vec<Entry>,
        }
        #[derive(Deserialize)]
        struct Entry {
            slug: String,
            label: String,
            blurb: String,
            category: String,
        }
        let file: File =
            serde_json::from_str(include_str!("../../../shared/apps.json")).unwrap_or(File { apps: vec![] });
        file.apps
            .into_iter()
            .map(|e| ToolkitInfo {
                slug: e.slug,
                name: e.label,
                logo: None,
                description: e.blurb,
                categories: vec![e.category],
                managed: true,
            })
            .collect()
    })
}

/// Every app Composio lists, bundled with the app (`scripts/build-composio-catalog.mjs`),
/// so Browse apps shows them all with no key and no network. A Connect key
/// cannot list toolkits, so this is the list there too.
pub fn bundled() -> &'static [ToolkitInfo] {
    static LIST: OnceLock<Vec<ToolkitInfo>> = OnceLock::new();
    LIST.get_or_init(|| {
        #[derive(Deserialize)]
        struct File {
            apps: Vec<Entry>,
        }
        #[derive(Deserialize)]
        struct Entry {
            slug: String,
            name: String,
            description: String,
            category: String,
            managed: bool,
            logo: bool,
        }
        let file: File =
            serde_json::from_str(include_str!("../../../shared/composio-catalog.json")).unwrap_or(File { apps: vec![] });
        file.apps
            .into_iter()
            .map(|e| ToolkitInfo {
                logo: e.logo.then(|| local_logo(&e.slug)),
                slug: e.slug,
                name: e.name,
                description: e.description,
                categories: vec![e.category],
                managed: e.managed,
            })
            .collect()
    })
}

/// Where the page finds an app's logo: a file shipped with the app, since the
/// webview loads no remote images.
pub fn local_logo(slug: &str) -> String {
    format!("/app-logos/{slug}.svg")
}

// ---- the connector row and its session (CAP-1, CAP-2) ----

fn config_of(c: &Connector) -> serde_json::Map<String, Value> {
    c.config_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn save_config(db: &Db, id: &str, config: &serde_json::Map<String, Value>) -> Result<(), String> {
    db.set_connector_config(id, &Value::Object(config.clone()).to_string()).map_err(|e| e.to_string())
}

/// The tools Composio last said the session offers, as cached by `refresh_tools`.
pub fn cached_tools(c: &Connector) -> Vec<crate::mcp::McpTool> {
    config_of(c)
        .get("tools")
        .and_then(|t| serde_json::from_value(t.clone()).ok())
        .unwrap_or_default()
}

/// The one Composio connector, made on first use. Its user id is minted once and
/// never changes while the row exists: connections belong to the user, not the
/// session, so a new session under the same id keeps every sign-in.
pub fn ensure_row(db: &Db) -> Result<Connector, String> {
    if let Some(c) = db.composio_connector().map_err(|e| e.to_string())? {
        let mut config = config_of(&c);
        if config.get("user_id").and_then(|u| u.as_str()).is_none() {
            config.insert("user_id".into(), json!(new_user_id()));
            save_config(db, &c.id, &config)?;
            return db.composio_connector().map_err(|e| e.to_string())?.ok_or_else(|| "The Composio connector is gone.".into());
        }
        return Ok(c);
    }
    let config = json!({ "user_id": new_user_id(), "tools": [], "prompts": [] });
    db.add_composio_connector(&config.to_string()).map_err(|e| e.to_string())
}

fn new_user_id() -> String {
    format!("poiesis_{}", uuid::Uuid::new_v4())
}

pub fn user_id_of(c: &Connector) -> Option<String> {
    config_of(c).get("user_id").and_then(|u| u.as_str()).map(str::to_string)
}

/// `(app, id)` pairs recorded when the session was made, to notice a new one.
fn session_id_of(c: &Connector) -> Option<String> {
    config_of(c).get("session_id").and_then(|u| u.as_str()).map(str::to_string)
}

/// What to do with a session that still exists.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    Recreate,
}

/// A session cannot be edited, so it is recreated (same user id) when
/// multi-account is off or when a sign-in setup the person made in their own
/// project is not in it.
pub fn judge(config: &Value, custom: &BTreeMap<String, String>) -> Verdict {
    let multi = config.pointer("/multi_account/enable").and_then(|b| b.as_bool()).unwrap_or(false);
    if !multi {
        return Verdict::Recreate;
    }
    let pinned = config.get("auth_configs");
    let all_pinned = custom
        .iter()
        .all(|(toolkit, id)| pinned.and_then(|p| p.get(toolkit)).and_then(|v| v.as_str()) == Some(id.as_str()));
    if all_pinned {
        Verdict::Keep
    } else {
        Verdict::Recreate
    }
}

fn tried() -> &'static Mutex<HashSet<String>> {
    static TRIED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    TRIED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The live session for this install: reused when it still fits, recreated with
/// the same user id when it does not. A recreate for a given (session, setups)
/// pair is attempted once per app start, so a session that comes back looking
/// wrong cannot make this loop.
pub async fn ensure_session(db: &Db, api: &Api<'_>) -> Result<(Session, Connector), String> {
    let row = ensure_row(db)?;
    let user_id = user_id_of(&row).ok_or("The Composio connector has no user id.")?;
    let custom = api.custom_auth_configs().await.unwrap_or_default();

    if let Some(id) = session_id_of(&row) {
        match api.get_session(&id).await.map_err(|e| e.plain())? {
            Some((session, config)) => {
                if judge(&config, &custom) == Verdict::Keep {
                    return Ok((session, row));
                }
                // `CAP-C-F3`: keyed on the setups, not the session id. A recreate
                // makes a new id, so keying on it would recreate on every run.
                let marker = format!("{user_id}|{custom:?}");
                if !tried().lock().unwrap().insert(marker) {
                    return Ok((session, row));
                }
            }
            None => {}
        }
    }

    let session = api.create_session(&user_id, &custom).await.map_err(|e| e.plain())?;
    let mut config = config_of(&row);
    config.insert("session_id".into(), json!(session.session_id));
    save_config(db, &row.id, &config)?;
    let row = db
        .composio_connector()
        .map_err(|e| e.to_string())?
        .ok_or("The Composio connector is gone.")?;
    Ok((session, row))
}

/// An MCP client for this install, with the key in its header. A Connect key
/// speaks to Connect's one address; a Project key to its live session.
pub async fn mcp_client(db: &Db, http: &reqwest::Client) -> Result<McpClient, String> {
    let key = read_key().ok_or("Add your Composio key under Settings, Apps first.")?;
    if mode_of(&key) == Mode::Connect {
        return Ok(connect_client(http, &key));
    }
    let api = Api::new(http, &key);
    let (session, _) = ensure_session(db, &api).await?;
    let header = auth_header(&key);
    Ok(McpClient::new_with_header(http.clone(), session.mcp_url, header, key))
}

/// `CAP-C1`: a client on Connect's address. No session and no user id.
pub fn connect_client(http: &reqwest::Client, key: &str) -> McpClient {
    McpClient::new_with_header(http.clone(), connect_url(), CONNECT_HEADER, key.to_string())
}

/// What Composio said when it refused a Connect key, in the person's words.
const CONNECT_REFUSED: &str = "Composio didn't accept this key. Copy it again in Composio under For You, Settings, \
     Sessions & API Key; it starts with ck_.";

/// Check a key without keeping it. A Connect key is tried over MCP and must list
/// the batch runner; a Project key must be able to make a session.
pub async fn validate_key(db: &Db, http: &reqwest::Client, key: &str) -> Result<(), String> {
    if mode_of(key) == Mode::Connect {
        let mut client = connect_client(http, key);
        let tools = client.discover().await.map_err(|e| match &e {
            crate::mcp::client::McpError::Http(h) if matches!(h.status().map(|s| s.as_u16()), Some(401 | 403)) => {
                CONNECT_REFUSED.to_string()
            }
            crate::mcp::client::McpError::Http(h) if h.status().is_none() => ApiError::Network.plain(),
            other => format!("Composio's answer wasn't what I expected ({}).", short_error(&other.to_string())),
        })?;
        return if tools.iter().any(|t| t.name == MULTI_EXECUTE) {
            Ok(())
        } else {
            Err("Composio accepted the key but didn't offer the tools I need to run apps. Try again in a moment.".into())
        };
    }
    // The install's own user id is used, so saving a key does not leave a
    // throwaway user behind in the person's project each time.
    let user_id = user_id_of(&ensure_row(db)?).ok_or("The Composio connector has no user id.")?;
    Api::new(http, key)
        .create_session(&user_id, &BTreeMap::new())
        .await
        .map(|_| ())
        .map_err(|e| e.plain())
}

/// The first line of an error, cut short, so a long body never reaches the page.
fn short_error(s: &str) -> String {
    let line = s.lines().next().unwrap_or("");
    crate::media::ellipsize(line, 80)
}

/// Ask Composio what the session offers and keep it, so the registry can build
/// its tool list with no network. Returns how many tools came back.
pub async fn refresh_tools(db: &Db, http: &reqwest::Client) -> Result<usize, String> {
    let mut client = mcp_client(db, http).await?;
    let tools = client.discover().await.map_err(|e| e.to_string())?;
    let row = ensure_row(db)?;
    let mut config = config_of(&row);
    config.insert("tools".into(), serde_json::to_value(&tools).map_err(|e| e.to_string())?);
    config.insert("checked_at".into(), json!(crate::db::now_ms()));
    save_config(db, &row.id, &config)?;
    Ok(tools.len())
}

/// Pull the accounts from Composio into the mirror. The person's labels are
/// Composio's aliases, so this loses nothing.
///
/// In Connect mode Composio has no account list, so this asks about the apps the
/// mirror already knows and are not mid sign-in (`CAP-C4`). It is what runs in
/// the background and for the checkup, so it never asks about an app whose
/// sign-in is still open.
pub async fn refresh_accounts(db: &Db, http: &reqwest::Client) -> Result<Vec<AppAccount>, String> {
    let key = read_key().ok_or("Add your Composio key first.")?;
    if mode_of(&key) == Mode::Connect {
        return check_accounts(db, http, None, false).await;
    }
    sync_accounts(db, &Api::new(http, &key)).await
}

/// The Apps page's "Check again" and the sign-in card's polling. In Project mode
/// it is the full sync. In Connect mode it asks about `apps` (or every app in the
/// mirror) and, when `waiting` is false, leaves out those whose sign-in is open.
pub async fn check_accounts(
    db: &Db,
    http: &reqwest::Client,
    apps: Option<&[String]>,
    waiting: bool,
) -> Result<Vec<AppAccount>, String> {
    let key = read_key().ok_or("Add your Composio key first.")?;
    if mode_of(&key) == Mode::Project {
        return sync_accounts(db, &Api::new(http, &key)).await;
    }
    let row = ensure_row(db)?;
    let held = db.list_app_accounts(&row.id).map_err(|e| e.to_string())?;
    let mut wanted: Vec<String> = match apps {
        Some(list) => list.iter().filter(|a| valid_slug(a)).cloned().collect(),
        None => held
            .iter()
            .filter(|a| waiting || !is_waiting(&a.status))
            .map(|a| a.app.clone())
            .collect(),
    };
    // An app with several accounts has several rows: it is asked about once.
    wanted.sort();
    wanted.dedup();
    if !wanted.is_empty() {
        check_with_shared_client(db, http, &key, &wanted).await?;
    }
    db.list_app_accounts(&row.id).map_err(|e| e.to_string())
}

/// One Connect client kept between checks, with the address and key it was made
/// for. The sign-in card asks every few seconds; without this each ask was a new
/// MCP session (a handshake, then the question). Holding the lock for the whole
/// check also keeps two checks from running over each other on a slow link.
fn shared_connect() -> &'static tokio::sync::Mutex<Option<(String, McpClient)>> {
    static SHARED: OnceLock<tokio::sync::Mutex<Option<(String, McpClient)>>> = OnceLock::new();
    SHARED.get_or_init(|| tokio::sync::Mutex::new(None))
}

async fn check_with_shared_client(db: &Db, http: &reqwest::Client, key: &str, apps: &[String]) -> Result<(), String> {
    // A fingerprint, so no second copy of the key sits beside the client.
    let made_for: String =
        Sha256::digest(format!("{}\n{key}", connect_url()).as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    let mut shared = shared_connect().lock().await;
    if shared.as_ref().is_none_or(|(f, _)| *f != made_for) {
        let mut client = connect_client(http, key);
        client.initialize().await.map_err(|e| connect_error(&e))?;
        *shared = Some((made_for, client));
    }
    let (_, client) = shared.as_mut().expect("made above");
    let result = check_connect(db, client, apps).await;
    if result.is_err() {
        // A session Composio has dropped is made again on the next check.
        *shared = None;
    }
    result.map(|_| ())
}

/// `refresh_accounts` with the API already in hand.
pub async fn sync_accounts(db: &Db, api: &Api<'_>) -> Result<Vec<AppAccount>, String> {
    let row = ensure_row(db)?;
    let user_id = user_id_of(&row).ok_or("The Composio connector has no user id.")?;
    let rows = api.accounts(&user_id).await.map_err(|e| e.plain())?;
    let now = crate::db::now_ms();
    let accounts: Vec<AppAccount> = rows
        .into_iter()
        // Another user's account in a shared project is not this install's.
        .filter(|r| r.user_id.is_empty() || r.user_id == user_id)
        .map(|r| AppAccount {
            id: r.id,
            connector_id: row.id.clone(),
            app: r.app,
            label: r.alias,
            status: r.status,
            updated_at: now,
        })
        .collect();
    db.replace_app_accounts(&row.id, &accounts).map_err(|e| e.to_string())?;
    Ok(accounts)
}

// ---- sign-in links held in memory (CAP-C3, CAP-C-F1) ----

/// How long a sign-in link stays good, as Composio documents it.
const LINK_TTL: Duration = Duration::from_secs(600);

/// A sign-in that is open in the person's browser. Kept in memory only: the
/// address is never written to the database, a message or a prompt.
#[derive(Debug, Clone)]
pub struct Pending {
    pub label: String,
    pub account_id: String,
    pub url: String,
    /// Composio's account ids for the app when this sign-in began. A sign-in for
    /// one more account is done when an active one appears that is not here.
    pub before: Vec<String>,
    at: Instant,
}

fn pending() -> &'static Mutex<HashMap<(String, String), Pending>> {
    static PENDING: OnceLock<Mutex<HashMap<(String, String), Pending>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remember the sign-in address for `app`. Every caller has already checked that
/// it is on Composio's own domain (`Api::link`, `entry_link`), and the command
/// that opens it checks again.
pub fn remember_link(connector_id: &str, app: &str, label: &str, account_id: &str, url: &str) {
    remember_sign_in(connector_id, app, label, account_id, url, Vec::new());
}

/// `remember_link`, with the accounts the app already had (`Pending::before`).
fn remember_sign_in(connector_id: &str, app: &str, label: &str, account_id: &str, url: &str, before: Vec<String>) {
    pending().lock().unwrap().insert(
        (connector_id.to_string(), app.to_string()),
        Pending {
            label: label.to_string(),
            account_id: account_id.to_string(),
            url: url.to_string(),
            before,
            at: Instant::now(),
        },
    );
}

/// The open sign-in for `app`, if it is younger than ten minutes.
pub fn recall_link(connector_id: &str, app: &str) -> Option<Pending> {
    recall_link_within(connector_id, app, LINK_TTL)
}

fn recall_link_within(connector_id: &str, app: &str, ttl: Duration) -> Option<Pending> {
    let mut map = pending().lock().unwrap();
    let key = (connector_id.to_string(), app.to_string());
    match map.get(&key) {
        Some(p) if p.at.elapsed() < ttl => Some(p.clone()),
        Some(_) => {
            map.remove(&key);
            None
        }
        None => None,
    }
}

pub fn forget_link(connector_id: &str, app: &str) {
    pending().lock().unwrap().remove(&(connector_id.to_string(), app.to_string()));
}

pub fn forget_all_links() {
    pending().lock().unwrap().clear();
}

/// Is `url` an address Poiesis will open or keep for a sign-in?
fn trusted_link(url: &str) -> bool {
    #[cfg(any(test, debug_assertions))]
    let stub = {
        let u = connect_url();
        (u != CONNECT_MCP).then_some(u)
    };
    #[cfg(not(any(test, debug_assertions)))]
    let stub: Option<String> = None;
    trusted(url, stub.as_deref())
}

/// May this address be opened in the person's browser? The same rule as for
/// keeping it; the command that opens it asks once more.
pub fn safe_to_open(url: &str) -> bool {
    trusted_link(url)
}

pub fn is_waiting(status: &str) -> bool {
    ["INITIATED", "INITIALIZING"].iter().any(|s| status.eq_ignore_ascii_case(s))
}

// ---- Connect mode: reading what Composio says (CAP-C2, CAP-C5) ----

/// What Composio says about one app's sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conn {
    Active,
    Initiated,
    Failed,
    Expired,
    /// Asked about, and Composio holds no account for it at all: removed in
    /// Composio, or by another AI client. The mirror stops listing it.
    Gone,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnState {
    pub app: String,
    pub status: Conn,
    /// A sign-in address on Composio's domain. Never shown to the model.
    pub link: Option<String>,
}

fn conn_of(word: &str) -> Conn {
    match word.trim().to_uppercase().replace([' ', '-'], "_").as_str() {
        "ACTIVE" | "CONNECTED" | "ENABLED" | "ALREADY_CONNECTED" => Conn::Active,
        "INITIATED" | "INITIALIZING" | "PENDING" | "WAITING" | "NEEDS_AUTH" | "AUTH_REQUIRED" | "INITIATE" => Conn::Initiated,
        "FAILED" | "ERROR" => Conn::Failed,
        "EXPIRED" | "INACTIVE" | "REVOKED" => Conn::Expired,
        "NOT_CONNECTED" | "DISCONNECTED" | "NO_CONNECTION" | "NO_ACCOUNTS" => Conn::Gone,
        _ => Conn::Unknown,
    }
}

/// Parse JSON that may have words around it: the outermost `{...}`.
fn parse_json_lenient(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Some(v);
    }
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    (a < b).then(|| serde_json::from_str(&t[a..=b]).ok()).flatten()
}

fn entry_app(e: &Value, key: Option<&str>) -> Option<String> {
    // A display name such as "Google Calendar" is not a slug, so the first field
    // that is one wins, and the entry's key is the last resort.
    let from_fields = ["toolkit", "toolkit_slug", "app", "slug", "name"].iter().filter_map(|k| match e.get(*k) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(o)) => o.get("slug").and_then(|s| s.as_str()).map(str::to_string),
        _ => None,
    });
    from_fields
        .chain(key.map(str::to_string))
        .map(|s| s.trim().to_lowercase())
        .find(|s| valid_slug(s))
}

fn entry_status(e: &Value) -> Conn {
    // One working account makes the app connected, whatever the entry says: an
    // `add` refused for the account limit is `failed` and still lists the active one.
    if nested_accounts(e).any(|a| entry_status(a) == Conn::Active) {
        return Conn::Active;
    }
    for k in ["status", "connection_status", "connectionStatus", "state"] {
        if let Some(s) = e.get(k).and_then(|s| s.as_str()) {
            let c = conn_of(s);
            if c != Conn::Unknown {
                return c;
            }
        }
    }
    for k in ["is_active", "active", "connected", "is_connected", "has_active_connection"] {
        if e.get(k).and_then(|b| b.as_bool()) == Some(true) {
            return Conn::Active;
        }
    }
    let mut best = Conn::Unknown;
    for inner in nested_accounts(e) {
        match entry_status(inner) {
            Conn::Unknown => {}
            other => best = other,
        }
    }
    // An account list that is there and empty says there is no sign-in at all.
    if best == Conn::Unknown && has_account_list(e) && nested_accounts(e).next().is_none() {
        return Conn::Gone;
    }
    best
}

/// Does the entry carry a list of accounts at all, even an empty one?
fn has_account_list(e: &Value) -> bool {
    ["accounts", "connected_accounts", "connections"].iter().any(|k| e.get(*k).is_some_and(Value::is_array))
}

/// The accounts listed inside an entry.
fn nested_accounts(e: &Value) -> impl Iterator<Item = &Value> {
    ["accounts", "connected_accounts", "connections"]
        .into_iter()
        .flat_map(move |k| e.get(k).and_then(|a| a.as_array()).into_iter().flatten())
}

/// One account Composio lists for an app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Composio's id for it, e.g. `gmail_vole-negate`: what `remove`, `rename`
    /// and a batch's `account` take.
    pub id: String,
    /// The person's name for it. The recording had none set, so the field is
    /// looked for under each name it plausibly has.
    pub alias: Option<String>,
    pub status: Conn,
}

fn alias_of(a: &Value) -> Option<String> {
    ["alias", "account_alias", "label", "nickname"]
        .iter()
        .find_map(|k| a.get(*k).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 64 && !s.chars().any(char::is_control))
        .map(str::to_string)
}

/// The accounts inside one entry that have an id.
fn listed_in(e: &Value) -> Vec<Listed> {
    nested_accounts(e)
        .filter_map(|a| {
            let id = a.get("id").and_then(|i| i.as_str()).filter(|i| !i.is_empty() && i.len() <= 128)?;
            Some(Listed { id: id.to_string(), alias: alias_of(a), status: entry_status(a) })
        })
        .collect()
}

/// `(app, account)` for every account in a `MANAGE_CONNECTIONS` answer, for the
/// apps asked about.
fn listed(text: &str, asked: &[String]) -> Vec<(String, Listed)> {
    let Some(v) = parse_json_lenient(text) else { return vec![] };
    let mut out = Vec::new();
    for (key, e) in result_entries(&v) {
        let Some(app) = entry_app(e, key) else { continue };
        if asked.contains(&app) {
            out.extend(listed_in(e).into_iter().map(|a| (app.clone(), a)));
        }
    }
    out
}

/// `(app, account id, working)` for every account in a `list` answer, for the
/// apps asked about.
fn listed_accounts(text: &str, asked: &[String]) -> Vec<(String, String, bool)> {
    listed(text, asked).into_iter().map(|(app, a)| (app, a.id, a.status == Conn::Active)).collect()
}

/// The `(app, account id)` of every listed account that is not a working sign-in.
fn stale_accounts(text: &str, asked: &[String]) -> Vec<(String, String)> {
    listed_accounts(text, asked).into_iter().filter(|(_, _, working)| !working).map(|(a, id, _)| (a, id)).collect()
}

/// The per-app entries of a `MANAGE_CONNECTIONS` answer, keyed by app or listed.
fn result_entries(v: &Value) -> Vec<(Option<&str>, &Value)> {
    let results = v
        .pointer("/data/results")
        .or_else(|| v.get("results"))
        .or_else(|| v.pointer("/data/data/results"));
    match results {
        Some(Value::Object(map)) => map.iter().map(|(k, v)| (Some(k.as_str()), v)).collect(),
        Some(Value::Array(list)) => list.iter().map(|v| (None, v)).collect(),
        _ => vec![],
    }
}

/// The first address on Composio's domain inside an entry, preferring a field
/// named like a link and never one named like a picture.
fn entry_link(e: &Value) -> Option<String> {
    fn walk(v: &Value, key: &str, depth: u8, out: &mut Vec<(bool, String)>) {
        let low = key.to_lowercase();
        if ["logo", "icon", "image", "avatar"].iter().any(|k| low.contains(k)) {
            return;
        }
        match v {
            Value::String(s) if trusted_link(s) => {
                let named = ["url", "link", "redirect"].iter().any(|k| low.contains(k));
                out.push((named, s.clone()));
            }
            Value::Object(o) if depth < 4 => o.iter().for_each(|(k, v)| walk(v, k, depth + 1, out)),
            Value::Array(a) if depth < 4 => a.iter().for_each(|v| walk(v, key, depth + 1, out)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(e, "", 0, &mut found);
    found.iter().find(|(named, _)| *named).or_else(|| found.first()).map(|(_, s)| s.clone())
}

/// `CAP-C2`: what `COMPOSIO_MANAGE_CONNECTIONS` answered, per app. Lenient on
/// purpose: Composio does not document the fields inside each entry, so the app,
/// the status and the link are each looked for under every name they plausibly
/// have. An answer that can't be read is an empty list, never a crash.
pub fn parse_connections(text: &str) -> Vec<ConnState> {
    let Some(v) = parse_json_lenient(text) else { return vec![] };
    let entries = result_entries(&v);
    if entries.is_empty() {
        return vec![];
    }
    // The one place the answer is documented: how many apps are connected, waiting
    // or failed. With a single app in the answer it says what that app is, even
    // when the entry's own fields are named something unexpected.
    let counts = |k: &str| v.pointer(&format!("/data/summary/{k}")).and_then(|n| n.as_u64()).unwrap_or(0);
    let (active, initiated, failed) =
        (counts("active_connections"), counts("initiated_connections"), counts("failed_connections"));
    let only_one = entries.len() == 1;
    let mut out: Vec<ConnState> = Vec::new();
    for (key, e) in entries {
        let Some(app) = entry_app(e, key) else { continue };
        let mut status = entry_status(e);
        let link = if status == Conn::Active { None } else { entry_link(e) };
        // A link with no word about the account means a sign-in is waiting.
        if link.is_some() && status != Conn::Active {
            status = Conn::Initiated;
        }
        if status == Conn::Unknown && only_one {
            status = match (active, initiated, failed) {
                (1, 0, 0) => Conn::Active,
                (0, 1, 0) => Conn::Initiated,
                (0, 0, 1) => Conn::Failed,
                _ => Conn::Unknown,
            };
        }
        match out.iter_mut().find(|s| s.app == app) {
            Some(old) if old.status == Conn::Active => {}
            Some(old) => *old = ConnState { app, status, link },
            None => out.push(ConnState { app, status, link }),
        }
    }
    out
}

/// `parse_connections` for a question about `asked`. When only one app was asked
/// about and the entries could not be read, the documented summary counts still
/// say what happened to it.
pub fn parse_connections_of(text: &str, asked: &[String]) -> Vec<ConnState> {
    let mut states = parse_connections(text);
    let [app] = asked else { return states };
    if states.iter().any(|s| &s.app == app) {
        return states;
    }
    let Some(v) = parse_json_lenient(text) else { return states };
    let counts = |k: &str| v.pointer(&format!("/data/summary/{k}")).and_then(|n| n.as_u64()).unwrap_or(0);
    let status = match (counts("active_connections"), counts("initiated_connections"), counts("failed_connections")) {
        (1, 0, 0) => Conn::Active,
        (0, 1, 0) => Conn::Initiated,
        (0, 0, 1) => Conn::Failed,
        _ => return states,
    };
    let link = if status == Conn::Initiated { entry_link(&v) } else { None };
    states.push(ConnState { app: app.clone(), status, link });
    states
}

/// Rewrite every `scheme://...` address in `text`. `decide` sees the address with
/// JSON's `\/` undone and returns what to put there, or `None` to leave it as it
/// was written.
fn rewrite_urls(text: &str, decide: &mut dyn FnMut(&str) -> Option<String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        // `scheme://` or its JSON-escaped form `scheme:\/\/`.
        let sep = if chars[i] == ':' && i > 0 {
            if chars[i + 1..].starts_with(&['/', '/']) {
                Some(3)
            } else if chars[i + 1..].starts_with(&['\\', '/', '\\', '/']) {
                Some(5)
            } else {
                None
            }
        } else {
            None
        };
        let Some(sep) = sep else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        // Back up over the scheme letters already copied.
        let mut scheme = String::new();
        while out.chars().last().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.len() < 6 {
            scheme.insert(0, out.pop().unwrap());
        }
        let mut end = i + sep;
        while end < chars.len() {
            let c = chars[end];
            let escaped_slash = c == '\\' && chars.get(end + 1) == Some(&'/');
            if c.is_whitespace() || (!escaped_slash && "\"'<>)]}\\".contains(c)) {
                break;
            }
            end += 1;
        }
        while end > i + sep && ".,;:".contains(chars[end - 1]) {
            end -= 1;
        }
        let rest: String = chars[i..end].iter().collect();
        let url = format!("{scheme}{rest}").replace("\\/", "/");
        match decide(&url) {
            Some(replacement) if scheme == "https" || scheme == "http" => out.push_str(&replacement),
            _ => {
                out.push_str(&scheme);
                out.push_str(&rest);
            }
        }
        i = end;
    }
    out
}

fn looks_like_sign_in(url: &str) -> bool {
    let low = url.to_lowercase();
    low.contains("link") || low.contains("/ln_")
}

/// `CAP-C5`: replace every Composio sign-in address with a plain sentence. It
/// runs on everything Composio sends back before the model sees it, because a
/// refusal can carry a link too. Other addresses, and Composio pages that are
/// not a sign-in, are left alone.
pub fn redact_links(text: &str) -> String {
    rewrite_urls(text, &mut |url| {
        (trusted_link(url) && looks_like_sign_in(url)).then(|| "[sign-in link shown to the person]".to_string())
    })
}

/// A recording of a Composio answer made safe to keep in the repository: the
/// key, sign-in addresses, account and link ids and email addresses are replaced
/// by look-alikes. The shapes stay, so a parser test is still a fair test.
/// Hand-check what it writes all the same (`CAP-C9`).
pub fn scrub_for_fixture(text: &str, key: &str) -> String {
    let mut s = if key.is_empty() { text.to_string() } else { text.replace(key, "ck_REDACTED") };
    s = rewrite_urls(&s, &mut |url| {
        (trusted_link(url) && looks_like_sign_in(url)).then(|| "https://connect.composio.dev/link/ln_REDACTED".to_string())
    });
    // Ids: `ln_...`, `ca_...`, `ac_...`, `ct_...` when they stand alone as a word.
    let chars: Vec<char> = s.chars().collect();
    let mut ids = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || !(chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '_');
        let prefix = ["ln_", "ca_", "ac_", "ct_"]
            .iter()
            .find(|p| boundary && p.chars().enumerate().all(|(k, c)| chars.get(i + k) == Some(&c)));
        if let Some(p) = prefix {
            let start = i + p.len();
            let mut end = start;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_' || chars[end] == '-') {
                end += 1;
            }
            if end - start >= 6 {
                ids.push_str(p);
                ids.push_str("REDACTED");
                i = end;
                continue;
            }
        }
        ids.push(chars[i]);
        i += 1;
    }
    // Email addresses.
    let local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let chars: Vec<char> = ids.chars().collect();
    let mut done = String::with_capacity(ids.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            let mut a = i;
            while a > 0 && local(chars[a - 1]) {
                a -= 1;
            }
            let mut b = i + 1;
            while b < chars.len() && (chars[b].is_ascii_alphanumeric() || ".-".contains(chars[b])) {
                b += 1;
            }
            if a < i && b > i + 1 && chars[i + 1..b].contains(&'.') {
                // The local part was already copied; take it back out.
                for _ in a..i {
                    done.pop();
                }
                done.push_str("user@example.com");
                i = b;
                continue;
            }
        }
        done.push(chars[i]);
        i += 1;
    }
    done
}

/// A failure over MCP, in the person's words.
pub fn connect_error(e: &crate::mcp::client::McpError) -> String {
    use crate::mcp::client::McpError;
    match e {
        McpError::Http(h) => match h.status().map(|s| s.as_u16()) {
            Some(401 | 403) => CONNECT_REFUSED.to_string(),
            Some(429) => ApiError::Status { code: 429, message: String::new() }.plain(),
            Some(c) if c >= 500 => ApiError::Status { code: c, message: String::new() }.plain(),
            Some(_) => ApiError::Unreadable.plain(),
            None => ApiError::Network.plain(),
        },
        McpError::Rpc { message, .. } => format!("Composio said no: {}", redact_links(&short_error(message))),
        McpError::Protocol(_) => ApiError::Unreadable.plain(),
    }
}

// ---- Connect mode: apps and sign-ins (CAP-C3, CAP-C4, CAP-C6) ----

/// The mirror row of an app in Connect mode while it has at most one account:
/// the sign-in being made, then the account it became. The card and the toast
/// watch this id, so it stays the same from "waiting" to "connected".
pub fn connect_row_id(app: &str) -> String {
    format!("connect:{app}")
}

/// `CAP-C6`: with two accounts or more, each gets a row of its own,
/// `connect:{app}:{Composio's account id}`, named by its alias.
fn connect_account_row_id(app: &str, account: &str) -> String {
    format!("connect:{app}:{account}")
}

/// The row of a sign-in for one more account, while it is open.
pub fn connect_new_row_id(app: &str) -> String {
    connect_account_row_id(app, NEW_ACCOUNT)
}

const NEW_ACCOUNT: &str = "new";

/// Composio's account id inside a row id of [`connect_account_row_id`]. `None`
/// for the app's own row and for a sign-in still open.
pub fn connect_account_of(row_id: &str, app: &str) -> Option<String> {
    let rest = row_id.strip_prefix(&format!("connect:{app}:"))?;
    (!rest.is_empty() && rest != NEW_ACCOUNT).then(|| rest.to_string())
}

/// Is this one of the app's own Connect rows, of whichever shape?
fn is_connect_row_of(row_id: &str, app: &str) -> bool {
    row_id == connect_row_id(app) || row_id.starts_with(&format!("connect:{app}:"))
}

/// What `COMPOSIO_MANAGE_CONNECTIONS` takes (recorded 2026-10-19): a list of
/// `{name, action}`. `add` is the default and **always makes a new sign-in
/// link and a new account**, up to ten per app, so it is never used to find out
/// how a sign-in is going; `list` has no side effects and is what to ask.
pub fn connections_args(apps: &[String], action: &str) -> Value {
    json!({ "toolkits": apps.iter().map(|a| json!({ "name": a, "action": action })).collect::<Vec<_>>() })
}

/// Take away accounts that never became a sign-in (`initializing`, `failed`),
/// which `add` leaves behind each time a link is not used.
fn remove_args(stale: &[(String, String)]) -> Value {
    json!({ "toolkits": stale.iter().map(|(app, id)| json!({ "name": app, "action": "remove", "account_id": id })).collect::<Vec<_>>() })
}

fn put_status(db: &Db, row: &Connector, app: &str, status: &str) -> Result<(), String> {
    // An old row under another id (from before a change of key) would otherwise
    // sit beside this one. The app's own Connect rows (one per account, or a
    // sign-in for one more) stay: `apply_accounts` keeps those.
    let held = db.list_app_accounts(&row.id).map_err(|e| e.to_string())?;
    for old in held.iter().filter(|a| a.app == app && !is_connect_row_of(&a.id, app)) {
        db.delete_app_account(&old.id).map_err(|e| e.to_string())?;
    }
    let id = connect_row_id(app);
    // The person's name for the account outlives a change of status.
    let label = held.iter().find(|a| a.id == id).and_then(|a| a.label.clone());
    put_row(db, row, &id, app, label, status)
}

fn put_row(db: &Db, row: &Connector, id: &str, app: &str, label: Option<String>, status: &str) -> Result<(), String> {
    db.upsert_app_account(&AppAccount {
        id: id.to_string(),
        connector_id: row.id.clone(),
        app: app.to_string(),
        label,
        status: status.to_string(),
        updated_at: crate::db::now_ms(),
    })
    .map_err(|e| e.to_string())
}

/// Write what Composio said about the apps that were asked about into the
/// mirror. An app that was not asked about is never learned from here; one that
/// was working and now is not reads as expired, which is what it is to the
/// person: a sign-in that stopped.
pub fn apply_connections(db: &Db, row: &Connector, asked: &[String], states: &[ConnState]) -> Result<(), String> {
    let held = db.list_app_accounts(&row.id).map_err(|e| e.to_string())?;
    for s in states.iter().filter(|s| asked.contains(&s.app)) {
        let was_active = held.iter().any(|a| a.app == s.app && a.status.eq_ignore_ascii_case("ACTIVE"));
        // A sign-in for one more account is open: the app is connected (by the
        // accounts it had), and the open link is that sign-in's, so it is kept.
        let extra_open = held.iter().any(|a| a.id == connect_new_row_id(&s.app));
        if s.status == Conn::Gone {
            // Nothing left at Composio: a row that still said ACTIVE would have the
            // model told "already connected" while every call fails.
            for a in held.iter().filter(|a| a.app == s.app) {
                db.delete_app_account(&a.id).map_err(|e| e.to_string())?;
            }
            forget_link(&row.id, &s.app);
            continue;
        }
        let status = match s.status {
            Conn::Active => "ACTIVE",
            Conn::Initiated if was_active => "EXPIRED",
            Conn::Initiated => "INITIATED",
            Conn::Failed => "FAILED",
            Conn::Expired => "EXPIRED",
            Conn::Gone | Conn::Unknown => continue,
        };
        put_status(db, row, &s.app, status)?;
        if extra_open {
            continue;
        }
        match (&s.link, s.status) {
            // The latest address wins: it is the one that works.
            (Some(link), Conn::Initiated) => remember_link(&row.id, &s.app, "", &connect_row_id(&s.app), link),
            // A `list` answer has no address. The one already open stays open.
            (None, Conn::Initiated) => {}
            _ => forget_link(&row.id, &s.app),
        }
    }
    Ok(())
}

/// `CAP-C6`: shape an app's rows to the accounts Composio lists for it, after
/// `apply_connections` has written what the app as a whole is.
///
/// - One working or expired account: the app's own row, named by its alias.
/// - Two or more: a row each, `connect:{app}:{id}`, and no app row.
/// - Accounts that never became a sign-in (`initializing`, `failed`) get no row:
///   the next sign-in clears them away.
///
/// A sign-in for one more account (`connect:{app}:new`) is done when an active
/// account appears that the app did not have when it began; its name passes to
/// that account. One whose link is gone (ten minutes, or a restart) can no longer
/// be finished and goes. A name Poiesis knows is kept when Composio's answer
/// carries none, since where Composio puts the alias has not been recorded.
///
/// Only an answer that lists accounts is read here; one without says nothing
/// about them and leaves the rows as they are.
fn apply_accounts(db: &Db, row: &Connector, asked: &[String], text: &str) -> Result<(), String> {
    let Some(v) = parse_json_lenient(text) else { return Ok(()) };
    let fail = |e: crate::db::DbError| e.to_string();
    for (key, e) in result_entries(&v) {
        let Some(app) = entry_app(e, key) else { continue };
        if !asked.contains(&app) || !has_account_list(e) {
            continue;
        }
        let accounts = listed_in(e);
        let usable: Vec<&Listed> = accounts.iter().filter(|a| matches!(a.status, Conn::Active | Conn::Expired)).collect();
        let held: Vec<AppAccount> = db.list_app_accounts(&row.id).map_err(fail)?.into_iter().filter(|a| a.app == app).collect();
        let own = connect_row_id(&app);
        let extra = connect_new_row_id(&app);

        // The name an account had here, under whichever row it had.
        let mut names: HashMap<String, String> = HashMap::new();
        for h in &held {
            if let (Some(id), Some(label)) = (connect_account_of(&h.id, &app), h.label.clone()) {
                names.insert(id, label);
            }
        }
        if let (Some(only), Some(label)) = (usable.first(), held.iter().find(|h| h.id == own).and_then(|h| h.label.clone())) {
            if usable.len() == 1 {
                names.entry(only.id.clone()).or_insert(label);
            }
        }
        let mut extra_open = false;
        if let Some(open) = held.iter().find(|h| h.id == extra) {
            match recall_link(&row.id, &app) {
                Some(p) => match usable.iter().find(|a| a.status == Conn::Active && !p.before.contains(&a.id)) {
                    Some(done) => {
                        if let Some(label) = open.label.clone() {
                            names.insert(done.id.clone(), label);
                        }
                        db.delete_app_account(&extra).map_err(fail)?;
                        forget_link(&row.id, &app);
                    }
                    None => extra_open = true,
                },
                None => db.delete_app_account(&extra).map_err(fail)?,
            }
        }
        let name_of = |a: &Listed| a.alias.clone().or_else(|| names.get(&a.id).cloned());
        let word = |c: Conn| if c == Conn::Active { "ACTIVE" } else { "EXPIRED" };

        let keep: Vec<String> = if usable.len() >= 2 {
            for a in &usable {
                put_row(db, row, &connect_account_row_id(&app, &a.id), &app, name_of(a), word(a.status))?;
            }
            usable.iter().map(|a| connect_account_row_id(&app, &a.id)).collect()
        } else {
            if let Some(a) = usable.first() {
                let current = held.iter().find(|h| h.id == own);
                // An expired account beside a first sign-in still open: the row is
                // that sign-in's, and the card watching it must not read "failed".
                let status = match current {
                    Some(h) if a.status != Conn::Active && is_waiting(&h.status) => h.status.clone(),
                    _ => word(a.status).to_string(),
                };
                put_row(db, row, &own, &app, name_of(a), &status)?;
            }
            vec![own.clone()]
        };
        for h in &held {
            let stays = keep.contains(&h.id)
                || (h.id == extra && extra_open)
                // The app's row while its first sign-in is open, whatever is listed.
                || (h.id == own && is_waiting(&h.status) && recall_link(&row.id, &app).is_some());
            if is_connect_row_of(&h.id, &app) && h.id != extra && !stays {
                db.delete_app_account(&h.id).map_err(fail)?;
            }
        }
    }
    Ok(())
}

/// Ask Composio whether these apps are signed in, with `list`: it changes nothing,
/// so it can be asked every few seconds while a sign-in is open.
pub async fn check_connect(db: &Db, client: &mut McpClient, apps: &[String]) -> Result<Vec<ConnState>, String> {
    let row = ensure_row(db)?;
    let asked: Vec<String> = apps.iter().filter(|a| valid_slug(a)).cloned().collect();
    if asked.is_empty() {
        return Ok(vec![]);
    }
    let text = client
        .call_tool(MANAGE_CONNECTIONS, connections_args(&asked, "list"))
        .await
        .map_err(|e| connect_error(&e))?;
    let states = parse_connections_of(&text, &asked);
    apply_connections(db, &row, &asked, &states)?;
    apply_accounts(db, &row, &asked, &text)?;
    Ok(states.into_iter().filter(|s| asked.contains(&s.app)).collect())
}

/// Start sign-ins for these apps. `add` makes a new link and a new account every
/// time, so it is asked only for an app that is not connected: first `list`, then
/// clear away the accounts that are not a working sign-in, then `add`.
///
/// A sign-in Poiesis opened in the last ten minutes is handed back as it is. Its
/// account is still `initializing` at Composio, and clearing it would break the
/// page the person may have open in their browser right now (the model asking
/// for the same app twice, or the card and Browse apps both asking).
///
/// An `expired` account is cleared too when a new sign-in replaces it, so a dead
/// account cannot stay Composio's default for the app.
pub async fn start_sign_ins(db: &Db, client: &mut McpClient, apps: &[String]) -> Result<Vec<ConnState>, String> {
    let row = ensure_row(db)?;
    let mut out: Vec<ConnState> = Vec::new();
    let mut apps_left: Vec<String> = Vec::new();
    for app in apps.iter().filter(|a| valid_slug(a)) {
        match reopen_pending(db, app) {
            Some(open) => out.push(ConnState { app: app.clone(), status: Conn::Initiated, link: open.redirect_url }),
            None => apps_left.push(app.clone()),
        }
    }
    let apps = apps_left;
    if apps.is_empty() {
        return Ok(out);
    }
    let listed = client
        .call_tool(MANAGE_CONNECTIONS, connections_args(&apps, "list"))
        .await
        .map_err(|e| connect_error(&e))?;
    let known = parse_connections_of(&listed, &apps);
    apply_connections(db, &row, &apps, &known)?;
    apply_accounts(db, &row, &apps, &listed)?;
    let stale = stale_accounts(&listed, &apps);
    if !stale.is_empty() {
        // Best effort: a refusal here only leaves the clutter where it was.
        let _ = client.call_tool(MANAGE_CONNECTIONS, remove_args(&stale)).await;
    }
    out.extend(known.into_iter().filter(|s| s.status == Conn::Active));
    let needed: Vec<String> = apps.iter().filter(|a| !out.iter().any(|s| &s.app == *a)).cloned().collect();
    if needed.is_empty() {
        return Ok(out);
    }
    let text = client
        .call_tool(MANAGE_CONNECTIONS, connections_args(&needed, "add"))
        .await
        .map_err(|e| connect_error(&e))?;
    let states = parse_connections_of(&text, &needed);
    apply_connections(db, &row, &needed, &states)?;
    out.extend(states.into_iter().filter(|s| needed.contains(&s.app)));
    Ok(out)
}

/// The sign-in for `app` that is already open and still good, with its address.
pub fn reopen_pending(db: &Db, app: &str) -> Option<Started> {
    let row = db.composio_connector().ok().flatten()?;
    let open = db.list_app_accounts(&row.id).ok()?.into_iter().find(|a| a.app == app && is_waiting(&a.status))?;
    let pending = recall_link(&row.id, app)?;
    Some(Started { account: open, redirect_url: Some(pending.url) })
}

fn couldnt_start(app: &str) -> String {
    format!("Composio couldn't start the sign-in for {}.", app_label(app))
}

/// `CAP-C3`: start (or reopen) a sign-in in Connect mode.
pub async fn connect_connect(db: &Db, client: &mut McpClient, app: &str) -> Result<Started, String> {
    if !valid_slug(app) {
        return Err("I don't know that app.".to_string());
    }
    if let Some(open) = reopen_pending(db, app) {
        return Ok(open);
    }
    let row = ensure_row(db)?;
    let states = start_sign_ins(db, client, &[app.to_string()]).await?;
    let state = states.into_iter().find(|s| s.app == app).ok_or_else(|| couldnt_start(app))?;
    let held: Vec<AppAccount> =
        db.list_app_accounts(&row.id).map_err(|e| e.to_string())?.into_iter().filter(|a| a.app == app).collect();
    // With several accounts the row to hand back is the one the answer is about.
    let fits = |a: &&AppAccount| match state.status {
        Conn::Initiated => is_waiting(&a.status),
        _ => a.status.eq_ignore_ascii_case("ACTIVE"),
    };
    let account = held.iter().find(fits).or_else(|| held.first()).cloned().ok_or_else(|| couldnt_start(app))?;
    match (state.status, state.link) {
        (Conn::Active, _) => Ok(Started { account, redirect_url: None }),
        (Conn::Initiated, Some(link)) => Ok(Started { account, redirect_url: Some(link) }),
        _ => Err(couldnt_start(app)),
    }
}

/// `CAP-C6`: sign in to one more account of `app` under the person's name for it
/// (Composio's `alias`). An app with no working account yet gets its first one
/// this way too, named.
///
/// Like `start_sign_ins` it lists first: an open sign-in is handed back, the
/// accounts that never worked are cleared, and only then `add`. The accounts the
/// app had are remembered with the link, so the poll can tell the new one apart.
pub async fn add_account(db: &Db, client: &mut McpClient, app: &str, alias: &str) -> Result<Started, String> {
    if !valid_slug(app) {
        return Err("I don't know that app.".to_string());
    }
    if let Some(open) = reopen_pending(db, app) {
        return Ok(open);
    }
    let row = ensure_row(db)?;
    let asked = [app.to_string()];
    let text = client.call_tool(MANAGE_CONNECTIONS, connections_args(&asked, "list")).await.map_err(|e| connect_error(&e))?;
    let states = parse_connections_of(&text, &asked);
    apply_connections(db, &row, &asked, &states)?;
    apply_accounts(db, &row, &asked, &text)?;
    let accounts: Vec<Listed> = listed(&text, &asked).into_iter().map(|(_, a)| a).collect();
    let working: Vec<&Listed> = accounts.iter().filter(|a| a.status == Conn::Active).collect();
    // Names Poiesis gave that Composio's answer did not carry count too.
    let held = db.list_app_accounts(&row.id).map_err(|e| e.to_string())?;
    let names: Vec<Option<String>> = working
        .iter()
        .map(|a| {
            a.alias.clone().or_else(|| {
                held.iter()
                    .find(|h| connect_account_of(&h.id, app).as_deref() == Some(&a.id) || (working.len() == 1 && h.id == connect_row_id(app)))
                    .and_then(|h| h.label.clone())
            })
        })
        .collect();
    let existing: Vec<Existing<'_>> = names.iter().map(|n| Existing { label: n.as_deref(), status: "ACTIVE" }).collect();
    let short = uuid::Uuid::new_v4().simple().to_string()[..6].to_string();
    let label = plan_label(&app_label(app), &existing, Some(alias), &short)?;

    let stale = stale_accounts(&text, &asked);
    if !stale.is_empty() {
        let _ = client.call_tool(MANAGE_CONNECTIONS, remove_args(&stale)).await;
    }
    let args = json!({ "toolkits": [{ "name": app, "action": "add", "alias": label }] });
    let answer = client.call_tool(MANAGE_CONNECTIONS, args).await.map_err(|e| connect_error(&e))?;
    let parsed = parse_json_lenient(&answer);
    let entry = parsed.as_ref().and_then(|v| result_entries(v).into_iter().find(|(k, e)| entry_app(e, *k).as_deref() == Some(app)).map(|(_, e)| e));
    // The link is read whatever the entry's status says: the app's other
    // accounts make it "active" while this one is only starting.
    let link = entry.and_then(entry_link).or_else(|| parsed.as_ref().and_then(entry_link));
    let Some(link) = link else {
        let why = entry
            .and_then(|e| ["error_message", "error", "message"].iter().find_map(|k| e.get(*k).and_then(|m| m.as_str())))
            .filter(|m| !m.trim().is_empty())
            .map(|m| format!("Composio said no: {}", redact_links(&short_error(m))));
        return Err(why.unwrap_or_else(|| couldnt_start(app)));
    };
    let id = if working.is_empty() { connect_row_id(app) } else { connect_new_row_id(app) };
    put_row(db, &row, &id, app, Some(label.clone()), "INITIATED")?;
    let before = accounts.iter().map(|a| a.id.clone()).collect();
    remember_sign_in(&row.id, app, &label, &id, &link, before);
    let account = db
        .list_app_accounts(&row.id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| couldnt_start(app))?;
    Ok(Started { account, redirect_url: Some(link) })
}

/// `CAP-C6`: give one account of an app a new name, at Composio (`rename`) and
/// here. The app's only account is found by listing.
pub async fn rename_account(db: &Db, client: &mut McpClient, account_id: &str, alias: &str) -> Result<(), String> {
    let row = ensure_row(db)?;
    let held = db.list_app_accounts(&row.id).map_err(|e| e.to_string())?;
    let mine = held.iter().find(|a| a.id == account_id).ok_or("That account isn't one of yours.")?;
    if is_waiting(&mine.status) {
        return Err("Finish signing in first, then name it.".to_string());
    }
    let alias = alias.trim();
    if alias.is_empty() || alias.chars().count() > 64 || alias.chars().any(char::is_control) {
        return Err("A name is up to 64 characters, with nothing unprintable in it.".to_string());
    }
    let app = mine.app.clone();
    if held.iter().any(|a| a.app == app && a.id != account_id && a.label.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(alias))) {
        return Err(format!("You already have a {} account called {alias}.", app_label(&app)));
    }
    let asked = [app.clone()];
    let composio_id = match connect_account_of(account_id, &app) {
        Some(id) => id,
        None => {
            let text = client.call_tool(MANAGE_CONNECTIONS, connections_args(&asked, "list")).await.map_err(|e| connect_error(&e))?;
            let working: Vec<Listed> =
                listed(&text, &asked).into_iter().map(|(_, a)| a).filter(|a| matches!(a.status, Conn::Active | Conn::Expired)).collect();
            match working.as_slice() {
                [one] => one.id.clone(),
                [] => return Err(format!("Composio doesn't list a {} account to name.", app_label(&app))),
                _ => return Err("Composio lists several accounts for this app. Check again, then name the one you mean.".to_string()),
            }
        }
    };
    let args = json!({ "toolkits": [{ "name": app, "action": "rename", "account_id": composio_id, "alias": alias }] });
    let answer = client.call_tool(MANAGE_CONNECTIONS, args).await.map_err(|e| connect_error(&e))?;
    if let Some(v) = parse_json_lenient(&answer) {
        let refused = v.get("successful").and_then(|b| b.as_bool()) == Some(false)
            || result_entries(&v).iter().any(|(_, e)| e.get("status").and_then(|s| s.as_str()).is_some_and(|s| conn_of(s) == Conn::Failed));
        if refused {
            let why = result_entries(&v)
                .iter()
                .find_map(|(_, e)| e.get("error_message").and_then(|m| m.as_str()).map(str::to_string))
                .or_else(|| v.get("error").and_then(|m| m.as_str()).map(str::to_string))
                .unwrap_or_else(|| "it gave no reason".to_string());
            return Err(format!("Composio didn't rename it: {}", redact_links(&short_error(&why))));
        }
    }
    put_row(db, &row, account_id, &app, Some(alias.to_string()), &mine.status)
}

/// `CAP-C6`: remove one account of an app at Composio, then look again.
async fn remove_one(db: &Db, client: &mut McpClient, app: &str, composio_id: &str) -> Result<(), String> {
    let row = ensure_row(db)?;
    let asked = [app.to_string()];
    client
        .call_tool(MANAGE_CONNECTIONS, remove_args(&[(app.to_string(), composio_id.to_string())]))
        .await
        .map_err(|e| connect_error(&e))?;
    let text = client.call_tool(MANAGE_CONNECTIONS, connections_args(&asked, "list")).await.map_err(|e| connect_error(&e))?;
    if listed(&text, &asked).iter().any(|(_, a)| a.id == composio_id) {
        return Err("Composio still lists that account. Try again, or remove it in Composio.".to_string());
    }
    db.delete_app_account(&connect_account_row_id(app, composio_id)).map_err(|e| e.to_string())?;
    let states = parse_connections_of(&text, &asked);
    apply_connections(db, &row, &asked, &states)?;
    apply_accounts(db, &row, &asked, &text)
}

/// Stop listing an app here. Only a row of this install's mirror can go; what
/// Composio holds is taken away by `remove_everywhere`.
pub fn remove_from_list(db: &Db, account_id: &str) -> Result<(), String> {
    let row = ensure_row(db)?;
    let held = db.list_app_accounts(&row.id).map_err(|e| e.to_string())?;
    let Some(account) = held.iter().find(|a| a.id == account_id) else {
        return Err("That account isn't one of yours.".to_string());
    };
    forget_link(&row.id, &account.app);
    db.delete_app_account(account_id).map_err(|e| e.to_string())
}

/// `CAP-C6`: take an app away at Composio. Connect keeps one set of sign-ins for
/// all of a person's AI clients, so this removes every account it holds for `app`,
/// not only the one Poiesis made. It looks again afterwards, and says so if the
/// app is still there: the local row must not be dropped while Composio still
/// lets the app act.
pub async fn remove_everywhere(client: &mut McpClient, app: &str) -> Result<(), String> {
    let asked = [app.to_string()];
    let ask = |action: &'static str| connections_args(&asked, action);
    let listed = client.call_tool(MANAGE_CONNECTIONS, ask("list")).await.map_err(|e| connect_error(&e))?;
    let all: Vec<(String, String)> = listed_accounts(&listed, &asked).into_iter().map(|(a, id, _)| (a, id)).collect();
    if !all.is_empty() {
        client.call_tool(MANAGE_CONNECTIONS, remove_args(&all)).await.map_err(|e| connect_error(&e))?;
    }
    let after = client.call_tool(MANAGE_CONNECTIONS, ask("list")).await.map_err(|e| connect_error(&e))?;
    let left = !listed_accounts(&after, &asked).is_empty()
        || parse_connections_of(&after, &asked).iter().any(|s| s.status == Conn::Active);
    if left {
        return Err(format!("Composio still lists {} as connected. Try again, or remove it in Composio.", app_label(app)));
    }
    Ok(())
}

/// Clear the accounts of `app` that never became a sign-in, so giving up on one
/// does not leave it at Composio counting toward the limit. Best effort.
async fn clear_unused(client: &mut McpClient, app: &str) {
    let asked = [app.to_string()];
    let Ok(listed) = client.call_tool(MANAGE_CONNECTIONS, connections_args(&asked, "list")).await else { return };
    let stale = stale_accounts(&listed, &asked);
    if !stale.is_empty() {
        let _ = client.call_tool(MANAGE_CONNECTIONS, remove_args(&stale)).await;
    }
}

/// `CAP-C4`: an app that answered a call without an error is connected, even if
/// the person signed in to it in Composio for another AI client. `targets` are
/// `(tool slug, app)` pairs for the call that was made.
pub fn learn_from_results(db: &Db, connector_id: &str, output: &str, targets: &[(String, String)]) -> Vec<String> {
    let Some(v) = parse_json_lenient(output) else { return vec![] };
    let Some(results) = v.pointer("/data/results").and_then(|r| r.as_array()) else { return vec![] };
    let Ok(Some(row)) = db.composio_connector() else { return vec![] };
    if row.id != connector_id {
        return vec![];
    }
    let held = db.list_app_accounts(&row.id).unwrap_or_default();
    let mut learned = Vec::new();
    for (i, entry) in results.iter().enumerate() {
        let index = entry.get("index").and_then(|n| n.as_u64()).map(|n| n as usize).unwrap_or(i);
        let slug = entry
            .get("tool_slug")
            .and_then(|s| s.as_str())
            .map(str::to_string)
            .or_else(|| targets.get(index).map(|t| t.0.clone()));
        let Some(slug) = slug else { continue };
        let Some((_, app)) = targets.iter().find(|t| t.0.eq_ignore_ascii_case(&slug)) else { continue };
        if entry_failed(entry) || !valid_slug(app) || app == "composio" || learned.contains(app) {
            continue;
        }
        if held.iter().any(|a| &a.app == app && a.status.eq_ignore_ascii_case("ACTIVE")) {
            continue;
        }
        if put_status(db, &row, app, "ACTIVE").is_ok() {
            forget_link(&row.id, app);
            learned.push(app.clone());
        }
    }
    learned
}

/// `CAP-12`: which targets of a batch reached their app, in the order they were
/// asked. One Composio refused before running it ("Tool ... not found") carries
/// its own `error` and did not; one the app answered with an error (a Google 403)
/// did. An entry is matched by its `index` when that names the same tool, else by
/// its `tool_slug`, else by its place. A target the answer says nothing about, or
/// an answer that can't be read, counts as having run: usage is never shown lower
/// than it might be.
pub fn target_outcomes(output: &str, slugs: &[String]) -> Vec<bool> {
    let mut ran = vec![true; slugs.len()];
    let Some(v) = parse_json_lenient(output) else { return ran };
    let Some(results) = v.pointer("/data/results").and_then(|r| r.as_array()) else { return ran };
    for (place, entry) in results.iter().enumerate() {
        let slug = entry.get("tool_slug").and_then(|s| s.as_str());
        let names = |i: usize| slug.is_none_or(|s| slugs[i].eq_ignore_ascii_case(s));
        let index = entry
            .get("index")
            .and_then(|n| n.as_u64())
            .map(|n| n as usize)
            .filter(|&i| i < slugs.len() && names(i))
            .or_else(|| slug.and_then(|s| slugs.iter().position(|t| t.eq_ignore_ascii_case(s))))
            .or_else(|| (place < slugs.len()).then_some(place));
        let refused = entry.get("error").is_some_and(|x| !x.is_null() && x.as_str() != Some(""));
        if let (Some(i), true) = (index, refused) {
            ran[i] = false;
        }
    }
    ran
}

/// Did one entry of a batch fail? Its own `error`, or the app's reply saying so.
fn entry_failed(e: &Value) -> bool {
    let set = |v: Option<&Value>| v.is_some_and(|x| !x.is_null() && x.as_str() != Some(""));
    if set(e.get("error")) || e.get("successful").and_then(|b| b.as_bool()) == Some(false) {
        return true;
    }
    match e.get("response") {
        Some(r) => set(r.get("error")) || r.get("successful").and_then(|b| b.as_bool()) == Some(false),
        None => false,
    }
}

// ---- one entry point per action, whichever kind of key (CAP-C7) ----

/// Start a sign-in with whichever kind of key is saved.
pub async fn connect_any(db: &Db, http: &reqwest::Client, app: &str, label: Option<&str>) -> Result<Started, String> {
    let key = read_key().ok_or("Add your Composio key first.")?;
    match mode_of(&key) {
        Mode::Project => connect(db, &Api::new(http, &key), app, label).await,
        Mode::Connect => {
            if let Some(open) = reopen_pending(db, app) {
                return Ok(open);
            }
            let mut client = connect_client(http, &key);
            client.initialize().await.map_err(|e| connect_error(&e))?;
            // A name means "one more account, called this" (Add account).
            match label.map(str::trim).filter(|l| !l.is_empty()) {
                Some(label) => add_account(db, &mut client, app, label).await,
                None => connect_connect(db, &mut client, app).await,
            }
        }
    }
}

/// `CAP-C6`: rename one account. Connect only: a project key's accounts are
/// named when they are made.
pub async fn rename_any(db: &Db, http: &reqwest::Client, account_id: &str, label: &str) -> Result<(), String> {
    let key = read_key().ok_or("Add your Composio key first.")?;
    if mode_of(&key) != Mode::Connect {
        return Err("Accounts are named when they're added with this kind of key.".to_string());
    }
    let mut client = connect_client(http, &key);
    client.initialize().await.map_err(|e| connect_error(&e))?;
    rename_account(db, &mut client, account_id, label).await
}

/// Reconnect a sign-in that stopped working.
pub async fn reconnect_any(db: &Db, http: &reqwest::Client, account_id: &str) -> Result<Started, String> {
    let key = read_key().ok_or("Add your Composio key first.")?;
    match mode_of(&key) {
        Mode::Project => reconnect(db, &Api::new(http, &key), account_id).await,
        Mode::Connect => {
            let row = ensure_row(db)?;
            let held = db.list_app_accounts(&row.id).map_err(|e| e.to_string())?;
            let mine = held.iter().find(|a| a.id == account_id).ok_or("That account isn't one of yours.")?;
            // One expired account beside a working one is signed in again as one
            // more account under its old name; the expired one is cleared on the way.
            let others_work = held
                .iter()
                .any(|a| a.app == mine.app && a.id != mine.id && a.status.eq_ignore_ascii_case("ACTIVE"));
            if others_work {
                let name = mine.label.clone().unwrap_or_else(|| "reconnected".to_string());
                let mut client = connect_client(http, &key);
                client.initialize().await.map_err(|e| connect_error(&e))?;
                return add_account(db, &mut client, &mine.app, &name).await;
            }
            let app = mine.app.clone();
            connect_any(db, http, &app, None).await
        }
    }
}

/// Give up on a sign-in that is still open.
pub async fn cancel_any(db: &Db, http: &reqwest::Client, account_id: &str) -> Result<(), String> {
    let key = read_key().ok_or("Add your Composio key first.")?;
    match mode_of(&key) {
        Mode::Project => cancel_signin(db, &Api::new(http, &key), account_id).await,
        Mode::Connect => {
            let row = ensure_row(db)?;
            let open = db
                .list_app_accounts(&row.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|a| a.id == account_id && is_waiting(&a.status));
            if let Some(open) = open {
                let mut client = connect_client(http, &key);
                if client.initialize().await.is_ok() {
                    clear_unused(&mut client, &open.app).await;
                }
                remove_from_list(db, account_id)?;
            }
            Ok(())
        }
    }
}

/// Disconnect: Project revokes the one account at the app, Connect removes every
/// account Composio holds for the app (`remove_everywhere`) and then the local row.
pub async fn disconnect_any(db: &Db, http: &reqwest::Client, account_id: &str) -> Result<(), String> {
    let key = read_key().ok_or("Add your Composio key first.")?;
    match mode_of(&key) {
        Mode::Project => disconnect(db, &Api::new(http, &key), account_id).await,
        Mode::Connect => {
            let row = ensure_row(db)?;
            let held = db.list_app_accounts(&row.id).map_err(|e| e.to_string())?;
            let mine = held.iter().find(|a| a.id == account_id).ok_or("That account isn't one of yours.")?;
            // A sign-in still open is given up, never "every account of the app".
            if is_waiting(&mine.status) {
                return cancel_any(db, http, account_id).await;
            }
            let app = mine.app.clone();
            let mut client = connect_client(http, &key);
            client.initialize().await.map_err(|e| connect_error(&e))?;
            // One of several accounts: only that one goes.
            if let Some(composio_id) = connect_account_of(account_id, &app) {
                return remove_one(db, &mut client, &app, &composio_id).await;
            }
            remove_everywhere(&mut client, &app).await?;
            forget_link(&row.id, &app);
            for a in held.iter().filter(|a| a.app == app) {
                db.delete_app_account(&a.id).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    }
}

/// A new key may be of the other kind. What the mirror holds then belongs to the
/// old one, so it is emptied; the cached tool list is dropped too, since a
/// session's tools and Connect's are asked for separately.
pub fn note_key_kind(db: &Db, mode: Mode) -> Result<(), String> {
    let row = ensure_row(db)?;
    let mut config = config_of(&row);
    let word = match mode {
        Mode::Connect => "connect",
        Mode::Project => "project",
    };
    let before = config.get("mirror_mode").and_then(|m| m.as_str()).map(str::to_string);
    if before.as_deref().is_some_and(|b| b != word) {
        db.replace_app_accounts(&row.id, &[]).map_err(|e| e.to_string())?;
        config.insert("tools".into(), json!([]));
        forget_all_links();
    }
    if before.as_deref() != Some(word) {
        config.insert("mirror_mode".into(), json!(word));
        save_config(db, &row.id, &config)?;
    }
    Ok(())
}

fn catalog_cache() -> &'static Mutex<Option<(String, Instant, Vec<ToolkitInfo>)>> {
    static CACHE: OnceLock<Mutex<Option<(String, Instant, Vec<ToolkitInfo>)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// The catalog (`CAP-8`): one call, cached ten minutes per key. `partial` is
/// true when Composio has more than it sent; the caller then searches.
pub async fn catalog(api: &Api<'_>) -> Result<Vec<ToolkitInfo>, String> {
    let fp = api.fingerprint();
    if let Some((cached_fp, at, list)) = catalog_cache().lock().unwrap().as_ref() {
        if *cached_fp == fp && at.elapsed() < CATALOG_TTL {
            return Ok(list.clone());
        }
    }
    let list = api.toolkits(None).await.map_err(|e| e.plain())?;
    *catalog_cache().lock().unwrap() = Some((fp, Instant::now(), list.clone()));
    Ok(list)
}

// ---- labels (CAP-7) ----

/// What `plan_label` needs to know about each existing sign-in for the app.
pub struct Existing<'a> {
    pub label: Option<&'a str>,
    pub status: &'a str,
}

fn usable(status: &str) -> bool {
    status.eq_ignore_ascii_case("ACTIVE")
}

fn dormant(status: &str) -> bool {
    ["INITIATED", "INITIALIZING", "EXPIRED"].iter().any(|s| status.eq_ignore_ascii_case(s))
}

/// The label a new sign-in gets, or why it cannot be made.
///
/// - A label is 1-64 printable characters and unique per app, ignoring case.
/// - The first sign-in is `default` unless the person types one.
/// - A later one needs a label of the person's choosing.
/// - Retrying when every existing sign-in is still waiting or has expired is
///   not a new account: it gets `retry-<short>` and nothing is deleted, since
///   an expired one may be a grant that used to work.
/// - At most five usable accounts per app.
pub fn plan_label(app_name: &str, existing: &[Existing<'_>], requested: Option<&str>, short: &str) -> Result<String, String> {
    if existing.iter().filter(|e| usable(e.status)).count() >= MAX_ACCOUNTS_PER_APP {
        return Err(format!("You already have {MAX_ACCOUNTS_PER_APP} {app_name} accounts. Disconnect one first."));
    }
    let requested = requested.map(str::trim).filter(|r| !r.is_empty());
    if let Some(label) = requested {
        if label.chars().count() > 64 || label.chars().any(|c| c.is_control()) {
            return Err("A name is up to 64 characters, with nothing unprintable in it.".to_string());
        }
        if existing.iter().any(|e| e.label.is_some_and(|l| l.eq_ignore_ascii_case(label))) {
            return Err(format!("You already have a {app_name} account called {label}."));
        }
        return Ok(label.to_string());
    }
    if existing.is_empty() {
        return Ok("default".to_string());
    }
    if existing.iter().all(|e| dormant(e.status)) {
        return Ok(format!("retry-{short}"));
    }
    Err(format!("Give this {app_name} account a name, like work or personal."))
}

// ---- connecting and disconnecting (CAP-6, CAP-7) ----

/// An app slug is what Composio calls a toolkit: lowercase letters, digits, `_`
/// and `-`. Anything else never reaches a URL or a request body.
pub fn valid_slug(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// The name shown for an app: the curated one, else the slug in words.
pub fn app_label(slug: &str) -> String {
    if let Some(a) = curated_ref().iter().find(|a| a.slug == slug) {
        return a.name.clone();
    }
    if let Some(a) = bundled().iter().find(|a| a.slug == slug) {
        return a.name.clone();
    }
    slug.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A sign-in that was started, and where the person finishes it.
pub struct Started {
    pub account: AppAccount,
    /// Where to sign in. `None` means the app was already connected, so there is
    /// nothing to open (`CAP-C7`).
    pub redirect_url: Option<String>,
}

/// Start a sign-in for `app` under a label (`plan_label` decides it). The
/// account is recorded as `INITIATED` until the person finishes; the address is
/// handed back for the caller to open and goes nowhere else.
///
/// `CAP-C-F1`: a sign-in still open within ten minutes is reopened, not made
/// again, so "Open again" does not leave a trail of half-made accounts. A label
/// held only by a sign-in that never worked may be used again: that one is
/// deleted first.
pub async fn connect(db: &Db, api: &Api<'_>, app: &str, label: Option<&str>) -> Result<Started, String> {
    if !valid_slug(app) {
        return Err("I don't know that app.".to_string());
    }
    let (session, row) = ensure_session(db, api).await?;
    // Labels are unique per app, so look at what Composio holds now. A failure
    // here falls back to what was last seen.
    let _ = sync_accounts(db, api).await;
    let name = app_label(app);
    let mut existing: Vec<AppAccount> = db
        .list_app_accounts(&row.id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|a| a.app == app)
        .collect();

    let requested = label.map(str::trim).filter(|l| !l.is_empty());
    if let Some(p) = recall_link(&row.id, app) {
        let same_label = requested.is_none_or(|l| l.eq_ignore_ascii_case(&p.label));
        if let Some(open) = existing.iter().find(|a| a.id == p.account_id && is_waiting(&a.status)) {
            if same_label {
                return Ok(Started { account: open.clone(), redirect_url: Some(p.url) });
            }
        }
    }
    if let Some(l) = requested {
        let stale: Vec<AppAccount> = existing
            .iter()
            .filter(|a| a.label.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(l)) && !usable(&a.status))
            .cloned()
            .collect();
        for old in stale {
            api.delete_account(&old.id).await.map_err(|e| e.plain())?;
            db.delete_app_account(&old.id).map_err(|e| e.to_string())?;
            forget_link(&row.id, app);
            existing.retain(|a| a.id != old.id);
        }
    }

    let view: Vec<Existing> = existing.iter().map(|a| Existing { label: a.label.as_deref(), status: &a.status }).collect();
    let short = uuid::Uuid::new_v4().simple().to_string()[..4].to_string();
    let alias = plan_label(&name, &view, label, &short)?;
    let link = api.link(&session.session_id, app, &alias).await.map_err(|e| link_error_text(&name, &e))?;
    let account = AppAccount {
        id: link.connected_account_id,
        connector_id: row.id.clone(),
        app: app.to_string(),
        label: Some(alias.clone()),
        status: "INITIATED".to_string(),
        updated_at: crate::db::now_ms(),
    };
    db.upsert_app_account(&account).map_err(|e| e.to_string())?;
    remember_link(&row.id, app, &alias, &account.id, &link.redirect_url);
    Ok(Started { account, redirect_url: Some(link.redirect_url) })
}

/// Is `account_id` one Composio lists under this install's user id? Only such an
/// account may be changed, so a stale or forged id from the frontend cannot
/// reach someone else's.
async fn held_by_me(db: &Db, api: &Api<'_>, account_id: &str) -> Result<AccountRow, String> {
    let row = ensure_row(db)?;
    let user_id = user_id_of(&row).ok_or("The Composio connector has no user id.")?;
    let held = api.accounts(&user_id).await.map_err(|e| e.plain())?;
    held.into_iter()
        .find(|a| a.id == account_id && (a.user_id.is_empty() || a.user_id == user_id))
        .ok_or_else(|| "That account isn't one of yours.".to_string())
}

/// Remove one sign-in, and revoke it at the app.
pub async fn disconnect(db: &Db, api: &Api<'_>, account_id: &str) -> Result<(), String> {
    let mine = held_by_me(db, api, account_id).await?;
    api.delete_account(account_id).await.map_err(|e| e.plain())?;
    if let Some(row) = db.composio_connector().map_err(|e| e.to_string())? {
        forget_link(&row.id, &mine.app);
    }
    db.delete_app_account(account_id).map_err(|e| e.to_string())
}

/// `CAP-C-F1`: a sign-in that stopped working is revoked and made again under
/// its own label, not left to pile up beside a new one.
pub async fn reconnect(db: &Db, api: &Api<'_>, account_id: &str) -> Result<Started, String> {
    let mine = held_by_me(db, api, account_id).await?;
    if usable(&mine.status) {
        return Err("That sign-in is working, so there is nothing to reconnect.".to_string());
    }
    disconnect(db, api, account_id).await?;
    connect(db, api, &mine.app, mine.alias.as_deref()).await
}

/// Stop waiting for a sign-in the person gave up on. Without this it would sit
/// `INITIATED` at Composio, turn `FAILED` later, and show up as a problem.
pub async fn cancel_signin(db: &Db, api: &Api<'_>, account_id: &str) -> Result<(), String> {
    let mine = held_by_me(db, api, account_id).await?;
    if !is_waiting(&mine.status) {
        return Ok(());
    }
    disconnect(db, api, account_id).await
}

/// What the Apps page shows about the connection itself. No secret in it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Health {
    pub tool_count: usize,
    pub checked_at: Option<i64>,
    pub last_error: Option<String>,
}

pub fn health_of(c: &Connector) -> Health {
    let config = config_of(c);
    Health {
        tool_count: config.get("tools").and_then(|t| t.as_array()).map(Vec::len).unwrap_or(0),
        checked_at: config.get("checked_at").and_then(|t| t.as_i64()),
        last_error: config.get("last_error").and_then(|t| t.as_str()).map(str::to_string),
    }
}

/// Remember (or clear) the last thing that went wrong, in the person's words.
pub fn set_last_error(db: &Db, error: Option<&str>) {
    let Ok(row) = ensure_row(db) else { return };
    let mut config = config_of(&row);
    match error {
        Some(e) => config.insert("last_error".into(), json!(e)),
        None => config.remove("last_error"),
    };
    let _ = save_config(db, &row.id, &config);
}

// ---- errors the model can act on (CAP-9) ----

const SCOPE_MARKERS: [&str; 3] = [
    "ACCESS_TOKEN_SCOPE_INSUFFICIENT",
    "insufficient authentication scopes",
    "insufficientPermissions",
];

/// Did a Composio result report a failure? Reads the result as Composio shapes
/// it: `successful: false`, or an `error` on any entry of a batch.
fn failed(result: &Value) -> bool {
    if result.get("successful").and_then(|b| b.as_bool()) == Some(false) {
        return true;
    }
    result
        .pointer("/data/results")
        .and_then(|r| r.as_array())
        .is_some_and(|rs| rs.iter().any(|r| r.get("error").is_some_and(|e| !e.is_null())))
}

/// A note for the model when an app refused a call for want of a permission.
///
/// Only a failed call gets one. Composio's discovery results quote the same
/// words as documentation, and a note there would tell the model something that
/// is not true.
pub fn scope_hint(mode: Mode, app_name: &str, tool_slug: &str, output: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(output).ok()?;
    if !failed(&parsed) || !SCOPE_MARKERS.iter().any(|m| output.contains(m)) {
        return None;
    }
    let slug = tool_slug.to_uppercase();
    let permission = if slug.starts_with("GMAIL_") {
        if ["FORWARD", "SEND_AS", "DELEGATE"].iter().any(|k| slug.contains(k)) {
            "https://www.googleapis.com/auth/gmail.settings.sharing".to_string()
        } else if ["FILTER", "VACATION", "IMAP", "POP"].iter().any(|k| slug.contains(k)) {
            "https://www.googleapis.com/auth/gmail.settings.basic".to_string()
        } else {
            "the permission this action needs".to_string()
        }
    } else {
        "the permission this action needs".to_string()
    };
    Some(match mode {
        // The first real try (2026-10-19) hit this with Google Calendar: the token
        // had no calendar scope. Google's sign-in page lets a person untick
        // permissions, and a Connect account has no auth config to change, so the
        // cure is to disconnect and sign in again, allowing everything.
        Mode::Connect => format!(
            "Poiesis note: {app_name} refused this because the connected account hasn't granted {permission}. \
             The usual cause is a permission left unticked on {app_name}'s sign-in page. The person can fix it by \
             choosing Disconnect for {app_name} under Settings, Apps, then connecting it again and allowing \
             everything it asks for. Tell them this; don't retry."
        ),
        Mode::Project => format!(
            "Poiesis note: {app_name} refused this because the connected account hasn't granted {permission}. \
             Reconnecting won't add it. The person can create their own {app_name} sign-in setup (an auth config) \
             in their Composio project, then reconnect under Settings, Apps. Tell them this; don't retry."
        ),
    })
}

/// A sign-in that Composio cannot start for an app, said in the person's words.
pub fn link_error_text(app_name: &str, err: &ApiError) -> String {
    match err {
        ApiError::Status { code, message }
            if matches!(code, 400 | 404 | 422 | 501)
                && ["auth config", "managed", "not supported", "no auth"]
                    .iter()
                    .any(|k| message.to_lowercase().contains(k)) =>
        {
            format!(
                "{app_name} needs its own sign-in setup in your Composio project. \
                 In Composio, add an auth config for {app_name}, then connect it here."
            )
        }
        other => other.plain(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ---- trust (CAP-T-2) ----

    #[test]
    fn only_https_on_composio_dev_is_followed() {
        assert!(trusted("https://backend.composio.dev/x", None));
        assert!(trusted("https://composio.dev/x", None));
        assert!(!trusted("http://backend.composio.dev/x", None), "plain http is refused");
        assert!(!trusted("https://composio.dev.evil.com/x", None));
        assert!(!trusted("https://evilcomposio.dev/x", None));
        assert!(!trusted("https://evil.example/composio.dev", None));
        assert!(!trusted("not a url", None));
    }

    #[test]
    fn a_stub_host_is_trusted_only_when_named_as_the_stub() {
        assert!(trusted("http://127.0.0.1:9/mcp", Some("http://127.0.0.1:9/api")));
        assert!(!trusted("http://127.0.0.1:10/mcp", Some("http://127.0.0.1:9/api")));
        assert!(!trusted("http://127.0.0.1:9/mcp", None));
    }

    // ---- shapes ----

    #[test]
    fn the_newest_enabled_setup_per_app_wins() {
        let v = json!({ "items": [
            { "id": "ac_old", "status": "ENABLED", "toolkit": { "slug": "gmail" }, "created_at": "2026-01-01T00:00:00Z" },
            { "id": "ac_new", "status": "ENABLED", "toolkit": { "slug": "GMAIL" }, "created_at": "2026-06-01T00:00:00Z" },
            { "id": "ac_off", "status": "DISABLED", "toolkit": { "slug": "slack" }, "created_at": "2026-07-01T00:00:00Z" },
            { "id": "ac_x", "toolkit": { "slug": "twitter" } },
            { "status": "ENABLED" },
        ]});
        let got = newest_enabled(&v);
        assert_eq!(got.get("gmail").map(String::as_str), Some("ac_new"));
        assert_eq!(got.get("twitter").map(String::as_str), Some("ac_x"));
        assert!(!got.contains_key("slack"), "a disabled setup is not used");
    }

    #[test]
    fn accounts_and_toolkits_parse_leniently() {
        let accounts = parse_accounts(&json!({ "items": [
            { "id": "ca_1", "alias": "work", "status": "active", "user_id": "u", "toolkit": { "slug": "Gmail" }, "extra": 1 },
            { "id": "ca_2", "alias": "", "status": "EXPIRED", "toolkit": { "slug": "slack" } },
            { "alias": "no id" },
        ]}));
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].app, "gmail");
        assert_eq!(accounts[0].status, "ACTIVE");
        assert_eq!(accounts[1].alias, None);

        let toolkits = parse_toolkits(&json!({ "items": [
            { "slug": "gmail", "name": "Gmail", "composio_managed_auth_schemes": ["OAUTH2"],
              "meta": { "logo": "https://logos.composio.dev/gmail.png", "description": "Mail",
                        "categories": [{ "name": "Communication", "slug": "communication" }] } },
            { "slug": "twitter", "name": "X", "composio_managed_auth_schemes": [], "meta": {} },
        ]}));
        assert_eq!(toolkits[0].categories, vec!["Communication"]);
        assert!(toolkits[0].managed);
        assert!(!toolkits[1].managed, "no managed sign-in is said on the card");
    }

    #[test]
    fn the_curated_list_is_real() {
        let list = curated();
        assert!(list.len() >= 20, "about two dozen common apps");
        assert!(list.iter().all(|a| !a.slug.is_empty() && !a.name.is_empty()));
        assert!(list.iter().any(|a| a.slug == "gmail"));
    }

    // ---- labels (CAP-T-8) ----

    #[test]
    fn labels_are_unique_limited_and_never_silent() {
        let active = |l| Existing { label: Some(l), status: "ACTIVE" };
        assert_eq!(plan_label("Gmail", &[], None, "ab12").unwrap(), "default");
        assert_eq!(plan_label("Gmail", &[], Some("  work "), "ab12").unwrap(), "work");
        // A duplicate label is refused, ignoring case.
        assert!(plan_label("Gmail", &[active("Work")], Some("work"), "ab12").is_err());
        // A second account must be named.
        assert!(plan_label("Gmail", &[active("default")], None, "ab12").is_err());
        // Too long, or unprintable.
        assert!(plan_label("Gmail", &[], Some(&"x".repeat(65)), "ab12").is_err());
        assert!(plan_label("Gmail", &[], Some("a\u{7}b"), "ab12").is_err());
    }

    #[test]
    fn a_sixth_account_is_refused() {
        let five: Vec<Existing> = ["a", "b", "c", "d", "e"].into_iter().map(|l| Existing { label: Some(l), status: "ACTIVE" }).collect();
        let err = plan_label("Gmail", &five, Some("f"), "ab12").unwrap_err();
        assert!(err.contains("Disconnect one first"), "{err}");
        // Five that are not all usable leaves room.
        let mut four_and_expired = five;
        four_and_expired[4].status = "EXPIRED";
        assert!(plan_label("Gmail", &four_and_expired, Some("f"), "ab12").is_ok());
    }

    #[test]
    fn retrying_an_expired_sign_in_gets_a_retry_label_and_deletes_nothing() {
        let expired = [Existing { label: Some("work"), status: "EXPIRED" }];
        assert_eq!(plan_label("Gmail", &expired, None, "ab12").unwrap(), "retry-ab12");
        let waiting = [Existing { label: Some("default"), status: "INITIATED" }];
        assert_eq!(plan_label("Gmail", &waiting, None, "cd34").unwrap(), "retry-cd34");
    }

    // ---- hints (CAP-T-9) ----

    #[test]
    fn a_failed_call_gets_the_scope_note_and_a_search_result_does_not() {
        let failed_call = json!({
            "successful": false,
            "data": { "results": [{ "tool_slug": "GMAIL_CREATE_FILTER", "error": "ACCESS_TOKEN_SCOPE_INSUFFICIENT: Request had insufficient authentication scopes." }] }
        })
        .to_string();
        let note = scope_hint(Mode::Project, "Gmail", "GMAIL_CREATE_FILTER", &failed_call).expect("a failed call gets a note");
        assert!(note.contains("gmail.settings.basic"), "{note}");
        assert!(note.contains("don't retry"), "{note}");

        let sharing = scope_hint(Mode::Project, "Gmail", "GMAIL_SET_FORWARDING", &failed_call).unwrap();
        assert!(sharing.contains("gmail.settings.sharing"), "{sharing}");

        // Composio's discovery output quotes the same words as documentation.
        let search = json!({
            "successful": true,
            "data": { "results": [{ "description": "Fails with ACCESS_TOKEN_SCOPE_INSUFFICIENT when the scope is missing." }] }
        })
        .to_string();
        assert!(scope_hint(Mode::Project, "Gmail", "COMPOSIO_SEARCH_TOOLS", &search).is_none());
        // Text that is not a result at all is left alone.
        assert!(scope_hint(Mode::Project, "Gmail", "X", "insufficientPermissions").is_none());
    }

    #[test]
    fn a_missing_managed_sign_in_is_said_plainly() {
        let err = ApiError::Status { code: 400, message: "No managed auth config for toolkit twitter".into() };
        let text = link_error_text("X", &err);
        assert!(text.contains("needs its own sign-in setup in your Composio project"), "{text}");
        assert!(!text.contains("No managed auth config"), "Composio's raw hint is not echoed");
    }

    #[test]
    fn errors_never_carry_the_key_or_a_raw_body() {
        for e in [
            ApiError::Status { code: 401, message: "key [key] rejected".into() },
            ApiError::Status { code: 500, message: "boom ak_secret".into() },
            ApiError::Network,
        ] {
            assert!(!e.plain().contains("ak_secret"));
        }
        let refused = ApiError::Status { code: 403, message: String::new() }.plain();
        assert!(refused.starts_with("Composio didn't accept this key."), "{refused}");
        assert!(refused.contains("project API key") && refused.contains("Session management"), "{refused}");
        let said = ApiError::Status { code: 401, message: "Invalid API key".into() }.plain();
        assert!(said.contains("It said: Invalid API key"), "{said}");
    }

    /// Composio's key kinds each have their own header; a user key sent as a
    /// project key is refused, and pasted keys arrive with quotes and spaces.
    #[test]
    fn each_kind_of_key_travels_in_its_own_header_and_is_cleaned() {
        assert_eq!(auth_header("uak_abc"), "x-user-api-key");
        assert_eq!(auth_header("ak_abc"), "x-api-key");
        assert_eq!(auth_header("whatever_a_project_key_looks_like"), "x-api-key");
        assert_eq!(clean_key("  \"ak_abc\"
"), "ak_abc");
        assert_eq!(clean_key("'uak_abc'"), "uak_abc");
        assert_eq!(clean_key("ak_abc"), "ak_abc");
    }

    // ---- the session lifecycle against a stub (CAP-T-1) ----

    #[test]
    fn a_session_is_judged_by_multi_account_and_the_pinned_setups() {
        let custom: BTreeMap<String, String> = [("gmail".to_string(), "ac_1".to_string())].into();
        let good = json!({ "multi_account": { "enable": true }, "auth_configs": { "gmail": "ac_1" } });
        assert_eq!(judge(&good, &custom), Verdict::Keep);
        assert_eq!(judge(&good, &BTreeMap::new()), Verdict::Keep);
        assert_eq!(judge(&json!({ "multi_account": { "enable": false } }), &BTreeMap::new()), Verdict::Recreate);
        assert_eq!(judge(&json!({}), &BTreeMap::new()), Verdict::Recreate, "silence is not multi-account");
        let other = json!({ "multi_account": { "enable": true }, "auth_configs": { "gmail": "ac_OLD" } });
        assert_eq!(judge(&other, &custom), Verdict::Recreate);
    }

    /// A stand-in for Composio: counts session creates, serves a session that is
    /// 404 once `gone` is set, and lets a test choose the multi-account flag and
    /// whether a custom auth config exists.
    struct Stub {
        base: String,
        creates: Arc<AtomicUsize>,
        gone: Arc<std::sync::atomic::AtomicBool>,
        user_ids: Arc<Mutex<Vec<String>>>,
        keys: Arc<Mutex<Vec<String>>>,
        /// Bodies posted to `.../link`.
        links: Arc<Mutex<Vec<Value>>>,
        /// What `GET connected_accounts` returns; a test fills it.
        accounts: Arc<Mutex<Vec<Value>>>,
        /// Ids sent to `DELETE connected_accounts/{id}`.
        deleted: Arc<Mutex<Vec<String>>>,
    }

    async fn stub(multi_account: bool, custom_auth: bool, mcp_host: &'static str) -> Stub {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}/api");
        let creates = Arc::new(AtomicUsize::new(0));
        let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let user_ids = Arc::new(Mutex::new(Vec::new()));
        let keys = Arc::new(Mutex::new(Vec::new()));
        let links = Arc::new(Mutex::new(Vec::new()));
        let accounts = Arc::new(Mutex::new(Vec::<Value>::new()));
        let deleted = Arc::new(Mutex::new(Vec::new()));
        let (c2, g2, u2, k2) = (creates.clone(), gone.clone(), user_ids.clone(), keys.clone());
        let (l2, a2, d2) = (links.clone(), accounts.clone(), deleted.clone());
        let mcp = if mcp_host.is_empty() { format!("http://{addr}/mcp") } else { mcp_host.to_string() };
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let (c, g, u, k, mcp) = (c2.clone(), g2.clone(), u2.clone(), k2.clone(), mcp.clone());
                let (links, accounts, deleted) = (l2.clone(), a2.clone(), d2.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    let (head_end, len, head) = loop {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..p]).to_string();
                            let len = head
                                .to_lowercase()
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                                .unwrap_or(0);
                            break (p + 4, len, head);
                        }
                    };
                    while buf.len() < head_end + len {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let body: Value = serde_json::from_slice(&buf[head_end..head_end + len]).unwrap_or(Value::Null);
                    let request_line = head.lines().next().unwrap_or("").to_string();
                    // Which header carried the key, then the key: "x-user-api-key=uak_...".
                    for name in ["x-api-key", "x-user-api-key"] {
                        let prefix = format!("{name}:");
                        if let Some(key) = head
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix(&prefix).map(|v| v.trim().to_string()))
                        {
                            k.lock().unwrap().push(if name == "x-api-key" { key } else { format!("user:{key}") });
                        }
                    }
                    let session = |multi: bool, with_auth: bool| {
                        let mut config = json!({ "user_id": "u", "multi_account": { "enable": multi } });
                        if with_auth {
                            config["auth_configs"] = json!({ "gmail": "ac_custom" });
                        }
                        json!({ "session_id": "sess_1", "mcp": { "type": "http", "url": mcp }, "config": config })
                    };
                    let (status, out) = if request_line.starts_with("POST /api/tool_router/session ") {
                        c.fetch_add(1, Ordering::SeqCst);
                        if let Some(uid) = body.get("user_id").and_then(|s| s.as_str()) {
                            u.lock().unwrap().push(uid.to_string());
                        }
                        ("200 OK", session(true, body.get("auth_configs").is_some()))
                    } else if request_line.starts_with("GET /api/tool_router/session/sess_1 ") {
                        if g.load(Ordering::SeqCst) {
                            ("404 Not Found", json!({ "error": { "message": "gone" } }))
                        } else {
                            ("200 OK", session(multi_account, false))
                        }
                    } else if request_line.starts_with("POST /api/tool_router/session/sess_1/link ") {
                        links.lock().unwrap().push(body.clone());
                        (
                            "200 OK",
                            json!({
                                "link_token": "lt_1",
                                "redirect_url": mcp.replace("/mcp", "/connect/xyz"),
                                "connected_account_id": format!("ca_new{}", links.lock().unwrap().len()),
                            }),
                        )
                    } else if request_line.starts_with("GET /api/connected_accounts") {
                        ("200 OK", json!({ "items": accounts.lock().unwrap().clone() }))
                    } else if request_line.starts_with("DELETE /api/connected_accounts/") {
                        let id = request_line
                            .trim_start_matches("DELETE /api/connected_accounts/")
                            .split(['?', ' '])
                            .next()
                            .unwrap_or("")
                            .to_string();
                        accounts.lock().unwrap().retain(|a| a["id"] != id.as_str());
                        deleted.lock().unwrap().push(id);
                        ("200 OK", json!({ "success": true }))
                    } else if request_line.starts_with("GET /api/auth_configs") {
                        let items = if custom_auth {
                            json!([{ "id": "ac_custom", "status": "ENABLED", "toolkit": { "slug": "gmail" } }])
                        } else {
                            json!([])
                        };
                        ("200 OK", json!({ "items": items }))
                    } else {
                        ("404 Not Found", json!({ "error": { "message": "no route" } }))
                    };
                    let text = out.to_string();
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                        text.len()
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                });
            }
        });
        Stub { base, creates, gone, user_ids, keys, links, accounts, deleted }
    }

    fn held(id: &str, app: &str, alias: &str, status: &str, user_id: &str) -> Value {
        json!({ "id": id, "alias": alias, "status": status, "user_id": user_id, "toolkit": { "slug": app } })
    }

    #[tokio::test]
    async fn a_session_is_created_reused_and_recreated_with_the_same_user_id() {
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);

        let (first, row) = ensure_session(&db, &api).await.unwrap();
        assert_eq!(stub.creates.load(Ordering::SeqCst), 1);
        let user_id = user_id_of(&row).unwrap();
        assert!(user_id.starts_with("poiesis_"));

        // Reused while it still exists.
        let (again, _) = ensure_session(&db, &api).await.unwrap();
        assert_eq!(again, first);
        assert_eq!(stub.creates.load(Ordering::SeqCst), 1, "no second create");

        // Recreated on a 404, under the same user id.
        stub.gone.store(true, Ordering::SeqCst);
        ensure_session(&db, &api).await.unwrap();
        assert_eq!(stub.creates.load(Ordering::SeqCst), 2);
        assert_eq!(stub.user_ids.lock().unwrap().as_slice(), [user_id.clone(), user_id.clone()]);
        assert!(stub.keys.lock().unwrap().iter().all(|k| k == "ak_test"), "the key travels in the header");
    }

    #[tokio::test]
    async fn a_session_without_multi_account_is_recreated_once_per_boot_not_in_a_loop() {
        let stub = stub(false, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);

        ensure_session(&db, &api).await.unwrap();
        // The session exists but reports multi-account off: one upgrade.
        ensure_session(&db, &api).await.unwrap();
        assert_eq!(stub.creates.load(Ordering::SeqCst), 2);
        // The stub still says off. A third look does not make a third session.
        ensure_session(&db, &api).await.unwrap();
        ensure_session(&db, &api).await.unwrap();
        assert_eq!(stub.creates.load(Ordering::SeqCst), 2, "no loop");
    }

    #[tokio::test]
    async fn a_new_custom_sign_in_setup_recreates_the_session_with_it_pinned() {
        let stub = stub(true, true, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);
        ensure_session(&db, &api).await.unwrap();
        // The first create already pinned it; the stub's GET reports none pinned,
        // so one upgrade follows, then it settles.
        ensure_session(&db, &api).await.unwrap();
        ensure_session(&db, &api).await.unwrap();
        assert_eq!(stub.creates.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_address_off_composio_is_refused() {
        let stub = stub(true, false, "https://evil.example/mcp").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);
        let err = ensure_session(&db, &api).await.unwrap_err();
        assert!(err.contains("don't trust"), "{err}");
    }

    // ---- connecting and disconnecting ----

    #[test]
    fn slugs_and_labels_are_safe_to_show_and_send() {
        assert!(valid_slug("gmail") && valid_slug("microsoft_teams") && valid_slug("one-drive"));
        assert!(!valid_slug("") && !valid_slug("Gmail") && !valid_slug("a/b") && !valid_slug("a b") && !valid_slug("a?b=1"));
        assert_eq!(app_label("googlecalendar"), "Google Calendar");
        assert_eq!(app_label("some_new-app"), "Some New App");
    }

    #[tokio::test]
    async fn connecting_names_the_account_records_it_and_hands_back_only_a_composio_address() {
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);

        let started = connect(&db, &api, "gmail", None).await.unwrap();
        assert_eq!(started.account.label.as_deref(), Some("default"));
        assert_eq!(started.account.status, "INITIATED");
        assert!(started.redirect_url.as_deref().unwrap().ends_with("/connect/xyz"));
        let sent = stub.links.lock().unwrap().clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["toolkit"], "gmail");
        assert_eq!(sent[0]["alias"], "default");
        let row = ensure_row(&db).unwrap();
        let mirror = db.list_app_accounts(&row.id).unwrap();
        assert_eq!(mirror.len(), 1);
        assert_eq!(mirror[0].id, started.account.id);

        // The same app's second account must be named, and a bad slug never goes out.
        let user_id = user_id_of(&row).unwrap();
        stub.accounts.lock().unwrap().push(held(&started.account.id, "gmail", "default", "ACTIVE", &user_id));
        let err = connect(&db, &api, "gmail", None).await.err().expect("a second account needs a name");
        assert!(err.contains("Give this Gmail account a name"), "{err}");
        let again = connect(&db, &api, "gmail", Some("work")).await.unwrap();
        assert_eq!(again.account.label.as_deref(), Some("work"));
        assert!(connect(&db, &api, "../etc", None).await.is_err());
        assert_eq!(stub.links.lock().unwrap().len(), 2, "the bad slug made no request");
    }

    #[tokio::test]
    async fn disconnecting_removes_only_an_account_composio_lists_under_this_user() {
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);
        let (_, row) = ensure_session(&db, &api).await.unwrap();
        let user_id = user_id_of(&row).unwrap();
        stub.accounts.lock().unwrap().extend([
            held("ca_mine", "slack", "work", "ACTIVE", &user_id),
            held("ca_theirs", "slack", "other", "ACTIVE", "someone_else"),
        ]);
        sync_accounts(&db, &api).await.unwrap();
        assert_eq!(db.list_app_accounts(&row.id).unwrap().len(), 1, "another user's account is not mirrored");

        let err = disconnect(&db, &api, "ca_theirs").await.unwrap_err();
        assert!(err.contains("isn't one of yours"), "{err}");
        assert!(disconnect(&db, &api, "ca_unknown").await.is_err());
        assert!(stub.deleted.lock().unwrap().is_empty(), "nothing was deleted for a stranger");

        disconnect(&db, &api, "ca_mine").await.unwrap();
        assert_eq!(stub.deleted.lock().unwrap().as_slice(), ["ca_mine"]);
        assert!(db.list_app_accounts(&row.id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_user_key_is_sent_as_a_user_key() {
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "uak_user_key", &stub.base);
        ensure_session(&db, &api).await.unwrap();
        let seen = stub.keys.lock().unwrap().clone();
        assert!(!seen.is_empty() && seen.iter().all(|k| k == "user:uak_user_key"), "{seen:?}");
    }

    /// `CAP-T-10`: the key is a header on requests to Composio and nowhere else.
    /// After a session, a sign-in and an account sync, nothing stored holds it,
    /// and nothing in this module prints.
    #[tokio::test]
    async fn the_key_is_stored_nowhere_and_never_printed() {
        const SECRET: &str = "ak_SECRET_KEY_9f3a";
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, SECRET, &stub.base);
        let started = connect(&db, &api, "gmail", Some("work")).await.unwrap();
        sync_accounts(&db, &api).await.unwrap();
        set_last_error(&db, Some(&ApiError::Status { code: 401, message: "rejected [key]".into() }.plain()));

        let row = db.composio_connector().unwrap().unwrap();
        let everything = format!(
            "{:?} {:?} {}",
            db.list_connectors().unwrap(),
            db.list_app_accounts(&row.id).unwrap(),
            started.account.id
        );
        assert!(!everything.contains(SECRET), "the key reached storage: {everything}");
        let seen = stub.keys.lock().unwrap().clone();
        assert!(!seen.is_empty() && seen.iter().all(|k| k.eq_ignore_ascii_case(SECRET)), "it travelled in the header");

        // No line of this module prints, so no log can carry it.
        let source = include_str!("composio.rs");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        for macro_name in ["println!", "eprintln!", "dbg!", "tracing::", "log::"] {
            assert!(!production.contains(macro_name), "composio.rs must not use {macro_name}");
        }
    }

    #[test]
    fn health_reports_the_count_the_time_and_the_last_error_and_clears_it() {
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        assert_eq!(health_of(&row).tool_count, 0);
        set_last_error(&db, Some("I couldn't reach Composio."));
        assert_eq!(health_of(&db.composio_connector().unwrap().unwrap()).last_error.as_deref(), Some("I couldn't reach Composio."));
        set_last_error(&db, None);
        assert!(health_of(&db.composio_connector().unwrap().unwrap()).last_error.is_none());
    }

    // ================= Phase 6: Composio Connect (CAP-C) =================

    use crate::connectors::stub::{connect_stub, connections_answer, Reply};

    /// Point Connect mode at a stub for this thread, and say a `ck_` key is saved.
    fn connect_mode(url: &str) {
        tests_pretend::key(true);
        tests_pretend::mode(Mode::Connect);
        tests_pretend::connect_url(Some(url));
    }

    fn reset_pretend() {
        tests_pretend::key(false);
        tests_pretend::mode(Mode::Project);
        tests_pretend::connect_url(None);
    }

    fn link_for(app: &str) -> String {
        format!("https://connect.composio.dev/link/ln_{app}12345")
    }

    /// `CAP-C-T1`: the kind of key picks the mode and the header.
    #[test]
    fn the_kind_of_key_picks_the_mode_and_the_header() {
        assert_eq!(mode_of("ck_abc"), Mode::Connect);
        assert_eq!(mode_of("ak_abc"), Mode::Project);
        assert_eq!(mode_of("uak_abc"), Mode::Project);
        assert_eq!(mode_of("whatever"), Mode::Project);
        assert_eq!(auth_header("ck_abc"), "x-consumer-api-key");
        assert_eq!(auth_header("ak_abc"), "x-api-key");
        assert_eq!(auth_header("uak_abc"), "x-user-api-key");
        assert_eq!(clean_key(" \"ck_abc\" "), "ck_abc");
    }

    /// `CAP-C-T1`: a Connect client sends exactly the consumer header to the
    /// Connect address, and makes no REST call.
    #[tokio::test]
    async fn a_connect_client_sends_the_consumer_header_and_nothing_is_rest() {
        let stub = connect_stub(|_, _| Reply::Default).await;
        connect_mode(&stub.url);
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        let tools = client.discover().await.unwrap();
        assert!(tools.iter().any(|t| t.name == MULTI_EXECUTE));
        let keys = stub.keys.lock().unwrap().clone();
        assert!(!keys.is_empty() && keys.iter().all(|k| k == "ck_test"), "{keys:?}");
        assert!(stub.paths.lock().unwrap().iter().all(|p| p == "/mcp"), "only the MCP address was spoken to");
        reset_pretend();
    }

    /// `CAP-C-T2`: a refusal reads as the `ck_` sentence, and a key whose tool
    /// list lacks the batch runner is not accepted.
    #[tokio::test]
    async fn a_connect_key_is_checked_over_mcp() {
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();

        let refusing = connect_stub(|_, _| Reply::Default).await;
        *refusing.refuse.lock().unwrap() = Some(401);
        connect_mode(&refusing.url);
        let err = validate_key(&db, &http, "ck_bad").await.unwrap_err();
        assert!(err.contains("starts with ck_") && err.contains("Sessions & API Key"), "{err}");

        let thin = connect_stub(|_, _| Reply::Default).await;
        *thin.tools.lock().unwrap() = vec![SEARCH_TOOLS.to_string()];
        connect_mode(&thin.url);
        let err = validate_key(&db, &http, "ck_thin").await.unwrap_err();
        assert!(err.contains("didn't offer the tools"), "{err}");

        let good = connect_stub(|_, _| Reply::Default).await;
        connect_mode(&good.url);
        validate_key(&db, &http, "ck_good").await.unwrap();
        assert!(db.composio_connector().unwrap().is_none(), "checking a Connect key stores nothing");
        reset_pretend();
    }

    /// `CAP-C-T3`: the answer is read leniently, whatever it calls its fields.
    #[test]
    fn connections_are_read_leniently_from_every_plausible_shape() {
        let link = link_for("gmail");
        // Keyed by app, status word, link under a "redirect_url" field.
        let keyed = connections_answer(json!({
            "gmail": { "status": "INITIATED", "redirect_url": link, "logo": "https://logos.composio.dev/gmail.png" },
            "slack": { "connection_status": "active" },
        }));
        let got = parse_connections(&keyed);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], ConnState { app: "gmail".into(), status: Conn::Initiated, link: Some(link.clone()) });
        assert_eq!(got[1], ConnState { app: "slack".into(), status: Conn::Active, link: None });

        // A list, the app named by a field, a link and no status word at all.
        let list = connections_answer(json!([{ "toolkit": "Notion", "connect_url": link }]));
        let got = parse_connections(&list);
        assert_eq!(got[0].app, "notion");
        assert_eq!(got[0].status, Conn::Initiated, "a link with no status means a sign-in is waiting");

        // A boolean, and an account nested under the entry.
        let flags = connections_answer(json!({
            "linear": { "is_active": true },
            "github": { "accounts": [{ "status": "EXPIRED" }, { "status": "ACTIVE" }] },
            "asana": { "accounts": [{ "status": "EXPIRED" }] },
        }));
        let got = parse_connections(&flags);
        let by = |app: &str| got.iter().find(|s| s.app == app).unwrap().status;
        assert_eq!(by("linear"), Conn::Active);
        assert_eq!(by("github"), Conn::Active);
        assert_eq!(by("asana"), Conn::Expired);

        // An active app has no link to hand out, even if one is sent.
        let active_with_link = connections_answer(json!({ "gmail": { "status": "ACTIVE", "redirect_url": link } }));
        assert_eq!(parse_connections(&active_with_link)[0].link, None);
    }

    #[test]
    fn a_connections_answer_that_cannot_be_trusted_is_ignored() {
        // No results, not JSON, an untrusted host, a picture, an invalid slug.
        assert!(parse_connections("").is_empty());
        assert!(parse_connections("sorry, no").is_empty());
        assert!(parse_connections(r#"{"successful":true,"data":{"message":"ok"}}"#).is_empty());
        let off_site = connections_answer(json!({ "gmail": { "redirect_url": "https://evil.example/link/ln_x" } }));
        let got = parse_connections(&off_site);
        assert_eq!(got[0].link, None, "an address off Composio's domain is not a sign-in");
        assert_eq!(got[0].status, Conn::Unknown);
        let only_logo = connections_answer(json!({ "gmail": { "logo": "https://logos.composio.dev/link/gmail.png" } }));
        assert_eq!(parse_connections(&only_logo)[0].link, None, "a picture is never the link");
        let bad_slug = connections_answer(json!({ "../etc": { "status": "ACTIVE" }, "Gmail Pro": { "status": "ACTIVE" } }));
        assert!(parse_connections(&bad_slug).is_empty());
        // Words around the JSON do not hide it.
        let wrapped = format!("Here you go:\n{}\nThanks", connections_answer(json!({ "gmail": { "status": "ACTIVE" } })));
        assert_eq!(parse_connections(&wrapped)[0].status, Conn::Active);
    }

    /// A finished sign-in must be seen even when Composio names the entry's fields
    /// something unexpected: the documented summary counts decide it.
    #[test]
    fn a_finished_sign_in_is_read_from_the_summary_when_the_entry_is_not_clear() {
        let answer = |entry: Value, summary: Value| {
            json!({ "successful": true, "data": { "results": { "googlecalendar": entry }, "summary": summary } }).to_string()
        };
        let done = answer(json!({ "message": "all set" }), json!({ "active_connections": 1 }));
        assert_eq!(parse_connections(&done)[0].status, Conn::Active);
        let waiting = answer(json!({ "message": "sign in" }), json!({ "initiated_connections": 1 }));
        assert_eq!(parse_connections(&waiting)[0].status, Conn::Initiated);
        let failed = answer(json!({}), json!({ "failed_connections": 1 }));
        assert_eq!(parse_connections(&failed)[0].status, Conn::Failed);
        // An entry that says something itself is not overruled.
        let own = answer(json!({ "status": "INITIATED" }), json!({ "active_connections": 1 }));
        assert_eq!(parse_connections(&own)[0].status, Conn::Initiated);
        // Two apps in one answer: the counts cannot say which is which.
        let two = json!({ "data": { "results": { "gmail": {}, "slack": {} }, "summary": { "active_connections": 1 } } });
        assert!(parse_connections(&two.to_string()).iter().all(|s| s.status == Conn::Unknown));
    }

    #[test]
    fn a_display_name_does_not_hide_the_apps_slug() {
        let answer = json!({ "data": { "results": { "googlecalendar": { "name": "Google Calendar", "status": "ACTIVE" } } } });
        let got = parse_connections(&answer.to_string());
        assert_eq!(got[0].app, "googlecalendar");
        assert_eq!(got[0].status, Conn::Active);
    }

    #[test]
    fn a_single_app_asked_about_is_answered_by_the_summary_even_with_no_entries() {
        let asked = vec!["googlecalendar".to_string()];
        let no_entries = json!({ "data": { "results": {}, "summary": { "active_connections": 1 } } }).to_string();
        let got = parse_connections_of(&no_entries, &asked);
        assert_eq!(got, vec![ConnState { app: "googlecalendar".into(), status: Conn::Active, link: None }]);
        // Two apps asked: nothing is guessed.
        let two = vec!["gmail".to_string(), "slack".to_string()];
        assert!(parse_connections_of(&no_entries, &two).is_empty());
    }

    #[test]
    fn the_bundled_catalog_is_whole_and_every_logo_it_names_exists() {
        let apps = bundled();
        assert!(apps.len() > 1500, "the whole list ships: {}", apps.len());
        for slug in ["gmail", "googlecalendar", "slack", "notion", "github"] {
            let a = apps.iter().find(|a| a.slug == slug).unwrap_or_else(|| panic!("{slug} missing"));
            assert!(valid_slug(&a.slug) && !a.name.is_empty() && !a.categories.is_empty());
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("public").join("app-logos");
        for a in apps.iter().filter(|a| a.logo.is_some()) {
            assert!(dir.join(format!("{}.svg", a.slug)).exists(), "{} names a logo that is not there", a.slug);
        }
        assert!(apps.iter().all(|a| valid_slug(&a.slug)), "every slug can be asked for");
        assert_eq!(app_label("googlecalendar"), "Google Calendar");
        assert_eq!(app_label("active_campaign"), "ActiveCampaign", "a bundled name beats the slug in words");
    }

    /// `CAP-C-T5`.
    #[test]
    fn sign_in_links_are_redacted_and_other_text_is_left_alone() {
        let mask = "[sign-in link shown to the person]";
        assert_eq!(
            redact_links("Open https://connect.composio.dev/link/ln_abc123 to continue."),
            format!("Open {mask} to continue.")
        );
        // Inside a JSON string, escaped and not.
        let json_text = r#"{"redirect_url":"https:\/\/connect.composio.dev\/link\/ln_abc123","n":1}"#;
        let out = redact_links(json_text);
        assert!(!out.contains("ln_abc123") && out.contains(mask), "{out}");
        assert!(serde_json::from_str::<Value>(&out).is_ok(), "still valid JSON: {out}");
        // Composio's ordinary pages, and other hosts, stay.
        for keep in [
            "See https://docs.composio.dev/docs/composio-connect for more.",
            "Visit https://evil.example/link/ln_abc123 now",
            "https://composio.dev/pricing",
        ] {
            assert_eq!(redact_links(keep), keep);
        }
        // More than one, with punctuation after.
        let two = redact_links("a https://backend.composio.dev/api/link/ln_1abcdef, b https://connect.composio.dev/link/ln_2abcdef.");
        assert_eq!(two, format!("a {mask}, b {mask}."));
        assert_eq!(redact_links("no links here"), "no links here");
    }

    #[test]
    fn a_recording_is_scrubbed_of_keys_links_ids_and_emails() {
        let raw = r#"{"key":"ck_SECRET1","url":"https://connect.composio.dev/link/ln_realtoken99","id":"ca_AbCdEf123","mail":"erich@example.org","keep":"https://docs.composio.dev/x","word":"ca_x"}"#;
        let out = scrub_for_fixture(raw, "ck_SECRET1");
        assert!(!out.contains("SECRET1") && !out.contains("realtoken99") && !out.contains("AbCdEf123") && !out.contains("erich@"), "{out}");
        assert!(out.contains("ln_REDACTED") && out.contains("ca_REDACTED") && out.contains("user@example.com"), "{out}");
        assert!(out.contains("https://docs.composio.dev/x"), "ordinary pages stay");
        assert!(out.contains("\"ca_x\""), "a short word is not an id");
        assert!(serde_json::from_str::<Value>(&out).is_ok(), "still valid JSON: {out}");
    }

    /// A stand-in for Composio's accounts that follows the rules the recording
    /// showed: `add` makes a new account and link each time, ten is the most an app
    /// can hold, `list` changes nothing, `remove` deletes one.
    type Held = std::sync::Arc<Mutex<(Vec<(String, String, String, String)>, u32)>>;

    fn world(initial: &[(&str, &str)]) -> (Held, impl Fn(&str, &Value) -> Reply + Send + Sync + 'static) {
        let state: Held = std::sync::Arc::new(Mutex::new((
            initial
                .iter()
                .enumerate()
                .map(|(i, (app, status))| (app.to_string(), format!("{app}_old{i}"), status.to_string(), String::new()))
                .collect(),
            0,
        )));
        let held = state.clone();
        let answer = move |_tool: &str, args: &Value| {
            let mut st = state.lock().unwrap();
            let accounts_of = |st: &(Vec<(String, String, String, String)>, u32), app: &str| -> Vec<Value> {
                st.0.iter()
                    .filter(|(a, ..)| a == app)
                    .map(|(_, id, status, alias)| match alias.as_str() {
                        "" => json!({ "id": id, "status": status }),
                        _ => json!({ "id": id, "status": status, "alias": alias }),
                    })
                    .collect()
            };
            let mut results = serde_json::Map::new();
            for op in args["toolkits"].as_array().expect("the documented list of {name, action}") {
                let app = op["name"].as_str().expect("a name").to_string();
                match op["action"].as_str().unwrap_or("add") {
                    "remove" => {
                        let id = op["account_id"].as_str().unwrap().to_string();
                        st.0.retain(|(a, i, ..)| !(a == &app && i == &id));
                    }
                    "rename" => {
                        let id = op["account_id"].as_str().unwrap().to_string();
                        let alias = op["alias"].as_str().unwrap().to_string();
                        for acc in st.0.iter_mut().filter(|(a, i, ..)| a == &app && i == &id) {
                            acc.3 = alias.clone();
                        }
                        let accounts = accounts_of(&st, &app);
                        results.insert(app.clone(), json!({ "toolkit": app, "status": "success", "accounts": accounts }));
                    }
                    "add" => {
                        if st.0.iter().filter(|(a, ..)| a == &app).count() >= 10 {
                            let accounts = accounts_of(&st, &app);
                            results.insert(
                                app.clone(),
                                json!({ "toolkit": app, "status": "failed",
                                    "error_message": "Maximum of 10 accounts per toolkit reached.", "accounts": accounts }),
                            );
                        } else {
                            st.1 += 1;
                            let id = format!("{app}_new{}", st.1);
                            let alias = op["alias"].as_str().unwrap_or("").to_string();
                            st.0.push((app.clone(), id, "initializing".into(), alias));
                            let accounts = accounts_of(&st, &app);
                            results.insert(
                                app.clone(),
                                json!({ "toolkit": app, "status": "initiated",
                                    "redirect_url": link_for(&app), "accounts": accounts }),
                            );
                        }
                    }
                    _ => {
                        let accounts = accounts_of(&st, &app);
                        results.insert(app.clone(), json!({ "toolkit": app, "accounts": accounts }));
                    }
                }
            }
            Reply::Text(json!({ "successful": true, "data": { "message": "ok", "results": results } }).to_string())
        };
        (held, answer)
    }

    fn actions(stub: &crate::connectors::stub::ConnectStub) -> Vec<String> {
        stub.calls
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(_, a)| {
                a["toolkits"].as_array().unwrap().iter().map(|o| o["action"].as_str().unwrap().to_string()).collect::<Vec<_>>()
            })
            .collect()
    }

    /// `CAP-C-T7`: "Open again" within ten minutes reuses the link and asks
    /// Composio nothing more. The first ask looks, then adds once.
    #[tokio::test]
    async fn an_open_sign_in_is_reopened_not_asked_for_again() {
        let (held, answer) = world(&[]);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();

        let first = connect_connect(&db, &mut client, "gmail").await.unwrap();
        assert_eq!(first.redirect_url.as_deref(), Some(link_for("gmail").as_str()));
        assert_eq!(first.account.id, "connect:gmail");
        assert_eq!(first.account.status, "INITIATED");
        assert!(first.account.label.is_none(), "Connect has no labels");
        assert_eq!(actions(&stub), ["list", "add"]);

        let again = connect_connect(&db, &mut client, "gmail").await.unwrap();
        assert_eq!(again.redirect_url, first.redirect_url);
        assert_eq!(actions(&stub), ["list", "add"], "no second call of any kind");

        // After the ten minutes (or a restart) the half-made account is cleared
        // and one fresh sign-in made: never a pile of them.
        let row = db.composio_connector().unwrap().unwrap();
        assert!(recall_link_within(&row.id, "gmail", Duration::ZERO).is_none(), "an old link is gone");
        connect_connect(&db, &mut client, "gmail").await.unwrap();
        assert_eq!(held.lock().unwrap().0.len(), 1, "the old one was removed before the new one was added");
        reset_pretend();
    }

    /// The fault the first real try found: the sign-in card checked every three
    /// seconds with `add`, which made a new account each time until Composio
    /// refused at ten. Checking must change nothing, and must see the sign-in finish.
    #[tokio::test]
    async fn checking_a_sign_in_makes_no_accounts_and_sees_it_finish() {
        let (held, answer) = world(&[]);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        connect_connect(&db, &mut client, "googlecalendar").await.unwrap();
        for _ in 0..12 {
            check_connect(&db, &mut client, &["googlecalendar".into()]).await.unwrap();
        }
        assert_eq!(held.lock().unwrap().0.len(), 1, "twelve checks, still the one account");
        assert!(actions(&stub)[2..].iter().all(|a| a == "list"), "checking only lists: {:?}", actions(&stub));
        let row = db.composio_connector().unwrap().unwrap();
        assert_eq!(db.list_app_accounts(&row.id).unwrap()[0].status, "INITIATED");
        assert!(recall_link(&row.id, "googlecalendar").is_some(), "checking does not lose the open link");

        // The person signs in; the next check sees it.
        held.lock().unwrap().0[0].2 = "active".into();
        check_connect(&db, &mut client, &["googlecalendar".into()]).await.unwrap();
        assert_eq!(db.list_app_accounts(&row.id).unwrap()[0].status, "ACTIVE");
        assert!(recall_link(&row.id, "googlecalendar").is_none());
        reset_pretend();
    }

    /// What the real account looked like after the fault: nine unused sign-ins and
    /// one that works. Connecting is "already connected", and the clutter goes.
    #[tokio::test]
    async fn a_connected_app_is_not_signed_in_again_and_its_clutter_is_cleared() {
        let mut initial = vec![("googlecalendar", "initializing"); 9];
        initial.push(("googlecalendar", "active"));
        let (held, answer) = world(&initial);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let started = connect_connect(&db, &mut client, "googlecalendar").await.unwrap();
        assert!(started.redirect_url.is_none(), "nothing to open");
        assert_eq!(started.account.status, "ACTIVE");
        assert!(!actions(&stub).contains(&"add".to_string()), "no new account for a connected app");
        let left = held.lock().unwrap().0.clone();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].2, "active", "the working account is the one kept");

        // An app at the limit with nothing working is cleared, then signed in.
        let (held, answer) = world(&[("slack", "initializing"); 10]);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let started = connect_connect(&db, &mut client, "slack").await.unwrap();
        assert!(started.redirect_url.is_some());
        assert_eq!(held.lock().unwrap().0.len(), 1);
        reset_pretend();
    }

    /// Read from the recording made on 2026-10-19 (`fixtures/composio/connect/`).
    #[test]
    fn the_recorded_answer_reads_as_connected_and_names_the_clutter() {
        let file: Value =
            serde_json::from_str(include_str!("../../../fixtures/composio/connect/manage_connections_googlecalendar.json")).unwrap();
        let text = file["answer"]["text"].as_str().unwrap();
        // The entry says `failed` (an add refused at ten accounts) but lists a working one.
        let got = parse_connections_of(text, &["googlecalendar".to_string()]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].status, Conn::Active);
        assert_eq!(got[0].link, None);
        let stale = stale_accounts(text, &["googlecalendar".to_string()]);
        assert_eq!(stale.len(), 9);
        assert!(stale.iter().all(|(app, id)| app == "googlecalendar" && !id.contains("livor-fultz")));
        assert!(stale_accounts(text, &["gmail".to_string()]).is_empty(), "only the apps asked about");
    }

    #[tokio::test]
    async fn an_app_that_is_already_connected_needs_no_sign_in() {
        let stub = connect_stub(|_, _| Reply::Text(connections_answer(json!({ "slack": { "status": "ACTIVE" } })))).await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let started = connect_connect(&db, &mut client, "slack").await.unwrap();
        assert!(started.redirect_url.is_none(), "nothing to open");
        assert_eq!(started.account.status, "ACTIVE");

        // An answer with neither a link nor ACTIVE is a plain sentence, not a crash.
        let silent = connect_stub(|_, _| Reply::Text(connections_answer(json!({ "notion": { "message": "hm" } })))).await;
        connect_mode(&silent.url);
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let err = connect_connect(&db, &mut client, "notion").await.err().unwrap();
        assert_eq!(err, "Composio couldn't start the sign-in for Notion.");
        assert!(connect_connect(&db, &mut client, "../etc").await.is_err());
        reset_pretend();
    }

    /// A sign-in that stopped is made again from scratch: its dead accounts go, a
    /// new one is added, and a working app asked about alongside it is left alone.
    #[tokio::test]
    async fn only_an_app_that_is_not_connected_gets_a_new_sign_in() {
        let (held, answer) = world(&[("gmail", "expired"), ("linear", "active")]);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        put_status(&db, &row, "gmail", "EXPIRED").unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let got = start_sign_ins(&db, &mut client, &["gmail".into(), "linear".into()]).await.unwrap();
        let by = |app: &str| got.iter().find(|s| s.app == app).unwrap().clone();
        assert_eq!(by("linear").status, Conn::Active);
        assert_eq!(by("gmail").status, Conn::Initiated);
        let calls = stub.calls.lock().unwrap().clone();
        let add = calls.iter().find(|(_, a)| a["toolkits"][0]["action"] == "add").unwrap();
        assert_eq!(add.1["toolkits"], json!([{ "name": "gmail", "action": "add" }]), "linear is not added again");
        let left = held.lock().unwrap().0.clone();
        assert!(left.iter().any(|(a, _, s, _)| a == "linear" && s == "active"));
        assert_eq!(left.iter().filter(|(a, ..)| a == "gmail").count(), 1, "the dead gmail account was replaced");
        reset_pretend();
    }

    /// `CAP-C4`: asking about apps updates the mirror; an app that was working
    /// and now wants a sign-in has expired; an app not asked about is not touched.
    #[tokio::test]
    async fn checking_apps_updates_the_mirror_and_an_active_one_that_asks_again_has_expired() {
        let stub = connect_stub(|_, _| {
            Reply::Text(connections_answer(json!({
                "gmail": { "status": "INITIATED", "redirect_url": link_for("gmail") },
                "slack": { "status": "ACTIVE" },
                "notion": { "status": "ACTIVE" },
            })))
        })
        .await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        put_status(&db, &row, "gmail", "ACTIVE").unwrap();
        put_status(&db, &row, "slack", "INITIATED").unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        check_connect(&db, &mut client, &["gmail".into(), "slack".into()]).await.unwrap();
        let held = db.list_app_accounts(&row.id).unwrap();
        let status = |app: &str| held.iter().find(|a| a.app == app).map(|a| a.status.clone());
        assert_eq!(status("gmail").as_deref(), Some("EXPIRED"));
        assert_eq!(status("slack").as_deref(), Some("ACTIVE"));
        assert_eq!(status("notion"), None, "notion was in the answer but not asked about");
        assert_eq!(actions(&stub), ["list", "list"], "checking only lists, so it can never make an account");
        reset_pretend();
    }

    /// `CAP-C-T6`: an app that answered a call is connected; one that failed is not.
    #[test]
    fn an_app_that_answers_a_call_is_learned_as_connected() {
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        let targets = vec![
            ("GMAIL_FETCH_EMAILS".to_string(), "gmail".to_string()),
            ("SLACK_LIST_CHANNELS".to_string(), "slack".to_string()),
            ("COMPOSIO_SEARCH_TOOLS".to_string(), "composio".to_string()),
        ];
        let output = json!({ "successful": true, "data": { "results": [
            { "index": 0, "tool_slug": "GMAIL_FETCH_EMAILS", "response": { "successful": true, "data": {} } },
            { "index": 1, "tool_slug": "SLACK_LIST_CHANNELS", "error": "No connected account for slack" },
            { "index": 2, "tool_slug": "COMPOSIO_SEARCH_TOOLS", "response": {} },
        ] } })
        .to_string();
        let learned = learn_from_results(&db, &row.id, &output, &targets);
        assert_eq!(learned, ["gmail"]);
        let held = db.list_app_accounts(&row.id).unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!((held[0].app.as_str(), held[0].status.as_str()), ("gmail", "ACTIVE"));
        // An app's own reply saying it failed is a failure too.
        let reply_failed = json!({ "data": { "results": [
            { "tool_slug": "SLACK_LIST_CHANNELS", "response": { "successful": false, "error": "not_authed" } }
        ] } })
        .to_string();
        assert!(learn_from_results(&db, &row.id, &reply_failed, &targets).is_empty());
        // Nonsense changes nothing.
        assert!(learn_from_results(&db, &row.id, "oops", &targets).is_empty());
        assert!(learn_from_results(&db, "someone-else", &output, &targets).is_empty());
    }

    /// An app removed in Composio (or by another AI client) leaves the mirror on
    /// the next look. A row left at ACTIVE would have the model told "already
    /// connected" while every call to the app fails.
    #[tokio::test]
    async fn an_app_composio_no_longer_holds_leaves_the_mirror() {
        let (_held, answer) = world(&[("slack", "active")]);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        put_status(&db, &row, "gmail", "ACTIVE").unwrap();
        put_status(&db, &row, "slack", "ACTIVE").unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let got = check_connect(&db, &mut client, &["gmail".into(), "slack".into()]).await.unwrap();
        assert!(got.iter().any(|s| s.app == "gmail" && s.status == Conn::Gone), "{got:?}");
        let left: Vec<String> = db.list_app_accounts(&row.id).unwrap().into_iter().map(|a| a.app).collect();
        assert_eq!(left, ["slack"]);
        // The words Composio may use for it, and an entry that says nothing, which stays unknown.
        let said = connections_answer(json!({ "gmail": { "status": "not_connected" }, "slack": {} }));
        let parsed = parse_connections(&said);
        assert_eq!(parsed[0].status, Conn::Gone);
        assert_eq!(parsed[1].status, Conn::Unknown);
        reset_pretend();
    }

    /// Asking for the same app again while its sign-in is open hands back the open
    /// one. Clearing it as clutter would break the page in the person's browser.
    #[tokio::test]
    async fn an_open_sign_in_is_not_cleared_when_the_same_app_is_asked_for_again() {
        let (held, answer) = world(&[]);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let first = start_sign_ins(&db, &mut client, &["gmail".into()]).await.unwrap();
        let again = start_sign_ins(&db, &mut client, &["gmail".into(), "slack".into()]).await.unwrap();
        let gmail = again.iter().find(|s| s.app == "gmail").unwrap();
        assert_eq!((gmail.status, &gmail.link), (Conn::Initiated, &first[0].link));
        assert_eq!(actions(&stub), ["list", "add", "list", "add"], "only slack was asked about the second time");
        let gmail_accounts = held.lock().unwrap().0.iter().filter(|(a, ..)| a == "gmail").count();
        assert_eq!(gmail_accounts, 1, "the open gmail sign-in is still there");
        reset_pretend();
    }

    /// `CAP-12`: a target Composio refused before running it is not counted; one
    /// the app itself answered with an error did reach the app, and is.
    #[test]
    fn usage_counts_the_targets_that_reached_their_app() {
        let slugs: Vec<String> = ["GOOGLECALENDAR_LIST_EVENTS", "GOOGLECALENDAR_EVENTS_LIST", "GMAIL_FETCH_EMAILS"]
            .map(String::from)
            .to_vec();
        let output = json!({ "successful": false, "data": { "results": [
            { "index": 1, "tool_slug": "GOOGLECALENDAR_EVENTS_LIST", "response": { "successful": false, "data": { "http_error": "403" } } },
            { "index": 0, "tool_slug": "GOOGLECALENDAR_LIST_EVENTS", "error": "Tool GOOGLECALENDAR_LIST_EVENTS not found" },
        ] } })
        .to_string();
        assert_eq!(target_outcomes(&output, &slugs), [false, true, true]);
        // An index that names another tool is not trusted; the slug decides.
        let shifted = json!({ "data": { "results": [{ "index": 1, "tool_slug": "GMAIL_FETCH_EMAILS", "error": "nope" }] } }).to_string();
        assert_eq!(target_outcomes(&shifted, &slugs), [true, true, false]);
        // Nothing readable: everything counts.
        assert_eq!(target_outcomes("oops", &slugs), [true, true, true]);
    }

    /// `CAP-C-T8`.
    #[test]
    fn removing_from_the_list_deletes_only_this_installs_row() {
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        put_status(&db, &row, "gmail", "ACTIVE").unwrap();
        put_status(&db, &row, "slack", "ACTIVE").unwrap();
        remember_link(&row.id, "gmail", "", "connect:gmail", "https://connect.composio.dev/link/ln_abcdef1");
        assert!(remove_from_list(&db, "connect:other").is_err(), "an id that isn't a row of this mirror");
        assert_eq!(db.list_app_accounts(&row.id).unwrap().len(), 2);
        remove_from_list(&db, "connect:gmail").unwrap();
        let left = db.list_app_accounts(&row.id).unwrap();
        assert_eq!(left.iter().map(|a| a.app.as_str()).collect::<Vec<_>>(), ["slack"]);
        assert!(recall_link(&row.id, "gmail").is_none(), "the remembered link goes with it");
    }

    /// Disconnecting in Connect mode takes the app away at Composio, every account
    /// it holds, and only then drops the local row. Other apps are not touched.
    #[tokio::test]
    async fn disconnecting_removes_every_account_at_composio_then_the_row() {
        let (held, answer) = world(&[("gmail", "active"), ("gmail", "initializing"), ("slack", "active")]);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        put_status(&db, &row, "gmail", "ACTIVE").unwrap();
        put_status(&db, &row, "slack", "ACTIVE").unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        remove_everywhere(&mut client, "gmail").await.unwrap();
        let left = held.lock().unwrap().0.clone();
        assert_eq!(left.iter().map(|(a, ..)| a.as_str()).collect::<Vec<_>>(), ["slack"], "only gmail went");
        reset_pretend();
    }

    /// If Composio still lists the app afterwards, the row stays and the person is told.
    #[tokio::test]
    async fn an_app_composio_will_not_let_go_of_keeps_its_row_and_says_so() {
        // A world that answers `remove` with nothing removed.
        let stub = connect_stub(|_, args| {
            let op = &args["toolkits"][0];
            let accounts = json!([{ "id": "gmail_stuck", "status": "active" }]);
            let entry = if op["action"] == "remove" { json!({}) } else { json!({ "toolkit": "gmail", "accounts": accounts }) };
            Reply::Text(json!({ "successful": true, "data": { "results": { "gmail": entry } } }).to_string())
        })
        .await;
        connect_mode(&stub.url);
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        let err = remove_everywhere(&mut client, "gmail").await.err().unwrap();
        assert!(err.contains("still lists Gmail"), "{err}");
        reset_pretend();
    }

    // ---- several accounts of one app (CAP-C6) ----

    async fn connect_world(initial: &[(&str, &str)]) -> (Held, crate::connectors::stub::ConnectStub, McpClient, Db) {
        let (held, answer) = world(initial);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        (held, stub, client, Db::open_in_memory().unwrap())
    }

    fn rows_of(db: &Db) -> Vec<(String, Option<String>, String)> {
        let row = db.composio_connector().unwrap().unwrap();
        db.list_app_accounts(&row.id).unwrap().into_iter().map(|a| (a.id, a.label, a.status)).collect()
    }

    /// Add account in Connect mode: one more sign-in under the person's name for
    /// it. While it is open the app stays connected by the account it had; once
    /// the new one works each account has a row of its own, the new one named.
    #[tokio::test]
    async fn a_second_account_is_added_under_its_name_and_each_gets_a_row() {
        let (held, stub, mut client, db) = connect_world(&[("gmail", "active")]).await;
        check_connect(&db, &mut client, &["gmail".into()]).await.unwrap();
        assert_eq!(rows_of(&db), [("connect:gmail".to_string(), None, "ACTIVE".to_string())]);

        let started = add_account(&db, &mut client, "gmail", "work").await.unwrap();
        assert_eq!(started.redirect_url.as_deref(), Some(link_for("gmail").as_str()));
        assert_eq!(started.account.id, "connect:gmail:new");
        assert_eq!(started.account.label.as_deref(), Some("work"));
        assert_eq!(started.account.status, "INITIATED");
        let calls = stub.calls.lock().unwrap().clone();
        let add = calls.iter().find(|(_, a)| a["toolkits"][0]["action"] == "add").expect("an add");
        assert_eq!(add.1["toolkits"][0]["alias"], "work", "Composio is given the name");

        // Checking while it is open keeps both, and keeps the link.
        check_connect(&db, &mut client, &["gmail".into()]).await.unwrap();
        let ids: Vec<String> = rows_of(&db).into_iter().map(|r| r.0).collect();
        assert_eq!(ids, ["connect:gmail", "connect:gmail:new"]);
        let row = db.composio_connector().unwrap().unwrap();
        assert!(recall_link(&row.id, "gmail").is_some());

        // The person signs in.
        for acc in held.lock().unwrap().0.iter_mut().filter(|a| a.2 == "initializing") {
            acc.2 = "active".into();
        }
        check_connect(&db, &mut client, &["gmail".into()]).await.unwrap();
        assert_eq!(
            rows_of(&db),
            [
                ("connect:gmail:gmail_old0".to_string(), None, "ACTIVE".to_string()),
                ("connect:gmail:gmail_new1".to_string(), Some("work".to_string()), "ACTIVE".to_string()),
            ]
        );
        assert!(recall_link(&row.id, "gmail").is_none(), "the sign-in is done");
        // The same name twice is refused before anything is made.
        let again = add_account(&db, &mut client, "gmail", "Work").await.err().unwrap();
        assert!(again.contains("already have a Gmail account called Work"), "{again}");
        reset_pretend();
    }

    /// An extra sign-in whose link is gone (ten minutes, a restart) can no longer
    /// be finished: its row goes and the accounts stay as they were.
    #[tokio::test]
    async fn an_extra_sign_in_that_cannot_be_finished_goes() {
        let (_held, _stub, mut client, db) = connect_world(&[("gmail", "active")]).await;
        add_account(&db, &mut client, "gmail", "work").await.unwrap();
        let row = db.composio_connector().unwrap().unwrap();
        assert!(recall_link_within(&row.id, "gmail", Duration::ZERO).is_none());
        check_connect(&db, &mut client, &["gmail".into()]).await.unwrap();
        assert_eq!(rows_of(&db), [("connect:gmail".to_string(), None, "ACTIVE".to_string())]);
        reset_pretend();
    }

    /// Names Composio holds are shown; a rename reaches Composio and is kept here.
    #[tokio::test]
    async fn accounts_named_in_composio_are_shown_and_a_rename_reaches_it() {
        let (held, _stub, mut client, db) = connect_world(&[("gmail", "active"), ("gmail", "active")]).await;
        held.lock().unwrap().0[0].3 = "personal".into();
        check_connect(&db, &mut client, &["gmail".into()]).await.unwrap();
        let rows = rows_of(&db);
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&("connect:gmail:gmail_old0".into(), Some("personal".into()), "ACTIVE".into())), "{rows:?}");

        rename_account(&db, &mut client, "connect:gmail:gmail_old1", "work").await.unwrap();
        assert_eq!(held.lock().unwrap().0[1].3, "work");
        check_connect(&db, &mut client, &["gmail".into()]).await.unwrap();
        assert!(rows_of(&db).contains(&("connect:gmail:gmail_old1".into(), Some("work".into()), "ACTIVE".into())));
        let dup = rename_account(&db, &mut client, "connect:gmail:gmail_old1", "PERSONAL").await.err().unwrap();
        assert!(dup.contains("already have a Gmail account called PERSONAL"), "{dup}");

        // The app's only account is found by listing.
        let (held, _stub, mut client, db) = connect_world(&[("slack", "active")]).await;
        check_connect(&db, &mut client, &["slack".into()]).await.unwrap();
        rename_account(&db, &mut client, "connect:slack", "team").await.unwrap();
        assert_eq!(held.lock().unwrap().0[0].3, "team");
        assert_eq!(rows_of(&db), [("connect:slack".to_string(), Some("team".to_string()), "ACTIVE".to_string())]);
        reset_pretend();
    }

    /// A name Poiesis gave survives a check whose answer carries none, since where
    /// Composio puts the alias has not been recorded.
    #[tokio::test]
    async fn a_name_is_kept_when_composios_answer_carries_none() {
        let (_held, _stub, mut client, db) = connect_world(&[("slack", "active")]).await;
        check_connect(&db, &mut client, &["slack".into()]).await.unwrap();
        let row = db.composio_connector().unwrap().unwrap();
        put_row(&db, &row, "connect:slack", "slack", Some("team".into()), "ACTIVE").unwrap();
        check_connect(&db, &mut client, &["slack".into()]).await.unwrap();
        assert_eq!(rows_of(&db), [("connect:slack".to_string(), Some("team".to_string()), "ACTIVE".to_string())]);
        reset_pretend();
    }

    /// Disconnecting one of two accounts removes only that one at Composio; the
    /// one left is the app's row again, with its name.
    #[tokio::test]
    async fn disconnecting_one_of_two_accounts_leaves_the_other() {
        let (held, _stub, mut client, db) = connect_world(&[("gmail", "active"), ("gmail", "active")]).await;
        held.lock().unwrap().0[1].3 = "work".into();
        check_connect(&db, &mut client, &["gmail".into()]).await.unwrap();
        remove_one(&db, &mut client, "gmail", "gmail_old0").await.unwrap();
        let left = held.lock().unwrap().0.clone();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].1, "gmail_old1");
        assert_eq!(rows_of(&db), [("connect:gmail".to_string(), Some("work".to_string()), "ACTIVE".to_string())]);
        assert_eq!(connect_account_of("connect:gmail:gmail_old1", "gmail").as_deref(), Some("gmail_old1"));
        assert_eq!(connect_account_of("connect:gmail:new", "gmail"), None, "an open sign-in is not an account");
        assert_eq!(connect_account_of("connect:gmail", "gmail"), None);
        reset_pretend();
    }

    /// Giving up on an open sign-in clears the half-made account at Composio too,
    /// and leaves a working account of the same app alone.
    #[tokio::test]
    async fn giving_up_clears_only_the_unused_accounts_at_composio() {
        let (held, answer) = world(&[("gmail", "active"), ("gmail", "initializing"), ("gmail", "initializing")]);
        let stub = connect_stub(answer).await;
        connect_mode(&stub.url);
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, "ck_test");
        client.initialize().await.unwrap();
        clear_unused(&mut client, "gmail").await;
        let left = held.lock().unwrap().0.clone();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].2, "active");
        reset_pretend();
    }

    #[test]
    fn a_key_of_the_other_kind_starts_the_list_afresh() {
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        db.upsert_app_account(&AppAccount {
            id: "ca_old".into(),
            connector_id: row.id.clone(),
            app: "gmail".into(),
            label: Some("work".into()),
            status: "ACTIVE".into(),
            updated_at: 1,
        })
        .unwrap();
        note_key_kind(&db, Mode::Project).unwrap();
        assert_eq!(db.list_app_accounts(&row.id).unwrap().len(), 1, "same kind again keeps it");
        note_key_kind(&db, Mode::Connect).unwrap();
        assert!(db.list_app_accounts(&row.id).unwrap().is_empty(), "a new kind of key empties the mirror");
        put_status(&db, &row, "slack", "ACTIVE").unwrap();
        note_key_kind(&db, Mode::Connect).unwrap();
        assert_eq!(db.list_app_accounts(&row.id).unwrap().len(), 1);
    }

    /// One row per app in Connect mode, even if an older row under another id exists.
    #[test]
    fn connect_mode_keeps_one_row_per_app() {
        let db = Db::open_in_memory().unwrap();
        let row = ensure_row(&db).unwrap();
        db.upsert_app_account(&AppAccount {
            id: "ca_old".into(),
            connector_id: row.id.clone(),
            app: "gmail".into(),
            label: None,
            status: "EXPIRED".into(),
            updated_at: 1,
        })
        .unwrap();
        put_status(&db, &row, "gmail", "ACTIVE").unwrap();
        let held = db.list_app_accounts(&row.id).unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].id, "connect:gmail");
    }

    /// `CAP-C-T9`: `CAP-T-10` again, with a `ck_` key. Nothing stored holds it,
    /// and it travelled only in the header.
    #[tokio::test]
    async fn a_connect_key_is_stored_nowhere_and_never_printed() {
        const SECRET: &str = "ck_SECRET_KEY_7b21";
        let stub = connect_stub(|_, _| {
            Reply::Text(connections_answer(json!({ "gmail": { "status": "INITIATED", "redirect_url": link_for("gmail") } })))
        })
        .await;
        connect_mode(&stub.url);
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let mut client = connect_client(&http, SECRET);
        client.initialize().await.unwrap();
        connect_connect(&db, &mut client, "gmail").await.unwrap();
        check_connect(&db, &mut client, &["gmail".into()]).await.unwrap();
        set_last_error(&db, Some(&CONNECT_REFUSED.to_string()));
        let row = db.composio_connector().unwrap().unwrap();
        let everything = format!("{:?} {:?}", db.list_connectors().unwrap(), db.list_app_accounts(&row.id).unwrap());
        assert!(!everything.contains(SECRET), "the key reached storage: {everything}");
        assert!(!everything.contains("ln_gmail12345"), "a sign-in address reached storage: {everything}");
        let keys = stub.keys.lock().unwrap().clone();
        assert!(!keys.is_empty() && keys.iter().all(|k| k == SECRET), "it travelled in the header only");
        reset_pretend();
    }

    // ---- Project mode fixes (CAP-C-F1, CAP-C-F2) ----

    /// `CAP-C-F1`: reopening a sign-in that is still open makes no second account.
    #[tokio::test]
    async fn project_mode_reopens_an_open_sign_in_instead_of_making_another() {
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);
        let first = connect(&db, &api, "gmail", None).await.unwrap();
        let row = ensure_row(&db).unwrap();
        // Composio lists the account as waiting.
        stub.accounts.lock().unwrap().push(held(&first.account.id, "gmail", "default", "INITIATED", &user_id_of(&row).unwrap()));
        let again = connect(&db, &api, "gmail", None).await.unwrap();
        assert_eq!(again.account.id, first.account.id);
        assert_eq!(again.redirect_url, first.redirect_url);
        assert_eq!(stub.links.lock().unwrap().len(), 1, "no second link was made");
        // A different label is a different sign-in.
        connect(&db, &api, "gmail", Some("work")).await.unwrap();
        assert_eq!(stub.links.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_label_held_only_by_a_sign_in_that_never_worked_can_be_used_again() {
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);
        let (_, row) = ensure_session(&db, &api).await.unwrap();
        let user_id = user_id_of(&row).unwrap();
        stub.accounts.lock().unwrap().extend([
            held("ca_stuck", "gmail", "work", "FAILED", &user_id),
            held("ca_fine", "gmail", "home", "ACTIVE", &user_id),
        ]);
        let started = connect(&db, &api, "gmail", Some("work")).await.unwrap();
        assert_eq!(started.account.label.as_deref(), Some("work"));
        assert_eq!(stub.deleted.lock().unwrap().as_slice(), ["ca_stuck"], "only the one that never worked went");
        // A working sign-in's label is still taken.
        let err = connect(&db, &api, "gmail", Some("home")).await.err().unwrap();
        assert!(err.contains("already have a Gmail account called home"), "{err}");
    }

    #[tokio::test]
    async fn reconnecting_revokes_the_stopped_sign_in_and_links_again_under_its_label() {
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);
        let (_, row) = ensure_session(&db, &api).await.unwrap();
        let user_id = user_id_of(&row).unwrap();
        stub.accounts.lock().unwrap().extend([
            held("ca_dead", "slack", "acme", "EXPIRED", &user_id),
            held("ca_live", "gmail", "work", "ACTIVE", &user_id),
        ]);
        let again = reconnect(&db, &api, "ca_dead").await.unwrap();
        assert_eq!(stub.deleted.lock().unwrap().as_slice(), ["ca_dead"]);
        assert_eq!(again.account.label.as_deref(), Some("acme"));
        assert_eq!(stub.links.lock().unwrap()[0]["alias"], "acme");
        assert!(reconnect(&db, &api, "ca_live").await.is_err(), "a working sign-in has nothing to reconnect");
        assert!(reconnect(&db, &api, "ca_unknown").await.is_err());
    }

    #[tokio::test]
    async fn giving_up_on_a_sign_in_removes_it_instead_of_leaving_it_to_fail_later() {
        let stub = stub(true, false, "").await;
        let db = Db::open_in_memory().unwrap();
        let http = reqwest::Client::new();
        let api = Api::at(&http, "ak_test", &stub.base);
        let (_, row) = ensure_session(&db, &api).await.unwrap();
        let user_id = user_id_of(&row).unwrap();
        stub.accounts.lock().unwrap().extend([
            held("ca_waiting", "slack", "x", "INITIATED", &user_id),
            held("ca_live", "gmail", "work", "ACTIVE", &user_id),
        ]);
        cancel_signin(&db, &api, "ca_waiting").await.unwrap();
        cancel_signin(&db, &api, "ca_live").await.unwrap();
        assert_eq!(stub.deleted.lock().unwrap().as_slice(), ["ca_waiting"], "a working sign-in is never cancelled");
    }

    #[test]
    fn a_scope_note_in_connect_mode_says_to_disconnect_and_allow_everything() {
        let failed_call = json!({
            "successful": false,
            "data": { "results": [{ "tool_slug": "GMAIL_CREATE_FILTER", "error": "ACCESS_TOKEN_SCOPE_INSUFFICIENT" }] }
        })
        .to_string();
        let note = scope_hint(Mode::Connect, "Gmail", "GMAIL_CREATE_FILTER", &failed_call).unwrap();
        assert!(note.contains("Disconnect") && note.contains("allowing everything"), "{note}");
        assert!(!note.contains("auth config") && !note.contains("Reconnecting won't"), "{note}");
        assert!(note.contains("gmail.settings.basic") && note.contains("don't retry"), "{note}");
    }
}
