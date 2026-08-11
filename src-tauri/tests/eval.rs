//! EVL — the agent regression harness (`plans/PERCEPTION_PLAN.md` Part 0).
//! See `tests/eval/README.md` for how to run it and its current scope.

use std::path::PathBuf;

use poiesis_lib::agent::golden::{describe_failures, parse_cases, GoldenCase};
use poiesis_lib::agent::run::run_agent;
use poiesis_lib::agent::run::AgentEventSink;
use poiesis_lib::cloud::ChatEndpoint;
use poiesis_lib::db::Db;
use poiesis_lib::memory::MemoryStore;
use poiesis_lib::permissions::{Mode, PermissionManager};
use poiesis_lib::runtime::proxy::CancelFlag;
use poiesis_lib::runtime::{EmbedManager, RerankManager, RuntimeManager};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/eval/fixtures")
}

/// `GLD-1`: `GoldenCase` and its parsing now live in the library
/// (`poiesis_lib::agent::golden`) so `EVL` and `GLD` share one case format —
/// this is the migration that has to leave `golden.json` itself untouched.
fn golden_cases() -> Vec<GoldenCase> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/eval/golden.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("couldn't read {}: {e}", path.display()));
    parse_cases(&raw).unwrap_or_else(|e| panic!("{} is not valid: {e}", path.display()))
}

/// Run one question through a real agent turn, tools enabled, grounded in the
/// fixtures folder. Returns the final assistant prose.
async fn run_case(
    db: &Db,
    perms: &PermissionManager,
    memory: &MemoryStore,
    conversation_id: &str,
    question: &str,
) -> String {
    let base_url = std::env::var("EVAL_ENGINE_URL").expect(
        "set EVAL_ENGINE_URL to a running OpenAI-compatible endpoint, e.g. http://127.0.0.1:8080",
    );
    let api_key = std::env::var("EVAL_ENGINE_TOKEN").ok();
    let endpoint = ChatEndpoint::OpenAi { base_url, api_key, model: None };

    perms.add_chat_grant(conversation_id, fixtures_dir(), Mode::Read);

    // No live webview on the other end — an empty handler is fine since
    // assertions only need `run_agent`'s return value, not the event stream.
    let channel = tauri::ipc::Channel::new(|_| Ok(()));
    let sink = AgentEventSink::new(channel);
    let client = reqwest::Client::new();
    let messages = vec![serde_json::json!({ "role": "user", "content": question })];
    // Eval cases don't exercise RET, so these never actually spin up an
    // engine — they only need to exist for `run_agent`'s signature.
    let mgr = RuntimeManager::new(fixtures_dir());
    let embed_mgr = EmbedManager::new();
    let rerank_mgr = RerankManager::new();

    run_agent(
        &client,
        &endpoint,
        // The eval endpoint *is* the local engine, so a skill's side call
        // (SCP-1) can use it too.
        Some(&endpoint),
        db,
        &mgr,
        &embed_mgr,
        &rerank_mgr,
        perms,
        memory,
        // `EVL` dispatches real tool calls but never a real browser session.
        None,
        conversation_id,
        None,
        &fixtures_dir(),
        "eval-model",
        messages,
        0.2,
        true,
        false,
        CancelFlag::new(),
        &sink,
    )
    .await
}

/// EVL-2/EVL-3: run every case in `golden.json` (or just `EVAL_FILTER`),
/// asserting `must_contain`/`must_not_contain` against the final answer.
/// Non-zero exit (via panic) on any failure, with a summary line per case.
#[tokio::test]
#[ignore]
async fn eval() {
    let tmp = tempfile::tempdir().expect("temp app-data dir");
    let db = Db::open(&tmp.path().join("eval.db")).expect("open eval db");
    let memory = MemoryStore::new(tmp.path()).expect("open eval memory store");
    let perms = PermissionManager::new();

    let filter = std::env::var("EVAL_FILTER").ok();
    let cases: Vec<GoldenCase> = golden_cases()
        .into_iter()
        .filter(|c| filter.as_deref().map(|f| f == c.id).unwrap_or(true))
        .collect();
    assert!(
        !cases.is_empty(),
        "no golden cases matched (EVAL_FILTER={filter:?})"
    );

    let mut failures = Vec::new();
    for case in &cases {
        let conversation_id = format!("eval-{}", case.id);
        let answer = run_case(&db, &perms, &memory, &conversation_id, &case.question).await;
        // EVL's real-dispatch mode: `tool_stats` records every call the
        // agent actually made (GLD's mode instead parses a call it never
        // dispatches) — `describe_failures` doesn't care which.
        let used = db.tools_used_in(&conversation_id).expect("read tool stats");
        let chosen_tools: Vec<String> = used.into_iter().map(|(name, _)| name).collect();
        let problems = describe_failures(case, &answer, &chosen_tools);

        if problems.is_empty() {
            println!("PASS {} — {}", case.id, case.question);
        } else {
            println!("FAIL {} — {}", case.id, problems.join(", "));
            println!("     answer: {answer}");
            failures.push(case.id.clone());
        }
    }

    assert!(
        failures.is_empty(),
        "eval cases failed: {}",
        failures.join(", ")
    );
}

// ---- EVL-4: threshold calibration ----

/// A query with passages that should match it and passages that shouldn't.
#[derive(serde::Deserialize)]
struct CalibrationCase {
    query: String,
    relevant: Vec<String>,
    irrelevant: Vec<String>,
}

fn calibration_cases() -> Vec<CalibrationCase> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/eval/calibration.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("couldn't read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{} is not valid: {e}", path.display()))
}

/// Quantile of an already-sorted slice, nearest-rank.
fn quantile(sorted: &[f32], q: f32) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    let ix = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[ix]
}

fn describe(label: &str, scores: &mut Vec<f32>) {
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = scores.iter().sum::<f32>() / scores.len() as f32;
    println!(
        "  {label:<11} n={:<3} min={:.3}  p10={:.3}  median={:.3}  mean={:.3}  max={:.3}",
        scores.len(),
        scores[0],
        quantile(scores, 0.10),
        quantile(scores, 0.50),
        mean,
        scores[scores.len() - 1],
    );
}

/// EVL-4. Every similarity floor in `plans/PERCEPTION_PLAN.md` (`SEM-3`'s
/// 0.58, `RET-2`'s 0.40/0.50/0.55) is a *starting* value measured for one
/// embedding model. Swap the model and those numbers mean something else.
/// This prints the score distribution for known-relevant and known-irrelevant
/// pairs so they can be re-measured rather than guessed at:
///
/// ```text
/// EMBED_SERVER_BIN=...\llama-server.exe EMBED_MODEL_PATH=...\bge-small-en-v1.5-f16.gguf \
///   cargo test --ignored eval_calibrate -- --nocapture
/// ```
#[tokio::test]
#[ignore]
async fn eval_calibrate() {
    use poiesis_lib::db::vectors::similarity;
    use poiesis_lib::runtime::embedserver::EmbedManager;

    let server_binary = PathBuf::from(
        std::env::var("EMBED_SERVER_BIN").expect("set EMBED_SERVER_BIN to a llama-server(.exe) path"),
    );
    let model_path = PathBuf::from(
        std::env::var("EMBED_MODEL_PATH").expect("set EMBED_MODEL_PATH to a GGUF embedding model"),
    );
    let model_label = model_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let cases = calibration_cases();
    assert!(!cases.is_empty(), "calibration.json is empty");

    // One flat list, one round trip: queries first, then every passage.
    let mut texts: Vec<String> = Vec::new();
    for case in &cases {
        texts.push(case.query.clone());
        texts.extend(case.relevant.iter().cloned());
        texts.extend(case.irrelevant.iter().cloned());
    }

    let client = reqwest::Client::new();
    let mgr = EmbedManager::new();
    let vectors = mgr
        .embed_texts(&client, server_binary, model_path, &texts)
        .await
        .expect("the embedding engine should start and embed");
    mgr.stop().await;

    println!("\nthreshold calibration — {model_label}\n");

    let mut relevant_scores: Vec<f32> = Vec::new();
    let mut irrelevant_scores: Vec<f32> = Vec::new();
    let mut cursor = 0;
    for case in &cases {
        let query = &vectors[cursor];
        cursor += 1;
        println!("  \"{}\"", case.query);
        for text in &case.relevant {
            let score = similarity(query, &vectors[cursor]);
            cursor += 1;
            relevant_scores.push(score);
            println!("    {score:.3}  relevant    {text}");
        }
        for text in &case.irrelevant {
            let score = similarity(query, &vectors[cursor]);
            cursor += 1;
            irrelevant_scores.push(score);
            println!("    {score:.3}  irrelevant  {text}");
        }
        println!();
    }

    println!("distribution");
    describe("relevant", &mut relevant_scores);
    describe("irrelevant", &mut irrelevant_scores);

    // The widest gap that still admits every relevant pair, and the tightest
    // that still excludes every irrelevant one. A floor between them separates
    // the two classes cleanly; if they cross, no single floor can.
    let lowest_relevant = relevant_scores[0];
    let highest_irrelevant = irrelevant_scores[irrelevant_scores.len() - 1];
    println!("\nlowest relevant   {lowest_relevant:.3}");
    println!("highest irrelevant {highest_irrelevant:.3}");
    if lowest_relevant > highest_irrelevant {
        let floor = (lowest_relevant + highest_irrelevant) / 2.0;
        println!(
            "\n  the classes separate cleanly — a floor of {floor:.2} keeps every relevant\n  \
             passage and rejects every irrelevant one. Use it for SEM-3; RET-2's\n  \
             floor should sit lower, since its keyword bonus can only add."
        );
    } else {
        println!(
            "\n  the classes overlap — no single floor separates them. Prefer a floor\n  \
             near p10 of relevant ({:.2}) and rely on RET-3/RET-4 to catch the rest.",
            quantile(&relevant_scores, 0.10)
        );
    }

    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(
        mean(&relevant_scores) > mean(&irrelevant_scores),
        "relevant passages must score higher on average than irrelevant ones — \
         if they don't, this model can't support retrieval at all"
    );
}
