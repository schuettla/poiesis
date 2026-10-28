//! `INT-1`/`INT-2`: things I was asked to do later, kept as facts of kind
//! `intention`.
//!
//! An intention is a note with a trigger. It sits in the facts folder like any
//! other note and never reaches the memory index. It reaches a turn only when its
//! trigger fires, once, and is then done. A time is not a trigger here: the
//! schedule already runs things at a time, and a second clock would only disagree
//! with it, so a time goes there instead (`TIME`).

use super::{today, Fact, MemoryStore, FACTS};
use crate::db::Db;

pub const KIND: &str = "intention";

/// Shown in the index above the lines of the intentions that fired.
pub(crate) const REMINDERS_HEADER: &str = "\n## Reminders (the user asked for these; say so now)\n";

const WHICH: &str = "a reminder needs a trigger: project:<id>, topic:<words> or mail:<sender>";

/// What to do with a reminder for a time. It is the schedule's job, not a note's.
pub const TIME: &str = "a reminder for a time is not saved as a note. Use the harness tool with name schedule: put the time in when and the task in task, and the user sets it up.";

/// When an intention fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// A chat in this project (by id).
    Project(String),
    /// A message that carries all of these words.
    Topic(String),
    /// A new mail whose sender carries this text.
    Mail(String),
}

impl Trigger {
    /// `project:<id>`, `topic:<words>` or `mail:<sender>`. A time is refused with
    /// the way to the schedule.
    pub fn parse(raw: &str) -> Result<Trigger, String> {
        let (kind, value) = raw.split_once(':').ok_or_else(|| WHICH.to_string())?;
        let value = value.trim();
        match kind.trim().to_lowercase().as_str() {
            "time" | "at" | "on" | "when" | "every" | "date" | "in" => Err(TIME.to_string()),
            _ if value.is_empty() => Err(WHICH.to_string()),
            "project" => Ok(Trigger::Project(value.to_string())),
            "topic" => {
                let words = topic_words(value);
                if words.is_empty() {
                    return Err("a topic needs at least one word of three letters or more".to_string());
                }
                Ok(Trigger::Topic(words.join(" ")))
            }
            "mail" => Ok(Trigger::Mail(value.to_string())),
            _ => Err(WHICH.to_string()),
        }
    }

    /// The on-disk spelling, which `parse` reads back.
    pub fn render(&self) -> String {
        match self {
            Trigger::Project(id) => format!("project:{id}"),
            Trigger::Topic(words) => format!("topic:{words}"),
            Trigger::Mail(from) => format!("mail:{from}"),
        }
    }

    /// The trigger in words, to finish "when ...". `project_name` is the project's
    /// name when it is known.
    pub fn when_words(&self, project_name: Option<&str>) -> String {
        match self {
            Trigger::Project(id) => format!("you are in the project \"{}\"", project_name.unwrap_or(id)),
            Trigger::Topic(words) => format!("\"{words}\" comes up"),
            Trigger::Mail(from) => format!("{from} writes"),
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// Lowercase words of three letters or more. Shorter ones ("a", "of") say nothing
/// about a topic.
fn topic_words(text: &str) -> Vec<String> {
    text.split(|c: char| !is_word_char(c))
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 3)
        .collect()
}

/// Two words match when they are the same, or when one is the other with up to
/// three letters added at the end: a plural or an ending ("tax", "taxes").
fn same_word(a: &str, b: &str) -> bool {
    let (short, long) = if a.chars().count() <= b.chars().count() { (a, b) } else { (b, a) };
    long.starts_with(short) && long.chars().count() - short.chars().count() <= 3
}

/// Does the message carry every word of the topic?
pub fn topic_in(topic: &str, message: &str) -> bool {
    let wanted = topic_words(topic);
    let said = topic_words(message);
    !wanted.is_empty() && wanted.iter().all(|w| said.iter().any(|s| same_word(w, s)))
}

/// Is this the sender a mail trigger is about? `header` is the From line as it
/// arrives (a name and an address).
pub fn sender_is(wanted: &str, header: &str) -> bool {
    let wanted = wanted.trim().to_lowercase();
    !wanted.is_empty() && header.to_lowercase().contains(&wanted)
}

impl Fact {
    pub fn is_intention(&self) -> bool {
        self.kind == KIND
    }

    /// An intention that has not fired yet, or whose mail has come and not been told.
    pub fn is_pending_intention(&self) -> bool {
        self.is_intention() && self.status.as_deref() != Some("done")
    }

    /// The trigger, when the file says one that reads.
    pub fn trigger_of(&self) -> Option<Trigger> {
        self.trigger.as_deref().and_then(|t| Trigger::parse(t).ok())
    }
}

/// What a reminder waits for, as the person wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cond {
    /// A project, by name. The caller finds its id.
    Project(String),
    Topic(String),
    Mail(String),
    /// A time, in the person's words. It goes to the schedule.
    Time(String),
}

/// `remind me <what> when <condition>`, read apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub task: String,
    pub cond: Cond,
}

const TEACH: &str = "I did not catch when. Say it like this: \"remind me to call Anna when I open the project Book\", \"... when we talk about taxes\" or \"... when Anna writes\".";

/// `/remember remind me to ... when ...`. `None` when the text is not a
/// reminder at all, so `/remember` saves it as a note as before. A reminder I
/// cannot read is an error and not a note: a note would never fire.
pub fn parse_reminder(text: &str) -> Option<Result<Reminder, String>> {
    let text = text.trim();
    // ASCII only, so a position found in `lower` is the same position in `text`.
    let lower = text.to_ascii_lowercase();
    let rest = lower.strip_prefix("remind me")?;
    // `remind meal`: the word must end here.
    if rest.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    let offset = text.len() - rest.len();
    let rest_original = text[offset..].trim();
    let rest = rest.trim();
    let split = [" when ", " next time ", " the next time ", " as soon as ", " once "]
        .iter()
        .filter_map(|m| rest.find(m).map(|at| (at, m.len())))
        .min_by_key(|(at, _)| *at);
    let Some((at, len)) = split else {
        // No "when": a time said at the end ("... tomorrow", "... on Friday") is still a time.
        let at = rest.match_indices(' ').map(|(i, _)| i).find(|i| starts_time(&rest[i + 1..]));
        let Some(at) = at else { return Some(Err(TEACH.to_string())) };
        return Some(Ok(Reminder { task: clean_task(&rest_original[..at]), cond: Cond::Time(rest_original[at + 1..].trim().to_string()) }));
    };
    let task = clean_task(&rest_original[..at]);
    let cond = rest_original[at + len..].trim().trim_end_matches(['.', '!']).trim();
    if task.is_empty() || cond.is_empty() {
        return Some(Err(TEACH.to_string()));
    }
    Some(read_cond(cond).map(|cond| Reminder { task, cond }).ok_or_else(|| TEACH.to_string()))
}

/// The task without its lead-in ("to call Anna" is "call Anna").
fn clean_task(raw: &str) -> String {
    let raw = raw.trim();
    ["to ", "about ", "of "].iter().find_map(|p| strip_prefix_ci(raw, p)).unwrap_or(raw).trim().to_string()
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len()).filter(|head| head.eq_ignore_ascii_case(prefix)).map(|_| &text[prefix.len()..])
}

/// The condition after "when". Time is looked for first: "on Monday" says more than
/// any word in it.
fn read_cond(cond: &str) -> Option<Cond> {
    let lower = cond.to_ascii_lowercase();
    if is_time(&lower) {
        return Some(Cond::Time(cond.to_string()));
    }
    // Who I or we are is no part of the condition.
    let skip = ["i ", "we ", "you "].iter().find(|p| lower.starts_with(*p)).map_or(0, |p| p.len());
    let (lower, cond) = (&lower[skip..], &cond[skip..]);

    for verb in ["open the project ", "open project ", "open ", "enter the project ", "enter ", "am in the project ", "am in ", "work on the project ", "work on ", "am working on "] {
        if let Some(name) = lower.strip_prefix(verb) {
            let name = cond[cond.len() - name.len()..].trim().trim_matches('"').trim();
            let name = name.strip_prefix("the project ").or(name.strip_prefix("project ")).unwrap_or(name).trim();
            return (!name.is_empty()).then(|| Cond::Project(name.to_string()));
        }
    }
    for verb in [" about ", " mention "] {
        if let Some(at) = lower.find(verb) {
            let head = lower[..at].trim();
            if verb == " mention " || ["talk", "speak", "chat", "ask", "write", "think"].contains(&head) {
                let topic = cond[at + verb.len()..].trim().trim_matches('"').trim();
                return (!topic.is_empty()).then(|| Cond::Topic(topic.to_string()));
            }
        }
    }
    if let Some(topic) = lower.strip_prefix("mention ") {
        return Some(Cond::Topic(cond[cond.len() - topic.len()..].trim().trim_matches('"').to_string()));
    }
    for tail in [" comes up", " is mentioned", " are mentioned", " come up"] {
        if let Some(topic) = lower.strip_suffix(tail) {
            return Some(Cond::Topic(cond[..topic.len()].trim().trim_matches('"').to_string()));
        }
    }
    for verb in [" writes", " emails", " mails", " sends", " messages", " replies", " answers", " responds"] {
        if let Some(at) = lower.find(verb) {
            let name = cond[..at].trim().trim_matches('"').trim();
            let pronoun = ["i", "we", "you", "he", "she", "they", "it"].contains(&name.to_lowercase().as_str());
            return (!name.is_empty() && !pronoun).then(|| Cond::Mail(name.to_string()));
        }
    }
    None
}

const DAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
const MONTHS: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december",
];

fn clock(word: &str) -> bool {
    word.contains(':') && word.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Does the text begin with a time: "tomorrow", "every week", "at 9", "in 3
/// days", "on Friday", "next week", "Friday", "9:30"? "on the light" does not.
fn starts_time(lower: &str) -> bool {
    let mut words = lower.split_whitespace();
    let (first, second) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let digit = |w: &str| w.chars().next().is_some_and(|c| c.is_ascii_digit());
    let second = second.trim_end_matches(',');
    match first {
        "tomorrow" | "today" | "tonight" | "every" => true,
        "at" => digit(second) || matches!(second, "noon" | "midnight"),
        "in" => digit(second),
        "on" => DAYS.contains(&second) || digit(second) || MONTHS.contains(&second),
        "next" => matches!(second, "week" | "month" | "year") || DAYS.contains(&second),
        "this" => matches!(second, "morning" | "evening" | "afternoon"),
        f => DAYS.contains(&f) || clock(f),
    }
}

/// Does the condition after "when" name a time? A day or a clock time anywhere
/// in it will do ("when it is Friday").
fn is_time(lower: &str) -> bool {
    starts_time(lower)
        || lower.split(|c: char| !c.is_alphanumeric() && c != ':').any(|w| DAYS.contains(&w) || clock(w))
}

/// One intention that fired for a turn.
#[derive(Debug, Clone)]
pub struct Fired {
    pub name: String,
    /// The line the model gets.
    pub line: String,
    /// What the person set, in words ("you are in the project ...").
    pub why: String,
    pub description: String,
    pub created: String,
    pub source_conversation: Option<String>,
}

impl MemoryStore {
    /// Save an intention. The trigger must read; the note is shared, not tagged to
    /// a project, because its trigger already says where it applies.
    pub fn save_intention(&self, db: &Db, f: &Fact) -> Result<String, String> {
        let trigger = Trigger::parse(f.trigger.as_deref().unwrap_or(""))?;
        let fact = Fact {
            kind: KIND.to_string(),
            trigger: Some(trigger.render()),
            scope: Some("global".to_string()),
            project: None,
            status: None,
            ..f.clone()
        };
        self.save(db, &fact)
    }

    /// Everything still to do, newest first.
    pub fn intentions(&self) -> Vec<Fact> {
        self.list_in(FACTS).into_iter().filter(Fact::is_pending_intention).collect()
    }

    /// The intentions this turn fires: a project or topic that matches, and any
    /// whose mail has come in (`ready`).
    pub fn due_intentions(&self, project: Option<&str>, message: &str) -> Vec<Fact> {
        self.intentions()
            .into_iter()
            .filter(|f| {
                f.status.as_deref() == Some("ready")
                    || match f.trigger_of() {
                        Some(Trigger::Project(id)) => project == Some(id.as_str()),
                        Some(Trigger::Topic(words)) => topic_in(&words, message),
                        _ => false,
                    }
            })
            .collect()
    }

    /// Fire what is due for this turn: each one is done after this and is returned
    /// with its line. The chat it fired in becomes its evidence.
    pub fn fire_due(&self, db: &Db, project: Option<&str>, message: &str, conversation_id: &str) -> Vec<Fired> {
        let mut out = Vec::new();
        for f in self.due_intentions(project, message) {
            let Some(trigger) = f.trigger_of() else { continue };
            if self.finish_intention(db, &f.name, conversation_id).is_err() {
                continue;
            }
            let project_name = match &trigger {
                Trigger::Project(id) => db.get_project(id).ok().flatten().map(|p| p.name),
                _ => None,
            };
            let mut why = trigger.when_words(project_name.as_deref());
            if f.status.as_deref() == Some("ready") {
                why.push_str(", and a message has come in");
            }
            let line = format!(
                "You asked me to remind the user about \"{}\" when {why}. Tell them now, briefly.",
                f.description
            );
            out.push(Fired {
                name: f.name,
                line,
                why: format!("you asked me to remind you when {why}"),
                description: f.description,
                created: f.created,
                source_conversation: f.source_conversation,
            });
        }
        out
    }

    /// Done: it fired, and it will not fire again. The note stays, so the person
    /// can still read what it was.
    fn finish_intention(&self, db: &Db, name: &str, conversation_id: &str) -> Result<(), String> {
        self.rewrite(FACTS, name, |f| {
            f.status = Some("done".to_string());
            f.last_seen = Some(today());
            f.add_evidence(conversation_id);
        })?;
        self.write_index();
        self.sync_fts(db);
        Ok(())
    }

    /// New mail has come from these senders (From lines). Every waiting mail
    /// intention that one of them matches is marked ready, to be told at the next
    /// turn. Returns their names.
    pub fn mail_arrived(&self, db: &Db, senders: &[String]) -> Vec<String> {
        let mut ready = Vec::new();
        for f in self.intentions() {
            if f.status.as_deref() == Some("ready") {
                continue;
            }
            let Some(Trigger::Mail(from)) = f.trigger_of() else { continue };
            if senders.iter().any(|s| sender_is(&from, s))
                && self.rewrite(FACTS, &f.name, |e| e.status = Some("ready".to_string())).is_ok()
            {
                ready.push(f.name);
            }
        }
        if !ready.is_empty() {
            self.write_index();
            self.sync_fts(db);
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, MemoryStore, Db) {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path()).unwrap();
        (dir, store, Db::open_in_memory().unwrap())
    }

    fn intention(name: &str, what: &str, trigger: &str) -> Fact {
        Fact {
            name: name.into(),
            description: what.into(),
            body: what.into(),
            trigger: Some(trigger.into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_trigger_reads_back_as_it_was_written() {
        for raw in ["project:p-book", "topic:tax return", "mail:Anna"] {
            assert_eq!(Trigger::parse(raw).unwrap().render(), raw);
        }
    }

    #[test]
    fn a_time_goes_to_the_schedule_and_nonsense_is_refused() {
        for raw in ["time:tomorrow at 9", "at:9", "every:monday", "when:later"] {
            assert_eq!(Trigger::parse(raw).unwrap_err(), TIME, "{raw}");
        }
        assert!(Trigger::parse("tomorrow").is_err());
        assert!(Trigger::parse("project:").is_err());
        assert!(Trigger::parse("topic:of a").is_err(), "no word of three letters");
        assert!(Trigger::parse("colour:red").is_err());
    }

    #[test]
    fn a_topic_needs_every_word_and_tolerates_an_ending() {
        assert!(topic_in("tax return", "Can you help with my tax returns?"));
        assert!(topic_in("tax", "what about taxes this year"));
        assert!(!topic_in("tax return", "just the tax, please"));
        assert!(!topic_in("cat", "concatenate these lists"), "more than an ending is a different word");
        assert!(!topic_in("", "anything"));
    }

    fn read(text: &str) -> Reminder {
        parse_reminder(text).expect("a reminder").expect("one that reads")
    }

    #[test]
    fn a_reminder_is_read_into_what_and_when() {
        let r = read("remind me to call Anna when I open the project Book");
        assert_eq!((r.task.as_str(), r.cond), ("call Anna", Cond::Project("Book".into())));
        let r = read("Remind me about my return when we talk about taxes");
        assert_eq!((r.task.as_str(), r.cond), ("my return", Cond::Topic("taxes".into())));
        let r = read("remind me to answer when Anna writes.");
        assert_eq!((r.task.as_str(), r.cond), ("answer", Cond::Mail("Anna".into())));
        let r = read("remind me to check the budget next time the budget comes up");
        assert_eq!((r.task.as_str(), r.cond), ("check the budget", Cond::Topic("the budget".into())));
    }

    #[test]
    fn a_reminder_for_a_time_is_a_time_with_or_without_when() {
        for (text, task, when) in [
            ("remind me to buy milk tomorrow", "buy milk", "tomorrow"),
            ("remind me to call Anna on Friday", "call Anna", "on Friday"),
            ("remind me to pay rent every month", "pay rent", "every month"),
            ("remind me to stretch when it is Friday", "stretch", "it is Friday"),
            ("remind me to leave when it is 17:30", "leave", "it is 17:30"),
        ] {
            let r = read(text);
            assert_eq!((r.task.as_str(), r.cond), (task, Cond::Time(when.into())), "{text}");
        }
    }

    #[test]
    fn text_that_is_not_a_reminder_stays_a_note_and_a_reminder_that_does_not_read_is_an_error() {
        assert!(parse_reminder("prefers tea").is_none());
        assert!(parse_reminder("remindful people").is_none());
        for text in ["remind me to turn on the light", "remind me to sing when it rains", "remind me when Anna writes", "remind me to x when I write"] {
            let err = parse_reminder(text).expect("it is a reminder").unwrap_err();
            assert!(err.starts_with("I did not catch when"), "{text}");
        }
    }

    #[test]
    fn a_sender_matches_by_name_or_address_in_any_case() {
        assert!(sender_is("anna", "Anna Berg <anna.berg@example.com>"));
        assert!(sender_is("@example.com", "Bo <bo@example.com>"));
        assert!(!sender_is("anna", "Bo <bo@example.com>"));
        assert!(!sender_is("  ", "Bo <bo@example.com>"));
    }

    #[test]
    fn an_intention_is_kept_out_of_the_index_and_the_fact_list() {
        let (_d, s, db) = store();
        s.save_intention(&db, &intention("call-dentist", "call the dentist", "topic:dentist")).unwrap();
        assert!(!s.index_markdown().contains("dentist"));
        assert!(s.list().iter().all(|f| !f.is_intention()), "the cards do not list it");
        assert_eq!(s.intentions().len(), 1);
    }

    #[test]
    fn an_intention_with_no_readable_trigger_is_not_saved() {
        let (_d, s, db) = store();
        assert!(s.save_intention(&db, &intention("a", "do a", "later")).is_err());
        assert!(s.save_intention(&db, &intention("b", "do b", "time:friday")).is_err());
        assert!(s.intentions().is_empty());
    }

    #[test]
    fn a_topic_fires_once_then_is_done() {
        let (_d, s, db) = store();
        s.save_intention(&db, &intention("call-dentist", "call the dentist", "topic:dentist")).unwrap();
        assert!(s.fire_due(&db, None, "what is the weather", "c1").is_empty(), "no match, nothing fires");
        let fired = s.fire_due(&db, None, "I need a dentist appointment", "c1");
        assert_eq!(fired.len(), 1);
        assert!(fired[0].line.contains("call the dentist") && fired[0].line.contains("\"dentist\" comes up"));
        assert!(s.fire_due(&db, None, "I need a dentist appointment", "c2").is_empty(), "once");
        let kept = s.read("call-dentist").unwrap();
        assert_eq!(kept.status.as_deref(), Some("done"));
        assert!(kept.evidence.contains(&"c1".to_string()));
        assert!(s.intentions().is_empty(), "nothing is waiting");
    }

    #[test]
    fn a_project_fires_only_inside_it() {
        let (_d, s, db) = store();
        s.save_intention(&db, &intention("send-draft", "send the draft", "project:p-book")).unwrap();
        assert!(s.fire_due(&db, None, "hello", "c1").is_empty());
        assert!(s.fire_due(&db, Some("p-other"), "hello", "c1").is_empty());
        assert_eq!(s.fire_due(&db, Some("p-book"), "hello", "c1").len(), 1);
    }

    #[test]
    fn mail_marks_ready_and_the_next_turn_tells_it_once() {
        let (_d, s, db) = store();
        s.save_intention(&db, &intention("answer-anna", "answer Anna", "mail:Anna")).unwrap();
        assert!(s.fire_due(&db, None, "hello", "c1").is_empty(), "nothing has arrived");
        assert!(s.mail_arrived(&db, &["Bo <bo@example.com>".to_string()]).is_empty());
        assert_eq!(s.mail_arrived(&db, &["Anna Berg <anna@example.com>".to_string()]), ["answer-anna"]);
        let fired = s.fire_due(&db, None, "hello", "c2");
        assert_eq!(fired.len(), 1);
        assert!(fired[0].line.contains("Anna writes") && fired[0].line.contains("a message has come in"));
        assert!(s.fire_due(&db, None, "hello", "c3").is_empty());
    }

    #[test]
    fn an_intention_survives_a_round_trip_through_the_file() {
        let (_d, s, db) = store();
        s.save_intention(&db, &intention("call-dentist", "call the dentist", "topic:dentist")).unwrap();
        let text = std::fs::read_to_string(s.dir().join("facts").join("call-dentist.md")).unwrap();
        assert!(text.contains("type: intention") && text.contains("trigger: topic:dentist"));
        assert_eq!(s.read("call-dentist").unwrap().trigger.as_deref(), Some("topic:dentist"));
    }
}
