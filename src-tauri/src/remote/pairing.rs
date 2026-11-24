//! REM-3: pairing. A phone is paired with a one-time code that only the desktop window shows, and
//! the paired senders are kept as a short list in the settings (no new table).
//!
//! Codes live in memory only. A code is eight characters, valid for five minutes, used once, and
//! dropped after five wrong tries. A new code replaces the old one.

use std::sync::Mutex;

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::db::Db;

/// No `I`, `O`, `0` or `1`: they are easy to misread when typed from a screen.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const CODE_LEN: usize = 8;
pub const CODE_TTL_MS: i64 = 5 * 60 * 1000;
pub const MAX_WRONG: u32 = 5;

/// What the window shows: the code and when it stops working (epoch ms).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PairingCode {
    pub code: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairError {
    /// Nobody asked the desktop for a code, or it was already used or dropped.
    NoCode,
    Expired,
    Wrong,
    /// The fifth wrong try. The code is gone; the desktop has to show a new one.
    TooMany,
}

impl PairError {
    /// One plain line for the phone.
    pub fn words(self) -> &'static str {
        match self {
            PairError::NoCode => "There is no pairing code on the desktop. Open Settings > Remote and ask for one.",
            PairError::Expired => "That code has run out. Ask the desktop for a new one.",
            PairError::Wrong => "That code is not right.",
            PairError::TooMany => "Too many wrong tries. Ask the desktop for a new code.",
        }
    }
}

struct Active {
    code: String,
    expires_at: i64,
    wrong: u32,
}

/// The one code that can be redeemed right now.
#[derive(Default)]
pub struct Codes {
    active: Mutex<Option<Active>>,
}

fn random_code() -> String {
    let mut rng = rand::thread_rng();
    (0..CODE_LEN).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect()
}

/// Typed codes are forgiven case, spaces and dashes.
fn normal(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_uppercase()
}

/// Compare without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Codes {
    /// A fresh code. Any earlier one stops working.
    pub fn issue(&self, now: i64) -> PairingCode {
        self.issue_code(random_code(), now)
    }

    pub(crate) fn issue_code(&self, code: String, now: i64) -> PairingCode {
        let expires_at = now + CODE_TTL_MS;
        *self.active.lock().unwrap() = Some(Active { code: code.clone(), expires_at, wrong: 0 });
        PairingCode { code, expires_at }
    }

    /// Try a code. Right: it is used up and `Ok` comes back. Wrong: a try is counted, and the
    /// fifth drops the code.
    pub fn redeem(&self, attempt: &str, now: i64) -> Result<(), PairError> {
        let mut slot = self.active.lock().unwrap();
        let Some(active) = slot.as_mut() else { return Err(PairError::NoCode) };
        if now >= active.expires_at {
            *slot = None;
            return Err(PairError::Expired);
        }
        if same(&normal(attempt), &active.code) {
            *slot = None;
            return Ok(());
        }
        active.wrong += 1;
        if active.wrong >= MAX_WRONG {
            *slot = None;
            return Err(PairError::TooMany);
        }
        Err(PairError::Wrong)
    }

    /// A code is waiting right now. A phone that is not paired may only knock while this is true.
    pub fn is_open(&self, now: i64) -> bool {
        self.active.lock().unwrap().as_ref().is_some_and(|a| now < a.expires_at)
    }

    /// Drop the code, for the kill switch and a closed dialog.
    pub fn clear(&self) {
        *self.active.lock().unwrap() = None;
    }
}

// ---- the paired senders ----

/// The setting that holds the list. Only the desktop window changes it.
pub const SENDERS_KEY: &str = "remote.senders";

/// One paired phone (or, later, Telegram account).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sender {
    /// `phone` | `telegram`.
    pub transport: String,
    /// The phone's public key, or the Telegram id.
    pub id: String,
    /// What the person calls it, shown in the window and the chat list.
    pub label: String,
    pub paired_at: i64,
    pub last_seen: i64,
    /// What it may do. Only `chat` exists today; the field keeps room for less.
    pub level: String,
    /// The chat this sender is in now.
    #[serde(default)]
    pub conversation_id: Option<String>,
}

pub fn load(db: &Db) -> Vec<Sender> {
    db.get_setting(SENDERS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(db: &Db, senders: &[Sender]) -> Result<(), String> {
    let raw = serde_json::to_string(senders).map_err(|e| e.to_string())?;
    db.set_setting(SENDERS_KEY, &raw).map_err(|e| e.to_string())
}

pub fn find(db: &Db, transport: &str, id: &str) -> Option<Sender> {
    load(db).into_iter().find(|s| s.transport == transport && s.id == id)
}

/// Add a sender, or replace the one with the same transport and id (a phone paired twice).
pub fn add(db: &Db, sender: Sender) -> Result<(), String> {
    let mut all = load(db);
    all.retain(|s| !(s.transport == sender.transport && s.id == sender.id));
    all.push(sender);
    save(db, &all)
}

/// Remove a sender. `true` when there was one.
pub fn remove(db: &Db, transport: &str, id: &str) -> Result<bool, String> {
    let mut all = load(db);
    let before = all.len();
    all.retain(|s| !(s.transport == transport && s.id == id));
    let removed = all.len() != before;
    if removed {
        save(db, &all)?;
    }
    Ok(removed)
}

/// Change one sender in place.
pub fn update(db: &Db, transport: &str, id: &str, change: impl FnOnce(&mut Sender)) -> Result<(), String> {
    let mut all = load(db);
    if let Some(s) = all.iter_mut().find(|s| s.transport == transport && s.id == id) {
        change(s);
        save(db, &all)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_eight_characters_from_the_plain_alphabet() {
        let codes = Codes::default();
        let c = codes.issue(0);
        assert_eq!(c.code.len(), CODE_LEN);
        assert!(c.code.bytes().all(|b| ALPHABET.contains(&b)));
        assert_eq!(c.expires_at, CODE_TTL_MS);
    }

    #[test]
    fn a_code_works_once() {
        let codes = Codes::default();
        let c = codes.issue(0).code;
        assert_eq!(codes.redeem(&c, 1_000), Ok(()));
        assert_eq!(codes.redeem(&c, 1_001), Err(PairError::NoCode), "code reuse is refused");
    }

    #[test]
    fn a_typed_code_forgives_case_spaces_and_dashes() {
        let codes = Codes::default();
        codes.issue_code("ABCD2345".into(), 0);
        assert_eq!(codes.redeem("abcd-2345", 10), Ok(()));
        codes.issue_code("ABCD2345".into(), 0);
        assert_eq!(codes.redeem(" ab cd 23 45 ", 10), Ok(()));
    }

    #[test]
    fn a_code_runs_out_after_five_minutes() {
        let codes = Codes::default();
        let c = codes.issue(0).code;
        assert_eq!(codes.redeem(&c, CODE_TTL_MS), Err(PairError::Expired));
        assert_eq!(codes.redeem(&c, CODE_TTL_MS + 1), Err(PairError::NoCode), "and is gone");
    }

    #[test]
    fn the_fifth_wrong_try_drops_the_code_even_for_the_right_answer() {
        let codes = Codes::default();
        let c = codes.issue(0).code;
        for _ in 0..4 {
            assert_eq!(codes.redeem("WRONGWRONG", 1), Err(PairError::Wrong));
        }
        assert_eq!(codes.redeem("WRONGWRONG", 1), Err(PairError::TooMany));
        assert_eq!(codes.redeem(&c, 2), Err(PairError::NoCode));
    }

    #[test]
    fn a_code_is_open_until_it_is_used_or_runs_out() {
        let codes = Codes::default();
        assert!(!codes.is_open(0), "nobody asked for a code");
        let c = codes.issue(0).code;
        assert!(codes.is_open(1));
        assert!(!codes.is_open(CODE_TTL_MS));
        codes.redeem(&c, 1).unwrap();
        assert!(!codes.is_open(2));
    }

    #[test]
    fn a_new_code_replaces_the_old_one() {
        let codes = Codes::default();
        codes.issue_code("AAAAAAAA".into(), 0);
        codes.issue_code("BBBBBBBB".into(), 0);
        assert_eq!(codes.redeem("AAAAAAAA", 1), Err(PairError::Wrong));
        assert_eq!(codes.redeem("BBBBBBBB", 1), Ok(()));
    }

    #[test]
    fn a_wrong_length_never_matches() {
        assert!(!same("ABC", "ABCD"));
        assert!(same("ABCD", "ABCD"));
        assert!(!same("ABCD", "ABCE"));
    }

    fn phone(id: &str) -> Sender {
        Sender {
            transport: "phone".into(),
            id: id.into(),
            label: "Pixel".into(),
            paired_at: 1,
            last_seen: 1,
            level: "chat".into(),
            conversation_id: None,
        }
    }

    #[test]
    fn senders_are_a_list_in_the_settings() {
        let db = Db::open_in_memory().unwrap();
        assert!(load(&db).is_empty());
        add(&db, phone("k1")).unwrap();
        add(&db, phone("k2")).unwrap();
        assert_eq!(load(&db).len(), 2);
        assert!(find(&db, "phone", "k1").is_some());
        assert!(find(&db, "telegram", "k1").is_none(), "the transport is part of the identity");
        // Paired twice: one row, the newer one.
        let mut again = phone("k1");
        again.label = "New label".into();
        add(&db, again).unwrap();
        assert_eq!(load(&db).len(), 2);
        assert_eq!(find(&db, "phone", "k1").unwrap().label, "New label");
        assert!(remove(&db, "phone", "k1").unwrap());
        assert!(!remove(&db, "phone", "k1").unwrap());
        assert_eq!(load(&db).len(), 1);
    }
}
