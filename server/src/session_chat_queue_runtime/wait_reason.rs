//! Why the head of a session's queue has not gone out, in the words its row shows, and the
//! thresholds of the ladder that heals a row waiting on an agent that is ready by every other sign.
//!
//! SEE-ALSO: scheduler.rs (records a reason at every gate, runs the ladder and the watchdog),
//! held_redraw.rs (the Ctrl+L step), repaint_nudge.rs (the resize step), session_chat_queue.rs
//! (`set_queue_wait_reason`, which puts the words on the row), packages/gx-chat-core
//! (`waitReason` on queue rows, held sends and agent message cards).

use chrono::{DateTime, Utc};

/// A row's reason shows once it has waited this long: shorter waits are a normal turn ending.
pub(super) const WAIT_REASON_SHOWN_AFTER_MS: i64 = 60_000;

/// The input-box ladder's Ctrl+L step (held_redraw.rs owns the clock; kept here for the words).
pub(super) const INPUT_BOX_REDRAW_AFTER_MS: i64 = 20_000;

/// The input-box ladder's last resort: by now the fresh reads, two Ctrl+L redraws and a resize
/// nudge have had their chance, so the row goes to the send path, whose own capture decides.
pub(super) const INPUT_BOX_LAST_RESORT_AFTER_MS: i64 = 180_000;

/// The watchdog: a head row held this long by a gate that should have let it go, on a session
/// idle by every signal, is logged and handed to the same ladder's last resort.
pub(super) const WATCHDOG_AFTER_MS: i64 = 300_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InputBoxStep {
    /// Fresh screen reads on the startup send's backoff.
    Reading,
    /// Ctrl+L (Claude) and the resize nudge (every agent) are being tried.
    Repainting,
    /// The send path is trying; it gives up with Not sent after its two-minute hold window.
    SendingAnyway,
}

impl InputBoxStep {
    pub(super) fn after(waited_ms: i64) -> Self {
        if waited_ms >= INPUT_BOX_LAST_RESORT_AFTER_MS {
            Self::SendingAnyway
        } else if waited_ms >= INPUT_BOX_REDRAW_AFTER_MS {
            Self::Repainting
        } else {
            Self::Reading
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WaitReason {
    Asleep,
    Closed,
    ModelChange,
    ModelChangeFailed,
    AccountRecovery,
    MissingLogin,
    StartFailed,
    Starting,
    Compacting,
    Working,
    Answer,
    Settling,
    InputBox(InputBoxStep),
    /// The send path found no input box a moment ago and put the row back.
    Retrying,
}

impl WaitReason {
    /// The sentence the row shows.
    pub(super) fn text(&self) -> &'static str {
        match self {
            Self::Asleep => "Waiting for the session to wake up.",
            Self::Closed => "Waiting for the session to be opened again.",
            Self::ModelChange => "Waiting for the model change before it to apply.",
            Self::ModelChangeFailed => {
                "Waiting: the model change before this message failed. Pick a model again to send it."
            }
            Self::AccountRecovery => "Waiting for the agent's account to be ready again.",
            Self::MissingLogin => "Waiting for the agent's account to be signed in.",
            Self::StartFailed => "Waiting: the agent couldn't start. Start it again to send this.",
            Self::Starting => "Waiting for the agent to start.",
            Self::Compacting => "Waiting for the agent to finish compacting the conversation.",
            Self::Working => "Waiting for the agent to finish its turn.",
            Self::Answer => "Waiting for you to answer the agent's question.",
            Self::Settling => "Waiting for the agent to settle after its turn.",
            Self::InputBox(InputBoxStep::Reading) => {
                "Waiting: the agent's input box can't be read yet, trying again."
            }
            Self::InputBox(InputBoxStep::Repainting) => {
                "Waiting: the agent's input box can't be read yet, asking it to redraw its screen."
            }
            Self::InputBox(InputBoxStep::SendingAnyway) | Self::Retrying => {
                "Waiting: the agent's input box can't be read yet, trying to send anyway."
            }
        }
    }

    /// A stable name for logs; a change of kind restarts the watchdog's clock.
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Asleep => "asleep",
            Self::Closed => "closed",
            Self::ModelChange => "modelChange",
            Self::ModelChangeFailed => "modelChangeFailed",
            Self::AccountRecovery => "accountRecovery",
            Self::MissingLogin => "missingLogin",
            Self::StartFailed => "startFailed",
            Self::Starting => "starting",
            Self::Compacting => "compacting",
            Self::Working => "working",
            Self::Answer => "answer",
            Self::Settling => "settling",
            Self::InputBox(_) | Self::Retrying => "inputBox",
        }
    }

    /// Part of the input-box ladder, whose clock must survive the send path's retry spacing.
    pub(super) fn is_input_box(&self) -> bool {
        matches!(self, Self::InputBox(_) | Self::Retrying)
    }

    /// Whether the watchdog may hand the row to the send path once every other signal says the
    /// agent is idle. The other reasons are states a person or another process must end (a
    /// question, a sign-in, a model pick, a closed or sleeping session, a booting agent), each
    /// already shown on the row; the send path's own guards apply either way.
    pub(super) fn escalates(&self) -> bool {
        matches!(self, Self::Settling | Self::InputBox(_) | Self::Retrying)
    }
}

/// How long the current head row has waited, and for what kind of reason since when.
#[derive(Clone, Debug)]
pub(super) struct HeadWait {
    pub(super) prompt_id: String,
    pub(super) first_seen: DateTime<Utc>,
    pub(super) kind: &'static str,
    pub(super) kind_since: DateTime<Utc>,
    pub(super) watchdog_logged: bool,
}

impl HeadWait {
    /// Moves the clock to `prompt_id` held for `reason` at `now` (a new head row starts a new
    /// clock). Answers the words the row should show: none until it has waited
    /// [`WAIT_REASON_SHOWN_AFTER_MS`].
    pub(super) fn observe(
        wait: &mut Option<HeadWait>,
        prompt_id: &str,
        reason: &WaitReason,
        now: DateTime<Utc>,
    ) -> Option<&'static str> {
        let wait = match wait {
            Some(wait) if wait.prompt_id == prompt_id => wait,
            _ => wait.insert(HeadWait {
                prompt_id: prompt_id.to_string(),
                first_seen: now,
                kind: reason.kind(),
                kind_since: now,
                watchdog_logged: false,
            }),
        };
        if wait.kind != reason.kind() {
            wait.kind = reason.kind();
            wait.kind_since = now;
            wait.watchdog_logged = false;
        }
        (now.signed_duration_since(wait.first_seen)
            .num_milliseconds()
            >= WAIT_REASON_SHOWN_AFTER_MS)
            .then(|| reason.text())
    }

    pub(super) fn kind_waited_ms(&self, now: DateTime<Utc>) -> i64 {
        now.signed_duration_since(self.kind_since)
            .num_milliseconds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + seconds, 0).unwrap()
    }

    #[test]
    fn a_reason_shows_after_a_minute_and_follows_the_ladder() {
        let mut wait = None;
        let working = WaitReason::Working;
        assert_eq!(HeadWait::observe(&mut wait, "Q1", &working, at(0)), None);
        assert_eq!(HeadWait::observe(&mut wait, "Q1", &working, at(59)), None);
        assert_eq!(
            HeadWait::observe(&mut wait, "Q1", &working, at(60)),
            Some("Waiting for the agent to finish its turn.")
        );
        // The turn ends and the box cannot be read: the words move with the ladder at once.
        let reading = WaitReason::InputBox(InputBoxStep::after(0));
        assert_eq!(
            HeadWait::observe(&mut wait, "Q1", &reading, at(61)),
            Some("Waiting: the agent's input box can't be read yet, trying again.")
        );
        assert_eq!(wait.as_ref().unwrap().kind_waited_ms(at(61)), 0);
        let anyway = WaitReason::InputBox(InputBoxStep::after(180_000));
        assert_eq!(
            HeadWait::observe(&mut wait, "Q1", &anyway, at(241)),
            Some("Waiting: the agent's input box can't be read yet, trying to send anyway.")
        );
        // The send path's retry spacing is the same kind, so the watchdog clock keeps running.
        HeadWait::observe(&mut wait, "Q1", &WaitReason::Retrying, at(245));
        assert_eq!(wait.as_ref().unwrap().kind_waited_ms(at(245)), 184_000);
        // The next head row starts its own minute.
        assert_eq!(HeadWait::observe(&mut wait, "Q2", &working, at(300)), None);
    }

    #[test]
    fn the_ladder_steps_and_what_the_watchdog_may_escalate() {
        assert_eq!(InputBoxStep::after(19_999), InputBoxStep::Reading);
        assert_eq!(InputBoxStep::after(20_000), InputBoxStep::Repainting);
        assert_eq!(InputBoxStep::after(179_999), InputBoxStep::Repainting);
        assert_eq!(InputBoxStep::after(180_000), InputBoxStep::SendingAnyway);
        for reason in [
            WaitReason::Settling,
            WaitReason::Retrying,
            WaitReason::InputBox(InputBoxStep::Reading),
        ] {
            assert!(reason.escalates(), "{reason:?}");
        }
        for reason in [
            WaitReason::Working,
            WaitReason::Answer,
            WaitReason::Compacting,
            WaitReason::Asleep,
            WaitReason::Closed,
            WaitReason::Starting,
            WaitReason::MissingLogin,
            WaitReason::ModelChangeFailed,
        ] {
            assert!(!reason.escalates(), "{reason:?}");
        }
    }
}
