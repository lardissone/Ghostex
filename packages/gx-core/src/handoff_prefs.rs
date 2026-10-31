//! What Handoff / Export remembers between opens, and what it makes of an older saved choice.
//!
//! CDXC:TranscriptExport 2026-10-31 DECISION:
//! User: "The 'include' part needs to be collapsed by default please, and default to all deselected except if user changes it then we remember", the top holds just Handoff to an agent and Handoff to MD, and "we should remember what the user picked last time so we keep that selection for next time". So this remembers the card, the Include switches and whether Include is open, the agent picked under Continue with, and the destination.
//! A value saved before this shape carries switches the old dialog defaulted on, written whenever anything changed, so they can't tell a choice from a default: everyone starts at the new default, and only a change from here on counts. An old mode for another computer or project (`handoffRemote`) comes back as Handoff to an agent with the destination choices open; it never saved a project, so there is none to restore.
//! A remembered destination is only a suggestion: one that is gone, or is the project the session is already in, falls back to this project here. Handing off from a different project can still open on a project remembered from before, so the dialog shows the destination row open and the button names it; nothing goes anywhere the button doesn't say.
//!
//! SEE-ALSO: apps/desktop/src/app/window/export_transcript_modal.rs (the desktop's dialog),
//! apps/mobile/app/src/contract/handoff/handoff.ts (the phone's port, held to this by
//! `handoffFixture.json`).

use serde_json::{json, Value};

const PREFS_VERSION: u64 = 2;

/// The two cards at the top of the dialog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HandoffCard {
    #[default]
    Agent,
    Markdown,
}

/// What the conversation's file holds beyond its messages. All off until the user changes them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HandoffInclude {
    pub commands: bool,
    pub patches: bool,
    pub reasoning: bool,
}

/// Where a handoff starts its new session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffDestination {
    pub machine_id: String,
    pub project_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandoffPrefs {
    pub card: HandoffCard,
    pub include: HandoffInclude,
    pub include_open: bool,
    /// The agent the user picked under Continue with; absent means the session's own.
    pub agent_id: Option<String>,
    pub destination: Option<HandoffDestination>,
    /// The destination's computer and project choices open with nothing remembered to show.
    pub destination_open: bool,
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool) == Some(true)
}

/// Reads a saved value, of this shape or an older one. Anything unreadable is the default.
pub fn parse_handoff_prefs(value: &Value) -> HandoffPrefs {
    let card = match text(value, "mode") {
        Some("export") => HandoffCard::Markdown,
        _ => HandoffCard::Agent,
    };
    if value.get("version").and_then(Value::as_u64) != Some(PREFS_VERSION) {
        return HandoffPrefs {
            card,
            destination_open: text(value, "mode") == Some("handoffRemote"),
            ..HandoffPrefs::default()
        };
    }
    HandoffPrefs {
        card,
        include: HandoffInclude {
            commands: flag(value, "includeCommands"),
            patches: flag(value, "includePatches"),
            reasoning: flag(value, "includeReasoning"),
        },
        include_open: flag(value, "includeOpen"),
        agent_id: text(value, "agentId").map(str::to_string),
        destination: value.get("destination").and_then(|destination| {
            Some(HandoffDestination {
                machine_id: text(destination, "machineId")?.to_string(),
                project_id: text(destination, "projectId")?.to_string(),
            })
        }),
        destination_open: flag(value, "destinationOpen"),
    }
}

/// The value to save, in this shape.
pub fn handoff_prefs_json(prefs: &HandoffPrefs) -> Value {
    let mut value = json!({
        "version": PREFS_VERSION,
        "mode": match prefs.card {
            HandoffCard::Agent => "handoff",
            HandoffCard::Markdown => "export",
        },
        "includeCommands": prefs.include.commands,
        "includePatches": prefs.include.patches,
        "includeReasoning": prefs.include.reasoning,
        "includeOpen": prefs.include_open,
        "destinationOpen": prefs.destination_open,
    });
    if let Some(agent_id) = &prefs.agent_id {
        value["agentId"] = json!(agent_id);
    }
    if let Some(destination) = &prefs.destination {
        value["destination"] = json!({
            "machineId": destination.machine_id,
            "projectId": destination.project_id,
        });
    }
    value
}

/// The remembered destination this session opens on, or `None` for this project here: `None` when
/// it is the session's own project, or `offered` says that computer no longer offers that project.
pub fn remembered_handoff_destination(
    remembered: Option<&HandoffDestination>,
    own: &HandoffDestination,
    offered: impl Fn(&HandoffDestination) -> bool,
) -> Option<HandoffDestination> {
    remembered
        .filter(|destination| *destination != own && offered(destination))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn destination(machine: &str, project: &str) -> HandoffDestination {
        HandoffDestination {
            machine_id: machine.to_string(),
            project_id: project.to_string(),
        }
    }

    #[test]
    fn with_nothing_saved_it_is_the_agent_card_with_include_closed_and_off() {
        let prefs = parse_handoff_prefs(&Value::Null);
        assert_eq!(prefs, HandoffPrefs::default());
        assert_eq!(prefs.card, HandoffCard::Agent);
        assert!(!prefs.include_open);
        assert_eq!(prefs.include, HandoffInclude::default());
        assert!(!prefs.include.commands && !prefs.include.patches && !prefs.include.reasoning);
    }

    #[test]
    fn an_old_saved_value_starts_at_the_new_default_and_keeps_its_card() {
        // Exactly what the old dialog wrote, with its switches at its old defaults.
        let old = json!({
            "mode": "export",
            "includeCommands": true,
            "includePatches": true,
            "includeReasoning": false,
        });
        let prefs = parse_handoff_prefs(&old);
        assert_eq!(prefs.card, HandoffCard::Markdown);
        assert_eq!(prefs.include, HandoffInclude::default());
        assert!(!prefs.include_open);
    }

    #[test]
    fn an_old_handoff_to_remote_is_a_handoff_to_an_agent_with_its_choices_open() {
        let prefs = parse_handoff_prefs(&json!({"mode": "handoffRemote", "includeCommands": true}));
        assert_eq!(prefs.card, HandoffCard::Agent);
        assert!(prefs.destination_open);
        assert_eq!(prefs.destination, None);
    }

    #[test]
    fn what_the_user_changes_comes_back() {
        let prefs = HandoffPrefs {
            card: HandoffCard::Markdown,
            include: HandoffInclude {
                commands: true,
                patches: false,
                reasoning: true,
            },
            include_open: true,
            agent_id: Some("codex".to_string()),
            destination: Some(destination("studio", "acme")),
            destination_open: false,
        };
        assert_eq!(parse_handoff_prefs(&handoff_prefs_json(&prefs)), prefs);
        // Switching a switch back off stays off, rather than reading as an old value.
        let cleared = HandoffPrefs::default();
        assert_eq!(parse_handoff_prefs(&handoff_prefs_json(&cleared)), cleared);
    }

    #[test]
    fn a_half_written_destination_is_no_destination() {
        let prefs = parse_handoff_prefs(&json!({
            "version": 2,
            "destination": {"machineId": "studio"},
        }));
        assert_eq!(prefs.destination, None);
    }

    #[test]
    fn a_remembered_destination_that_no_longer_applies_falls_back_to_here() {
        let own = destination("local", "scratch");
        let acme = destination("local", "acme");
        // Still offered, and elsewhere: it comes back.
        assert_eq!(
            remembered_handoff_destination(Some(&acme), &own, |_| true),
            Some(acme.clone())
        );
        // Its computer or project is gone.
        assert_eq!(
            remembered_handoff_destination(Some(&acme), &own, |_| false),
            None
        );
        // It is the project this session is already in.
        assert_eq!(
            remembered_handoff_destination(Some(&own), &own, |_| true),
            None
        );
        // Nothing remembered.
        assert_eq!(remembered_handoff_destination(None, &own, |_| true), None);
    }
}
