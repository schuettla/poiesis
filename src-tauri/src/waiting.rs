//! `TEAM-19`, `TEAM-21`: what waits for the person while they are away.
//!
//! A run nobody watches (a scheduled task, a team working on one) cannot ask. When it needs a yes,
//! it parks the question as a pending proposal (`waiting-file`, `waiting-ask`) and goes on. This
//! module is what turns that into something the person can see: it tells the window to look again,
//! and, when the window is not in front, shows a desktop notification. It keeps no entries of its
//! own; they are `change_proposals` rows.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter};

use crate::db::{ChangeProposal, Db};
use crate::memory::MemoryStore;

/// The app event the window listens for. Its payload is the session that changed.
pub const AWAY_EVENT: &str = "poiesis-away";
/// The app event that asks the window to open a session (a click on the notification).
pub const OPEN_EVENT: &str = "poiesis-open-session";
/// The setting under Interface: "Tell me when someone is waiting for me". On unless it says `off`.
pub const NOTIFY_KEY: &str = "notify.waiting";

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Called once from `lib.rs` setup, like `agent::background::init`. A run that waits for the person
/// has no command to borrow the app from.
pub fn init(app: AppHandle) {
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    on_click(&app);
    let _ = APP.set(app);
}

/// A session was changed by a run nobody watched: the window refreshes what waits and what is
/// unread. Nothing happens in tests, where there is no app.
pub fn changed(session: &str) {
    if let Some(app) = APP.get() {
        let _ = app.emit(AWAY_EVENT, session);
    }
}

/// Someone started to wait for the person: the window is told, and the person too when they are
/// not looking at it.
pub fn announce(db: &Db, entry: &ChangeProposal) {
    changed(entry.session_id.as_deref().unwrap_or_default());
    // `REM-7`: a paired Telegram account hears of it too, when remote is on.
    if let Some(app) = APP.get() {
        crate::remote::telegram::notify_waiting(app, db, entry);
    }
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    notify(db, entry);
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    let _ = db;
}

/// Is the switch on? It is, unless the person turned it off.
pub fn switched_on(db: &Db) -> bool {
    db.get_setting(NOTIFY_KEY).ok().flatten().as_deref() != Some("off")
}

/// A notification is shown only when the switch is on and the window is not in front: someone
/// who is looking at the app sees the sidebar row.
pub fn should_notify(switch_on: bool, window_focused: bool) -> bool {
    switch_on && !window_focused
}

/// Each entry is told once, however many times it is announced.
fn first_time(entry_id: &str) -> bool {
    static TOLD: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    TOLD.get_or_init(Mutex::default).lock().map(|mut t| t.insert(entry_id.to_string())).unwrap_or(false)
}

/// Who is waiting, in the words of the notification: the persona that plays the member who asked
/// ("Atlas", not its role "api"), a helper by its role, else the persona that leads the session, else
/// Poiesis. Without the memory store (in tests) a member is named by its role.
pub(crate) fn who_waits(db: &Db, mem: Option<&MemoryStore>, entry: &ChangeProposal) -> String {
    let session = entry.session_id.as_deref();
    if let Some(role) = entry.author.as_deref().map(str::trim).filter(|a| !a.is_empty() && *a != crate::agent::team::COORDINATOR) {
        let persona = mem
            .zip(session)
            .and_then(|(mem, s)| crate::agent::team::of_session(db, mem, s))
            .and_then(|team| crate::agent::team::member_named(db, &team, role))
            .and_then(|a| a.persona);
        return persona.map_or_else(|| role.to_string(), |p| p.name);
    }
    leading(db, session)
}

/// `PAR-10`: who a live question in a session is from: the helper that asks, else the persona
/// that leads the session, else Poiesis.
pub(crate) fn who_asks(db: &Db, session: &str, agent: Option<&str>) -> String {
    agent.map(str::trim).filter(|a| !a.is_empty()).map(str::to_string).unwrap_or_else(|| leading(db, Some(session)))
}

/// The persona that leads a session, else Poiesis.
fn leading(db: &Db, session: Option<&str>) -> String {
    session
        .and_then(|s| db.get_conversation(s).ok().flatten())
        .and_then(|c| c.persona_id)
        .and_then(|id| db.get_persona(&id).ok().flatten())
        .map(|p| p.name)
        .unwrap_or_else(|| "Poiesis".to_string())
}

/// The title and the body: "Mira is waiting for you" and what it wants, in a few words.
pub fn words(who: &str, line: &str) -> (String, String) {
    let body: String = line.chars().take(140).collect();
    (format!("{who} is waiting for you"), body)
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
mod desktop {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::{Mutex, OnceLock};

    /// The session each shown notification opens, by notification id.
    pub fn shown() -> &'static Mutex<HashMap<i32, String>> {
        static SHOWN: OnceLock<Mutex<HashMap<i32, String>>> = OnceLock::new();
        SHOWN.get_or_init(Mutex::default)
    }

    pub fn next_id() -> i32 {
        static NEXT: AtomicI32 = AtomicI32::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn notify(db: &Db, entry: &ChangeProposal) {
    use tauri::Manager;

    let Some(app) = APP.get() else { return };
    let mem = app.state::<MemoryStore>();
    let who = who_waits(db, Some(&mem), entry);
    show(db, &entry.id, entry.session_id.as_deref(), &who, &entry.rationale);
}

/// `PAR-10`: a question or a permission of a chat that works while you are somewhere else. The
/// window says when the chat is not on screen; the same switch, focus rule, once-only rule and
/// click as a parked question apply.
pub fn tell(db: &Db, session: &str, id: &str, agent: Option<&str>, line: &str) {
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    show(db, id, Some(session), &who_asks(db, session, agent), line);
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    let _ = (db, session, id, agent, line);
}

/// One desktop notification, once for each `key`, that opens `session` on a click.
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn show(db: &Db, key: &str, session: Option<&str>, who: &str, line: &str) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;

    let Some(app) = APP.get() else { return };
    let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if !should_notify(switched_on(db), focused) || !first_time(key) {
        return;
    }
    let (title, body) = words(who, line);
    let id = desktop::next_id();
    if let Some(session) = session {
        if let Ok(mut shown) = desktop::shown().lock() {
            shown.insert(id, session.to_string());
        }
    }
    let _ = app.notification().builder().id(id).title(title).body(body).show();
}

/// A click on a notification brings the window forward and opens the session it was about. Where
/// the platform does not say which notification was clicked, the window still comes forward.
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn on_click(app: &AppHandle) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;

    let handle = app.clone();
    let _ = app.notification().on_action(move |performed| {
        if performed.action_id() != "tap" {
            return;
        }
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        let session = performed
            .notification()
            .and_then(|n| desktop::shown().lock().ok().and_then(|m| m.get(&n.id()).cloned()));
        if let Some(session) = session {
            let _ = handle.emit(OPEN_EVENT, session);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notification_is_shown_only_when_the_switch_is_on_and_the_window_is_not_in_front() {
        assert!(should_notify(true, false));
        assert!(!should_notify(true, true), "the person is looking at the app");
        assert!(!should_notify(false, false), "the person turned it off");
        assert!(!should_notify(false, true));
    }

    #[test]
    fn the_switch_is_on_until_the_person_turns_it_off() {
        let db = Db::open_in_memory().unwrap();
        assert!(switched_on(&db));
        db.set_setting(NOTIFY_KEY, "off").unwrap();
        assert!(!switched_on(&db));
        db.set_setting(NOTIFY_KEY, "on").unwrap();
        assert!(switched_on(&db));
    }

    #[test]
    fn an_entry_is_told_once() {
        assert!(first_time("entry-once"));
        assert!(!first_time("entry-once"));
        assert!(first_time("entry-other"));
    }

    #[test]
    fn a_live_question_is_from_the_helper_else_the_leading_persona_else_poiesis() {
        let db = Db::open_in_memory().unwrap();
        let session = db.create_conversation("Plan the trip", None, false).unwrap().id;
        assert_eq!(who_asks(&db, &session, None), "Poiesis");
        assert_eq!(who_asks(&db, &session, Some("Claude Code")), "Claude Code");
        assert_eq!(who_asks(&db, &session, Some("  ")), "Poiesis");
    }

    #[test]
    fn the_words_name_who_waits_and_stay_short() {
        let (title, body) = words("Mira", "Send the summary to the team list?");
        assert_eq!(title, "Mira is waiting for you");
        assert_eq!(body, "Send the summary to the team list?");
        assert_eq!(words("Mira", &"x".repeat(500)).1.chars().count(), 140);
    }

    #[test]
    fn who_waits_is_the_member_else_the_leading_persona_else_poiesis() {
        let db = Db::open_in_memory().unwrap();
        let persona = db
            .create_persona(&crate::db::NewPersona {
                name: "Mira".into(),
                system_prompt: "You are Mira.".into(),
                model_id: None,
                params_json: None,
                tools_json: None,
                skills_json: None,
                description: None,
                spawnable: false,
            })
            .unwrap();
        let session = db.create_conversation("Morning run", None, false).unwrap().id;
        let entry = db.add_waiting("waiting-ask", &session, None, "{}", "Mira asks: which list?").unwrap();
        assert_eq!(who_waits(&db, None, &entry), "Poiesis");
        db.set_conversation_persona(&session, Some(&persona.id), None).unwrap();
        assert_eq!(who_waits(&db, None, &entry), "Mira");
        let member = db.add_waiting("waiting-ask", &session, Some("api"), "{}", "api asks: which table?").unwrap();
        assert_eq!(who_waits(&db, None, &member), "api", "with no memory store, a member is its role");
    }

    #[test]
    fn a_member_who_waits_is_named_by_the_persona_that_plays_it() {
        use crate::memory::ways::{Member, Team};
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("poiesis_waiting_mem_{}", uuid::Uuid::new_v4().simple()));
        let mem = MemoryStore::new(&dir).unwrap();
        mem.ensure_ways();
        let atlas = db
            .create_persona(&crate::db::NewPersona {
                name: "Atlas".into(),
                system_prompt: "You are Atlas.".into(),
                model_id: None,
                params_json: None,
                tools_json: None,
                skills_json: None,
                description: None,
                spawnable: false,
            })
            .unwrap();
        let mut team = Team::blank();
        team.name = "Shop team".into();
        team.members = vec![
            Member { role: "api".into(), persona: format!("persona:{}", atlas.id), job: String::new(), owns: vec![] },
            Member { role: "docs".into(), persona: "helper".into(), job: "Writes docs".into(), owns: vec![] },
        ];
        let team = mem.save_team(team, "you", "").unwrap();
        let session = db.create_conversation("Morning run", None, false).unwrap().id;
        crate::agent::leader::set_leader(&db, &mem, &session, Some(&crate::agent::leader::Leader::Team(team.slug.clone()))).unwrap();
        let api = db.add_waiting("waiting-file", &session, Some("api"), "{}", "Write a.txt").unwrap();
        assert_eq!(who_waits(&db, Some(&mem), &api), "Atlas");
        let docs = db.add_waiting("waiting-file", &session, Some("docs"), "{}", "Write b.txt").unwrap();
        assert_eq!(who_waits(&db, Some(&mem), &docs), "docs", "a helper has no name but its role");
        let _ = std::fs::remove_dir_all(dir);
    }
}
