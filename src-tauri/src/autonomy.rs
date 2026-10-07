//! The autonomy ladder (AUT-1) — the membrane between what Poiesis may change
//! on its own and what it must ask about.
//!
//! Every site that writes to the durable self consults this gate first. The
//! classes are deliberately few and the defaults deliberately conservative:
//! anything with a clean undo may be `auto`, anything that changes *identity*
//! (standing instructions, procedures) starts at `ask`.

use crate::db::Db;

/// How much freedom a class of self-change has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    /// Do it, tell the user, offer undo.
    Auto,
    /// Write a proposal the user answers.
    Ask,
    /// Don't do it at all — the capability is withdrawn.
    Off,
}

/// Rung when the user hasn't chosen. Undoable classes are `auto`; identity
/// classes are `ask`.
pub const AUTONOMY_DEFAULTS: &[(&str, &str)] = &[
    ("facts", "auto"),       // memory tool save/update/forget (undoable)
    ("lessons", "auto"),     // reflection saves high-confidence lessons (undoable)
    ("consolidate", "ask"),  // tidy-up apply (already ask-only via the MEM-5 flow)
    ("soul", "ask"),         // standing instructions (identity)
    ("profile", "auto"),     // synthesized style, derived + regenerable (PRO-8)
    ("email_send", "ask"),   // mail leaving the machine on the user's behalf (MAIL-3)
    ("skills", "ask"),       // new Agent Skills (identity, SKL-4)
    ("screen", "ask"),       // screenshot can contain anything (SYS-1)
    ("context", "auto"),     // clearing old tool output mid-run; undo = read_result (CLR)
    ("suggest", "auto"),     // suggestion chips; off means I never suggest commands (AGC-2)
    ("modes", "ask"),        // turning Workspace or Plan first on (AGC-1)
    ("schedule", "ask"),     // proposing scheduled jobs; auto is never offered (AGC-1)
];

/// Classes that may never be `auto`. A run nobody clicked into being is out of
/// bounds, so a hand-edited `auto` reads as `ask` rather than failing open.
pub const NO_AUTO: &[&str] = &["schedule"];

/// Settings key for a class. Public so the frontend and backend can't drift.
pub fn setting_key(class: &str) -> String {
    format!("autonomy.{class}")
}

fn default_for(class: &str) -> Rung {
    AUTONOMY_DEFAULTS
        .iter()
        .find(|(c, _)| *c == class)
        .map(|(_, r)| parse(r))
        // An unknown class is a programming error, not a user choice — the safe
        // reading of "I don't know what this is" is to ask.
        .unwrap_or(Rung::Ask)
}

fn parse(value: &str) -> Rung {
    match value {
        "auto" => Rung::Auto,
        "off" => Rung::Off,
        _ => Rung::Ask,
    }
}

/// The current rung for a self-change class. A missing or unreadable setting
/// falls back to the class default — the gate never fails open.
pub fn autonomy_gate(db: &Db, class: &str) -> Rung {
    let rung = match db.get_setting(&setting_key(class)).ok().flatten() {
        Some(v) if !v.trim().is_empty() => parse(v.trim()),
        _ => default_for(class),
    };
    if rung == Rung::Auto && NO_AUTO.contains(&class) {
        return Rung::Ask;
    }
    rung
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_until_the_user_chooses() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(autonomy_gate(&db, "facts"), Rung::Auto);
        assert_eq!(autonomy_gate(&db, "lessons"), Rung::Auto);
        assert_eq!(autonomy_gate(&db, "soul"), Rung::Ask);
        assert_eq!(autonomy_gate(&db, "skills"), Rung::Ask);
        assert_eq!(autonomy_gate(&db, "consolidate"), Rung::Ask);
        assert_eq!(autonomy_gate(&db, "screen"), Rung::Ask);
        // MAIL-3: mail leaving the machine is never the default.
        assert_eq!(autonomy_gate(&db, "email_send"), Rung::Ask);

        db.set_setting(&setting_key("lessons"), "off").unwrap();
        assert_eq!(autonomy_gate(&db, "lessons"), Rung::Off);
        db.set_setting(&setting_key("soul"), "auto").unwrap();
        assert_eq!(autonomy_gate(&db, "soul"), Rung::Auto);
    }

    /// `AGC-T3`: scheduling is never `auto`, whatever the setting says.
    #[test]
    fn a_class_that_may_never_be_auto_reads_auto_as_ask() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(autonomy_gate(&db, "schedule"), Rung::Ask);
        db.set_setting(&setting_key("schedule"), "auto").unwrap();
        assert_eq!(autonomy_gate(&db, "schedule"), Rung::Ask, "a hand-edited auto still asks");
        db.set_setting(&setting_key("schedule"), "off").unwrap();
        assert_eq!(autonomy_gate(&db, "schedule"), Rung::Off, "off is still off");
    }

    /// The four classes the harness tool reads, with the defaults the plan sets.
    #[test]
    fn the_harness_classes_have_their_defaults() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(autonomy_gate(&db, "context"), Rung::Auto);
        assert_eq!(autonomy_gate(&db, "suggest"), Rung::Auto);
        assert_eq!(autonomy_gate(&db, "modes"), Rung::Ask);
        assert_eq!(autonomy_gate(&db, "schedule"), Rung::Ask);
    }

    #[test]
    fn unknown_values_and_classes_fall_back_to_asking() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(autonomy_gate(&db, "not-a-class"), Rung::Ask);
        db.set_setting(&setting_key("facts"), "wat").unwrap();
        assert_eq!(autonomy_gate(&db, "facts"), Rung::Ask, "garbage never means auto");
        db.set_setting(&setting_key("facts"), "").unwrap();
        assert_eq!(autonomy_gate(&db, "facts"), Rung::Auto, "cleared → default");
    }
}
