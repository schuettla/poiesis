//! Built-in Code Execution skill (TOOL-5). Runs short Python or Node snippets in
//! an isolated **Job-Object-confined subprocess** (reusing `runtime/jobobject`):
//! no network, a scratch working directory, and a hard kill-on-close so a runaway
//! script can't outlive the run. Defaults off; opt-in per chat.

use super::sandbox;
use super::skills::SkillContext;

/// The OpenAI tool schema advertised to the model for this skill.
pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "run_code",
                "description": "Run a short Python or Node.js snippet in an isolated, time- and memory-limited sandbox and return its output. Use for calculation, data wrangling, or generating a small artifact.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "language": {
                            "type": "string",
                            "enum": ["python", "node"],
                            "description": "Which runtime to use"
                        },
                        "code": { "type": "string", "description": "The source code to execute" }
                    },
                    "required": ["language", "code"]
                }
            }
        }
    ])
}

/// Is this a Code Execution tool name?
pub fn handles(name: &str) -> bool {
    name == "run_code"
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let lang = args.get("language").and_then(|l| l.as_str()).unwrap_or("code");
    match name {
        "run_code" => ("ran".into(), lang.to_string()),
        other => (other.into(), lang.to_string()),
    }
}

/// Execute a Code Execution tool call: write the snippet to a throwaway scratch
/// directory, run it in the confined sandbox, and return its captured output.
pub async fn execute(
    ctx: &SkillContext<'_>,
    _name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let language = args
        .get("language")
        .and_then(|l| l.as_str())
        .unwrap_or("python");
    let code = args
        .get("code")
        .and_then(|c| c.as_str())
        .ok_or("missing 'code' argument")?;

    let (program, filename) = match language {
        "python" => ("python", "main.py"),
        "node" => ("node", "main.js"),
        other => return Err(format!("Unsupported language '{other}'. Use 'python' or 'node'.")),
    };

    // Isolated scratch directory, removed after the run.
    let dir = std::env::temp_dir().join(format!("nexus-exec-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create the sandbox dir: {e}"))?;
    let script = dir.join(filename);
    std::fs::write(&script, code).map_err(|e| format!("couldn't write the snippet: {e}"))?;

    let result = sandbox::run(program, &[script.to_string_lossy().into_owned()], &dir).await;
    let _ = std::fs::remove_dir_all(&dir);

    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "code", &format!("ran {language}"));

    let out = result?;
    if out.timed_out {
        return Ok("The code ran longer than the 10-second limit and was stopped.".to_string());
    }

    let mut report = String::new();
    if !out.stdout.is_empty() {
        report.push_str(&format!("stdout:\n{}\n", out.stdout.trim_end()));
    }
    if !out.stderr.is_empty() {
        report.push_str(&format!("stderr:\n{}\n", out.stderr.trim_end()));
    }
    match out.exit_code {
        Some(0) | None => {}
        Some(code) => report.push_str(&format!("(exited with code {code})\n")),
    }
    if report.is_empty() {
        report.push_str("(the snippet produced no output)");
    }
    Ok(report)
}
