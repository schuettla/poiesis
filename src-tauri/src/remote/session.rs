//! REM-3: which chat a sender is in, and the rule that a chat runs one turn at a time.
//!
//! A sender has one active conversation. Its first message makes it. The chat is an ordinary
//! conversation row, so it shows in the desktop sidebar like any other. A message that arrives
//! while a turn runs is steering, not a second turn.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::pairing::{self, Sender};
use crate::db::Db;

/// The chat the sender is in. A new one is made when the sender has none, or the old one was
/// deleted on the desktop.
pub fn conversation_for(db: &Db, sender: &Sender) -> Result<String, String> {
    if let Some(id) = &sender.conversation_id {
        if db.get_conversation(id).map_err(|e| e.to_string())?.is_some() {
            return Ok(id.clone());
        }
    }
    new_conversation(db, sender)
}

/// Start another chat for the sender and make it the active one (`/new`).
pub fn new_conversation(db: &Db, sender: &Sender) -> Result<String, String> {
    let title = match sender.transport.as_str() {
        "telegram" => "Telegram",
        _ => "Phone",
    };
    // No folder and not a workspace chat: file tools have nothing to write to unless the person
    // grants it (plan, I.2).
    let conv = db.create_conversation(title, None, false).map_err(|e| e.to_string())?;
    let id = conv.id.clone();
    pairing::update(db, &sender.transport, &sender.id, |s| s.conversation_id = Some(id.clone()))?;
    Ok(conv.id)
}

/// The title a chat takes from its first message, the same rule as a chat typed in the window
/// (`deriveTitle` in `store.ts`): spaces folded, cut at 48 characters. `None` for a message with no
/// words (a photo alone), which keeps the chat's first name.
pub fn title_from(text: &str) -> Option<String> {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.is_empty() {
        return None;
    }
    Some(if folded.chars().count() > 48 { format!("{}…", folded.chars().take(48).collect::<String>()) } else { folded })
}

/// Chats with a turn in flight. Shared with the turn's closure, so the mark is lifted however the
/// turn ends.
#[derive(Default, Clone)]
pub struct Busy(Arc<Mutex<HashSet<String>>>);

/// Held while a turn runs; dropping it lets the chat take the next one.
pub struct BusyGuard {
    set: Arc<Mutex<HashSet<String>>>,
    conversation: String,
}

impl Busy {
    /// Mark the chat busy. `None` when it already is.
    pub fn try_enter(&self, conversation: &str) -> Option<BusyGuard> {
        let mut set = self.0.lock().unwrap();
        if !set.insert(conversation.to_string()) {
            return None;
        }
        Some(BusyGuard { set: self.0.clone(), conversation: conversation.to_string() })
    }

    pub fn is_busy(&self, conversation: &str) -> bool {
        self.0.lock().unwrap().contains(conversation)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = self.set.lock() {
            set.remove(&self.conversation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paired(db: &Db, id: &str) -> Sender {
        let s = Sender {
            transport: "phone".into(),
            id: id.into(),
            label: "Pixel".into(),
            paired_at: 1,
            last_seen: 1,
            level: "chat".into(),
            conversation_id: None,
        };
        pairing::add(db, s.clone()).unwrap();
        s
    }

    #[test]
    fn the_first_message_makes_a_chat_and_the_next_finds_it() {
        let db = Db::open_in_memory().unwrap();
        let s = paired(&db, "k1");
        let first = conversation_for(&db, &s).unwrap();
        let conv = db.get_conversation(&first).unwrap().unwrap();
        assert_eq!(conv.title, "Phone");
        assert!(!conv.workspace);
        assert!(conv.folder_path.is_none(), "no folder: file tools have nothing to write to");
        // The sender row remembers it; the next message lands in the same chat.
        let again = pairing::find(&db, "phone", "k1").unwrap();
        assert_eq!(conversation_for(&db, &again).unwrap(), first);
    }

    #[test]
    fn two_senders_never_share_a_chat() {
        let db = Db::open_in_memory().unwrap();
        let a = paired(&db, "k1");
        let b = paired(&db, "k2");
        assert_ne!(conversation_for(&db, &a).unwrap(), conversation_for(&db, &b).unwrap());
    }

    #[test]
    fn new_starts_another_chat_and_makes_it_the_active_one() {
        let db = Db::open_in_memory().unwrap();
        let s = paired(&db, "k1");
        let first = conversation_for(&db, &s).unwrap();
        let second = new_conversation(&db, &s).unwrap();
        assert_ne!(first, second);
        let now = pairing::find(&db, "phone", "k1").unwrap();
        assert_eq!(conversation_for(&db, &now).unwrap(), second);
    }

    #[test]
    fn a_chat_deleted_on_the_desktop_is_replaced() {
        let db = Db::open_in_memory().unwrap();
        let s = paired(&db, "k1");
        let first = conversation_for(&db, &s).unwrap();
        db.delete_conversation(&first).unwrap();
        let now = pairing::find(&db, "phone", "k1").unwrap();
        assert_ne!(conversation_for(&db, &now).unwrap(), first);
    }

    #[test]
    fn the_first_message_gives_the_title_as_in_the_window() {
        assert_eq!(title_from("  What is\n your   name? ").as_deref(), Some("What is your name?"));
        let long = "ü".repeat(60);
        assert_eq!(title_from(&long), Some(format!("{}…", "ü".repeat(48))));
        assert_eq!(title_from(" \n "), None, "a photo alone keeps the first name");
    }

    #[test]
    fn a_chat_takes_one_turn_at_a_time() {
        let busy = Busy::default();
        let guard = busy.try_enter("c1").expect("free");
        assert!(busy.try_enter("c1").is_none(), "a second turn in the same chat is refused");
        assert!(busy.try_enter("c2").is_some(), "another chat is not held up");
        assert!(busy.is_busy("c1"));
        drop(guard);
        assert!(!busy.is_busy("c1"));
        assert!(busy.try_enter("c1").is_some(), "free again once the turn ends");
    }
}
