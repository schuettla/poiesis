//! The Experiments framework (`EXP-1`, `EXP-2`): behaviour worth shipping before
//! it is worth defaulting. An experiment is a named switch with a contract:
//! what it changes, what it records, what it needs, and the gate for leaving.
//!
//! The registry is this file. The frontend reads it through one command
//! (`experiments_list_cmd`), so the two sides cannot drift. State is
//! `experiment.<id>` in settings, `on` or `off`.
//!
//! Both sides gate: the backend checks here before it advertises a tool or runs a
//! purpose, and the frontend hides the entry points.
//!
//! What is **not** an experiment: an agent app's mode, rules and guards, *Let it
//! use my memory*, *Let it use my connected apps* and the connector policies.
//! They are the user saying what is allowed, not Poiesis trying a new behaviour.

use serde::Serialize;

use crate::cloud::Provider;
use crate::db::Db;

/// How sure we are about it. Shown as a tag on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Early,
    Beta,
}

/// Something an experiment cannot run without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// A decision model is installed (`DEC-3`).
    DecisionModel,
    /// The installed decision model can read images (its projector is there).
    Vision,
    /// A key for this provider (`cloud::Provider::id`) is saved.
    Provider(&'static str),
    /// Another experiment is on.
    Experiment(&'static str),
    /// A mail account (IMAP) is set up and switched on.
    MailAccount,
}

pub struct Experiment {
    pub id: &'static str,
    pub title: &'static str,
    /// One sentence, in the user's language.
    pub summary: &'static str,
    pub what_changes: &'static [&'static str],
    /// What it logs locally.
    pub records: &'static str,
    pub requires: &'static [Requirement],
    pub stage: Stage,
    pub default_on: bool,
    /// The gate for leaving Experiments. Graduating is a code change, not a toggle.
    pub graduation: &'static str,
}

/// Every experiment there is. A new one is added here by the step that builds
/// its behaviour, never before: a card whose switch does nothing is a lie.
pub const REGISTRY: &[Experiment] = &[
    Experiment {
        id: "fast-slow",
        title: "Fast & slow thinking",
        summary: "I answer small questions with a quick decision model and save the slow thinking for what needs it.",
        what_changes: &[
            "I may hold back tools a message clearly doesn't need",
            "I may raise a handover card before I start, when your own rule clearly fits",
            "I may skip the lesson pass on a chat where nothing went wrong",
            "I may mark outside text that reads like orders to me",
            "I get a tool to sort a long list with one question about each item, and show you the table",
            "I may tick off plan steps my answer finished, and tell you what a message you send me mid-run is, such as a stop",
            "I may skip a scheduled task whose \"only run if\" condition does not hold",
            "I may look at a picture of each page I open in the browser, and stop to ask you to take over at a login or a captcha. The picture stays on this computer unless you chose a cloud decision model",
            "I may ask you before a command, a message or a click that I think you did not ask for or that cannot be undone, and say why on the question. I may also add a line to a question I was going to ask anyway, including one from a connected app or an agent app. The start of the call and of your request go to the decision model, and to a cloud one if you chose it",
            "I may take a connected app's tool for something that sends, whatever its name says, and ask before it like any send. I never take one for less than its name says",
            "I check each answer against what I read for it, and underline a statement only when it looks wrong, with the reason one click away. A check that finds nothing says nothing. This adds no search; with a cloud decision model, the sentences go to it",
            "While you type, I may say in one line what your message will do: that a rule of yours hands it to an agent app, that it needs the web, or that it reads as a stop for a running task. With a cloud decision model, what you type goes to it before you send",
            "Double-check, under an answer, searches the web for its claims (their words leave this computer) and has my slower model review up to four",
        ],
        records: "Each decision, its confidence, what you did about it, and the check of each answer: its claims, the passages they were judged against and each verdict.",
        requires: &[Requirement::DecisionModel],
        stage: Stage::Early,
        default_on: false,
        graduation: "The tool-menu ablation passes, held-back tools are asked for back on fewer than 5% of turns, the golden set does not regress, the claim set shows the blind review catching more wrong claims than the quick verdict alone.",
    },
    Experiment {
        id: "mail-triage",
        title: "Mail triage",
        summary: "I look over new mail in your accounts and tag it, so you can see what matters first. I never move, answer or delete anything.",
        what_changes: &[
            "I check your mail accounts every 15 minutes while Poiesis is open, and keep who wrote, the subject and the date of the newest 200 messages for 30 days",
            "Mail gets an Inbox with a kind, an urgency bar and a mark when a message wants a reply",
            "I read each new message on this computer for that. Its text is not kept and never leaves this computer",
        ],
        records: "Each check and its confidence, and the kind you set when you change one.",
        requires: &[Requirement::DecisionModel, Requirement::MailAccount],
        stage: Stage::Early,
        default_on: false,
        graduation: "A month on a real mailbox with at least 100 mails tagged and fewer than 10% of the tags changed by you.",
    },
    Experiment {
        id: "relax-prompts",
        title: "Let quick decisions skip low-risk prompts",
        summary: "When a quick decision is very sure that a small change to one file is what you asked for, I may make it without asking first, and tell you that I did.",
        what_changes: &[
            "I may skip the question before writing or editing one file in your attached folder, but only once the decision model has been calibrated and is at least 95% sure that the change is small and what you asked for",
            "I never skip a question about deleting, moving, sending, running a command, a connected app, an agent app or a file outside your attached folder",
            "A skipped question is written on that step and in Activity",
        ],
        records: "Each skipped question, the tool, and how sure the decision was.",
        requires: &[Requirement::DecisionModel, Requirement::Experiment("fast-slow")],
        stage: Stage::Early,
        default_on: false,
        graduation: "A month of use with no skipped change that you undid or called wrong, and a calibration of the risk check fitted on at least 100 labels.",
    },
    Experiment {
        id: "workspace",
        title: "Workspace mode",
        summary: "I build an interface for the task (a board, a tracker, a picker) next to the chat.",
        what_changes: &[
            "A chat can open as a Workspace, with a canvas I compose for the task",
            "I get a tool for drawing that canvas",
            "I may offer to switch a chat into Workspace mode",
        ],
        records: "Nothing of its own. A Workspace chat is stored like any chat.",
        requires: &[],
        stage: Stage::Beta,
        // Off for a new user. `migrate` turns it on for anyone who already has a
        // Workspace chat, so nobody loses what they use.
        default_on: false,
        graduation: "Used in a quarter of active chats for a month, with no open layout bugs.",
    },
];

pub fn find(id: &str) -> Option<&'static Experiment> {
    REGISTRY.iter().find(|e| e.id == id)
}

pub fn setting_key(id: &str) -> String {
    format!("experiment.{id}")
}

/// The stored state, or the experiment's default. An id nobody registered is off:
/// a typo in a gate must never switch something on.
pub fn is_on(db: &Db, id: &str) -> bool {
    is_on_in(REGISTRY, db, id)
}

fn is_on_in(registry: &[Experiment], db: &Db, id: &str) -> bool {
    let Some(exp) = registry.iter().find(|e| e.id == id) else {
        return false;
    };
    match db.get_setting(&setting_key(id)).ok().flatten().as_deref().map(str::trim) {
        Some("on") => true,
        Some("off") => false,
        _ => exp.default_on,
    }
}

/// Is this chat a Workspace chat *now*? The chat's own flag is kept when the
/// experiment is off (nothing is deleted), but then it behaves as a normal chat:
/// no canvas tool, no canvas guidance. One answer for every backend site that
/// used to read `conversation.workspace`.
pub fn in_workspace(db: &Db, conv: &crate::db::Conversation) -> bool {
    conv.workspace && is_on(db, "workspace")
}

/// What this computer has, for the requirements that depend on it. Gathered by
/// the caller, so the rules below can be tested without a model on disk.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts {
    pub decision_model: bool,
    pub vision: bool,
}

/// One requirement, as the card shows it.
#[derive(Debug, Clone, Serialize)]
pub struct RequirementView {
    /// `decision-model` | `vision` | `provider` | `experiment` | `mail-account` | `evidence`
    pub kind: &'static str,
    pub label: String,
    pub met: bool,
    /// The settings view that fixes it, when there is one.
    pub fix_view: Option<&'static str>,
}

fn requirement_view(registry: &[Experiment], db: &Db, facts: Facts, req: Requirement) -> RequirementView {
    match req {
        Requirement::DecisionModel => RequirementView {
            kind: "decision-model",
            label: "A decision model".into(),
            met: facts.decision_model,
            fix_view: Some("models"),
        },
        Requirement::Vision => RequirementView {
            kind: "vision",
            label: "A decision model that can read images".into(),
            met: facts.vision,
            fix_view: Some("models"),
        },
        Requirement::Provider(id) => RequirementView {
            kind: "provider",
            label: format!("A key for {}", Provider::from_id(id).map(|p| p.name()).unwrap_or(id)),
            met: Provider::from_id(id).is_some_and(crate::cloud::has_key),
            fix_view: Some("providers"),
        },
        Requirement::Experiment(id) => RequirementView {
            kind: "experiment",
            label: format!("{} turned on", registry.iter().find(|e| e.id == id).map(|e| e.title).unwrap_or(id)),
            met: is_on_in(registry, db, id),
            fix_view: None,
        },
        Requirement::MailAccount => RequirementView {
            kind: "mail-account",
            label: "A mail account".into(),
            met: db.list_mail_accounts().is_ok_and(|accounts| accounts.iter().any(|a| a.enabled)),
            fix_view: Some("mail"),
        },
    }
}

/// One experiment, for the Experiments screen.
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentView {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub what_changes: &'static [&'static str],
    pub records: &'static str,
    pub stage: Stage,
    pub graduation: &'static str,
    pub on: bool,
    pub requires: Vec<RequirementView>,
    /// Every requirement is met, so the switch may be turned on.
    pub can_turn_on: bool,
    /// Not seen on the Experiments screen yet (the tab's dot).
    pub is_new: bool,
}

const SEEN_KEY: &str = "experiments.seen";

fn seen(db: &Db) -> Vec<String> {
    db.get_setting(SEEN_KEY)
        .ok()
        .flatten()
        .map(|s| s.split(',').filter(|p| !p.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn list(db: &Db, facts: Facts) -> Vec<ExperimentView> {
    list_in(REGISTRY, db, facts)
}

fn list_in(registry: &[Experiment], db: &Db, facts: Facts) -> Vec<ExperimentView> {
    let seen = seen(db);
    registry
        .iter()
        .map(|e| {
            let requires: Vec<RequirementView> =
                e.requires.iter().map(|r| requirement_view(registry, db, facts, *r)).collect();
            ExperimentView {
                id: e.id,
                title: e.title,
                summary: e.summary,
                what_changes: e.what_changes,
                records: e.records,
                stage: e.stage,
                graduation: e.graduation,
                on: is_on_in(registry, db, e.id),
                can_turn_on: requires.iter().all(|r| r.met),
                requires,
                is_new: !seen.iter().any(|s| s == e.id),
            }
        })
        .collect()
}

/// The user opened the screen: nothing on it is new any more.
pub fn mark_seen(db: &Db) -> Result<(), String> {
    let ids = REGISTRY.iter().map(|e| e.id).collect::<Vec<_>>().join(",");
    db.set_setting(SEEN_KEY, &ids).map_err(|e| e.to_string())
}

/// Turn one on or off. Turning on needs every requirement met, here as well as
/// on the card, because the card can be stale. Turning off always works, and it
/// takes with it anything that needs this one.
pub fn set(db: &Db, id: &str, on: bool, facts: Facts) -> Result<(), String> {
    set_in(REGISTRY, db, id, on, facts)
}

fn set_in(registry: &[Experiment], db: &Db, id: &str, on: bool, facts: Facts) -> Result<(), String> {
    let exp = registry.iter().find(|e| e.id == id).ok_or_else(|| format!("There is no experiment \"{id}\"."))?;
    if on {
        let missing = exp
            .requires
            .iter()
            .map(|r| requirement_view(registry, db, facts, *r))
            .find(|r| !r.met);
        if let Some(r) = missing {
            return Err(format!("{} needs: {}.", exp.title, r.label.to_lowercase()));
        }
    }
    db.set_setting(&setting_key(id), if on { "on" } else { "off" }).map_err(|e| e.to_string())?;
    if !on {
        for dependant in registry.iter().filter(|e| e.requires.contains(&Requirement::Experiment(exp.id))) {
            set_in(registry, db, dependant.id, false, facts)?;
        }
    }
    Ok(())
}

const MIGRATED_KEY: &str = "experiments.migrated";
const VERIFY_FOLDED_KEY: &str = "experiments.verify-folded";

/// `EXP-3`: run once at startup. Workspace mode became an experiment after
/// people were already using it, so anyone with a Workspace chat keeps it on.
/// Everyone else finds it in Experiments. A choice already made is never
/// overwritten.
pub fn migrate(db: &Db) {
    fold_verify(db);
    if db.get_setting(MIGRATED_KEY).ok().flatten().is_some() {
        return;
    }
    let key = setting_key("workspace");
    if db.get_setting(&key).ok().flatten().is_none() {
        let used = db.has_workspace_conversation().unwrap_or(false);
        let _ = db.set_setting(&key, if used { "on" } else { "off" });
    }
    let _ = db.set_setting(MIGRATED_KEY, "1");
}

/// `SEE-5`: Verify answers stopped being an experiment of its own and became part
/// of `fast-slow`. Someone who had it on gets `fast-slow` on, because they asked for
/// the check; then the old key is deleted either way, so it can never switch
/// anything on again. It has its own marker because `migrate` ran once already on
/// every computer that had the earlier version.
fn fold_verify(db: &Db) {
    if db.get_setting(VERIFY_FOLDED_KEY).ok().flatten().is_some() {
        return;
    }
    let key = setting_key("verify");
    if db.get_setting(&key).ok().flatten().as_deref().map(str::trim) == Some("on") {
        let _ = db.set_setting(&setting_key("fast-slow"), "on");
    }
    let _ = db.delete_setting(&key);
    let _ = db.set_setting(VERIFY_FOLDED_KEY, "1");
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Experiment = Experiment {
        id: "base",
        title: "Base",
        summary: "",
        what_changes: &[],
        records: "",
        requires: &[Requirement::DecisionModel],
        stage: Stage::Early,
        default_on: false,
        graduation: "",
    };
    const ON_TOP: Experiment = Experiment {
        id: "top",
        title: "Top",
        requires: &[Requirement::Experiment("base")],
        ..BASE
    };
    const FIXTURE: &[Experiment] = &[BASE, ON_TOP];
    const HAVE_MODEL: Facts = Facts { decision_model: true, vision: false };

    #[test]
    fn the_workspace_experiment_is_registered_and_off_by_default() {
        let db = Db::open_in_memory().unwrap();
        assert!(find("workspace").is_some());
        assert!(!is_on(&db, "workspace"));
    }

    #[test]
    fn ids_are_unique_and_every_experiment_requirement_names_a_registered_one() {
        let mut seen = std::collections::HashSet::new();
        for e in REGISTRY {
            assert!(seen.insert(e.id), "duplicate id {}", e.id);
            for r in e.requires {
                if let Requirement::Experiment(other) = r {
                    assert!(find(other).is_some(), "{} needs an unknown experiment {other}", e.id);
                }
            }
        }
    }

    /// `EXP-3`: the chat keeps its flag, and behaves as a normal chat until the
    /// experiment is on.
    #[test]
    fn a_workspace_chat_is_a_workspace_chat_only_while_the_experiment_is_on() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("Trip", None, true).unwrap();
        assert!(conv.workspace);
        assert!(!in_workspace(&db, &conv));
        set(&db, "workspace", true, Facts::default()).unwrap();
        assert!(in_workspace(&db, &conv));
        let plain = db.create_conversation("Plain", None, false).unwrap();
        assert!(!in_workspace(&db, &plain));
    }

    #[test]
    fn an_unknown_id_is_off() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(&setting_key("nope"), "on").unwrap();
        assert!(!is_on(&db, "nope"));
        assert!(set(&db, "nope", true, Facts::default()).is_err());
    }

    #[test]
    fn the_stored_state_wins_over_the_default() {
        let db = Db::open_in_memory().unwrap();
        set(&db, "workspace", true, Facts::default()).unwrap();
        assert!(is_on(&db, "workspace"));
        set(&db, "workspace", false, Facts::default()).unwrap();
        assert!(!is_on(&db, "workspace"));
    }

    #[test]
    fn turning_on_needs_every_requirement_and_names_the_missing_one() {
        let db = Db::open_in_memory().unwrap();
        let err = set_in(FIXTURE, &db, "base", true, Facts::default()).unwrap_err();
        assert_eq!(err, "Base needs: a decision model.");
        assert!(!is_on_in(FIXTURE, &db, "base"));
        set_in(FIXTURE, &db, "base", true, HAVE_MODEL).unwrap();
        assert!(is_on_in(FIXTURE, &db, "base"));
    }

    #[test]
    fn an_experiment_needing_another_waits_for_it_and_goes_off_with_it() {
        let db = Db::open_in_memory().unwrap();
        assert!(set_in(FIXTURE, &db, "top", true, HAVE_MODEL).is_err(), "base is off");
        set_in(FIXTURE, &db, "base", true, HAVE_MODEL).unwrap();
        set_in(FIXTURE, &db, "top", true, HAVE_MODEL).unwrap();
        // Turning the first off takes the second with it, so it can't run on nothing.
        set_in(FIXTURE, &db, "base", false, HAVE_MODEL).unwrap();
        assert!(!is_on_in(FIXTURE, &db, "top"));
    }

    #[test]
    fn the_list_says_what_is_missing_and_whether_it_can_be_turned_on() {
        let db = Db::open_in_memory().unwrap();
        let rows = list_in(FIXTURE, &db, Facts::default());
        assert!(!rows[0].can_turn_on);
        assert_eq!(rows[0].requires[0].kind, "decision-model");
        assert_eq!(rows[0].requires[0].fix_view, Some("models"));
        assert!(!rows[1].can_turn_on, "top needs base");
        let rows = list_in(FIXTURE, &db, HAVE_MODEL);
        assert!(rows[0].can_turn_on);
        // A requirement-free experiment can always be turned on.
        let all = list(&db, Facts::default());
        assert!(all.iter().find(|e| e.id == "workspace").unwrap().can_turn_on);
        assert!(!all.iter().find(|e| e.id == "fast-slow").unwrap().can_turn_on, "it needs a decision model");
    }

    #[test]
    fn fast_slow_is_registered_off_by_default_and_needs_a_decision_model() {
        let db = Db::open_in_memory().unwrap();
        let exp = find("fast-slow").unwrap();
        assert!(!exp.default_on);
        assert_eq!(exp.requires, &[Requirement::DecisionModel]);
        assert!(!is_on(&db, "fast-slow"));
        assert!(set(&db, "fast-slow", true, Facts::default()).is_err());
        set(&db, "fast-slow", true, Facts { decision_model: true, vision: false }).unwrap();
        assert!(is_on(&db, "fast-slow"));
    }

    /// `USE-6`: Mail triage needs a decision model and a switched-on mail account,
    /// and names the Mail screen as the place to fix the second.
    #[test]
    fn mail_triage_needs_a_decision_model_and_a_mail_account() {
        let db = Db::open_in_memory().unwrap();
        let exp = find("mail-triage").unwrap();
        assert!(!exp.default_on);
        assert_eq!(exp.requires, &[Requirement::DecisionModel, Requirement::MailAccount]);
        let err = set(&db, "mail-triage", true, HAVE_MODEL).unwrap_err();
        assert_eq!(err, "Mail triage needs: a mail account.");
        let card = list(&db, HAVE_MODEL).into_iter().find(|e| e.id == "mail-triage").unwrap();
        let mail = card.requires.iter().find(|r| r.kind == "mail-account").unwrap();
        assert!(!mail.met);
        assert_eq!(mail.fix_view, Some("mail"));
        assert!(!card.can_turn_on);

        let account = db
            .add_mail_account(&crate::db::NewMailAccount {
                label: "Work".into(),
                email: "me@example.com".into(),
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                smtp_host: "smtp.example.com".into(),
                smtp_port: 465,
                username: "me@example.com".into(),
                security: "tls".into(),
            })
            .unwrap();
        set(&db, "mail-triage", true, HAVE_MODEL).unwrap();
        assert!(is_on(&db, "mail-triage"));
        // A switched-off account is not one the loop would read.
        db.set_mail_account_enabled(&account.id, false).unwrap();
        assert!(set(&db, "mail-triage", false, HAVE_MODEL).is_ok(), "turning off always works");
        assert!(set(&db, "mail-triage", true, HAVE_MODEL).is_err());
    }

    /// `SEE-5`: Verify is part of `fast-slow`. It has no card of its own, the card
    /// of `fast-slow` says what the check does, and the check needs no evidence
    /// switch because it reads only what the turn read.
    #[test]
    fn verify_is_part_of_fast_slow_and_has_no_card_of_its_own() {
        assert!(find("verify").is_none());
        let db = Db::open_in_memory().unwrap();
        // The old id is just an unknown one now: a stale gate can never switch it on.
        db.set_setting(&setting_key("verify"), "on").unwrap();
        assert!(!is_on(&db, "verify"));
        assert!(set(&db, "verify", true, HAVE_MODEL).is_err());
        let card = list(&db, HAVE_MODEL).into_iter().find(|e| e.id == "fast-slow").unwrap();
        assert!(card.what_changes.iter().any(|l| l.contains("I check each answer against what I read for it")));
        assert!(card.what_changes.iter().any(|l| l.contains("Double-check")));
        assert!(list(&db, HAVE_MODEL).iter().all(|e| e.id != "verify"));
        assert!(card.requires.iter().all(|r| r.kind == "decision-model"), "no evidence requirement");
    }

    /// `SEE-T-9`: the fold, both ways. Verify on turns `fast-slow` on; Verify off
    /// leaves it as it was; the old key is gone either way and the move happens
    /// once, even on a computer where the first migration already ran.
    #[test]
    fn the_fold_turns_fast_slow_on_for_someone_who_had_verify_on() {
        let had = Db::open_in_memory().unwrap();
        had.set_setting(MIGRATED_KEY, "1").unwrap();
        had.set_setting(&setting_key("verify"), "on").unwrap();
        migrate(&had);
        assert!(is_on(&had, "fast-slow"));
        assert_eq!(had.get_setting(&setting_key("verify")).unwrap(), None);

        let off = Db::open_in_memory().unwrap();
        off.set_setting(MIGRATED_KEY, "1").unwrap();
        off.set_setting(&setting_key("verify"), "off").unwrap();
        migrate(&off);
        assert!(!is_on(&off, "fast-slow"));
        assert_eq!(off.get_setting(&setting_key("verify")).unwrap(), None);

        let never = Db::open_in_memory().unwrap();
        migrate(&never);
        assert!(!is_on(&never, "fast-slow"));
    }

    #[test]
    fn the_fold_keeps_a_choice_made_for_fast_slow_and_runs_once() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(&setting_key("verify"), "off").unwrap();
        db.set_setting(&setting_key("fast-slow"), "on").unwrap();
        migrate(&db);
        assert!(is_on(&db, "fast-slow"), "verify off does not turn fast-slow off");

        // Later the old key cannot come back and switch anything on.
        db.set_setting(&setting_key("fast-slow"), "off").unwrap();
        db.set_setting(&setting_key("verify"), "on").unwrap();
        migrate(&db);
        assert!(!is_on(&db, "fast-slow"));
    }

    #[test]
    fn an_experiment_is_new_until_the_screen_has_been_opened() {
        let db = Db::open_in_memory().unwrap();
        assert!(list(&db, Facts::default()).iter().all(|e| e.is_new));
        mark_seen(&db).unwrap();
        assert!(list(&db, Facts::default()).iter().all(|e| !e.is_new));
    }

    /// `EXP-T-1`: the migration turns Workspace on only when a Workspace chat exists.
    #[test]
    fn the_migration_keeps_workspace_on_only_for_someone_who_used_it() {
        let fresh = Db::open_in_memory().unwrap();
        fresh.create_conversation("Plain", None, false).unwrap();
        migrate(&fresh);
        assert!(!is_on(&fresh, "workspace"));

        let user = Db::open_in_memory().unwrap();
        user.create_conversation("Plain", None, false).unwrap();
        user.create_conversation("Trip", None, true).unwrap();
        migrate(&user);
        assert!(is_on(&user, "workspace"));
    }

    #[test]
    fn the_migration_runs_once_and_never_overwrites_a_choice() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db);
        assert!(!is_on(&db, "workspace"));
        // A Workspace chat made later does not turn it on behind the user's back.
        db.create_conversation("Trip", None, true).unwrap();
        migrate(&db);
        assert!(!is_on(&db, "workspace"));

        let chosen = Db::open_in_memory().unwrap();
        chosen.set_setting(&setting_key("workspace"), "off").unwrap();
        chosen.create_conversation("Trip", None, true).unwrap();
        migrate(&chosen);
        assert!(!is_on(&chosen, "workspace"), "an explicit off stays off");
    }
}
