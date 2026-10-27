//! `CTL-T-3`: the ablation gate for the tool menu (`USE-4`). A fixed set of
//! requests, each labelled with the tool groups a person would say it needs, and
//! three ways to act on a decision model's answers:
//!
//! 1. **Keep everything** ("the language model alone"): nothing is held back.
//! 2. **Static narrowing**: hold a group back whenever its probability is under
//!    one fixed number, whatever the calibration.
//! 3. **The policy table**: hold a group back only on a confident no, with the
//!    row's calibrated or uncalibrated bar (`Purpose::ToolSelect`).
//!
//! This file is the scoring, which needs no engine, so it is tested. The numbers
//! it scores come from a real model in `tests/decide.rs`, which is opt-in. The
//! ship rule is the plan's: the policy table must not hold back a needed group
//! more often than static narrowing does, and `need_tools` must bring back every
//! group that was held and needed.

use serde::Deserialize;

use super::policy::Purpose;
use super::tools::{self, Held, HeldSet, Unit};
use super::Answer;

const FILE: &str = include_str!("../../../tests/eval/decide/tool-menu.jsonl");

/// The groups the golden requests are asked about, worded the way a run
/// describes them (`tools::units`). Shared with the seed set.
pub const CATALOGUE: [(&str, &str, &str); 7] = [
    ("web_search", "Web search", "Web search: web_search, fetch_url."),
    ("mail", "Mail", "Mail: list_mail, read_mail, search_mail, send_mail."),
    ("apps", "Connected apps", "Connected apps: Google Calendar, Slack, Notion."),
    ("subagents", "Other agents", "Hand a task to another agent: researcher, Claude Code."),
    ("browser", "Browser", "Browser: browse, browser_read, browser_click, browser_type."),
    ("code_exec", "Code execution", "Code execution: run_code."),
    ("image_gen", "Image generation", "Image generation: generate_image."),
];

/// A catalogue group by id.
pub fn unit(id: &str) -> Option<Unit> {
    let (id, label, about) = CATALOGUE.iter().find(|(i, ..)| *i == id)?;
    Some(Unit { id: (*id).to_string(), label: (*label).to_string(), about: (*about).to_string(), tools: Vec::new() })
}

/// What a group costs the prompt when it is on the menu, in tokens. The built-in
/// toolsets are measured from their real specs. Connected apps and the agent
/// apps' rule blocks are estimates, because their size depends on the person's
/// setup: Composio's four meta-tools, and one rule block per agent app.
pub fn spec_tokens(id: &str) -> usize {
    const APPS: usize = 1_200;
    const RULE_BLOCK: usize = 600;
    let chars = |t: crate::agent::toolsets::Toolset| t.tool_specs().iter().map(|s| s.to_string().len()).sum::<usize>();
    match id {
        "apps" => APPS,
        // The plain `delegate` spec, and the two agent apps the slice assumes.
        "subagents" => chars(crate::agent::toolsets::Toolset::Subagents) / 4 + 2 * RULE_BLOCK,
        other => crate::agent::toolsets::Toolset::from_id(other).map_or(0, |t| chars(t) / 4),
    }
}

/// One golden request.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Case {
    pub id: String,
    /// `base`, `apps` (three granted apps) or `delegate` (two agent apps).
    pub slice: String,
    pub request: String,
    /// The groups a person would say it needs. Empty is plain conversation.
    pub needs: Vec<String>,
}

pub fn cases() -> Vec<Case> {
    FILE.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The whole menu every case is asked about, in production's shape: one request,
/// one question per group.
pub fn menu() -> Vec<Unit> {
    CATALOGUE.iter().filter_map(|(id, ..)| unit(id)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Strategy {
    /// Nothing is held back.
    Keep,
    /// Hold a group back when the probability it is needed is under this.
    Static(f32),
    /// The `ToolSelect` row, with or without a fit behind the numbers.
    Policy { calibrated: bool },
}

impl Strategy {
    pub fn label(self) -> String {
        match self {
            Strategy::Keep => "keep everything".into(),
            Strategy::Static(p) => format!("static, under {p:.2}"),
            Strategy::Policy { calibrated: true } => "policy table, calibrated".into(),
            Strategy::Policy { calibrated: false } => "policy table, not calibrated".into(),
        }
    }

    /// Whether this strategy holds back a group the model gave `p_needed`.
    pub fn drops(self, p_needed: f32) -> bool {
        match self {
            Strategy::Keep => false,
            Strategy::Static(under) => p_needed < under,
            Strategy::Policy { calibrated } => {
                Purpose::ToolSelect.policy().act_at.reaches(&Answer::Noul { p_true: p_needed }, calibrated)
            }
        }
    }
}

/// What one strategy did over a set of cases.
#[derive(Debug, Clone, PartialEq)]
pub struct Scores {
    pub strategy: String,
    pub cases: usize,
    /// Groups asked about, over every case.
    pub asked: usize,
    pub held: usize,
    /// Held back although a person would say it was needed.
    pub missed: usize,
    /// Cases with at least one miss.
    pub cases_hurt: usize,
    /// Held and not needed: the saving was right.
    pub saved_right: usize,
    /// Prompt tokens not sent, summed over the right holds. Misses are not counted:
    /// `need_tools` brings them back and the turn pays for them after all.
    pub tokens_saved: usize,
    /// Every miss came back through `need_tools`.
    pub all_recovered: bool,
}

impl Scores {
    /// Misses as a share of the groups that were needed.
    pub fn miss_rate(&self, needed: usize) -> f32 {
        if needed == 0 {
            0.0
        } else {
            self.missed as f32 / needed as f32
        }
    }
}

/// Score a strategy. `p_needed(case, group)` is what the model said.
pub fn score(strategy: Strategy, cases: &[Case], p_needed: &dyn Fn(&Case, &str) -> f32) -> Scores {
    let menu = menu();
    let mut s = Scores {
        strategy: strategy.label(),
        cases: cases.len(),
        asked: 0,
        held: 0,
        missed: 0,
        cases_hurt: 0,
        saved_right: 0,
        tokens_saved: 0,
        all_recovered: true,
    };
    for case in cases {
        let mut set = HeldSet::default();
        let mut held = Vec::new();
        for unit in &menu {
            s.asked += 1;
            let p = p_needed(case, &unit.id);
            if strategy.drops(p) {
                held.push(Held { unit: unit.clone(), p_needed: p });
            }
        }
        s.held += held.len();
        set.hold("case", held.clone());
        let needed = |id: &str| case.needs.iter().any(|n| n == id);
        let missed: Vec<String> = held.iter().filter(|h| needed(&h.unit.id)).map(|h| h.unit.id.clone()).collect();
        s.missed += missed.len();
        s.cases_hurt += (!missed.is_empty()) as usize;
        for h in held.iter().filter(|h| !needed(&h.unit.id)) {
            s.saved_right += 1;
            s.tokens_saved += spec_tokens(&h.unit.id);
        }
        // The model asks for what it needs. Every miss has to come back.
        let reply = tools::bring_back(&mut set, &missed, |_| None);
        s.all_recovered &= missed.is_empty() || (set.out().all(|h| !needed(&h.unit.id)) && !reply.contains("Nothing was held"));
    }
    s
}

/// The ship rule, in the plan's words, and why it failed when it did.
pub fn ship(policy: &Scores, fixed: &Scores, needed: usize) -> Result<(), String> {
    if !policy.all_recovered {
        return Err("`need_tools` did not bring back every group that was held and needed".into());
    }
    if policy.miss_rate(needed) > fixed.miss_rate(needed) {
        return Err(format!(
            "the policy table missed {:.0}% of needed groups, static narrowing {:.0}%",
            policy.miss_rate(needed) * 100.0,
            fixed.miss_rate(needed) * 100.0
        ));
    }
    Ok(())
}

/// How many groups the cases need, over all of them.
pub fn needed_total(cases: &[Case]) -> usize {
    cases.iter().map(|c| c.needs.len()).sum()
}

pub fn report(rows: &[Scores], needed: usize) -> String {
    let mut out = String::from("strategy                     held  missed  hurt  saved-right  tokens-saved  miss-rate\n");
    for s in rows {
        out.push_str(&format!(
            "{:<28} {:>4}  {:>6}  {:>4}  {:>11}  {:>12}  {:>8.1}%\n",
            s.strategy,
            s.held,
            s.missed,
            s.cases_hurt,
            s.saved_right,
            s.tokens_saved,
            s.miss_rate(needed) * 100.0
        ));
    }
    out
}

/// Used by the opt-in test: a case's question for every group on the menu.
pub fn ask_for(case: &Case) -> Option<tools::ToolAsk> {
    tools::ask_for(&case.request, "", &menu())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A perfect model: certain of every group, right every time.
    fn oracle(case: &Case, id: &str) -> f32 {
        if case.needs.iter().any(|n| n == id) {
            0.97
        } else {
            0.02
        }
    }

    /// A model that is sure of nothing: every group is a coin toss.
    fn coin(_: &Case, _: &str) -> f32 {
        0.5
    }

    /// A model that is confidently wrong about the first group a case needs.
    fn wrong_about_the_first(case: &Case, id: &str) -> f32 {
        if case.needs.first().is_some_and(|n| n == id) {
            0.01
        } else {
            oracle(case, id)
        }
    }

    #[test]
    fn the_golden_set_is_readable_and_covers_every_slice() {
        let c = cases();
        let lines = FILE.lines().filter(|l| !l.trim().is_empty()).count();
        assert_eq!(c.len(), lines, "a row did not parse");
        for slice in ["base", "apps", "delegate"] {
            assert!(c.iter().filter(|c| c.slice == slice).count() >= 7, "{slice} is too thin");
        }
        let mut ids: Vec<&str> = c.iter().map(|c| c.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), c.len(), "duplicate ids");
        for case in &c {
            for need in &case.needs {
                assert!(unit(need).is_some(), "{}: unknown group {need}", case.id);
            }
        }
        assert!(c.iter().any(|c| c.needs.is_empty()), "plain conversation is the point");
        assert!(c.iter().any(|c| c.needs.len() > 1), "a request can need two groups");
    }

    #[test]
    fn every_case_builds_a_valid_ask_over_the_whole_menu() {
        for case in cases() {
            let ask = ask_for(&case).unwrap();
            assert_eq!(ask.questions.len(), CATALOGUE.len());
            assert!(super::super::local::check_ask(&ask.as_ask()).is_ok());
        }
    }

    #[test]
    fn the_catalogue_groups_match_what_a_run_would_describe() {
        for (id, label, _) in CATALOGUE {
            let u = unit(id).unwrap();
            assert_eq!((u.id.as_str(), u.label.as_str()), (id, label));
        }
        assert!(unit("nope").is_none());
    }

    #[test]
    fn a_perfect_model_loses_nothing_and_saves_the_most() {
        let c = cases();
        let needed = needed_total(&c);
        let keep = score(Strategy::Keep, &c, &oracle);
        let policy = score(Strategy::Policy { calibrated: false }, &c, &oracle);
        assert_eq!((keep.held, keep.tokens_saved), (0, 0));
        assert_eq!((policy.missed, policy.cases_hurt), (0, 0));
        assert_eq!(policy.held, c.len() * CATALOGUE.len() - needed, "every group nobody needed is held back");
        assert!(policy.tokens_saved > 10_000, "{} tokens", policy.tokens_saved);
        assert!(policy.all_recovered);
        assert!(ship(&policy, &score(Strategy::Static(0.3), &c, &oracle), needed).is_ok());
    }

    /// The reason the table asks for a confident no: with a model that is unsure,
    /// the policy holds back nothing, where a loose static cut-off holds back
    /// whatever the model put under it.
    #[test]
    fn an_unsure_model_gets_nothing_held_back_by_the_policy() {
        let c = cases();
        let unsure = |_: &Case, _: &str| 0.2;
        assert_eq!(score(Strategy::Policy { calibrated: true }, &c, &unsure).held, 0, "0.2 is not under 0.15");
        let loose = score(Strategy::Static(0.3), &c, &unsure);
        assert_eq!(loose.held, c.len() * CATALOGUE.len());
        assert!(loose.missed > 0, "and it held back what was needed");
        assert_eq!(score(Strategy::Policy { calibrated: false }, &c, &coin).held, 0);
    }

    #[test]
    fn calibrating_lets_the_policy_hold_back_a_little_more_but_never_less() {
        let c = cases();
        let between = |_: &Case, _: &str| 0.10;
        let before = score(Strategy::Policy { calibrated: false }, &c, &between);
        let after = score(Strategy::Policy { calibrated: true }, &c, &between);
        assert_eq!(before.held, 0, "0.10 is over 0.05");
        assert!(after.held > before.held, "0.10 is under 0.15");
    }

    #[test]
    fn a_confidently_wrong_model_misses_and_need_tools_still_brings_it_back() {
        let c = cases();
        let needed = needed_total(&c);
        let policy = score(Strategy::Policy { calibrated: false }, &c, &wrong_about_the_first);
        let hurt = c.iter().filter(|c| !c.needs.is_empty()).count();
        assert_eq!(policy.missed, hurt, "one miss for each case that needs something");
        assert_eq!(policy.cases_hurt, hurt);
        assert!(policy.all_recovered, "the group was held, so it can be asked for");
        // It is no worse than the static cut-off, which holds back the same group.
        let fixed = score(Strategy::Static(0.3), &c, &wrong_about_the_first);
        assert!(ship(&policy, &fixed, needed).is_ok());
    }

    #[test]
    fn the_ship_rule_fails_a_policy_that_misses_more_than_the_cut_off() {
        let c = cases();
        let needed = needed_total(&c);
        let worse = Scores { missed: 9, ..score(Strategy::Policy { calibrated: false }, &c, &oracle) };
        let better = Scores { missed: 2, ..worse.clone() };
        assert!(ship(&worse, &better, needed).unwrap_err().contains("missed"));
        let lost = Scores { all_recovered: false, ..better.clone() };
        assert!(ship(&lost, &better, needed).unwrap_err().contains("need_tools"));
        assert_eq!(better.miss_rate(0), 0.0, "nothing needed, nothing missed");
    }

    #[test]
    fn what_a_group_costs_comes_from_the_real_specs() {
        assert!(spec_tokens("web_search") > 50);
        assert!(spec_tokens("subagents") > spec_tokens("web_search"), "agent rule blocks make delegate the biggest");
        assert_eq!(spec_tokens("apps"), 1_200);
        assert_eq!(spec_tokens("nope"), 0);
    }

    #[test]
    fn the_report_lists_every_strategy() {
        let c = cases();
        let rows: Vec<Scores> = [Strategy::Keep, Strategy::Static(0.3), Strategy::Policy { calibrated: true }]
            .into_iter()
            .map(|s| score(s, &c, &oracle))
            .collect();
        let text = report(&rows, needed_total(&c));
        assert!(text.contains("keep everything") && text.contains("static, under 0.30") && text.contains("policy table, calibrated"));
    }
}
