//! `TEAM-1`: who leads a chat.
//!
//! A chat is led by Poiesis, by one of the personas, or by a team. The
//! choice lives on the conversation (`conversations.leader`); where nobody chose,
//! the chat starts where its place says (`defaults.md`, `MemoryStore::default_leader_for`).
//!
//! The turn code never learns about leaders. It reads `conversations.persona_id`
//! for whose instructions, model and tools a turn uses, so everything here ends in
//! that column being right: a persona leader writes it, Poiesis clears it.

use crate::db::{Conversation, Db};
use crate::memory::ways::{valid_leader, ScopeCtx};
use crate::memory::MemoryStore;

/// Who leads a chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leader {
    Poiesis,
    /// A persona's id.
    Persona(String),
    /// A team's slug (`memory/ways/teams/<slug>.md`). Its coordinator answers.
    Team(String),
}

impl Leader {
    pub fn parse(s: &str) -> Option<Leader> {
        let s = s.trim();
        if !valid_leader(s) {
            return None;
        }
        Some(match s.split_once(':') {
            None => Leader::Poiesis,
            Some(("persona", id)) => Leader::Persona(id.to_string()),
            Some((_, slug)) => Leader::Team(slug.to_string()),
        })
    }

    /// The text the column and `defaults.md` hold.
    pub fn text(&self) -> String {
        match self {
            Leader::Poiesis => "poiesis".into(),
            Leader::Persona(id) => format!("persona:{id}"),
            Leader::Team(slug) => format!("team:{slug}"),
        }
    }
}

/// Who leads, and how that came to be.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Led {
    /// `poiesis`, `persona:<id>` or `team:<slug>`.
    pub leader: String,
    /// `chat`: somebody chose it for this chat. `default`: where chats start says so.
    /// `none`: Poiesis, because nothing says otherwise.
    pub source: &'static str,
}

/// Does this persona still exist? A leader that is gone is no leader.
fn persona_exists(db: &Db, id: &str) -> bool {
    db.get_persona(id).ok().flatten().is_some()
}

/// Where this chat is, for reading what starts here: its project and its kind of
/// work so far. A persona's own place is not asked: it has no leader to give.
fn default_leader(db: &Db, mem: &MemoryStore, conv_id: &str) -> Option<Leader> {
    let project = db.conversation_project(conv_id).ok().flatten().map(|p| p.id);
    let kind = crate::agent::selfmodel::kind_now(db, conv_id);
    let ctx = ScopeCtx { project: project.as_deref(), persona: None, kind };
    Leader::parse(&mem.default_leader_for(&ctx)?)
}

/// Who leads this chat right now: the chat's own choice, else the persona it was
/// given before leaders existed (`persona_id` alone), else where chats start,
/// else Poiesis.
pub fn effective(db: &Db, mem: &MemoryStore, conv: &Conversation) -> Led {
    let chat = |leader: &Leader| Led { leader: leader.text(), source: "chat" };
    match conv.leader.as_deref().and_then(Leader::parse) {
        Some(Leader::Persona(id)) if !persona_exists(db, &id) => {}
        Some(Leader::Team(slug)) if mem.team(&slug).is_none() => {}
        Some(l) => return chat(&l),
        None => {}
    }
    if let Some(id) = conv.persona_id.as_deref().filter(|id| persona_exists(db, id)) {
        return chat(&Leader::Persona(id.to_string()));
    }
    match default_leader(db, mem, &conv.id) {
        Some(Leader::Persona(id)) if !persona_exists(db, &id) => {}
        Some(Leader::Team(slug)) if mem.team(&slug).is_none() => {}
        Some(l) => return Led { leader: l.text(), source: "default" },
        None => {}
    }
    Led { leader: Leader::Poiesis.text(), source: "none" }
}

/// Hand a chat to a leader, or (`None`) give the choice back so where chats start
/// decides again. Writes `persona_id` with it: a team's is its coordinator (none
/// when Poiesis coordinates), so the turn code reads the right instructions.
pub fn set_leader(db: &Db, mem: &MemoryStore, conv_id: &str, leader: Option<&Leader>) -> Result<(), String> {
    let persona_id: Option<String> = match leader {
        None | Some(Leader::Poiesis) => None,
        Some(Leader::Persona(id)) => {
            if !persona_exists(db, id) {
                return Err("I don't know that persona.".into());
            }
            Some(id.clone())
        }
        Some(Leader::Team(slug)) => {
            let team = mem.team(slug).ok_or_else(|| format!("I don't have a team called \u{201c}{slug}\u{201d}."))?;
            match team.coordinator_persona() {
                None => None,
                Some(id) if persona_exists(db, id) => Some(id.to_string()),
                Some(_) => return Err(format!("{} has no coordinator any more.", team.name)),
            }
        }
    };
    let text = leader.map(Leader::text);
    // `TEAM-12`: a hand-over to a team is a new start, so its cost card comes again.
    crate::agent::team::forget_cost_answer(db, conv_id);
    db.set_conversation_leader(conv_id, text.as_deref(), persona_id.as_deref()).map_err(|e| e.to_string())
}

/// Before a person's turn: a chat nobody led yet takes the leader its place
/// says, so the turn code finds the persona where it always looks. A chat that
/// was given a leader or a persona, and a helper's chat (it never has a leader of
/// its own), are left alone. True when the place gave the chat its leader just now.
pub fn start_of_turn(db: &Db, mem: &MemoryStore, conv_id: &str) -> bool {
    give_back_left(db, conv_id);
    let Some(conv) = db.get_conversation(conv_id).ok().flatten() else { return false };
    // A team's coordinator can be changed in its editor after the team took the chat: the turn
    // code reads the one the team names now.
    if let Some(Leader::Team(slug)) = conv.leader.as_deref().and_then(Leader::parse) {
        if let Some(team) = mem.team(&slug) {
            let now = team.coordinator_persona().filter(|id| persona_exists(db, id));
            if now != conv.persona_id.as_deref() {
                let _ = db.set_conversation_leader(conv_id, conv.leader.as_deref(), now);
            }
        }
        return false;
    }
    if conv.leader.is_some() || conv.persona_id.is_some() || conv.parent_conversation_id.is_some() {
        return false;
    }
    match default_leader(db, mem, conv_id) {
        // A persona or a team that is gone is no default: the chat stays Poiesis's.
        Some(l @ (Leader::Persona(_) | Leader::Team(_))) => set_leader(db, mem, conv_id, Some(&l)).is_ok(),
        _ => false,
    }
}

/// What a one-message lead put aside, to give back when the turn is over. It is written
/// down while the turn runs, so a turn the app never finished is given back by the next.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Lent {
    conv_id: String,
    leader: Option<String>,
    persona_id: Option<String>,
    /// Who was lent the turn (`None`: Poiesis).
    lent: Option<String>,
    /// `TEAM-17`: the leader the chat has while a member of its team answers alone: that
    /// member as a persona, so the turn is a plain answer and not a team's task. `None`
    /// for `/lead`, which leaves the chat's own leader where it is.
    #[serde(default)]
    as_leader: Option<String>,
}

fn lent_key(conv_id: &str) -> String {
    format!("leader.lent.{conv_id}")
}

/// A one-message lead the app closed during: give the chat back before anything reads it.
pub fn give_back_left(db: &Db, conv_id: &str) {
    let left = db.get_setting(&lent_key(conv_id)).ok().flatten();
    match left.and_then(|raw| serde_json::from_str::<Lent>(&raw).ok()) {
        Some(lent) => give_back(db, lent),
        None => {
            let _ = db.delete_setting(&lent_key(conv_id));
        }
    }
}

/// `/lead mira <message>`: let `who` (`poiesis` or `persona:<id>`) lead one turn.
/// Only the persona the turn code reads changes, so the chat's own leader is still
/// there when the turn is over (`give_back`). `None` when nothing needs lending:
/// no leader asked for, one that cannot lead (a team, a persona that is gone), or
/// the one that leads already.
pub fn lend(db: &Db, conv_id: &str, who: Option<&str>) -> Option<Lent> {
    let persona = match Leader::parse(who?)? {
        Leader::Poiesis => None,
        Leader::Persona(id) if persona_exists(db, &id) => Some(id),
        _ => return None,
    };
    let conv = db.get_conversation(conv_id).ok().flatten()?;
    if conv.persona_id == persona {
        return None;
    }
    let lent = Lent { conv_id: conv_id.to_string(), leader: conv.leader, persona_id: conv.persona_id, lent: persona, as_leader: None };
    // Written down first: if the app closes during the turn, the next turn gives it back.
    db.set_setting(&lent_key(conv_id), &serde_json::to_string(&lent).ok()?).ok()?;
    db.set_conversation_leader(conv_id, lent.leader.as_deref(), lent.lent.as_deref()).ok()?;
    Some(lent)
}

/// `TEAM-17`: a member of the team that leads this chat answers one message the person spoke
/// to it. The chat's leader is the member's persona for that turn (so it is a plain answer
/// with its persona's instructions and memory, not a team's task: no cost card, no roster),
/// and `give_back` puts the team back. `None` when the member has no persona that exists.
pub fn lend_member(db: &Db, conv_id: &str, persona_id: &str) -> Option<Lent> {
    if !persona_exists(db, persona_id) {
        return None;
    }
    let conv = db.get_conversation(conv_id).ok().flatten()?;
    let as_leader = Leader::Persona(persona_id.to_string()).text();
    let lent = Lent {
        conv_id: conv_id.to_string(),
        leader: conv.leader,
        persona_id: conv.persona_id,
        lent: Some(persona_id.to_string()),
        as_leader: Some(as_leader.clone()),
    };
    db.set_setting(&lent_key(conv_id), &serde_json::to_string(&lent).ok()?).ok()?;
    db.set_conversation_leader(conv_id, Some(&as_leader), Some(persona_id)).ok()?;
    Some(lent)
}

/// The turn is over: the chat is as it was. Only what the lend wrote is undone, so a
/// handover the person made while the turn ran stays.
pub fn give_back(db: &Db, lent: Lent) {
    if let Some(conv) = db.get_conversation(&lent.conv_id).ok().flatten() {
        let during = lent.as_leader.clone().or_else(|| lent.leader.clone());
        if conv.leader == during && conv.persona_id == lent.lent {
            let _ = db.set_conversation_leader(&lent.conv_id, lent.leader.as_deref(), lent.persona_id.as_deref());
        }
    }
    let _ = db.delete_setting(&lent_key(&lent.conv_id));
}

/// `TEAM-3`: who answers the turn that is starting, after `start_of_turn` and `lend`.
/// A lent turn is answered by whoever the turn code now reads, and says `message`.
pub fn speaker(db: &Db, mem: &MemoryStore, conv_id: &str, lent: bool) -> Led {
    let none = Led { leader: Leader::Poiesis.text(), source: "none" };
    let Some(conv) = db.get_conversation(conv_id).ok().flatten() else { return none };
    if lent {
        let leader = conv.persona_id.map_or_else(|| Leader::Poiesis.text(), |id| Leader::Persona(id).text());
        return Led { leader, source: "message" };
    }
    effective(db, mem, &conv)
}

/// The persona the turn code reads for this chat (`persona_id`, so a lent turn and a
/// member's chat count), when it still exists.
fn led_by(db: &Db, conv_id: &str) -> Option<crate::db::Persona> {
    let id = db.get_conversation(conv_id).ok().flatten()?.persona_id?;
    db.get_persona(&id).ok().flatten()
}

/// `TEAM-5`: whose memory this chat reads, besides what is shared. `None` is Poiesis, or a
/// persona told not to remember: it sees only the shared entries.
pub fn memory_persona(db: &Db, conv_id: &str) -> Option<String> {
    led_by(db, conv_id).filter(|p| p.remembers).map(|p| p.id)
}

/// Where something learned in a chat is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keep {
    /// For everyone.
    Shared,
    /// For this persona only.
    Persona(String),
    /// Not kept at all: a persona that does not remember learns nothing for itself.
    Nowhere,
}

/// `TEAM-5`: where to keep what was learned in this chat. Poiesis writes for everyone, as
/// before. A persona writes for itself, except what is about the person (`about_person`:
/// a fact or preference that is true everywhere), which stays shared. A persona that does
/// not remember keeps nothing of its own.
pub fn keep(db: &Db, conv_id: &str, about_person: bool) -> Keep {
    match led_by(db, conv_id) {
        None => Keep::Shared,
        Some(_) if about_person => Keep::Shared,
        Some(p) if p.remembers => Keep::Persona(p.id),
        Some(_) => Keep::Nowhere,
    }
}

const MIGRATED_KEY: &str = "leader.default_persona_migrated";

/// Run once at startup. The persona flagged "default" becomes who leads new chats
/// everywhere, which is what the flag always said and nothing did. A leader already
/// set for everywhere is never overwritten, and the flag is not read again.
pub fn migrate_default_persona(db: &Db, mem: &MemoryStore) {
    if db.get_setting(MIGRATED_KEY).ok().flatten().is_some() {
        return;
    }
    let flagged = db.list_personas().unwrap_or_default().into_iter().find(|p| p.is_default);
    let already = mem.default_leader_for(&ScopeCtx::default()).is_some();
    if let (Some(p), false) = (flagged, already) {
        if mem.set_default_leader("global", Some(&format!("persona:{}", p.id))).is_err() {
            return; // try again next start
        }
    }
    let _ = db.set_setting(MIGRATED_KEY, "1");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewPersona;

    fn memory() -> (MemoryStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("poiesis_leader_{}", uuid::Uuid::new_v4().simple()));
        let mem = MemoryStore::new(&dir).unwrap();
        mem.ensure_ways();
        (mem, dir)
    }

    fn persona(db: &Db, name: &str) -> String {
        db.create_persona(&NewPersona {
            name: name.into(),
            system_prompt: format!("You are {name}."),
            model_id: None,
            params_json: None,
            tools_json: None,
            skills_json: None,
            description: None,
            spawnable: false,
        })
        .unwrap()
        .id
    }

    fn led(db: &Db, mem: &MemoryStore, conv: &str) -> (String, &'static str) {
        let l = effective(db, mem, &db.get_conversation(conv).unwrap().unwrap());
        (l.leader, l.source)
    }

    #[test]
    fn leaders_read_and_write_the_same_text() {
        for text in ["poiesis", "persona:abc", "team:shop-team"] {
            assert_eq!(Leader::parse(text).unwrap().text(), text);
        }
        assert_eq!(Leader::parse("mira"), None);
        assert_eq!(Leader::parse("persona:"), None);
        assert_eq!(Leader::parse("persona:a b"), None);
    }

    #[test]
    fn a_chat_nobody_led_is_poiesis_until_a_place_says_otherwise_and_its_own_choice_wins() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let kai = persona(&db, "Kai");
        let c = db.create_conversation("t", None, false).unwrap();
        assert_eq!(led(&db, &mem, &c.id), ("poiesis".into(), "none"));

        mem.set_default_leader("global", Some(&format!("persona:{mira}"))).unwrap();
        assert_eq!(led(&db, &mem, &c.id), (format!("persona:{mira}"), "default"));

        set_leader(&db, &mem, &c.id, Some(&Leader::Persona(kai.clone()))).unwrap();
        assert_eq!(led(&db, &mem, &c.id), (format!("persona:{kai}"), "chat"), "the chat's choice beats the default");

        // Poiesis chosen on purpose is kept: no default overrides it.
        set_leader(&db, &mem, &c.id, Some(&Leader::Poiesis)).unwrap();
        assert_eq!(led(&db, &mem, &c.id), ("poiesis".into(), "chat"));
        assert_eq!(db.get_conversation(&c.id).unwrap().unwrap().persona_id, None);

        // Giving the choice back lets the place decide again.
        set_leader(&db, &mem, &c.id, None).unwrap();
        assert_eq!(led(&db, &mem, &c.id), (format!("persona:{mira}"), "default"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_persona_leader_writes_the_persona_the_turn_code_reads_and_keeps_the_overrides() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let c = db.create_conversation("t", None, false).unwrap();
        db.set_conversation_persona(&c.id, None, Some(r#"{"temperature":0.2}"#)).unwrap();
        set_leader(&db, &mem, &c.id, Some(&Leader::Persona(mira.clone()))).unwrap();
        let after = db.get_conversation(&c.id).unwrap().unwrap();
        assert_eq!(after.persona_id.as_deref(), Some(mira.as_str()));
        assert_eq!(after.leader, Some(format!("persona:{mira}")));
        assert_eq!(after.overrides_json.as_deref(), Some(r#"{"temperature":0.2}"#));

        assert!(set_leader(&db, &mem, &c.id, Some(&Leader::Persona("nobody".into()))).is_err());
        assert!(set_leader(&db, &mem, &c.id, Some(&Leader::Team("shop".into()))).is_err(), "no such team");
        assert_eq!(db.get_conversation(&c.id).unwrap().unwrap().persona_id.as_deref(), Some(mira.as_str()), "a refusal changes nothing");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `TEAM-8`: a team leads through its coordinator; a team that is gone is no leader.
    #[test]
    fn a_team_leads_through_its_coordinator_and_a_gone_team_is_no_leader() {
        use crate::memory::ways::{shape_lines, Folder, Limits, Report, Shape, Team};
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let team = |name: &str, coordinator: String| Team {
            slug: String::new(),
            name: name.into(),
            line: String::new(),
            coordinator,
            style: String::new(),
            member_style: String::new(),
            shape: Shape::Hub,
            report: Report::EachItem,
            limits: Limits::default(),
            folder: Folder::Copies,
            locked: false,
            version: 0,
            members: Vec::new(),
            lines: shape_lines(Shape::Hub, &[]),
        };
        let shop = mem.save_team(team("Shop", format!("persona:{mira}")), "you", "").unwrap();
        let desk = mem.save_team(team("Desk", "poiesis".into()), "you", "").unwrap();
        let c = db.create_conversation("t", None, false).unwrap();

        set_leader(&db, &mem, &c.id, Some(&Leader::Team(shop.slug.clone()))).unwrap();
        let after = db.get_conversation(&c.id).unwrap().unwrap();
        assert_eq!((after.leader.as_deref(), after.persona_id.as_deref()), (Some("team:shop"), Some(mira.as_str())));
        assert_eq!(led(&db, &mem, &c.id), ("team:shop".into(), "chat"));

        set_leader(&db, &mem, &c.id, Some(&Leader::Team(desk.slug.clone()))).unwrap();
        let after = db.get_conversation(&c.id).unwrap().unwrap();
        assert_eq!((after.leader.as_deref(), after.persona_id), (Some("team:desk"), None), "Poiesis coordinates");

        // The coordinator changed in the editor after the team took the chat: the next turn reads
        // the new one, and the cost card's answer is not forgotten for it.
        let mut changed = desk.clone();
        changed.coordinator = format!("persona:{mira}");
        mem.save_team(changed, "you", "").unwrap();
        db.set_setting(&format!("team.cost.{}", c.id), "desk").unwrap();
        assert!(!start_of_turn(&db, &mem, &c.id));
        let after = db.get_conversation(&c.id).unwrap().unwrap();
        assert_eq!((after.leader.as_deref(), after.persona_id.as_deref()), (Some("team:desk"), Some(mira.as_str())));
        assert!(crate::agent::team::cost_asked(&db, &c.id, "desk"));

        // A place that starts chats with the team gives its coordinator to the turn.
        mem.set_default_leader("global", Some("team:shop")).unwrap();
        let fresh = db.create_conversation("fresh", None, false).unwrap();
        assert_eq!(led(&db, &mem, &fresh.id), ("team:shop".into(), "default"));
        assert!(start_of_turn(&db, &mem, &fresh.id));
        assert_eq!(db.get_conversation(&fresh.id).unwrap().unwrap().persona_id.as_deref(), Some(mira.as_str()));

        // The team goes: the chat is still answered by the persona the turn code reads, and a
        // default that names the team is no default.
        mem.delete_team("shop").unwrap();
        assert_eq!(led(&db, &mem, &fresh.id), (format!("persona:{mira}"), "chat"));
        let later = db.create_conversation("later", None, false).unwrap();
        assert_eq!(led(&db, &mem, &later.id), ("poiesis".into(), "none"));
        assert!(!start_of_turn(&db, &mem, &later.id));
        assert!(set_leader(&db, &mem, &later.id, Some(&Leader::Team("shop".into()))).is_err());

        // A coordinator persona that is gone cannot lead.
        let kai = persona(&db, "Kai");
        let crew = mem.save_team(team("Crew", format!("persona:{kai}")), "you", "").unwrap();
        db.delete_persona(&kai).unwrap();
        assert!(set_leader(&db, &mem, &later.id, Some(&Leader::Team(crew.slug))).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_one_message_lead_changes_who_the_turn_reads_and_gives_the_chat_back_as_it_was() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let kai = persona(&db, "Kai");
        let c = db.create_conversation("t", None, false).unwrap();
        set_leader(&db, &mem, &c.id, Some(&Leader::Persona(kai.clone()))).unwrap();
        let read = |db: &Db| {
            let c = db.get_conversation(&c.id).unwrap().unwrap();
            (c.leader, c.persona_id)
        };
        let before = read(&db);

        let lent = lend(&db, &c.id, Some(&format!("persona:{mira}"))).expect("Mira can lead one message");
        assert_eq!(read(&db).1.as_deref(), Some(mira.as_str()), "the turn code finds Mira");
        assert_eq!(read(&db).0, before.0, "and the chat is still Kai's");
        give_back(&db, lent);
        assert_eq!(read(&db), before);

        // Poiesis for one message: no persona for the turn.
        let lent = lend(&db, &c.id, Some("poiesis")).expect("Poiesis can lead one message");
        assert_eq!(read(&db).1, None);
        give_back(&db, lent);
        assert_eq!(read(&db), before);

        // Nothing to lend: the same leader, nobody, a team, a persona that is gone.
        assert!(lend(&db, &c.id, Some(&format!("persona:{kai}"))).is_none());
        assert!(lend(&db, &c.id, None).is_none());
        assert!(lend(&db, &c.id, Some("team:shop")).is_none());
        assert!(lend(&db, &c.id, Some("persona:ghost")).is_none());
        assert!(lend(&db, &c.id, Some("nonsense")).is_none());
        assert_eq!(read(&db), before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_one_message_lead_the_app_never_finished_is_given_back_and_a_handover_meanwhile_stays() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let kai = persona(&db, "Kai");
        let atlas = persona(&db, "Atlas");
        let c = db.create_conversation("t", None, false).unwrap();
        set_leader(&db, &mem, &c.id, Some(&Leader::Persona(kai.clone()))).unwrap();
        let read = |db: &Db| {
            let c = db.get_conversation(&c.id).unwrap().unwrap();
            (c.leader, c.persona_id)
        };
        let before = read(&db);

        // The app closed during Mira's message: `give_back` never ran.
        let _never_given_back = lend(&db, &c.id, Some(&format!("persona:{mira}"))).unwrap();
        assert_eq!(read(&db).1.as_deref(), Some(mira.as_str()));
        assert!(!start_of_turn(&db, &mem, &c.id));
        assert_eq!(read(&db), before, "the next turn gives the chat back first");

        // Handed to Atlas while Mira's message ran: Atlas keeps it.
        let lent = lend(&db, &c.id, Some(&format!("persona:{mira}"))).unwrap();
        set_leader(&db, &mem, &c.id, Some(&Leader::Persona(atlas.clone()))).unwrap();
        give_back(&db, lent);
        assert_eq!(read(&db), (Some(format!("persona:{atlas}")), Some(atlas.clone())));
        start_of_turn(&db, &mem, &c.id);
        assert_eq!(read(&db).1.as_deref(), Some(atlas.as_str()), "nothing is left to give back");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_first_turn_says_when_the_place_gave_the_chat_its_leader() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let c = db.create_conversation("t", None, false).unwrap();
        assert!(!start_of_turn(&db, &mem, &c.id), "no place says anyone");
        mem.set_default_leader("global", Some(&format!("persona:{mira}"))).unwrap();
        assert!(start_of_turn(&db, &mem, &c.id));
        assert!(!start_of_turn(&db, &mem, &c.id), "once: it is the chat's own leader now");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_chat_with_only_a_persona_is_led_by_it_and_a_deleted_persona_hands_its_chats_back() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let c = db.create_conversation("t", None, false).unwrap();
        // Chosen before leaders existed: `persona_id` and no `leader`.
        db.set_conversation_persona(&c.id, Some(&mira), None).unwrap();
        assert_eq!(led(&db, &mem, &c.id), (format!("persona:{mira}"), "chat"));

        set_leader(&db, &mem, &c.id, Some(&Leader::Persona(mira.clone()))).unwrap();
        db.delete_persona(&mira).unwrap();
        let after = db.get_conversation(&c.id).unwrap().unwrap();
        assert_eq!((after.leader, after.persona_id), (None, None));
        assert_eq!(led(&db, &mem, &c.id), ("poiesis".into(), "none"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_persona_keeps_its_glyph_and_the_line_under_its_name() {
        let db = Db::open_in_memory().unwrap();
        let id = persona(&db, "Mira");
        let mut p = db.get_persona(&id).unwrap().unwrap();
        assert_eq!((p.look.as_deref(), p.line.as_deref()), (None, None), "made from the name until chosen");
        p.look = Some(r#"{"letter":"M","hue":3}"#.into());
        p.line = Some("Writes and edits, warm but direct".into());
        db.update_persona(&p).unwrap();
        let back = db.list_personas().unwrap().remove(0);
        assert_eq!(back.look.as_deref(), Some(r#"{"letter":"M","hue":3}"#));
        assert_eq!(back.line.as_deref(), Some("Writes and edits, warm but direct"));
    }

    /// `TEAM-5`: Poiesis writes for everyone; a persona writes for itself, except what is
    /// about the person; one that does not remember keeps nothing of its own.
    #[test]
    fn what_a_chat_learns_is_kept_for_who_leads_it() {
        let db = Db::open_in_memory().unwrap();
        let mira = persona(&db, "Mira");
        let c = db.create_conversation("t", None, false).unwrap();
        assert_eq!(keep(&db, &c.id, false), Keep::Shared, "Poiesis writes for everyone");
        assert_eq!(memory_persona(&db, &c.id), None);

        db.set_conversation_persona(&c.id, Some(&mira), None).unwrap();
        assert_eq!(keep(&db, &c.id, false), Keep::Persona(mira.clone()));
        assert_eq!(keep(&db, &c.id, true), Keep::Shared, "a fact about the person stays shared");
        assert_eq!(memory_persona(&db, &c.id), Some(mira.clone()));

        let mut p = db.get_persona(&mira).unwrap().unwrap();
        assert!(p.remembers, "on until switched off");
        p.remembers = false;
        db.update_persona(&p).unwrap();
        assert_eq!(keep(&db, &c.id, false), Keep::Nowhere);
        assert_eq!(keep(&db, &c.id, true), Keep::Shared);
        assert_eq!(memory_persona(&db, &c.id), None, "it sees only what is shared");
    }

    #[test]
    fn the_persona_flagged_default_leads_new_chats_once_and_never_overwrites_a_choice() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let kai = persona(&db, "Kai");
        db.set_default_persona(&mira).unwrap();
        migrate_default_persona(&db, &mem);
        assert_eq!(mem.default_leader_for(&ScopeCtx::default()), Some(format!("persona:{mira}")));

        // Said once: a later change of mind is not undone by the next start.
        mem.set_default_leader("global", Some("poiesis")).unwrap();
        db.set_default_persona(&kai).unwrap();
        migrate_default_persona(&db, &mem);
        assert_eq!(mem.default_leader_for(&ScopeCtx::default()), Some("poiesis".into()));
        let _ = std::fs::remove_dir_all(dir);

        // A leader somebody already set is kept.
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        db.set_default_persona(&mira).unwrap();
        mem.set_default_leader("global", Some("poiesis")).unwrap();
        migrate_default_persona(&db, &mem);
        assert_eq!(mem.default_leader_for(&ScopeCtx::default()), Some("poiesis".into()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_reply_says_who_gave_it_and_where_that_came_from_and_a_lent_turn_says_message() {
        use crate::db::NewMessage;
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let kai = persona(&db, "Kai");
        let c = db.create_conversation("t", None, false).unwrap();
        let said = |s: Led| (s.leader, s.source);

        assert_eq!(said(speaker(&db, &mem, &c.id, false)), ("poiesis".into(), "none"));
        mem.set_default_leader("global", Some(&format!("persona:{mira}"))).unwrap();
        assert_eq!(said(speaker(&db, &mem, &c.id, false)), (format!("persona:{mira}"), "default"));
        set_leader(&db, &mem, &c.id, Some(&Leader::Persona(kai.clone()))).unwrap();
        assert_eq!(said(speaker(&db, &mem, &c.id, false)), (format!("persona:{kai}"), "chat"));

        let lent = lend(&db, &c.id, Some(&format!("persona:{mira}"))).unwrap();
        assert_eq!(said(speaker(&db, &mem, &c.id, true)), (format!("persona:{mira}"), "message"));
        give_back(&db, lent);
        let lent = lend(&db, &c.id, Some("poiesis")).unwrap();
        assert_eq!(said(speaker(&db, &mem, &c.id, true)), ("poiesis".into(), "message"));
        give_back(&db, lent);

        // The author is kept on the reply.
        let msg = |role: &str| NewMessage { role: role.into(), content: "hi".into(), model_name: None, model_provenance: None, steps_json: None, attachments: Vec::new() };
        db.append_message(&c.id, &msg("user")).unwrap();
        let reply = db.append_message(&c.id, &msg("assistant")).unwrap();
        assert_eq!(reply.author, None, "nobody is named until the turn says so");
        db.set_message_author(&reply.id, Some(&format!("persona:{kai}"))).unwrap();
        let authors = |id: &str| db.list_messages(id).unwrap().into_iter().map(|m| m.author).collect::<Vec<_>>();
        assert_eq!(authors(&c.id), vec![None, Some(format!("persona:{kai}"))]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_default_that_names_a_persona_that_is_gone_is_no_default() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let c = db.create_conversation("t", None, false).unwrap();
        mem.set_default_leader("global", Some("persona:ghost")).unwrap();
        assert_eq!(led(&db, &mem, &c.id), ("poiesis".into(), "none"));
        start_of_turn(&db, &mem, &c.id);
        assert_eq!(db.get_conversation(&c.id).unwrap().unwrap().leader, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_first_turn_takes_the_leader_its_place_says_and_never_overrides_a_choice_or_a_helper() {
        let db = Db::open_in_memory().unwrap();
        let (mem, dir) = memory();
        let mira = persona(&db, "Mira");
        let kai = persona(&db, "Kai");
        mem.set_default_leader("global", Some(&format!("persona:{mira}"))).unwrap();

        let fresh = db.create_conversation("fresh", None, false).unwrap();
        start_of_turn(&db, &mem, &fresh.id);
        let after = db.get_conversation(&fresh.id).unwrap().unwrap();
        assert_eq!(after.persona_id.as_deref(), Some(mira.as_str()), "the turn code finds Mira where it looks");
        assert_eq!(after.leader, Some(format!("persona:{mira}")));

        // Poiesis chosen on purpose stays Poiesis.
        let chose = db.create_conversation("chose", None, false).unwrap();
        set_leader(&db, &mem, &chose.id, Some(&Leader::Poiesis)).unwrap();
        start_of_turn(&db, &mem, &chose.id);
        assert_eq!(db.get_conversation(&chose.id).unwrap().unwrap().persona_id, None);

        // Somebody else's choice stays.
        let kais = db.create_conversation("kai", None, false).unwrap();
        set_leader(&db, &mem, &kais.id, Some(&Leader::Persona(kai.clone()))).unwrap();
        start_of_turn(&db, &mem, &kais.id);
        assert_eq!(db.get_conversation(&kais.id).unwrap().unwrap().persona_id.as_deref(), Some(kai.as_str()));

        // A project that says Poiesis keeps its chats Poiesis, whatever the global leader.
        let project = db.create_project("Shop", None).unwrap();
        mem.set_default_leader(&format!("project:{}", project.id), Some("poiesis")).unwrap();
        let in_project = db.create_conversation("in project", None, false).unwrap();
        db.set_conversation_project(&in_project.id, Some(&project.id)).unwrap();
        start_of_turn(&db, &mem, &in_project.id);
        assert_eq!(db.get_conversation(&in_project.id).unwrap().unwrap().persona_id, None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
