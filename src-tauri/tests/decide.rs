//! DEC: decision engine integration test (`plans/DUAL_PROCESS_PLAN.md` `DEC-4`,
//! `DEC-T-3`). Starts a real `llama-server` on a real Clef-flash GGUF through
//! `DecideManager`, so the argument list we build is the one checked, and asks
//! it real questions. Needs the binary and the files on disk, so it is
//! `#[ignore]`d like `embed`:
//!
//! ```text
//! DECIDE_SERVER_BIN=C:\path\to\llama-server.exe \
//! DECIDE_MODEL_PATH=C:\path\to\Clef-Flash-Q4_K_M.gguf \
//! DECIDE_MMPROJ_PATH=C:\path\to\mmproj-Clef-Flash-Q8_0.gguf \
//! cargo test --test decide -- --ignored --nocapture
//! ```
//!
//! `DECIDE_NOT_A_DECISION_MODEL` (any chat GGUF) enables the head-check
//! rejection test.

use std::path::PathBuf;
use std::time::Duration;

use poiesis_lib::agent::decide::policy::ALL;
use poiesis_lib::agent::decide::ablation::{self, Strategy};
use poiesis_lib::agent::decide::{calibration, handover, local, seed, sites, Answer, Ask, ImageRef, Question};
use poiesis_lib::runtime::decideserver::{head_check, DecideManager, Local};

fn path(var: &str) -> PathBuf {
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("set {var}")))
}

fn local_for(model: PathBuf, mmproj: Option<PathBuf>) -> Local {
    Local {
        server_binary: path("DECIDE_SERVER_BIN"),
        model_name: model.file_stem().unwrap().to_string_lossy().to_string(),
        model_path: model,
        mmproj,
        gpu: false,
    }
}

async fn ask_live(
    client: &reqwest::Client,
    live: &poiesis_lib::runtime::decideserver::Live,
    ask: &Ask<'_>,
) -> std::collections::BTreeMap<String, Answer> {
    let body = local::build_body(ask, ask.state);
    let reply = local::post(client, &live.base_url, &live.token, &body, Duration::from_secs(300))
        .await
        .expect("the engine should answer");
    local::parse_response(ask, &reply).expect("the answer should be well formed").0
}

#[tokio::test]
#[ignore]
async fn a_real_decision_model_passes_the_head_check_and_answers_sensibly() {
    let client = reqwest::Client::new();
    let decide = DecideManager::new();
    let mmproj = std::env::var("DECIDE_MMPROJ_PATH").ok().map(PathBuf::from);
    let local = local_for(path("DECIDE_MODEL_PATH"), mmproj.clone());

    head_check(&decide, &client, &local).await.expect("the head check should pass");
    assert!(decide.is_warm(), "the engine should be up after the check");
    let live = decide.ensure_started(&client, &local).await.expect("the engine should be reusable");
    assert_eq!(live.vision, mmproj.is_some());

    // RET-4's question, on an excerpt that answers it and one that does not.
    for (excerpt, answers) in [
        ("Refunds are accepted within 30 days of purchase with a receipt.", true),
        ("Our office is open on weekdays from nine to five.", false),
    ] {
        let site = sites::sufficiency("What is the refund window?", excerpt);
        let ask = site.as_ask();
        let out = ask_live(&client, &live, &ask).await;
        let Answer::Noul { p_true } = out["enough"] else { panic!("not a noul") };
        assert_eq!(p_true >= 0.5, answers, "{excerpt} gave {p_true}");
    }

    // A state far over the default 512-token batch, which the batch sizes we
    // pass must let through.
    let long = format!("{}\nThe refund window is 30 days.", "This sentence is filler. ".repeat(600));
    let site = sites::sufficiency("What is the refund window?", &long);
    let ask = site.as_ask();
    let out = ask_live(&client, &live, &ask).await;
    assert!(matches!(out["enough"], Answer::Noul { .. }));

    // Vision, when the projector was given.
    if mmproj.is_some() {
        let ask = Ask {
            state: "Look at the image.",
            images: vec![ImageRef { data_url: red_square() }],
            questions: vec![(
                "red",
                Question::Noul {
                    instructions: "Is the image mostly red?".into(),
                    when_true: "Mostly red.".into(),
                    when_false: "Another colour.".into(),
                },
            )],
        };
        let out = ask_live(&client, &live, &ask).await;
        let Answer::Noul { p_true } = out["red"] else { panic!("not a noul") };
        assert!(p_true > 0.5, "a red square gave {p_true}");
    }

    decide.stop().await;
    assert!(!decide.is_warm());
}

/// `CTL-4`: measure the real model on the seed sets (`tests/eval/decide`), the
/// way "Calibrate now" does, and say what the numbers are. It prints accuracy,
/// the mean confidence of the winner, how often the plan's bars are reached, and
/// the fit. A CPU-only machine takes about 12 s a row; `DECIDE_SEED_ROWS=10`
/// limits each purpose to its first rows for a quick look.
#[tokio::test]
#[ignore]
async fn the_seed_sets_say_how_well_calibrated_the_real_model_is() {
    let client = reqwest::Client::new();
    let decide = DecideManager::new();
    let local = local_for(path("DECIDE_MODEL_PATH"), std::env::var("DECIDE_MMPROJ_PATH").ok().map(PathBuf::from));
    let live = decide.ensure_started(&client, &local).await.expect("the engine should start");
    let limit: usize = std::env::var("DECIDE_SEED_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);

    for purpose in ALL {
        let mut obs = Vec::new();
        let (mut right, mut reached) = (0usize, 0usize);
        let bar = purpose.policy().act_at;
        for row in seed::rows(purpose).into_iter().take(limit) {
            let ask = row.site.as_ask();
            let out = ask_live(&client, &live, &ask).await;
            let answer = &out[row.site.id];
            let probs: Vec<f32> = match answer {
                Answer::Noul { p_true } => vec![*p_true, 1.0 - *p_true],
                Answer::Choice { probs, .. } => probs.iter().map(|(_, p)| *p).collect(),
                Answer::Score { probs, .. } => probs.clone(),
            };
            let winner = probs.iter().enumerate().fold(0, |b, (i, p)| if *p > probs[b] { i } else { b });
            right += (winner == row.truth) as usize;
            reached += bar.reaches(answer, false) as usize;
            obs.push(calibration::Obs { probs, truth: calibration::Truth::Class(row.truth), weight: 1.0 });
        }
        let n = obs.len();
        let fit = calibration::fit(&obs, 0).expect("some rows");
        eprintln!(
            "{:<16} n={n:<3} accuracy={:.2} reaches-the-bar={:.2} ece-before={:.3} t={:.2} ece-after={:.3} calibrated={}",
            purpose.name(),
            right as f32 / n as f32,
            reached as f32 / n as f32,
            calibration::ece_before(&obs),
            fit.t,
            fit.ece,
            fit.is_calibrated(),
        );
    }
    decide.stop().await;
}

/// `CTL-T-3`: the tool menu's ablation, on the real model. Every golden request
/// (`tests/eval/decide/tool-menu.jsonl`) is asked the way a run asks it, one
/// question for each group at once, and the answers are scored three ways:
/// keeping everything, a fixed cut-off, and the policy table. It prints the
/// table, and fails when the policy table misses more needed groups than the
/// fixed cut-off, or `need_tools` could not bring back a group that was held and
/// needed. `DECIDE_MENU_CASES=8` limits it to the first cases for a quick look.
///
/// It measures the decision, not a chat model's use of `need_tools` and not
/// first-token time, which need a chat turn against a running model.
#[tokio::test]
#[ignore]
async fn the_tool_menu_ablation_on_the_golden_cases() {
    let client = reqwest::Client::new();
    let decide = DecideManager::new();
    let local = local_for(path("DECIDE_MODEL_PATH"), std::env::var("DECIDE_MMPROJ_PATH").ok().map(PathBuf::from));
    let live = decide.ensure_started(&client, &local).await.expect("the engine should start");
    let limit: usize = std::env::var("DECIDE_MENU_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);

    let cases: Vec<_> = ablation::cases().into_iter().take(limit).collect();
    let mut said = std::collections::HashMap::new();
    for case in &cases {
        let tool_ask = ablation::ask_for(case).expect("a case with a request");
        let ask = tool_ask.as_ask();
        let out = ask_live(&client, &live, &ask).await;
        for (id, answer) in out {
            if let Answer::Noul { p_true } = answer {
                said.insert((case.id.clone(), id), p_true);
            }
        }
    }
    let p = |case: &ablation::Case, unit: &str| said[&(case.id.clone(), unit.to_string())];

    let needed = ablation::needed_total(&cases);
    let static_cut = ablation::score(Strategy::Static(0.30), &cases, &p);
    let policy = ablation::score(Strategy::Policy { calibrated: false }, &cases, &p);
    let rows = [
        ablation::score(Strategy::Keep, &cases, &p),
        static_cut.clone(),
        policy.clone(),
        ablation::score(Strategy::Policy { calibrated: true }, &cases, &p),
    ];
    eprintln!("{}", ablation::report(&rows, needed));
    for slice in ["base", "apps", "delegate"] {
        let part: Vec<_> = cases.iter().filter(|c| c.slice == slice).cloned().collect();
        if part.is_empty() {
            continue;
        }
        let s = ablation::score(Strategy::Policy { calibrated: false }, &part, &p);
        eprintln!(
            "slice {slice:<9} cases={:<3} held={:<3} missed={:<3} tokens saved={}",
            part.len(),
            s.held,
            s.missed,
            s.tokens_saved
        );
    }
    decide.stop().await;
    if let Err(why) = ablation::ship(&policy, &static_cut, needed) {
        panic!("the tool menu does not pass the ablation: {why}");
    }
}

/// `CTL-T-3`, the handover slice (`USE-13`): 60+ requests labelled with the rule a
/// person would pick, asked exactly as a live turn asks them (the default Claude
/// Code rules, the last reply in the state). Scores the early card at 0.85 on the
/// model's own probabilities, so it measures the model, not a fit; the ship rule
/// is precision of at least 0.95.
///
/// ```text
/// DECIDE_SERVER_BIN=... DECIDE_MODEL_PATH=... \
/// cargo test --test decide -- --ignored --nocapture handover_prematch
/// ```
#[tokio::test]
#[ignore]
async fn the_handover_prematch_precision_on_the_golden_requests() {
    let client = reqwest::Client::new();
    let decide = DecideManager::new();
    let local = local_for(path("DECIDE_MODEL_PATH"), std::env::var("DECIDE_MMPROJ_PATH").ok().map(PathBuf::from));
    let live = decide.ensure_started(&client, &local).await.expect("the engine should start");
    let limit: usize = std::env::var("DECIDE_HANDOVER_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);

    let agent = vec![handover::default_agent()];
    let cases: Vec<_> = handover::cases().into_iter().take(limit).collect();
    let mut said = std::collections::HashMap::new();
    for case in &cases {
        let site = handover::ask_for(&case.request, &case.previous, &agent).expect("a case with a request");
        let ask = site.as_ask();
        let out = ask_live(&client, &live, &ask).await;
        said.insert(case.id.clone(), out.get("route").cloned().expect("a routing answer"));
    }
    let slice = handover::score(&cases, &|c| said.get(&c.id).cloned(), 0.85);
    eprintln!("{}", handover::report(&slice, 0.85));
    decide.stop().await;
    if let Err(why) = handover::ship(&slice) {
        panic!("the handover pre-match does not pass the ablation: {why}");
    }
}

#[tokio::test]
#[ignore]
async fn a_model_that_only_generates_text_fails_the_head_check() {
    let Ok(chat) = std::env::var("DECIDE_NOT_A_DECISION_MODEL") else {
        eprintln!("set DECIDE_NOT_A_DECISION_MODEL to a chat GGUF to run this");
        return;
    };
    let client = reqwest::Client::new();
    let decide = DecideManager::new();
    let err = head_check(&decide, &client, &local_for(PathBuf::from(chat), None))
        .await
        .expect_err("a chat model has no decision head");
    assert!(err.contains("can't make decisions"), "got: {err}");
    decide.stop().await;
}

/// A 224 by 224 red PNG, as a data URL.
fn red_square() -> String {
    use base64::Engine;
    use image::ImageEncoder;
    let (w, h) = (224u32, 224u32);
    let pixels = vec![[220u8, 20, 20]; (w * h) as usize].concat();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&pixels, w, h, image::ExtendedColorType::Rgb8)
        .expect("encode a png");
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}
