//! `UCM-9`: a conversation as a Markdown file you can read anywhere.
//!
//! Plain text only: the title, then each turn under who said it. An attachment is
//! its file name (the bytes stay where they are), and a plan is a checklist. What
//! the agent did between the words (its steps) is left out, so the file reads like
//! the chat, not like a log.

use crate::db::{Conversation, Message};

use super::plan::{Plan, PlanStatus};

/// `**You**` and `**Poiesis**`: the two voices in the file.
fn speaker(role: &str) -> Option<&'static str> {
    match role {
        "user" => Some("You"),
        "assistant" => Some("Poiesis"),
        _ => None,
    }
}

/// A plan as a checklist. A dropped item is struck through and says why.
fn checklist(plan: &Plan) -> String {
    plan.items
        .iter()
        .map(|item| match item.status {
            PlanStatus::Done => format!("- [x] {}", item.text),
            PlanStatus::Dropped => match item.why.as_deref().filter(|w| !w.trim().is_empty()) {
                Some(why) => format!("- [ ] ~~{}~~ (dropped: {})", item.text, why.trim()),
                None => format!("- [ ] ~~{}~~ (dropped)", item.text),
            },
            // A step being worked on is still not done.
            PlanStatus::Todo | PlanStatus::Doing => format!("- [ ] {}", item.text),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The whole conversation. An empty conversation still has its title, so the
/// file is never blank.
pub fn markdown(conversation: &Conversation, messages: &[Message]) -> String {
    let title = conversation.title.trim();
    let mut out = format!("# {}\n", if title.is_empty() { "Conversation" } else { title });

    for message in messages {
        let Some(who) = speaker(&message.role) else { continue };
        let body = message.content.trim();
        let plan = message
            .plan_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<Plan>(json).ok())
            .filter(|p| !p.is_empty());
        // A turn with nothing in it (a run that was stopped before it spoke) says
        // nothing, so it is not in the file.
        if body.is_empty() && message.attachments.is_empty() && plan.is_none() {
            continue;
        }

        out.push_str(&format!("\n**{who}**\n"));
        if !body.is_empty() {
            out.push_str(&format!("\n{body}\n"));
        }
        if !message.attachments.is_empty() {
            let names: Vec<&str> = message.attachments.iter().map(|a| a.name.as_str()).collect();
            out.push_str(&format!("\nAttached: {}\n", names.join(", ")));
        }
        if let Some(plan) = plan {
            out.push_str(&format!("\nPlan:\n\n{}\n", checklist(&plan)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Attachment;

    fn conversation(title: &str) -> Conversation {
        serde_json::from_value(serde_json::json!({
            "id": "c1",
            "title": title,
            "model_id": null,
            "persona_id": null,
            "overrides_json": null,
            "workspace": false,
            "summary": null,
            "summary_upto_message_id": null,
            "reflected_at": null,
            "folder_path": null,
            "folder_trust": "confirm",
            "created_at": 0,
            "updated_at": 0,
        }))
        .expect("a conversation row")
    }

    fn message(role: &str, content: &str) -> Message {
        Message {
            id: "m".into(),
            conversation_id: "c1".into(),
            role: role.into(),
            content: content.into(),
            model_name: None,
            model_provenance: None,
            steps_json: None,
            stop_reason: None,
            plan_json: None,
            spoken: false,
            created_at: 0,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn title_then_each_turn_under_who_said_it() {
        let md = markdown(
            &conversation("Fix the build"),
            &[message("user", "It fails."), message("assistant", "I fixed it.")],
        );
        assert_eq!(md, "# Fix the build\n\n**You**\n\nIt fails.\n\n**Poiesis**\n\nI fixed it.\n");
    }

    #[test]
    fn attachments_are_file_names_and_plans_are_checklists() {
        let mut ask = message("user", "Look at this");
        ask.attachments.push(Attachment {
            id: "a".into(),
            kind: "pdf".into(),
            name: "report.pdf".into(),
            path: "C:/x/report.pdf".into(),
            artifact_id: None,
        });
        let mut answer = message("assistant", "Done.");
        answer.plan_json = Some(
            serde_json::json!({
                "items": [
                    { "text": "read it", "status": "done" },
                    { "text": "write it up", "status": "doing" },
                    { "text": "email it", "status": "dropped", "why": "you said not to" },
                ],
                "revisions": 0
            })
            .to_string(),
        );
        let md = markdown(&conversation("Report"), &[ask, answer]);
        assert!(md.contains("Attached: report.pdf"));
        assert!(md.contains("- [x] read it\n- [ ] write it up\n- [ ] ~~email it~~ (dropped: you said not to)"));
    }

    #[test]
    fn an_empty_turn_and_a_blank_title_do_not_leave_holes() {
        let md = markdown(&conversation("  "), &[message("assistant", "  "), message("user", "Hi")]);
        assert_eq!(md, "# Conversation\n\n**You**\n\nHi\n");
    }
}
