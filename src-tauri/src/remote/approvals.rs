//! REM-4: what happens to a question a remote turn asks.
//!
//! The question is the turn's own `permission` event. The gateway notes who it was sent to, so only
//! that sender can answer it (plan, Part III.4), and starts a ten minute clock: a person who put the
//! phone down must not leave a run waiting for ever. A text "ok" never approves anything. Only a
//! button does, and a button can say yes once or no. "Always" and "for this chat" are for the
//! desktop.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::permissions::{Decision, PermissionManager};

/// How long a question waits for an answer from outside.
pub const EXPIRY: Duration = Duration::from_secs(10 * 60);

/// The line a chat gets when the time ran out.
pub const EXPIRED_WORDS: &str = "No answer in 10 minutes, so I did not do that.";

/// Who a question was sent to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub transport: String,
    pub id: String,
    pub conversation_id: String,
}

/// An open question: who it was sent to, and the turn's line to them, so they can be told when the
/// desktop answers it first.
struct Open {
    owner: Owner,
    events: UnboundedSender<Value>,
}

/// Open questions by request id.
#[derive(Default)]
pub struct Pending(Mutex<HashMap<String, Open>>);

impl Pending {
    pub fn insert(&self, request_id: &str, owner: Owner, events: UnboundedSender<Value>) {
        self.0.lock().unwrap().insert(request_id.to_string(), Open { owner, events });
    }

    /// Take the question if it was sent to this sender. A sender cannot answer another's.
    pub fn claim(&self, request_id: &str, transport: &str, id: &str) -> Option<Owner> {
        let mut map = self.0.lock().unwrap();
        match map.get(request_id) {
            Some(o) if o.owner.transport == transport && o.owner.id == id => map.remove(request_id).map(|o| o.owner),
            _ => None,
        }
    }

    /// Take the question whoever it was sent to (the clock ran out).
    pub fn remove(&self, request_id: &str) -> Option<Owner> {
        self.0.lock().unwrap().remove(request_id).map(|o| o.owner)
    }

    /// Take the question because the desktop answered it: who had it, and the line to tell them.
    pub fn answered_elsewhere(&self, request_id: &str) -> Option<(Owner, UnboundedSender<Value>)> {
        self.0.lock().unwrap().remove(request_id).map(|o| (o.owner, o.events))
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.0.lock().unwrap().is_empty()
    }
}

/// What a button means. Yes is one time only.
pub fn decision_for(allow: bool) -> Decision {
    if allow {
        Decision::Once
    } else {
        Decision::Deny
    }
}

/// Wait out the clock, then say no to a question nobody answered. `Some(owner)` when it did: the
/// caller tells the chat. `None` when somebody answered first, on the phone or at the desktop.
pub async fn expire(pending: &Pending, perms: &PermissionManager, request_id: &str, after: Duration) -> Option<Owner> {
    tokio::time::sleep(after).await;
    let owner = pending.remove(request_id)?;
    if !perms.is_pending(request_id) {
        // Answered at the desktop; the run has gone on.
        return None;
    }
    perms.resolve(request_id, Decision::Deny);
    Some(owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(id: &str) -> Owner {
        Owner { transport: "phone".into(), id: id.into(), conversation_id: "c1".into() }
    }

    fn line() -> UnboundedSender<Value> {
        tokio::sync::mpsc::unbounded_channel().0
    }

    #[test]
    fn only_the_sender_it_was_sent_to_can_answer() {
        let pending = Pending::default();
        pending.insert("perm_1", owner("k1"), line());
        assert!(pending.claim("perm_1", "phone", "k2").is_none(), "another phone");
        assert!(pending.claim("perm_1", "telegram", "k1").is_none(), "another transport");
        assert!(pending.claim("perm_1", "phone", "k1").is_some());
        assert!(pending.claim("perm_1", "phone", "k1").is_none(), "an answer counts once");
    }

    #[test]
    fn a_button_says_yes_once_or_no_never_always() {
        assert_eq!(decision_for(true), Decision::Once);
        assert_eq!(decision_for(false), Decision::Deny);
    }

    #[tokio::test]
    async fn an_unanswered_question_is_denied_when_the_time_is_up() {
        let (pending, perms) = (Pending::default(), PermissionManager::new());
        let rx = perms.open_request("perm_1");
        pending.insert("perm_1", owner("k1"), line());
        let said = expire(&pending, &perms, "perm_1", Duration::from_millis(20)).await;
        assert_eq!(said, Some(owner("k1")));
        assert_eq!(rx.await.unwrap(), Decision::Deny, "the run is told no, not left waiting");
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn an_answer_before_the_time_is_up_stands() {
        let (pending, perms) = (Pending::default(), PermissionManager::new());
        let rx = perms.open_request("perm_1");
        pending.insert("perm_1", owner("k1"), line());
        assert!(pending.claim("perm_1", "phone", "k1").is_some());
        perms.resolve("perm_1", decision_for(true));
        assert_eq!(expire(&pending, &perms, "perm_1", Duration::from_millis(10)).await, None);
        assert_eq!(rx.await.unwrap(), Decision::Once);
    }

    #[tokio::test]
    async fn a_question_answered_at_the_desktop_is_not_reported_as_expired() {
        let (pending, perms) = (Pending::default(), PermissionManager::new());
        let rx = perms.open_request("perm_1");
        pending.insert("perm_1", owner("k1"), line());
        // The window answered; the gateway was not asked.
        perms.resolve("perm_1", Decision::Once);
        assert_eq!(expire(&pending, &perms, "perm_1", Duration::from_millis(10)).await, None);
        assert_eq!(rx.await.unwrap(), Decision::Once);
    }

    #[test]
    fn a_question_the_desktop_answered_can_be_closed_on_the_remote_side_and_not_answered_there() {
        let pending = Pending::default();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        pending.insert("perm_1", owner("k1"), tx);
        let (who, events) = pending.answered_elsewhere("perm_1").expect("it was a remote question");
        assert_eq!(who, owner("k1"));
        events.send(serde_json::json!({ "type": "permission_gone", "id": "perm_1" })).unwrap();
        assert_eq!(rx.try_recv().unwrap()["type"], "permission_gone");
        assert!(pending.claim("perm_1", "phone", "k1").is_none(), "a late tap answers nothing");
        assert!(pending.answered_elsewhere("perm_2").is_none(), "a desktop-only question");
    }
}
