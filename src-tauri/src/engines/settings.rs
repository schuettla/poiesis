//! What the user may change on an engine, and the checks that keep it safe
//! (`CLI-12`). Every edit in Settings → External agents goes through
//! `apply_patch`; nothing is trusted because the UI sent it.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::db::engines::EngineRow;

use super::catalog::CatalogEntry;

/// Any subset of the editable columns. `""` clears a text value.
#[derive(Debug, Default, Deserialize)]
pub struct EnginePatch {
    pub enabled: Option<bool>,
    pub program: Option<String>,
    pub payment: Option<String>,
    pub trust_ceiling: Option<String>,
    pub headless: Option<bool>,
    pub memory_tools: Option<bool>,
    /// `CAP-14`.
    pub app_tools: Option<bool>,
    pub model: Option<String>,
    pub extra_args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    /// `RTE-1`: `on_request` | `suggest` | `auto`.
    pub use_mode: Option<String>,
    /// `RTE-3`: the guards, whole. The screen always sends all four.
    pub guards: Option<super::rules::Guards>,
    /// Custom agents only.
    pub display_name: Option<String>,
    /// Custom agents only: the arguments that start the protocol.
    pub args: Option<Vec<String>>,
}

const MAX_ARGS: usize = 32;
const MAX_ARG_LEN: usize = 200;
const MAX_ENV: usize = 32;

/// Flags that would take a decision away from the panel, change who pays or
/// where the agent works, or turn off its sign-in. Matched on the part before
/// any `=`, case-insensitively, as a prefix.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "--dangerously",
    "--permission",
    "--allowed",
    "--allow-",
    "--disallowed",
    "--yolo",
    "--full-auto",
    "--approval",
    "--ask-for-approval",
    "--sandbox",
    "--api-key",
    "--apikey",
    "--token",
    "--auth",
    "--login",
    "--mcp-config",
    "--settings",
    "--setting-sources",
    "--config",
    "--add-dir",
    "--cwd",
    "--cd",
    "--model",
    "--resume",
    "--continue",
    "--session",
    "--output-format",
    "--input-format",
    "--print",
    "--listen",
];
const FORBIDDEN_EXACT: &[&str] = &["-s", "-c", "-C", "-p", "-m", "-r"];

/// A character `cmd.exe` would read as its own when it starts an npm shim.
const SHELL_CHARS: &[char] = &['"', '%', '^', '&', '|', '<', '>', '!', '\n', '\r', '\0'];

pub fn validate_args(args: &[String]) -> Result<Vec<String>, String> {
    if args.len() > MAX_ARGS {
        return Err(format!("At most {MAX_ARGS} extra arguments."));
    }
    let mut out = Vec::new();
    for raw in args {
        let a = raw.trim();
        if a.is_empty() {
            continue;
        }
        if a.chars().count() > MAX_ARG_LEN {
            return Err(format!("An argument is longer than {MAX_ARG_LEN} characters."));
        }
        if a.chars().any(|c| SHELL_CHARS.contains(&c) || c.is_control()) {
            return Err(format!("“{a}” has a character I can't pass on safely."));
        }
        let name = a.split('=').next().unwrap_or(a);
        let lower = name.to_ascii_lowercase();
        if FORBIDDEN_EXACT.contains(&name) || FORBIDDEN_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            return Err(format!(
                "I set “{name}” myself, or it would change who decides or who pays. Use the settings above instead."
            ));
        }
        out.push(a.to_string());
    }
    Ok(out)
}

/// Variables whose name says they hold a secret. A secret never goes into the
/// database; the vault is for that.
fn looks_like_secret(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL"].iter().any(|w| up.contains(w))
}

pub fn validate_env(env: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    if env.len() > MAX_ENV {
        return Err(format!("At most {MAX_ENV} variables."));
    }
    let mut out = BTreeMap::new();
    for (k, v) in env {
        let name = k.trim();
        let ok_name = !name.is_empty()
            && name.len() <= 64
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok_name {
            return Err(format!("“{name}” is not a valid variable name."));
        }
        if name.to_ascii_uppercase().starts_with("POIESIS_") {
            return Err("Names starting with POIESIS_ are mine.".into());
        }
        if looks_like_secret(name) {
            return Err(format!("{name} looks like a secret. Use Billing → API key, so I keep it in the vault."));
        }
        if v.len() > 1000 || v.contains(['\0', '\n', '\r']) {
            return Err(format!("The value of {name} is not usable."));
        }
        out.insert(name.to_string(), v.clone());
    }
    Ok(out)
}

fn validate_program(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err("Give the full path to the app.".into());
    }
    if !p.is_file() {
        return Err("There is no file at that path.".into());
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if cfg!(windows) && !["exe", "cmd", "bat", "com"].contains(&ext.as_str()) {
        return Err("That doesn't look like a program (.exe, .cmd or .bat).".into());
    }
    Ok(())
}

fn validate_text(s: &str, max: usize, what: &str) -> Result<String, String> {
    let s = s.trim();
    if s.chars().count() > max || s.chars().any(|c| c.is_control()) {
        return Err(format!("{what} is not usable."));
    }
    Ok(s.to_string())
}

/// Apply `patch` to `row`, or say why not. Nothing is written here.
pub fn apply_patch(row: &mut EngineRow, patch: &EnginePatch, entry: Option<&CatalogEntry>) -> Result<(), String> {
    let custom = entry.is_none();
    if let Some(p) = &patch.program {
        let p = p.trim();
        if p.is_empty() {
            row.program = None;
        } else {
            validate_program(p)?;
            row.program = Some(p.to_string());
        }
    }
    if let Some(p) = &patch.payment {
        match p.as_str() {
            "plan" => {}
            "api-key" => {
                if entry.and_then(|e| e.api_key_env.as_ref()).is_none() {
                    return Err("I don't know an API-key mode for this app yet. Use its own sign-in.".into());
                }
            }
            _ => return Err("Pay with “plan” or “api-key”.".into()),
        }
        row.payment = p.clone();
    }
    if let Some(t) = &patch.trust_ceiling {
        if !["read-only", "confirm", "auto"].contains(&t.as_str()) {
            return Err("At most: read only, ask first, or edits without asking.".into());
        }
        row.trust_ceiling = t.clone();
    }
    if let Some(h) = patch.headless {
        row.headless = h;
    }
    if let Some(m) = &patch.use_mode {
        if !super::rules::valid_mode(m) {
            return Err("Use it only when asked, suggest it, or let it go on its own.".into());
        }
        row.use_mode = m.clone();
    }
    if let Some(g) = &patch.guards {
        g.validate()?;
        row.guards_json = g.to_json();
    }
    if let Some(m) = patch.memory_tools {
        row.memory_tools = m;
    }
    if let Some(a) = patch.app_tools {
        row.app_tools = a;
    }
    if let Some(m) = &patch.model {
        let m = validate_text(m, 100, "The model name")?;
        row.model = if m.is_empty() { None } else { Some(m) };
    }
    if let Some(a) = &patch.extra_args {
        row.extra_args = serde_json::to_string(&validate_args(a)?).unwrap_or_else(|_| "[]".into());
    }
    if let Some(e) = &patch.env {
        row.env_json = serde_json::to_string(&validate_env(e)?).unwrap_or_else(|_| "{}".into());
    }
    if patch.display_name.is_some() || patch.args.is_some() {
        if !custom {
            return Err("The name and start arguments of a built-in agent are fixed.".into());
        }
        if let Some(n) = &patch.display_name {
            let n = validate_text(n, 60, "The name")?;
            if n.is_empty() {
                return Err("Give it a name.".into());
            }
            row.display_name = n;
        }
        if let Some(a) = &patch.args {
            row.args_json = serde_json::to_string(&validate_args(a)?).unwrap_or_else(|_| "[]".into());
        }
    }
    if let Some(on) = patch.enabled {
        // Last, so a patch that also sets `program` is judged on the new path.
        if on && row.found_path.is_none() && row.program.is_none() {
            return Err("I can't find it on this computer yet.".into());
        }
        row.enabled = on;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::catalog;

    fn row(id: &str) -> EngineRow {
        EngineRow::new(id, "claude-code", "Claude Code")
    }

    fn strs(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extra_arguments_cannot_take_decisions_away_from_the_panel() {
        assert!(validate_args(&strs(&["--verbose", "--max-turns=3"])).is_ok());
        for bad in [
            "--dangerously-skip-permissions",
            "--permission-mode=plan",
            "--allowedTools=Bash",
            "--yolo",
            "--dangerously-bypass-approvals-and-sandbox",
            "--sandbox",
            "-c",
            "--config=approval_policy=never",
            "--api-key=sk-x",
            "--add-dir",
            "--model=x",
            "--mcp-config",
            "--resume",
        ] {
            assert!(validate_args(&strs(&[bad])).is_err(), "{bad}");
        }
    }

    #[test]
    fn extra_arguments_cannot_carry_shell_characters() {
        for bad in ["a&b", "a|b", "%PATH%", "a\"b", "a^b", "a>b"] {
            assert!(validate_args(&strs(&[bad])).is_err(), "{bad}");
        }
        assert_eq!(validate_args(&strs(&["  ", " --x "])).unwrap(), vec!["--x"]);
        assert!(validate_args(&vec!["--x".to_string(); 40]).is_err());
    }

    #[test]
    fn a_secret_cannot_be_stored_as_an_environment_variable() {
        let mut env = BTreeMap::new();
        env.insert("HTTP_PROXY".to_string(), "http://p:8080".to_string());
        assert!(validate_env(&env).is_ok());
        for bad in ["OPENAI_API_KEY", "github_token", "MY_SECRET", "DB_PASSWORD", "POIESIS_X", "1ABC", "A B"] {
            let mut e = BTreeMap::new();
            e.insert(bad.to_string(), "v".to_string());
            assert!(validate_env(&e).is_err(), "{bad}");
        }
    }

    #[test]
    fn api_key_mode_is_only_offered_where_the_catalog_knows_a_variable() {
        let mut r = row("claude-code");
        let claude = catalog::find("claude-code");
        let p = EnginePatch { payment: Some("api-key".into()), ..Default::default() };
        assert!(apply_patch(&mut r, &p, claude).is_ok());
        assert_eq!(r.payment, "api-key");
        let mut r = row("codex");
        assert!(apply_patch(&mut r, &p, catalog::find("codex")).is_err(), "Codex's API-key login is not recorded yet");
        assert_eq!(r.payment, "plan", "a refused patch changes nothing");
    }

    #[test]
    fn an_engine_that_was_never_found_cannot_be_turned_on() {
        let mut r = row("claude-code");
        let on = EnginePatch { enabled: Some(true), ..Default::default() };
        assert!(apply_patch(&mut r, &on, catalog::find("claude-code")).is_err());
        r.found_path = Some(r"C:\x\claude.exe".into());
        assert!(apply_patch(&mut r, &on, catalog::find("claude-code")).is_ok());
        assert!(r.enabled);
    }

    #[test]
    fn built_in_agents_keep_their_name_and_start_arguments() {
        let mut r = row("claude-code");
        let p = EnginePatch { display_name: Some("Mine".into()), ..Default::default() };
        assert!(apply_patch(&mut r, &p, catalog::find("claude-code")).is_err());
        let mut c = row("acp:custom-1");
        assert!(apply_patch(&mut c, &p, None).is_ok());
        assert_eq!(c.display_name, "Mine");
    }

    /// `CAP-14`: reaching the person's apps is its own choice, off until made, and
    /// turning memory on or off does not move it.
    #[test]
    fn the_apps_switch_is_a_choice_of_its_own() {
        let mut r = row("claude-code");
        assert!(!r.app_tools && !r.memory_tools);
        apply_patch(&mut r, &EnginePatch { memory_tools: Some(true), ..Default::default() }, None).unwrap();
        assert!(r.memory_tools && !r.app_tools);
        apply_patch(&mut r, &EnginePatch { app_tools: Some(true), ..Default::default() }, None).unwrap();
        apply_patch(&mut r, &EnginePatch { memory_tools: Some(false), ..Default::default() }, None).unwrap();
        assert!(r.app_tools && !r.memory_tools);
    }

    #[test]
    fn model_and_trust_are_checked() {
        let mut r = row("claude-code");
        let p = EnginePatch { trust_ceiling: Some("yolo".into()), ..Default::default() };
        assert!(apply_patch(&mut r, &p, None).is_err());
        let p = EnginePatch { model: Some("  sonnet ".into()), trust_ceiling: Some("read-only".into()), ..Default::default() };
        apply_patch(&mut r, &p, None).unwrap();
        assert_eq!(r.model.as_deref(), Some("sonnet"));
        let p = EnginePatch { model: Some(String::new()), ..Default::default() };
        apply_patch(&mut r, &p, None).unwrap();
        assert_eq!(r.model, None, "empty means the agent's own default");
    }

    #[test]
    fn a_program_must_be_a_real_file_at_a_full_path() {
        let mut r = row("claude-code");
        for bad in ["claude", r"C:\definitely\not\there.exe"] {
            let p = EnginePatch { program: Some(bad.into()), ..Default::default() };
            assert!(apply_patch(&mut r, &p, None).is_err(), "{bad}");
        }
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("claude.exe");
        std::fs::write(&exe, b"").unwrap();
        let p = EnginePatch { program: Some(exe.to_string_lossy().into()), ..Default::default() };
        apply_patch(&mut r, &p, None).unwrap();
        assert!(r.program.is_some());
        let p = EnginePatch { program: Some(String::new()), ..Default::default() };
        apply_patch(&mut r, &p, None).unwrap();
        assert!(r.program.is_none(), "reset goes back to auto-detect");
    }

    /// `RTE-1`/`RTE-3`: the mode and the guards are checked like the rest.
    #[test]
    fn the_mode_and_the_guards_are_checked() {
        use crate::engines::rules::Guards;
        let mut r = row("claude-code");
        assert_eq!(r.use_mode, "suggest");
        let bad = EnginePatch { use_mode: Some("always".into()), ..Default::default() };
        assert!(apply_patch(&mut r, &bad, None).is_err());
        let ok = EnginePatch { use_mode: Some("auto".into()), ..Default::default() };
        apply_patch(&mut r, &ok, None).unwrap();
        assert_eq!(r.use_mode, "auto");

        let mut g = Guards::resolve("{}", "claude-code");
        g.max_per_conversation = 0;
        let bad = EnginePatch { guards: Some(g.clone()), ..Default::default() };
        assert!(apply_patch(&mut r, &bad, None).is_err());
        assert_eq!(r.guards_json, "{}", "a refused patch changes nothing");
        g.max_per_conversation = 3;
        g.code_projects_only = false;
        apply_patch(&mut r, &EnginePatch { guards: Some(g), ..Default::default() }, None).unwrap();
        let back = Guards::resolve(&r.guards_json, &r.kind);
        assert_eq!((back.max_per_conversation, back.code_projects_only), (3, false));
    }
}
