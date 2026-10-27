//! `DEC-5` (cloud half) and `DEC-5a`: the decision model on OpenRouter
//! (`POST /api/alpha/decisions`), its live catalog, and the probe.
//!
//! Where it differs from the local engine (`local`): a Bearer key from Providers,
//! a `model` that is a pinned `canonical_slug`, a `provider` block that asks for
//! zero retention (decision 11), and the failures a network has. A cloud call
//! that cannot be made falls back to this computer for that call, never the
//! other way round.
//!
//! Checked against the live service on 2026-10-26: the public catalog
//! (`/api/v1/models?output_modalities=decisions`) and the public list of
//! zero-retention endpoints (`/api/v1/endpoints/zdr`), both recorded in
//! `fixtures/decide/`. **Not checked:** a decision request and its reply. Nobody
//! has run one with a key, so the body and the reply are built from the
//! documented shape, and the probe is how the first real call tells.

use std::collections::{HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use super::policy::Policy;
use super::{local, Answer, Ask, BackendId, DecideError, Opts, Purpose, Verdict};
use crate::db::{now_ms, Db};
use crate::runtime::RuntimeManager;

/// The default cloud model: the same weights as the local default (decision 2).
pub const DEFAULT_MODEL: &str = "cloudflare/clef-flash";
const DEFAULT_BASE: &str = "https://openrouter.ai/api";

/// `local` | `cloud`. Read by `policy::resolve_backend`.
pub const BACKEND_KEY: &str = "decide.backend";
/// The pinned `canonical_slug` of the chosen cloud model.
pub const MODEL_KEY: &str = "decide.cloud.model";
/// Where the service is, for a test or a proxy. Not shown anywhere.
const URL_KEY: &str = "decide.cloud.url";
const CATALOG_KEY: &str = "decide.cloud.catalog";
const PROBE_PREFIX: &str = "decide.cloud.probe.";
/// Only a test sets this: a unit test must never reach the real key store.
#[cfg(test)]
const TEST_KEY_KEY: &str = "decide.cloud.test_key";

const CATALOG_MAX_AGE_MS: i64 = 24 * 3_600_000;
/// The most state a cloud decision gets, whatever the model lists. Jev lists
/// 262k, but that needs an opt-in header nobody has seen get through (`DEC-8`).
const STATE_CAP_TOKENS: u32 = 65_536;
/// What a model that lists no context is assumed to take.
const UNKNOWN_CONTEXT_TOKENS: u32 = 16_384;
const PROBE_DEADLINE: Duration = Duration::from_secs(5);
/// Longer than a decision, because nobody waits on a catalog.
const CATALOG_DEADLINE: Duration = Duration::from_secs(15);

/// Three failures of the "busy" kind in five minutes stop cloud decisions for
/// ten (`DEC-5`).
const OVERLOAD_WINDOW_MS: i64 = 5 * 60_000;
const OVERLOAD_LIMIT: usize = 3;
const COOLDOWN_MS: i64 = 10 * 60_000;
/// A rejected key is not asked again for an hour, unless it is replaced or the
/// person presses Test.
const KEY_BLOCK_MS: i64 = 60 * 60_000;

// ---- settings ----

fn base(db: &Db) -> String {
    db.get_setting(URL_KEY).ok().flatten().unwrap_or_else(|| DEFAULT_BASE.to_string())
}

#[cfg(not(test))]
pub fn api_key(_db: &Db) -> Option<String> {
    crate::cloud::get_key(crate::cloud::Provider::OpenRouter)
}

#[cfg(test)]
pub fn api_key(db: &Db) -> Option<String> {
    db.get_setting(TEST_KEY_KEY).ok().flatten().filter(|k| !k.is_empty())
}

/// Is the cloud where quick decisions go: chosen in Runtime, with a key?
pub fn chosen(db: &Db) -> bool {
    db.get_setting(BACKEND_KEY).ok().flatten().as_deref() == Some("cloud") && api_key(db).is_some()
}

/// Has the person allowed the cloud for this one purpose by name (decision 12),
/// with a key to do it?
pub fn allowed_for(db: &Db, purpose: Purpose) -> bool {
    db.get_setting(&super::policy::override_key(purpose)).ok().flatten().as_deref() == Some("cloud")
        && api_key(db).is_some()
}

/// The pinned model, or the default.
pub fn selected_model(db: &Db) -> String {
    db.get_setting(MODEL_KEY)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

// ---- the request ----

/// A random id for a run, so the service can group its calls. It is a hash of
/// the run's own id and never the conversation's or its title (`DEC-5`). Without
/// a run it is one per launch.
pub fn session_id(run_id: Option<&str>) -> String {
    use sha2::{Digest, Sha256};
    static LAUNCH: OnceLock<String> = OnceLock::new();
    let seed = run_id.map(str::to_string).unwrap_or_else(|| LAUNCH.get_or_init(|| uuid::Uuid::new_v4().to_string()).clone());
    let digest = Sha256::digest(seed.as_bytes());
    format!("poiesis-{}", digest.iter().take(8).map(|b| format!("{b:02x}")).collect::<String>())
}

/// The request body for `ask`, with `state` already cut to fit. An image rides in
/// `state` as a content part: the published schema has no image field, and the
/// probe says whether a model reads it.
pub fn build_body(ask: &Ask<'_>, state: &str, model: &str, session: &str) -> Value {
    let mut questions = Map::new();
    for (id, q) in &ask.questions {
        questions.insert((*id).to_string(), local::question_body(q));
    }
    let state = if ask.images.is_empty() {
        Value::String(state.to_string())
    } else {
        let mut parts = vec![json!({ "type": "text", "text": state })];
        parts.extend(ask.images.iter().map(|i| json!({ "type": "image_url", "image_url": { "url": i.data_url } })));
        Value::Array(parts)
    };
    json!({
        "model": model,
        "state": state,
        "questions": questions,
        // Decision 11. If no endpoint meets this the service answers 404, which
        // is reported and never relaxed.
        "provider": { "data_collection": "deny", "zdr": true },
        "session_id": session,
    })
}

/// What a reply holds besides the answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub answers: std::collections::BTreeMap<String, Answer>,
    pub input_tokens: u32,
    pub cost_usd: Option<f64>,
    /// The version that answered, which is what a calibration is keyed on.
    pub model: Option<String>,
    pub provider: Option<String>,
}

/// Read a reply. The answers are read as the local engine's are, once a score's
/// probabilities (an array here, a map there) are in one shape.
pub fn parse_response(ask: &Ask<'_>, reply: &Value) -> Result<Parsed, String> {
    let mut reply = reply.clone();
    if let Some(answers) = reply.get_mut("answers").and_then(Value::as_object_mut) {
        for a in answers.values_mut() {
            let Some(list) = a.get("probabilities").and_then(Value::as_array).cloned() else { continue };
            let map: Map<String, Value> = list.into_iter().enumerate().map(|(i, p)| (i.to_string(), p)).collect();
            a["probabilities"] = Value::Object(map);
        }
    }
    let (answers, input_tokens) = local::parse_response(ask, &reply)?;
    let provider = match reply.get("provider") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(o)) => o.get("name").and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    Ok(Parsed {
        answers,
        input_tokens,
        cost_usd: reply.get("usage").and_then(|u| u.get("cost")).and_then(Value::as_f64).filter(|c| *c >= 0.0),
        model: reply.get("model").and_then(Value::as_str).map(str::to_string),
        provider,
    })
}

// ---- failures ----

/// What went wrong with a call, by what the person can do about it (`DEC-5`).
#[derive(Debug, Clone, PartialEq)]
pub enum Fault {
    /// 401 or 403.
    KeyRejected,
    /// 402.
    NoCredits,
    /// 429, 500, 502, 503, 524, 529: busy, try again later.
    Overloaded(u16),
    /// 404: no provider of this model meets the privacy rule.
    NoEndpoint,
    /// 400 or 413: a bug or a state too big. Not retried anywhere.
    Rejected(String),
    Timeout,
    /// Could not connect at all.
    Offline(String),
    /// Anything else: not counted against the service.
    Other(u16, String),
}

pub fn classify(status: u16, body: &str) -> Fault {
    let body: String = body.chars().take(300).collect();
    match status {
        401 | 403 => Fault::KeyRejected,
        402 => Fault::NoCredits,
        404 => Fault::NoEndpoint,
        400 | 413 => Fault::Rejected(format!("HTTP {status}: {body}")),
        429 | 500 | 502 | 503 | 524 | 529 => Fault::Overloaded(status),
        _ => Fault::Other(status, body),
    }
}

/// The words for a fault, for the screens.
pub fn fault_words(f: &Fault) -> String {
    match f {
        Fault::KeyRejected => "OpenRouter rejected the key.".into(),
        Fault::NoCredits => "Your OpenRouter account is out of credits.".into(),
        Fault::Overloaded(s) => format!("OpenRouter is busy (HTTP {s})."),
        Fault::NoEndpoint => "No provider of this model meets the zero-retention rule.".into(),
        Fault::Rejected(m) => format!("OpenRouter refused the request. {m}"),
        Fault::Timeout => "OpenRouter did not answer in time.".into(),
        Fault::Offline(m) => format!("Couldn't reach OpenRouter. {m}"),
        Fault::Other(s, m) => format!("OpenRouter answered HTTP {s}. {m}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockKind {
    Key,
    Credits,
    Overloaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Block {
    kind: BlockKind,
    /// Epoch ms.
    until: i64,
}

/// How the cloud has been doing, in this launch. A blocked cloud is not tried:
/// every ask goes to this computer until the block ends.
#[derive(Debug, Default)]
pub struct Health {
    block: Option<Block>,
    overloads: VecDeque<i64>,
    /// Models with no endpoint that meets the privacy rule.
    no_endpoint: HashSet<String>,
    samples: VecDeque<u32>,
}

impl Health {
    /// May this model be asked now?
    pub fn usable(&self, model: &str, now: i64) -> bool {
        !self.block.is_some_and(|b| b.until > now) && !self.no_endpoint.contains(model)
    }

    /// Note a failure. Returns a line for Activity when it just stopped the cloud
    /// (once per stop, not once per call).
    pub fn on_fault(&mut self, fault: &Fault, model: &str, now: i64) -> Option<String> {
        let already = self.block.is_some_and(|b| b.until > now);
        // A stop already in force is not extended by another failure, so it ends
        // when it was due to.
        let start = |h: &mut Health, kind, ms, line: &str| {
            if !already {
                h.block = Some(Block { kind, until: now + ms });
            }
            (!already).then(|| line.to_string())
        };
        match fault {
            Fault::KeyRejected => {
                start(self, BlockKind::Key, KEY_BLOCK_MS, "OpenRouter rejected the key; decisions stay on this computer")
            }
            Fault::NoCredits => {
                start(self, BlockKind::Credits, COOLDOWN_MS, "Out of OpenRouter credits; decisions stay on this computer")
            }
            Fault::Overloaded(_) => {
                self.overloads.push_back(now);
                while self.overloads.front().is_some_and(|t| now - t >= OVERLOAD_WINDOW_MS) {
                    self.overloads.pop_front();
                }
                if self.overloads.len() >= OVERLOAD_LIMIT {
                    self.overloads.clear();
                    start(self, BlockKind::Overloaded, COOLDOWN_MS, "OpenRouter is overloaded; decisions stay on this computer for 10 minutes")
                } else {
                    None
                }
            }
            Fault::NoEndpoint => self
                .no_endpoint
                .insert(model.to_string())
                .then(|| format!("{model} has no provider that meets the zero-retention rule; decisions stay on this computer")),
            _ => None,
        }
    }

    pub fn on_success(&mut self, ms: u32) {
        self.samples.push_back(ms);
        while self.samples.len() > 20 {
            self.samples.pop_front();
        }
    }

    /// The person fixed something (a new key, a new model, the Test button).
    pub fn clear(&mut self) {
        self.block = None;
        self.overloads.clear();
        self.no_endpoint.clear();
    }

    pub fn median_ms(&self) -> Option<u32> {
        if self.samples.is_empty() {
            return None;
        }
        let mut v: Vec<u32> = self.samples.iter().copied().collect();
        v.sort_unstable();
        Some(v[v.len() / 2])
    }

    fn block_at(&self, now: i64) -> Option<Block> {
        self.block.filter(|b| b.until > now)
    }
}

fn health() -> &'static Mutex<Health> {
    static HEALTH: OnceLock<Mutex<Health>> = OnceLock::new();
    HEALTH.get_or_init(Default::default)
}

/// A new key or a new choice: forget what went wrong with the old one.
pub fn forget_faults() {
    if let Ok(mut h) = health().lock() {
        h.clear();
    }
    crate::cloud::record_outcome(crate::cloud::Provider::OpenRouter.id(), None);
}

/// What Runtime shows about the cloud (`DEC-UI-2`).
#[derive(Debug, Clone, Serialize)]
pub struct CloudStatus {
    /// What is chosen: `local` | `cloud`.
    pub backend: String,
    pub model: String,
    pub key_set: bool,
    /// `ready` | `no-key` | `key` | `credits` | `overloaded` | `no-endpoint`
    pub state: &'static str,
    /// Epoch ms when a stop ends.
    pub until: Option<i64>,
    pub median_ms: Option<u32>,
}

pub fn status(db: &Db) -> CloudStatus {
    status_in(db, &health().lock().unwrap_or_else(|e| e.into_inner()), now_ms())
}

fn status_in(db: &Db, h: &Health, now: i64) -> CloudStatus {
    let model = selected_model(db);
    let key_set = api_key(db).is_some();
    let (state, until) = if !key_set {
        ("no-key", None)
    } else if let Some(b) = h.block_at(now) {
        (
            match b.kind {
                BlockKind::Key => "key",
                BlockKind::Credits => "credits",
                BlockKind::Overloaded => "overloaded",
            },
            Some(b.until),
        )
    } else if h.no_endpoint.contains(&model) {
        ("no-endpoint", None)
    } else {
        ("ready", None)
    };
    CloudStatus {
        backend: if db.get_setting(BACKEND_KEY).ok().flatten().as_deref() == Some("cloud") { "cloud" } else { "local" }.into(),
        model,
        key_set,
        state,
        until,
        median_ms: h.median_ms(),
    }
}

/// Tests that touch the shared health (a probe, a real ask) take turns, so one
/// cannot clear the stop another is looking at.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- the call ----

/// Post one request. Every failure is a [`Fault`].
pub async fn post(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    body: &Value,
    deadline: Duration,
) -> Result<Value, Fault> {
    let resp = client
        .post(format!("{base}/alpha/decisions"))
        .bearer_auth(key)
        .timeout(deadline)
        .json(body)
        .send()
        .await
        .map_err(|e| if e.is_timeout() { Fault::Timeout } else { Fault::Offline(e.to_string()) })?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let text = resp.text().await.unwrap_or_default();
        return Err(classify(status, &text));
    }
    resp.json::<Value>()
        .await
        .map_err(|e| if e.is_timeout() { Fault::Timeout } else { Fault::Other(status, e.to_string()) })
}

/// What [`attempt`] decided.
pub(super) enum Attempt {
    /// Answered.
    Done(Verdict),
    /// Not answered, and not to be asked again anywhere.
    Stop(DecideError),
    /// Ask this computer instead. `admitted`: the decision budget was already
    /// counted, so the local path must not count it twice.
    Local { admitted: bool },
}

/// Ask the cloud (`DEC-5`). A cloud that cannot be used right now (no key, a
/// stop in force, no vision for an image) is a quiet [`Attempt::Local`].
pub(super) async fn attempt<R: tauri::Runtime>(
    app: &AppHandle<R>,
    ask: &Ask<'_>,
    purpose: Purpose,
    opts: Opts<'_>,
    policy: &Policy,
    hash: &str,
) -> Attempt {
    let db = app.state::<Db>();
    let mgr = app.state::<RuntimeManager>();
    if local::check_ask(ask).is_err() {
        // The local path says why.
        return Attempt::Local { admitted: false };
    }
    let Some(key) = api_key(&db) else { return Attempt::Local { admitted: false } };
    let model = selected_model(&db);
    let now = now_ms();
    if !health().lock().unwrap_or_else(|e| e.into_inner()).usable(&model, now) {
        return Attempt::Local { admitted: false };
    }
    if !ask.images.is_empty() && !sees_images(&db, &model) {
        return Attempt::Local { admitted: false };
    }

    if let Err(limit) = super::control::controller().admit(
        opts.count_key(purpose).as_deref(),
        purpose,
        hash,
        policy.background,
        super::control::now(),
    ) {
        super::log_budget_skip(&db, opts, purpose, &format!("openrouter:{model}"), &model, ask.state, hash, limit);
        return Attempt::Stop(DecideError::Budget(limit));
    }

    let cap = policy.state_cap_tokens.min(context_of(&db, &model));
    let (state, cut) = super::truncate_middle(ask.state, cap);
    if cut {
        let _ = db.log_activity(
            opts.conversation_id,
            "decide",
            &format!("{}: the state was cut to fit ({} characters)", purpose.name(), ask.state.chars().count()),
        );
    }
    let body = build_body(ask, &state, &model, &session_id(opts.run_id));
    let started = Instant::now();
    let reply = post(&mgr.client, &base(&db), &key, &body, policy.cloud_deadline()).await;
    let ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;

    let reply = match reply {
        Ok(r) => r,
        Err(fault) => return after_fault(&db, fault, &model, opts, policy),
    };
    let parsed = match parse_response(ask, &reply) {
        Ok(p) => p,
        // A reply that cannot be read is a bug to be seen, not a reason to ask again.
        Err(e) => return Attempt::Stop(DecideError::Failed(e)),
    };
    if let Ok(mut h) = health().lock() {
        h.on_success(ms);
    }
    crate::cloud::record_outcome(crate::cloud::Provider::OpenRouter.id(), None);

    let backend = BackendId::OpenRouter { model: parsed.model.clone().unwrap_or_else(|| model.clone()), provider: parsed.provider.clone() };
    // Cloud answers are advisory until a fit backs them. A local fit is not
    // borrowed: the weights are the same but the arithmetic is not (BF16 here,
    // 4-bit there).
    let mut verdict = Verdict { id: uuid::Uuid::new_v4().to_string(), answers: parsed.answers, backend, calibrated: false, ms };
    super::record_verdict(&db, opts, purpose, &mut verdict, &model, hash, parsed.input_tokens, parsed.cost_usd, ask.state, policy.typing);
    Attempt::Done(verdict)
}

fn after_fault(db: &Db, fault: Fault, model: &str, opts: Opts<'_>, policy: &Policy) -> Attempt {
    let line = health().lock().unwrap_or_else(|e| e.into_inner()).on_fault(&fault, model, now_ms());
    if let Some(line) = line {
        let _ = db.log_activity(opts.conversation_id, "decide", &line);
    }
    match &fault {
        Fault::KeyRejected | Fault::NoCredits => {
            crate::cloud::record_outcome(crate::cloud::Provider::OpenRouter.id(), Some(account_words(&fault)));
        }
        _ => {}
    }
    match fault {
        Fault::Rejected(m) => Attempt::Stop(DecideError::Failed(m)),
        // A person is waiting: this computer would be slower still.
        Fault::Timeout if policy.needs_fast => Attempt::Stop(DecideError::Deadline),
        _ => Attempt::Local { admitted: true },
    }
}

/// The line the Providers card shows (`PRV-4`).
fn account_words(fault: &Fault) -> String {
    match fault {
        Fault::NoCredits => "Out of credits. Top up at OpenRouter to keep using its models.".into(),
        _ => "OpenRouter rejected the key. Replace it to keep using its models.".into(),
    }
}

// ---- the catalog ----

/// One decision model in OpenRouter's catalog (`DEC-5a`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudModel {
    /// The id the catalog lists it under.
    pub id: String,
    /// What is pinned (decision 10): a dated version, so a new version is a new
    /// calibration.
    pub slug: String,
    pub name: String,
    /// Tokens, 0 when the catalog does not say.
    pub context: u32,
    pub images: bool,
    /// USD per million input tokens. Output is free.
    pub price_per_mtok: Option<f64>,
    pub free: bool,
    /// Some provider of it keeps nothing (decision 11).
    pub zdr: bool,
    /// The local catalog entry with the same weights, if any.
    pub same_as_local: Option<String>,
    /// Shown without "Show all".
    pub featured: bool,
}

/// The three Clef models and both Jev versions (`DEC-UI-1`).
const FEATURED: [&str; 5] =
    ["cloudflare/clef-flash", "cloudflare/clef", "cloudflare/clef-omni", "typesafe/jev-1.14", "typesafe/jev-1.13"];

fn same_as_local(id: &str) -> Option<&'static str> {
    match id {
        "cloudflare/clef-flash" => Some("Clef-flash"),
        "cloudflare/clef" => Some("Clef"),
        _ => None,
    }
}

/// Read the two public lists. An alias (`~vendor/model-latest`) is left out: it
/// moves, and a moved model is a calibration break (decision 10).
pub fn parse_catalog(catalog: &Value, zdr: &Value) -> Vec<CloudModel> {
    let zdr_ids: HashSet<&str> =
        zdr.get("data").and_then(Value::as_array).into_iter().flatten().filter_map(|e| e.get("model_id").and_then(Value::as_str)).collect();
    let mut out: Vec<CloudModel> = catalog
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?;
            if id.starts_with('~') {
                return None;
            }
            let slug = m.get("canonical_slug").and_then(Value::as_str).unwrap_or(id);
            let modalities = m
                .get("architecture")
                .and_then(|a| a.get("input_modalities"))
                .or_else(|| m.get("input_modalities"))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            let price = m
                .get("pricing")
                .and_then(|p| p.get("prompt"))
                .and_then(Value::as_str)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|p| *p >= 0.0)
                .map(|p| p * 1_000_000.0);
            let name = m.get("name").and_then(Value::as_str).unwrap_or(id);
            Some(CloudModel {
                id: id.to_string(),
                slug: slug.to_string(),
                // "Cloudflare: Clef Flash" is shown as "Clef Flash".
                name: name.split_once(": ").map_or(name, |(_, n)| n).to_string(),
                context: m.get("context_length").and_then(Value::as_u64).unwrap_or(0).min(u32::MAX as u64) as u32,
                images: modalities.contains(&"image"),
                free: price == Some(0.0),
                price_per_mtok: price,
                zdr: zdr_ids.contains(id) || zdr_ids.contains(slug),
                same_as_local: same_as_local(id).map(str::to_string),
                featured: FEATURED.contains(&id),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        let rank = |m: &CloudModel| FEATURED.iter().position(|f| *f == m.id).unwrap_or(FEATURED.len());
        rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
    });
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogCache {
    pub fetched_at: i64,
    pub models: Vec<CloudModel>,
}

pub fn cached_catalog(db: &Db) -> Option<CatalogCache> {
    serde_json::from_str(&db.get_setting(CATALOG_KEY).ok().flatten()?).ok()
}

async fn get_public(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    let resp = client.get(url).timeout(CATALOG_DEADLINE).send().await.map_err(|_| "Couldn't reach OpenRouter. Are you online?".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("OpenRouter answered HTTP {} for its model list.", resp.status().as_u16()));
    }
    resp.json::<Value>().await.map_err(|_| "OpenRouter's model list was not readable.".to_string())
}

/// The catalog: from the cache when it is under a day old, otherwise fetched.
/// A failed fetch keeps whatever was cached and says why.
pub async fn load_catalog(client: &reqwest::Client, db: &Db, force: bool) -> (Option<CatalogCache>, Option<String>) {
    let cache = cached_catalog(db);
    if !force && cache.as_ref().is_some_and(|c| now_ms() - c.fetched_at < CATALOG_MAX_AGE_MS) {
        return (cache, None);
    }
    let base = base(db);
    let fetched = async {
        let catalog = get_public(client, &format!("{base}/v1/models?output_modalities=decisions")).await?;
        let zdr = get_public(client, &format!("{base}/v1/endpoints/zdr")).await?;
        Ok::<_, String>(parse_catalog(&catalog, &zdr))
    }
    .await;
    match fetched {
        Ok(models) if !models.is_empty() => {
            let fresh = CatalogCache { fetched_at: now_ms(), models };
            if let Ok(json) = serde_json::to_string(&fresh) {
                let _ = db.set_setting(CATALOG_KEY, &json);
            }
            (Some(fresh), None)
        }
        Ok(_) => (cache, Some("OpenRouter listed no decision models.".into())),
        Err(e) => (cache, Some(e)),
    }
}

/// A cloud model's context in tokens, from the cached catalog, never over the cap.
fn context_of(db: &Db, slug: &str) -> u32 {
    let listed = cached_catalog(db)
        .and_then(|c| c.models.into_iter().find(|m| m.slug == slug || m.id == slug))
        .map_or(0, |m| m.context);
    (if listed == 0 { UNKNOWN_CONTEXT_TOKENS } else { listed }).min(STATE_CAP_TOKENS)
}

// ---- the probe ----

/// What one Test found out (`DEC-5a`), kept per model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub at: i64,
    pub ok: bool,
    pub ms: u32,
    /// Whether a picture put in `state` was read. `None`: not tried (a text-only
    /// model) or not told.
    pub vision: Option<bool>,
    pub error: Option<String>,
}

pub fn saved_probe(db: &Db, slug: &str) -> Option<Probe> {
    serde_json::from_str(&db.get_setting(&format!("{PROBE_PREFIX}{slug}")).ok().flatten()?).ok()
}

/// Did a Test show that this model reads a picture put in `state`?
pub fn sees_images(db: &Db, slug: &str) -> bool {
    saved_probe(db, slug).is_some_and(|p| p.ok && p.vision == Some(true))
}

/// A solid red square, as a data URL. Big enough for the models that refuse a
/// small picture.
fn red_square() -> String {
    use base64::Engine;
    use image::ImageEncoder;
    let (w, h) = (320u32, 320u32);
    let pixels = vec![[220u8, 20, 20]; (w * h) as usize].concat();
    let mut png = Vec::new();
    // Encoding a flat buffer to memory cannot fail.
    let _ = image::codecs::png::PngEncoder::new(&mut png).write_image(&pixels, w, h, image::ExtendedColorType::Rgb8);
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

fn colour_ask(picture: String) -> Ask<'static> {
    Ask {
        state: "Look at the picture.",
        images: vec![super::ImageRef { data_url: picture }],
        questions: vec![(
            "colour",
            super::Question::Choice {
                instructions: "What colour is the picture?".into(),
                options: vec![
                    ("red".into(), "The picture is red.".into()),
                    ("blue".into(), "The picture is blue.".into()),
                    ("green".into(), "The picture is green.".into()),
                    ("unsure".into(), "It cannot be told.".into()),
                ],
            },
        )],
    }
}

/// Test a model: one fixed three-question request that must come back well formed
/// within five seconds, and for a model that lists images, a second that must
/// read a red picture as red. The result is kept per model, and a pass clears
/// whatever had stopped the cloud.
pub async fn probe(client: &reqwest::Client, db: &Db, model: &CloudModel) -> Probe {
    let at = now_ms();
    let fail = |ms: u32, e: String| Probe { at, ok: false, ms, vision: None, error: Some(e) };
    let Some(key) = api_key(db) else { return fail(0, "Add an OpenRouter key in Providers first.".into()) };
    let base = base(db);
    let session = session_id(None);

    let ask = local::head_check_ask();
    let started = Instant::now();
    let sent = post(client, &base, &key, &build_body(&ask, ask.state, &model.slug, &session), PROBE_DEADLINE).await;
    let ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
    let reply = match sent {
        Ok(r) => r,
        Err(f) => {
            // A key or credit problem is the same finding a real call would make.
            if let Ok(mut h) = health().lock() {
                h.on_fault(&f, &model.slug, now_ms());
            }
            return fail(ms, fault_words(&f));
        }
    };
    if parse_response(&ask, &reply).is_err() {
        return fail(ms, "The reply was not a set of decisions.".into());
    }
    if let Ok(mut h) = health().lock() {
        h.clear();
        h.on_success(ms);
    }
    crate::cloud::record_outcome(crate::cloud::Provider::OpenRouter.id(), None);

    let vision = if model.images {
        let ask = colour_ask(red_square());
        let body = build_body(&ask, ask.state, &model.slug, &session);
        match post(client, &base, &key, &body, PROBE_DEADLINE).await {
            Ok(r) => Some(matches!(
                parse_response(&ask, &r).ok().and_then(|p| p.answers.get("colour").cloned()),
                Some(Answer::Choice { choice, .. }) if choice == "red"
            )),
            Err(_) => Some(false),
        }
    } else {
        None
    };
    Probe { at, ok: true, ms, vision, error: None }
}

pub fn save_probe(db: &Db, slug: &str, probe: &Probe) {
    if let Ok(json) = serde_json::to_string(probe) {
        let _ = db.set_setting(&format!("{PROBE_PREFIX}{slug}"), &json);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent::decide::local::tests::real_reply;
    use crate::agent::decide::Question;

    const CATALOG: &str = include_str!("../../../../fixtures/decide/openrouter-catalog.json");
    const ZDR: &str = include_str!("../../../../fixtures/decide/openrouter-zdr.json");

    fn models() -> Vec<CloudModel> {
        parse_catalog(&serde_json::from_str(CATALOG).unwrap(), &serde_json::from_str(ZDR).unwrap())
    }

    fn mixed_ask() -> Ask<'static> {
        local::head_check_ask()
    }

    // ---- DEC-T-1 (cloud half): the request ----

    #[test]
    fn the_request_names_the_model_and_asks_for_zero_retention() {
        let ask = mixed_ask();
        let body = build_body(&ask, ask.state, "cloudflare/clef-flash", "poiesis-abc");
        assert_eq!(body["model"], "cloudflare/clef-flash");
        assert_eq!(body["session_id"], "poiesis-abc");
        assert_eq!(body["provider"]["data_collection"], "deny");
        assert_eq!(body["provider"]["zdr"], true);
        assert_eq!(body["state"], ask.state);
        // The same questions as the local request: both `noul` sides, a `choice`
        // map and a `score` array.
        assert_eq!(body["questions"]["a_enough"]["criteria"]["true"], "The excerpt contains the answer.");
        assert_eq!(body["questions"]["a_enough"]["criteria"]["false"], "The excerpt does not contain the answer.");
        assert_eq!(body["questions"]["b_topic"]["criteria"]["cooking"], "Food and recipes.");
        assert!(body["questions"]["c_direct"]["criteria"].is_array());
        assert!(body.get("images").is_none(), "there is no images field in the published schema");
    }

    #[test]
    fn a_picture_rides_in_the_state_as_a_content_part() {
        let ask = colour_ask("data:image/png;base64,AAAA".into());
        let body = build_body(&ask, "Look.", "m", "s");
        assert_eq!(body["state"][0], json!({ "type": "text", "text": "Look." }));
        assert_eq!(body["state"][1]["image_url"]["url"], "data:image/png;base64,AAAA");
    }

    /// Decision: the session id is a per-run random value and never a name of the
    /// conversation. It is stable inside one run and differs between runs.
    #[test]
    fn the_session_id_is_derived_from_the_run_and_shows_nothing_of_it() {
        let a = session_id(Some("run-1234-secret"));
        assert!(a.starts_with("poiesis-") && !a.contains("run-1234") && !a.contains("secret"), "{a}");
        assert_eq!(a, session_id(Some("run-1234-secret")));
        assert_ne!(a, session_id(Some("run-9999")));
        assert_eq!(session_id(None), session_id(None), "one per launch");
    }

    // ---- DEC-T-1 (cloud half): the reply ----

    #[test]
    fn a_reply_is_read_like_the_local_one_with_cost_and_provider() {
        let ask = mixed_ask();
        let mut reply = real_reply();
        reply["usage"] = json!({ "input_tokens": 334, "output_tokens": 0, "cost": 0.0000063 });
        reply["model"] = json!("cloudflare/clef-flash");
        reply["provider"] = json!("DeepInfra");
        let got = parse_response(&ask, &reply).unwrap();
        assert_eq!(got.input_tokens, 334);
        assert_eq!(got.cost_usd, Some(0.0000063));
        assert_eq!(got.model.as_deref(), Some("cloudflare/clef-flash"));
        assert_eq!(got.provider.as_deref(), Some("DeepInfra"));
        assert!(matches!(got.answers["a_enough"], Answer::Noul { p_true } if p_true > 0.9));
    }

    /// The service documents a score's probabilities as a list; the local engine
    /// answers a map. Either reads the same.
    #[test]
    fn a_score_with_its_probabilities_as_a_list_is_read() {
        let ask = mixed_ask();
        let mut reply = real_reply();
        reply["answers"]["c_direct"]["probabilities"] = json!([0.02, 0.04, 0.94]);
        reply["provider"] = json!({ "name": "Cloudflare" });
        let got = parse_response(&ask, &reply).unwrap();
        let Answer::Score { probs, .. } = &got.answers["c_direct"] else { panic!("not a score") };
        assert_eq!(probs.len(), 3);
        assert!(probs[2] > 0.9);
        assert_eq!(got.provider.as_deref(), Some("Cloudflare"));
        assert_eq!(got.cost_usd, None, "no cost, no number");
    }

    #[test]
    fn a_reply_without_answers_is_refused() {
        assert!(parse_response(&mixed_ask(), &json!({ "id": "x" })).is_err());
    }

    // ---- DEC-T-1: the per-status policy ----

    #[test]
    fn each_status_is_the_failure_the_plan_names() {
        assert_eq!(classify(401, ""), Fault::KeyRejected);
        assert_eq!(classify(403, ""), Fault::KeyRejected);
        assert_eq!(classify(402, ""), Fault::NoCredits);
        assert_eq!(classify(404, ""), Fault::NoEndpoint);
        for s in [429, 500, 502, 503, 524, 529] {
            assert_eq!(classify(s, ""), Fault::Overloaded(s), "{s}");
        }
        assert!(matches!(classify(400, "bad state"), Fault::Rejected(m) if m.contains("bad state")));
        assert!(matches!(classify(413, ""), Fault::Rejected(_)));
        assert!(matches!(classify(418, "x"), Fault::Other(418, _)));
    }

    #[test]
    fn a_rejected_key_and_missing_credits_stop_the_cloud_once() {
        let mut h = Health::default();
        let t = 1_000_000;
        assert!(h.usable("m", t));
        let line = h.on_fault(&Fault::KeyRejected, "m", t).expect("a first stop is told");
        assert!(line.contains("rejected the key"));
        assert!(!h.usable("m", t + 1));
        assert_eq!(h.on_fault(&Fault::KeyRejected, "m", t + 5), None, "told once, not on every call");
        assert!(h.usable("m", t + KEY_BLOCK_MS + 1), "an hour later it is tried again");

        let mut h = Health::default();
        assert!(h.on_fault(&Fault::NoCredits, "m", t).unwrap().contains("credits"));
        assert!(!h.usable("m", t + COOLDOWN_MS - 1));
        assert!(h.usable("m", t + COOLDOWN_MS + 1));
    }

    #[test]
    fn three_busy_answers_in_five_minutes_stop_it_for_ten() {
        let mut h = Health::default();
        let t = 5_000_000;
        assert_eq!(h.on_fault(&Fault::Overloaded(529), "m", t), None);
        assert_eq!(h.on_fault(&Fault::Overloaded(503), "m", t + 60_000), None);
        assert!(h.usable("m", t + 61_000), "two do not stop it");
        let line = h.on_fault(&Fault::Overloaded(429), "m", t + 120_000).expect("the third stops it");
        assert!(line.contains("overloaded"));
        assert!(!h.usable("m", t + 120_001 + COOLDOWN_MS - 2));
        assert!(h.usable("m", t + 120_000 + COOLDOWN_MS + 1));
    }

    #[test]
    fn busy_answers_spread_over_more_than_five_minutes_do_not_add_up() {
        let mut h = Health::default();
        for i in 0..6 {
            h.on_fault(&Fault::Overloaded(503), "m", i * (OVERLOAD_WINDOW_MS / 2 + 1));
        }
        assert!(h.usable("m", 6 * (OVERLOAD_WINDOW_MS / 2 + 1)));
    }

    #[test]
    fn no_endpoint_stops_only_that_model_until_the_person_changes_something() {
        let mut h = Health::default();
        assert!(h.on_fault(&Fault::NoEndpoint, "a/one", 1).unwrap().contains("zero-retention"));
        assert!(!h.usable("a/one", 2));
        assert!(h.usable("b/two", 2));
        h.clear();
        assert!(h.usable("a/one", 3));
    }

    #[test]
    fn a_failure_of_the_other_kinds_is_not_counted() {
        let mut h = Health::default();
        for f in [Fault::Timeout, Fault::Offline("x".into()), Fault::Rejected("x".into()), Fault::Other(418, "x".into())] {
            assert_eq!(h.on_fault(&f, "m", 1), None);
        }
        assert!(h.usable("m", 2));
    }

    #[test]
    fn the_median_of_the_last_answers_is_kept() {
        let mut h = Health::default();
        assert_eq!(h.median_ms(), None);
        for ms in [900, 100, 300] {
            h.on_success(ms);
        }
        assert_eq!(h.median_ms(), Some(300));
    }

    // ---- DEC-T-1a: the catalog ----

    #[test]
    fn the_recorded_catalog_is_read() {
        let all = models();
        assert_eq!(all.len(), 27, "30 listed, three aliases left out");
        assert!(all.iter().all(|m| !m.id.starts_with('~')));
        let flash = all.iter().find(|m| m.id == "cloudflare/clef-flash").unwrap();
        assert_eq!(flash.slug, "cloudflare/clef-flash");
        assert!(flash.images && flash.zdr && flash.featured);
        assert!((flash.price_per_mtok.unwrap() - 0.019).abs() < 1e-9, "{:?}", flash.price_per_mtok);
        assert_eq!(flash.same_as_local.as_deref(), Some("Clef-flash"));
        assert_eq!(flash.name, "Clef Flash");
        assert_eq!(flash.context, 24_576);
    }

    /// Decision 10: what is pinned is the dated version, not the id a vendor can
    /// move.
    #[test]
    fn a_dated_model_is_pinned_by_its_canonical_slug() {
        let all = models();
        let jev = all.iter().find(|m| m.id == "typesafe/jev-1.14").unwrap();
        assert_eq!(jev.slug, "typesafe/jev-1.14-20261014");
        assert!(jev.featured && jev.zdr && jev.images);
        assert_eq!(jev.same_as_local, None);
    }

    /// Decision 11: a model with no zero-retention endpoint is listed, and marked,
    /// so the screen can disable it.
    #[test]
    fn a_model_with_no_zero_retention_endpoint_is_marked() {
        let all = models();
        let luna = all.iter().find(|m| m.id == "openai/gpt-6-luna-decisions").unwrap();
        assert!(!luna.zdr);
        assert!(all.iter().filter(|m| m.zdr).count() >= 20);
    }

    #[test]
    fn free_models_are_free_and_text_only_models_have_no_images() {
        let all = models();
        let free = all.iter().find(|m| m.id == "baseten/elephant-1:free").unwrap();
        assert!(free.free && !free.images);
        assert_eq!(free.price_per_mtok, Some(0.0));
        assert!(!all.iter().find(|m| m.id == "nace-ai/drex-v2.0").unwrap().images);
    }

    #[test]
    fn the_featured_models_come_first_in_the_screens_order() {
        let all = models();
        let head: Vec<&str> = all.iter().take(5).map(|m| m.id.as_str()).collect();
        assert_eq!(
            head,
            ["cloudflare/clef-flash", "cloudflare/clef", "cloudflare/clef-omni", "typesafe/jev-1.14", "typesafe/jev-1.13"]
        );
        assert_eq!(all.iter().filter(|m| m.featured).count(), 5);
    }

    #[test]
    fn a_catalog_with_nothing_in_it_lists_nothing() {
        assert!(parse_catalog(&json!({}), &json!({})).is_empty());
    }

    // ---- the call, against a server that answers once ----

    /// Answer one request with `status` and `body`, and say what was asked.
    pub(crate) async fn serve_once(status: u16, body: String) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 8192];
            // Read until the headers and the whole body are in.
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&seen).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let len = text[..split]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if seen.len() >= split + 4 + len {
                        break;
                    }
                }
            }
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&seen).to_string()
        });
        (base, handle)
    }

    #[tokio::test]
    async fn a_request_carries_the_key_and_a_good_reply_comes_back() {
        let (base, seen) = serve_once(200, real_reply().to_string()).await;
        let ask = mixed_ask();
        let body = build_body(&ask, ask.state, "cloudflare/clef-flash", "poiesis-x");
        let got = post(&reqwest::Client::new(), &base, "sk-or-test", &body, Duration::from_secs(5)).await.unwrap();
        assert!(parse_response(&ask, &got).is_ok());
        let request = seen.await.unwrap();
        assert!(request.starts_with("POST /alpha/decisions"), "{request}");
        assert!(request.to_ascii_lowercase().contains("authorization: bearer sk-or-test"));
        assert!(request.contains("\"zdr\":true"));
    }

    #[tokio::test]
    async fn an_error_status_is_the_fault_it_means() {
        for (status, want) in [(402, Fault::NoCredits), (401, Fault::KeyRejected), (404, Fault::NoEndpoint), (529, Fault::Overloaded(529))] {
            let (base, _seen) = serve_once(status, "{}".into()).await;
            let got = post(&reqwest::Client::new(), &base, "k", &json!({}), Duration::from_secs(5)).await;
            assert_eq!(got.unwrap_err(), want, "{status}");
        }
    }

    #[tokio::test]
    async fn a_server_that_does_not_answer_in_time_is_a_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let _hold = tokio::spawn(async move {
            let _keep = listener.accept().await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let got = post(&reqwest::Client::new(), &base, "k", &json!({}), Duration::from_millis(150)).await;
        assert_eq!(got.unwrap_err(), Fault::Timeout);
    }

    #[tokio::test]
    async fn nothing_listening_is_offline() {
        // A port that was just open and is not any more refuses at once.
        let port = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        let got = post(&reqwest::Client::new(), &format!("http://127.0.0.1:{port}"), "k", &json!({}), Duration::from_secs(5)).await;
        assert!(matches!(got.unwrap_err(), Fault::Offline(_)));
    }

    // ---- DEC-5a: the probe ----

    fn clef(images: bool) -> CloudModel {
        CloudModel {
            id: "cloudflare/clef-flash".into(),
            slug: "cloudflare/clef-flash".into(),
            name: "Clef Flash".into(),
            context: 24_576,
            images,
            price_per_mtok: Some(0.019),
            free: false,
            zdr: true,
            same_as_local: None,
            featured: true,
        }
    }

    fn db_with(base: &str) -> Db {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(URL_KEY, base).unwrap();
        db.set_setting(TEST_KEY_KEY, "sk-or-test").unwrap();
        db
    }

    #[tokio::test]
    async fn a_text_probe_that_comes_back_well_formed_passes() {
        let _turn = test_lock();
        let (base, _seen) = serve_once(200, real_reply().to_string()).await;
        let p = probe(&reqwest::Client::new(), &db_with(&base), &clef(false)).await;
        assert!(p.ok, "{p:?}");
        assert_eq!(p.vision, None, "a text-only model is not asked about pictures");
        assert!(p.error.is_none());
    }

    #[tokio::test]
    async fn a_probe_with_no_key_says_where_to_add_one() {
        let _turn = test_lock();
        let db = Db::open_in_memory().unwrap();
        let p = probe(&reqwest::Client::new(), &db, &clef(false)).await;
        assert!(!p.ok);
        assert!(p.error.unwrap().contains("Providers"));
    }

    #[tokio::test]
    async fn a_probe_that_is_refused_says_why() {
        let _turn = test_lock();
        let (base, _seen) = serve_once(402, "{}".into()).await;
        let p = probe(&reqwest::Client::new(), &db_with(&base), &clef(false)).await;
        assert!(!p.ok);
        assert!(p.error.unwrap().contains("out of credits"));
        forget_faults();
    }

    #[tokio::test]
    async fn a_probe_reply_that_is_not_decisions_fails() {
        let _turn = test_lock();
        let (base, _seen) = serve_once(200, json!({ "choices": [{ "message": { "content": "Paris" } }] }).to_string()).await;
        let p = probe(&reqwest::Client::new(), &db_with(&base), &clef(false)).await;
        assert!(!p.ok);
        assert!(p.error.unwrap().contains("not a set of decisions"));
    }

    #[test]
    fn the_probe_picture_is_a_png_data_url() {
        let url = red_square();
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(url.len() > 100);
    }

    #[test]
    fn a_probe_is_kept_per_model_and_a_pass_with_pictures_unlocks_them() {
        let db = Db::open_in_memory().unwrap();
        assert!(!sees_images(&db, "m"));
        save_probe(&db, "m", &Probe { at: 1, ok: true, ms: 10, vision: Some(true), error: None });
        assert!(sees_images(&db, "m"));
        save_probe(&db, "m", &Probe { at: 2, ok: true, ms: 10, vision: Some(false), error: None });
        assert!(!sees_images(&db, "m"));
        assert!(!sees_images(&db, "other"));
    }

    #[test]
    fn the_cap_is_the_listed_context_but_never_over_the_ceiling() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(context_of(&db, "unknown/model"), UNKNOWN_CONTEXT_TOKENS);
        let cache = CatalogCache { fetched_at: now_ms(), models: models() };
        db.set_setting(CATALOG_KEY, &serde_json::to_string(&cache).unwrap()).unwrap();
        assert_eq!(context_of(&db, "cloudflare/clef-flash"), 24_576);
        assert_eq!(context_of(&db, "typesafe/jev-1.14-20261014"), STATE_CAP_TOKENS, "262k listed, 64k used");
        assert_eq!(context_of(&db, "respan/span-01-20260925"), UNKNOWN_CONTEXT_TOKENS, "0 listed means unknown");
    }

    #[test]
    fn the_status_says_why_the_cloud_is_not_used() {
        let db = Db::open_in_memory().unwrap();
        let mut h = Health::default();
        let s = status_in(&db, &h, 1);
        assert_eq!((s.state, s.backend.as_str(), s.model.as_str()), ("no-key", "local", DEFAULT_MODEL));
        db.set_setting(TEST_KEY_KEY, "k").unwrap();
        db.set_setting(BACKEND_KEY, "cloud").unwrap();
        db.set_setting(MODEL_KEY, "typesafe/jev-1.14-20261014").unwrap();
        let s = status_in(&db, &h, 1);
        assert_eq!((s.state, s.backend.as_str(), s.model.as_str()), ("ready", "cloud", "typesafe/jev-1.14-20261014"));
        h.on_fault(&Fault::NoCredits, "x", 1_000);
        let s = status_in(&db, &h, 2_000);
        assert_eq!(s.state, "credits");
        assert_eq!(s.until, Some(1_000 + COOLDOWN_MS));
        assert_eq!(status_in(&db, &h, 1_000 + COOLDOWN_MS + 1).state, "ready", "a stop that has ended is not shown");
        h.on_fault(&Fault::NoEndpoint, "typesafe/jev-1.14-20261014", 5);
        assert_eq!(status_in(&db, &h, 1_000 + COOLDOWN_MS + 1).state, "no-endpoint");
        let mut h = Health::default();
        h.on_fault(&Fault::KeyRejected, "x", 10);
        assert_eq!(status_in(&db, &h, 11).state, "key");
    }

    /// What lets the rest of the app count the cloud as a decision model: chosen
    /// with a key, or, for one background purpose, allowed by name with a key.
    #[test]
    fn the_cloud_counts_only_when_chosen_or_allowed_and_there_is_a_key() {
        let db = Db::open_in_memory().unwrap();
        assert!(!chosen(&db) && !allowed_for(&db, Purpose::MailTriage));
        db.set_setting(BACKEND_KEY, "cloud").unwrap();
        assert!(!chosen(&db), "chosen without a key is not usable");
        db.set_setting(TEST_KEY_KEY, "k").unwrap();
        assert!(chosen(&db));
        assert!(!allowed_for(&db, Purpose::MailTriage), "the global choice never allows a background purpose");
        db.set_setting(&super::super::policy::override_key(Purpose::MailTriage), "cloud").unwrap();
        assert!(allowed_for(&db, Purpose::MailTriage));
        db.set_setting(TEST_KEY_KEY, "").unwrap();
        assert!(!chosen(&db) && !allowed_for(&db, Purpose::MailTriage));
    }

    #[test]
    fn a_choice_question_would_be_built_the_same_for_both_engines() {
        let q = Question::Choice { instructions: "x".into(), options: vec![("a".into(), "A".into()), ("b".into(), "B".into())] };
        assert_eq!(local::question_body(&q)["criteria"]["a"], "A");
    }
}
