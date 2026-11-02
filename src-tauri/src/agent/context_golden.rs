//! `CTX-4`: the prompt snapshot.
//!
//! This module renders the shared fixture through `agent/context.rs` and the
//! history cut `run_turn` makes (`turn::fit`), and compares it to
//! `fixtures/prompt-assembly.golden.txt`. It began as the gate that held the Rust
//! and TypeScript prompt assemblies to the same bytes; the TypeScript copy went
//! with `HRW-6`, and what is left is a snapshot of what the model is sent. It
//! needs no running app, no dev-only command and no IPC.
//!
//! The rendering is deliberately dumb: plain concatenation with a marker line
//! between turns, no JSON serializer anywhere. Comparing serialized arrays would
//! have tested `serde_json` against `JSON.stringify` — two things that are
//! allowed to differ — instead of testing the two assemblies.
//!
//! **When this fails, the prompt changed.** If it was meant to, update the golden
//! in the same commit, so the change to what the model reads shows in review.

#![cfg(test)]

use super::context::*;

/// One turn per block, so a difference shows up as a diff on the offending turn
/// rather than as one enormous unreadable line.
const MARK: &str = "\n<<<turn role=";

fn render(turns: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for t in turns {
        out.push_str(MARK);
        out.push_str(t["role"].as_str().unwrap_or("?"));
        out.push_str(">>>\n");
        out.push_str(t["content"].as_str().unwrap_or_default());
        out.push('\n');
    }
    out
}

fn fixture_dir() -> std::path::PathBuf {
    // `CARGO_MANIFEST_DIR` is `src-tauri`; the fixture is shared with the
    // frontend, so it sits beside both rather than inside either.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures")
}

fn build(f: &serde_json::Value) -> PromptInputs {
    let s = |k: &str| f[k].as_str().map(str::to_string);
    PromptInputs {
        base: f["base"].as_str().unwrap_or_default().to_string(),
        about_you: s("about_you"),
        soul: s("soul"),
        project_name: s("project_name"),
        project_instructions: s("project_instructions"),
        memory_index: s("memory_index"),
        fact_count: f["fact_count"].as_u64().unwrap_or(0) as usize,
        tools_enabled: f["tools_enabled"].as_bool().unwrap_or(false),
        memory_enabled: f["memory_enabled"].as_bool().unwrap_or(false),
        plan_mode: super::plan::PlanMode::parse(f["plan_mode"].as_str().unwrap_or("auto")),
        skills: f["skills"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|s| SkillEntry {
                name: s["name"].as_str().unwrap_or_default().to_string(),
                description: s["description"].as_str().unwrap_or_default().to_string(),
                when_to_use: s["when_to_use"].as_str().map(str::to_string),
                // `SKC-3`: absent means the model may invoke it.
                model_invocable: s["model_invocable"].as_bool().unwrap_or(true),
            })
            .collect(),
        workspace: f["workspace"].as_bool().unwrap_or(false),
        surface: f["surface"].as_object().map(|s| SurfaceView {
            data_json: s["data_json"].as_str().unwrap_or_default().to_string(),
            state_json: s.get("state_json").and_then(|v| v.as_str()).map(str::to_string),
        }),
        session_state_json: s("session_state_json"),
        tool_health: f["tool_health"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|t| HealthEntry {
                tool_name: t["tool_name"].as_str().unwrap_or_default().to_string(),
                ok: t["ok"].as_i64().unwrap_or(0),
                total: t["total"].as_i64().unwrap_or(0),
            })
            .collect(),
        spoken: false,
        context_refs: Default::default(),
        recalled_matches: Vec::new(),
    }
}

/// Assemble the fixture exactly as a real turn would: compose the system prompt,
/// then `run_turn`'s own fit (`HRW-T-4`) folds in the conversation summary and
/// budgets the history around it.
fn assemble(f: &serde_json::Value) -> String {
    let system = compose_system_prompt(&build(f));
    let prior: Vec<serde_json::Value> = f["prior"].as_array().cloned().unwrap_or_default();
    let bt = super::turn::fit(
        &system,
        f["summary"].as_str().unwrap_or_default(),
        &prior,
        &f["current"],
        f["budget"].as_u64().unwrap_or(4096) as usize,
        f["keep_recent"].as_u64().unwrap_or(KEEP_RECENT as u64) as usize,
    );
    render(&bt.turns)
}

/// The snapshot itself. Set `UPDATE_PROMPT_GOLDEN=1` to rewrite the golden after
/// a deliberate change.
#[test]
fn the_rust_assembly_matches_the_shared_golden() {
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir().join("prompt-assembly.json"))
            .expect("fixtures/prompt-assembly.json"),
    )
    .expect("the fixture is valid JSON");

    let got = assemble(&fixture);
    let path = fixture_dir().join("prompt-assembly.golden.txt");

    if std::env::var("UPDATE_PROMPT_GOLDEN").is_ok() {
        std::fs::write(&path, &got).expect("write the golden");
        return;
    }

    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        got, want,
        "Prompt assembly no longer matches fixtures/prompt-assembly.golden.txt.\n\
         Either a change landed in agent/context.rs that was not meant to, or the\n\
         golden needs updating: run with UPDATE_PROMPT_GOLDEN=1."
    );
}

/// `HRW-T-4`: the same fixture with the composer's tools switch off. The index
/// stays, marked as the only detail there is; skills, the surface and the
/// guidance go. Its own golden, so the main one does not have to move.
#[test]
fn the_rust_assembly_with_tools_off_matches_its_golden() {
    let mut fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir().join("prompt-assembly.json")).unwrap(),
    )
    .unwrap();
    fixture["tools_enabled"] = serde_json::Value::Bool(false);

    let got = assemble(&fixture);
    let path = fixture_dir().join("prompt-assembly.tools-off.golden.txt");
    if std::env::var("UPDATE_PROMPT_GOLDEN").is_ok() {
        std::fs::write(&path, &got).expect("write the golden");
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(got, want, "tools-off prompt assembly changed: update the golden");
}

/// The budget must actually bite on this fixture. A golden recorded from a
/// window nothing overflowed would pass forever while proving nothing about the
/// half of assembly that decides what to drop.
#[test]
fn the_fixture_actually_exercises_the_budget() {
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir().join("prompt-assembly.json")).unwrap(),
    )
    .unwrap();
    let system = compose_system_prompt(&build(&fixture));
    let system = with_summary(&system, fixture["summary"].as_str().unwrap_or_default());
    let prior: Vec<serde_json::Value> = fixture["prior"].as_array().cloned().unwrap();
    let bt = budget_turns(
        &system,
        &prior,
        &fixture["current"],
        fixture["budget"].as_u64().unwrap() as usize,
        fixture["keep_recent"].as_u64().unwrap() as usize,
    );
    assert!(bt.needs_compaction, "the fixture must overflow its budget");
    assert!(bt.overflow > 0 && bt.overflow < prior.len(), "some turns drop, not all");
}

/// `VTN-2`: the spoken-reply sentence, held by a file of its own, same idea as
/// the snapshot above but small. The main fixture is not touched, so this one can
/// move on its own.
#[test]
fn the_spoken_sentence_matches_the_shared_golden() {
    let on = compose_system_prompt(&PromptInputs { base: "BASE".into(), spoken: true, ..Default::default() });
    let off = compose_system_prompt(&PromptInputs { base: "BASE".into(), ..Default::default() });
    assert_eq!(off, "BASE", "a typed turn must not change");

    let path = fixture_dir().join("voice").join("spoken-prompt.golden.txt");
    if std::env::var("UPDATE_PROMPT_GOLDEN").is_ok() {
        std::fs::write(&path, &on).expect("write the golden");
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(on, want, "SPOKEN_GUIDANCE changed: update the golden");
}

/// It goes last, after the tool guidance, so nothing above can undo it.
#[test]
fn the_spoken_sentence_comes_last() {
    let out = compose_system_prompt(&PromptInputs {
        base: "BASE".into(),
        tools_enabled: true,
        memory_enabled: true,
        spoken: true,
        ..Default::default()
    });
    assert!(out.ends_with(SPOKEN_GUIDANCE), "{out}");
}
