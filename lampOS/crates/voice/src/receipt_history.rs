//! Bounded live receipt lineage, independent of metadata retention or storage.
use lamp_audio::ownership::{PlaybackCursor, QueuedRange};
use lamp_turn_owner::TurnOwner;
use std::{collections::VecDeque, io};

const MAX_OWNERS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cancel {
    at_us: u64,
    user: bool,
    expected_discard: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    owner: TurnOwner,
    admitted: Option<u64>,
    cancelled: Option<Cancel>,
    successor: Option<(TurnOwner, u64)>,
    terminal: bool,
    pending: Option<(u64, usize)>,
    accepted: Option<PlaybackCursor>,
    tail_seen: bool,
    discard_seen: bool,
}
impl Entry {
    fn settled(&self) -> bool {
        self.terminal && self.pending.is_none() && self.accepted.is_none()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReceiptHistory {
    entries: VecDeque<Entry>,
}
/// The reasons of the local cancels (`hand::cancel`): each revokes the
/// speaker itself, so the queued audio the speaker then drops is expected.
/// The coordinator names its cancels with these, so they cannot drift apart.
pub(crate) const HEAD_SLEEP: &str = "head_sleep";
pub(crate) const SOFTWARE_MUTED: &str = "software_muted";
pub(crate) const HAND_STOP: &str = "hand_stop";
pub(crate) const HEAD_STOP: &str = "head_stop";
pub(crate) const CAPTURE_GAP: &str = "capture_gap";
/// A stop phrase said over the lamp's voice and heard while it speaks
/// (`LAMP_STOP_WORDS=heard-always`, #828): the hand's stop, by voice.
pub(crate) const SPOKEN_STOP: &str = "spoken_stop";
/// A push on the body while a skill's search goes on (`body/hand`, #2904):
/// the search's turn in hand is cut as a spoken stop cuts it.
pub(crate) const BODY_PUSHED: &str = "body_pushed";
/// The shutdown's last words or a sleep asked aloud cut the lamp's reply for
/// a sound of its own (`cut_for_last_words`, #3697, #3748), revoking the
/// speaker first as a hand's Stop does.
pub(crate) const LAST_WORDS: &str = "last_words";
pub(crate) const LOCAL_CANCELS: [&str; 8] = [
    LAST_WORDS,
    HEAD_SLEEP,
    SOFTWARE_MUTED,
    HAND_STOP,
    HEAD_STOP,
    CAPTURE_GAP,
    SPOKEN_STOP,
    BODY_PUSHED,
];

impl ReceiptHistory {
    /// Bounded live obligations, never inferred from retained metadata rows.
    pub(crate) fn is_settled(&self) -> bool {
        self.entries.iter().all(Entry::settled)
    }
    fn entry(&mut self, owner: TurnOwner) -> io::Result<&mut Entry> {
        if let Some(index) = self.entries.iter().position(|entry| entry.owner == owner) {
            return Ok(&mut self.entries[index]);
        }
        if self.entries.len() == MAX_OWNERS {
            let index = self
                .entries
                .iter()
                .position(Entry::settled)
                .ok_or_else(|| {
                    io::Error::other("unsettled speaker ownership history exceeded bound")
                })?;
            self.entries.remove(index);
        }
        self.entries.push_back(Entry {
            owner,
            admitted: None,
            cancelled: None,
            successor: None,
            terminal: false,
            pending: None,
            accepted: None,
            tail_seen: false,
            discard_seen: false,
        });
        Ok(self.entries.back_mut().expect("just inserted"))
    }
    pub(crate) fn admitted(&mut self, owner: TurnOwner, issued_at_us: u64) -> io::Result<()> {
        let entry = self.entry(owner)?;
        if issued_at_us == 0 || entry.admitted.is_some() || entry.terminal {
            return Err(io::Error::other(
                "duplicate or invalid admitted receipt lineage",
            ));
        }
        entry.admitted = Some(issued_at_us);
        // Capture the original next admission in the cancelled entry itself.
        // Completing/evicting that successor cannot erase an unsettled old tail.
        for entry in &mut self.entries {
            if entry.cancelled.is_some_and(|cancel| cancel.user)
                && entry.successor.is_none()
                && entry.owner.boot() == owner.boot()
                && entry.owner.turn() < owner.turn()
            {
                entry.successor = Some((owner, issued_at_us));
            }
        }
        Ok(())
    }
    pub(crate) fn cancelled(
        &mut self,
        owner: TurnOwner,
        at_us: u64,
        reason: &str,
    ) -> io::Result<()> {
        let entry = self.entry(owner)?;
        if entry.cancelled.is_some() {
            return Err(io::Error::other("duplicate cancellation lineage"));
        }
        entry.cancelled = Some(Cancel {
            at_us,
            user: reason == "user_interrupted",
            // A local Stop (the button, a tap on the head), the mute, sleep
            // and a capture gap revoke the speaker themselves, so the queued
            // audio it then drops is theirs: on 16 Oct at 12:20 a head tap
            // during a story ended the voice and it came back muted.
            // An answer cut as the speaker's authority lapsed (#1178) is
            // revoked by that cut too.
            expected_discard: matches!(
                reason,
                "user_interrupted"
                    | "provider_interrupted"
                    | "deadline_cancelled"
                    | "service_stopped"
                    | "speaker_lapsed"
            ) || LOCAL_CANCELS.contains(&reason),
        });
        entry.terminal = true;
        Ok(())
    }
    pub(crate) fn completed(&mut self, owner: TurnOwner) -> io::Result<()> {
        self.entry(owner)?.terminal = true;
        Ok(())
    }
    pub(crate) fn dispatched(&mut self, owner: TurnOwner, sequence: u64) -> io::Result<()> {
        let entry = self.entry(owner)?;
        if entry.pending.is_some() || entry.terminal {
            return Err(io::Error::other(
                "speaker dispatch replaced unsettled or terminal ownership",
            ));
        }
        entry.pending = Some((sequence, 0));
        Ok(())
    }
    /// Called only after the normal playback-mask/receipt validator succeeds.
    pub(crate) fn accepted(
        &mut self,
        owner: TurnOwner,
        sequence: u64,
        frames: usize,
        end: PlaybackCursor,
    ) -> io::Result<()> {
        let entry = self.entry(owner)?;
        if frames > 0 {
            entry.accepted = Some(end);
        }
        if let Some((pending, count)) = entry.pending.as_mut() {
            if *pending != sequence {
                return Err(io::Error::other(
                    "speaker receipt lineage sequence mismatch",
                ));
            }
            *count = count
                .checked_add(frames)
                .ok_or_else(|| io::Error::other("speaker receipt count overflow"))?;
            if *count > 240 {
                return Err(io::Error::other("speaker receipt lineage over-acceptance"));
            }
            if *count == 240 {
                entry.pending = None;
            }
        }
        Ok(())
    }
    pub(crate) fn rejected(&mut self, sequence: u64) {
        for entry in &mut self.entries {
            if entry.pending.is_some_and(|pending| pending.0 == sequence) {
                entry.pending = None;
            }
        }
    }
    pub(crate) fn drained(&mut self, owner: TurnOwner, end: PlaybackCursor) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.owner == owner)
            && entry
                .accepted
                .is_some_and(|cursor| cursor.epoch == end.epoch && cursor.frame <= end.frame)
        {
            entry.accepted = None;
        }
    }
    pub(crate) fn reset(&mut self, previous: QueuedRange) {
        for entry in &mut self.entries {
            if entry.accepted.is_some_and(|cursor| {
                cursor.epoch == previous.epoch && cursor.frame <= previous.accepted_through
            }) {
                entry.accepted = None;
            }
            // A reset is not the command's rejection acknowledgement.
        }
    }
    pub(crate) fn tail(
        &mut self,
        owner: TurnOwner,
        successor: TurnOwner,
        started_at_us: u64,
    ) -> io::Result<()> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.owner == owner)
            .ok_or_else(|| {
                io::Error::other("cancelled tail has no retained cancellation lineage")
            })?;
        if entry.tail_seen
            || entry.cancelled.is_none_or(|cancel| {
                !cancel.user
                    || entry.successor.is_none_or(|(admitted, issued)| {
                        admitted != successor || cancel.at_us > issued || issued > started_at_us
                    })
            })
        {
            return Err(io::Error::other(
                "cancelled tail has no unique matching supersession",
            ));
        }
        entry.tail_seen = true;
        entry.accepted = None;
        Ok(())
    }
    pub(crate) fn discard(&mut self, owner: TurnOwner) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.owner == owner) else {
            return false;
        };
        if entry.discard_seen
            || entry
                .cancelled
                .is_none_or(|cancel| !cancel.expected_discard)
        {
            return false;
        }
        entry.discard_seen = true;
        entry.accepted = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_turn_owner::{
        AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, Permission,
    };
    fn controller() -> Controller {
        let at = MonoTime::from_micros(1);
        let mut controller = Controller::new(BootId::try_from([1; 16]).unwrap(), at);
        controller
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        controller
            .set_capture(
                at,
                CaptureState::RetainingUntil(MonoTime::from_micros(1_000_000)),
            )
            .unwrap();
        controller
            .set_admission(
                at,
                AdmissionState::OpenUntil(MonoTime::from_micros(1_000_000)),
            )
            .unwrap();
        controller
    }
    #[test]
    fn settled_history_rotates_but_pending_cancelled_tail_and_its_successor_do_not_disappear() {
        let mut controller = controller();
        let mut history = ReceiptHistory::default();
        let old = controller
            .admit(MonoTime::from_micros(2), AdmittedInput::NewTurn)
            .unwrap();
        history.admitted(old, 2).unwrap();
        history.dispatched(old, 1).unwrap();
        let cursor = PlaybackCursor {
            epoch: 1,
            frame: 240,
        };
        history.accepted(old, 1, 240, cursor).unwrap();
        controller
            .cancel_turn(MonoTime::from_micros(3), old)
            .unwrap();
        history.cancelled(old, 3, "user_interrupted").unwrap();
        let successor = controller
            .admit(MonoTime::from_micros(4), AdmittedInput::NewTurn)
            .unwrap();
        history.admitted(successor, 4).unwrap();
        controller
            .complete_turn(MonoTime::from_micros(5), successor)
            .unwrap();
        history.completed(successor).unwrap();
        for n in 0..1000 {
            let at = MonoTime::from_micros(6 + n);
            let owner = controller.admit(at, AdmittedInput::NewTurn).unwrap();
            history.admitted(owner, at.as_micros()).unwrap();
            controller.complete_turn(at, owner).unwrap();
            history.completed(owner).unwrap();
            assert!(history.entries.len() <= MAX_OWNERS);
        }
        assert!(!history.entries.iter().any(|entry| entry.owner == successor));
        assert!(
            history
                .entries
                .iter()
                .any(|entry| entry.owner == old && !entry.settled())
        );
        assert!(!history.is_settled());
        history.tail(old, successor, 4).unwrap();
        assert!(history.is_settled());
        assert!(history.tail(old, successor, 4).is_err());
    }
    #[test]
    fn reset_does_not_erase_a_pending_command_rejection_obligation() {
        let mut controller = controller();
        let mut history = ReceiptHistory::default();
        let owner = controller
            .admit(MonoTime::from_micros(2), AdmittedInput::NewTurn)
            .unwrap();
        history.dispatched(owner, 1).unwrap();
        history
            .accepted(
                owner,
                1,
                120,
                PlaybackCursor {
                    epoch: 1,
                    frame: 120,
                },
            )
            .unwrap();
        history.cancelled(owner, 3, "provider_interrupted").unwrap();
        history.reset(QueuedRange {
            epoch: 1,
            retired_through: 0,
            accepted_through: 120,
        });
        assert!(!history.entries[0].settled());
        assert!(!history.is_settled());
        history.rejected(1);
        assert!(history.entries[0].settled());
        assert!(history.discard(owner));
        assert!(!history.discard(owner));
    }
    #[test]
    fn a_local_stop_expects_the_discard_it_causes_and_another_cancel_does_not() {
        for (reason, expected) in [
            ("hand_stop", true),
            ("head_stop", true),
            ("head_sleep", true),
            ("software_muted", true),
            ("capture_gap", true),
            ("reminder_authority_lost", false),
            ("playback_discarded", false),
        ] {
            let mut controller = controller();
            let mut history = ReceiptHistory::default();
            let owner = controller
                .admit(MonoTime::from_micros(2), AdmittedInput::NewTurn)
                .unwrap();
            history.dispatched(owner, 1).unwrap();
            history.cancelled(owner, 3, reason).unwrap();
            assert_eq!(history.discard(owner), expected, "{reason}");
            // Only once, whatever the reason.
            assert!(!history.discard(owner), "{reason}");
        }
    }
    #[test]
    fn unresolved_history_fails_closed_instead_of_eviction() {
        let mut controller = controller();
        let mut history = ReceiptHistory::default();
        for n in 0..MAX_OWNERS {
            let at = MonoTime::from_micros(2 + n as u64);
            let owner = controller.admit(at, AdmittedInput::NewTurn).unwrap();
            history.dispatched(owner, n as u64 + 1).unwrap();
            history
                .cancelled(owner, at.as_micros(), "user_interrupted")
                .unwrap();
        }
        let owner = controller
            .admit(MonoTime::from_micros(100), AdmittedInput::NewTurn)
            .unwrap();
        assert!(history.admitted(owner, 100).is_err());
        assert_eq!(history.entries.len(), MAX_OWNERS);
    }
}
