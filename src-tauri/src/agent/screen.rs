//! The System toolset (`SYS-1`): a screenshot, and launching an application
//! by name. Deliberately narrow — this is not desktop GUI automation. No
//! mouse/keyboard synthesis, no arbitrary shell: `run_code` covers
//! computation, `Browser` covers driving a page, and this covers exactly the
//! two things neither does.

use std::path::Path;

use crate::autonomy::{autonomy_gate, Rung};
use crate::permissions::{Decision, PermissionRequest};

use super::toolsets::ToolContext;

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "screenshot",
                "description": "Take a picture of the screen, for the person you're talking to. Asks first — a screenshot can contain anything.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "target": { "type": "string", "enum": ["screen", "window"], "description": "Defaults to the whole (primary) screen; \"window\" captures the focused window." }
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "open_app",
                "description": "Launch an application by name (e.g. \"Notepad\"), optionally with a document to open.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "document": { "type": "string", "description": "Optional path to open with it." }
                    },
                    "required": ["name"]
                }
            }
        }
    ])
}

pub fn handles(name: &str) -> bool {
    matches!(name, "screenshot" | "open_app")
}

pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
        "screenshot" => {
            let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("screen");
            ("photographed".into(), format!("the {target}"))
        }
        "open_app" => {
            let app = args.get("name").and_then(|v| v.as_str()).unwrap_or("an app");
            ("opened".into(), app.to_string())
        }
        other => (other.into(), String::new()),
    }
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "screenshot" => screenshot(ctx, args).await,
        "open_app" => open_app(ctx, args).await,
        other => Err(format!("System doesn't handle '{other}'.")),
    }
}

async fn screenshot(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let window = args.get("target").and_then(|v| v.as_str()) == Some("window");

    match autonomy_gate(ctx.db, "screen") {
        Rung::Off => return Ok("I'm not allowed to take screenshots right now.".to_string()),
        Rung::Ask => {
            if ctx.headless {
                return Err(
                    "Taking a screenshot needs someone to approve it, and an unattended run \
                     can't ask."
                        .to_string(),
                );
            }
            let id = format!("perm_{}", uuid::Uuid::new_v4());
            let rx = ctx.perms.open_request(&id);
            ctx.sink.send_permission(PermissionRequest::capability(
                id,
                "screen",
                "I'd like to take a picture of your screen".to_string(),
                String::new(),
            ));
            match rx.await.unwrap_or(Decision::Deny) {
                Decision::Deny => return Err("You declined the screenshot.".to_string()),
                Decision::Forever => {
                    let _ = ctx.db.set_setting(&crate::autonomy::setting_key("screen"), "auto");
                }
                Decision::Once | Decision::Chat => {}
            }
        }
        Rung::Auto => {}
    }

    let path = capture(ctx.data_dir, window)?;
    Ok(format!(
        "Saved a screenshot to {}. I have no way to look at images myself yet — tell me what \
         matters in it if you'd like me to act on it.",
        path.display()
    ))
}

fn capture(data_dir: &Path, window: bool) -> Result<std::path::PathBuf, String> {
    let dir = data_dir.join("screenshots");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.png", uuid::Uuid::new_v4()));

    let image = if window {
        let windows = xcap::Window::all().map_err(|e| e.to_string())?;
        let focused = windows
            .into_iter()
            .find(|w| w.is_focused().unwrap_or(false))
            .ok_or("I can't tell which window is focused")?;
        focused.capture_image().map_err(|e| e.to_string())?
    } else {
        let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
        let primary = monitors
            .into_iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .ok_or("I don't see a display to capture")?;
        primary.capture_image().map_err(|e| e.to_string())?
    };
    image.save(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Launches by name via `Command::new` (PATH lookup, no shell interpreter) —
/// exactly the "application name and optional document, never arbitrary
/// arguments" shape the tool advertises. Not full `ShellExecute`-style
/// App-Paths resolution: an app not on `PATH` fails with a clear error
/// rather than silently trying something cleverer.
async fn open_app(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let app_name = args
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or("missing 'name' argument")?;
    let document = args.get("document").and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
    // "An application name and optional document path, never arbitrary
    // arguments" — without this, `open_app("cmd", "/c ...")` is a shell by
    // another name, and the consent card would say only "open cmd".
    if let Some(doc) = document {
        if !Path::new(doc).is_file() {
            return Err(format!(
                "'{doc}' isn't a file I can see — `document` takes a path to open, not arguments."
            ));
        }
    }

    if !ctx.db.has_capability_grant("open-app", app_name).unwrap_or(false) {
        if ctx.headless {
            return Err(
                "Opening an app needs someone to approve it, and an unattended run can't ask."
                    .to_string(),
            );
        }
        let id = format!("perm_{}", uuid::Uuid::new_v4());
        let rx = ctx.perms.open_request(&id);
        let summary = match document {
            Some(doc) => format!("I'd like to open {app_name} with {doc}"),
            None => format!("I'd like to open {app_name}"),
        };
        ctx.sink.send_permission(PermissionRequest::capability(
            id,
            "open-app",
            summary,
            // The grant is per app, not per document — so "Always allow" says
            // the app's name and nothing more.
            app_name.to_string(),
        ));
        match rx.await.unwrap_or(Decision::Deny) {
            Decision::Deny => return Err(format!("You declined letting me open {app_name}.")),
            Decision::Forever => {
                let _ = ctx.db.add_capability_grant("open-app", app_name);
            }
            Decision::Once | Decision::Chat => {}
        }
    }

    let mut cmd = std::process::Command::new(app_name);
    if let Some(doc) = document {
        cmd.arg(doc);
    }
    cmd.spawn()
        .map(|_| format!("Opened {app_name}."))
        .map_err(|e| format!("couldn't open {app_name}: {e}"))
}
