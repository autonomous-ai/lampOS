//! One continuation of a spoken holding-only answer, on the same session.
//! The provider classifies the words; this owner decides whether it may ask.

use super::{
    BootId, Controller, InputAdmission, MonoTime, PlayoutStart, ProviderInput, Reply, Result,
    SessionState, Snapshot, VecDeque, commit_written_turn, json, record, reminder_quiet,
};
use crate::turn_taking::PlaybackMask;
use std::io;

/// The moment is gone if the continuation cannot begin within this window
/// after the prior reply's final speaker receipt, as for the second look.
const WAIT_US: u64 = 3_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Promise {
    pub request: u64,
    pub session: u64,
    worker: BootId,
    settled_at_us: Option<u64>,
}

#[derive(Default)]
pub(super) struct Continuation {
    enabled: bool,
    pending: Option<Promise>,
    last_offered: u64,
    nudged_turn: Option<u64>,
    last_speech_us: u64,
}

impl Continuation {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// Called only for the current owner's validated provider event. A
    /// repeated or late classification cannot resurrect a spent request.
    pub fn offer(&mut self, request: u64, session: u64, worker: BootId) {
        if !self.enabled || request <= self.last_offered || session == 0 {
            return;
        }
        self.last_offered = request;
        if self.nudged_turn == Some(request) {
            return;
        }
        self.pending = Some(Promise {
            request,
            session,
            worker,
            settled_at_us: None,
        });
    }

    /// Only a reply to heard input can extend the conversation. An
    /// automatic reminder, skill, look or news turn keeps its own limits.
    pub fn observed(
        &mut self,
        request: u64,
        session: u64,
        worker: BootId,
        current: Option<&Reply>,
    ) {
        if current.is_some_and(|current| {
            current.owner.turn() == request
                && current.cloud.session == session
                && current.input_sequence > 0
                && current.reminder.is_none()
                && current.say.is_none()
        }) {
            self.offer(request, session, worker);
        }
    }

    /// Read the validated frame itself, before busy-room or faint-sound
    /// admission rules can replace its probability with silence. Own voice
    /// may delay a continuation too; it must not hide a person's last word.
    pub fn observe_capture(&mut self, frame: &crate::wire::MicrophoneFrame) {
        if self.enabled && frame.probability.is_finite() && frame.probability >= 0.5 {
            self.last_speech_us = self.last_speech_us.max(frame.read_completed_at_us);
        }
    }

    /// Provider completion alone is insufficient: the reply must finish
    /// successfully through the speaker's normal retirement join.
    pub fn settled(&mut self, request: u64, spoken: bool, at_us: u64) {
        if let Some(pending) = self.pending.as_mut().filter(|p| p.request == request) {
            if spoken {
                pending.settled_at_us = Some(at_us);
            } else {
                self.pending = None;
            }
        }
    }

    pub fn clear(&mut self) {
        self.pending = None;
    }

    /// A different turn, session loss/change, or an expired opportunity
    /// drops the promise. Ordinary busy/quiet gates may defer it briefly.
    pub fn take(
        &mut self,
        at_us: u64,
        current: Option<u64>,
        session: Option<u64>,
        worker: BootId,
        ready: bool,
    ) -> Option<Promise> {
        let pending = self.pending?;
        if worker != pending.worker
            || session != Some(pending.session)
            || current.is_some_and(|turn| turn != pending.request)
            || pending.settled_at_us.is_none() && current.is_none()
            || pending
                .settled_at_us
                .is_some_and(|at| at_us.checked_sub(at).is_none_or(|age| age >= WAIT_US))
        {
            self.clear();
            return None;
        }
        if pending.settled_at_us.is_some() && current.is_none() && ready {
            self.pending.take()
        } else {
            None
        }
    }

    pub fn started(&mut self, turn: u64) {
        self.nudged_turn = Some(turn);
    }
}

pub(super) struct Gates<'a> {
    pub at_us: u64,
    pub cycle_started_us: u64,
    pub last_speech_us: u64,
    pub last_capture_us: Option<u64>,
    pub session: Option<u64>,
    pub worker: BootId,
    pub snapshot: &'a Snapshot,
    pub admission: &'a InputAdmission,
    pub playback: &'a PlaybackMask,
    pub speaker_pending: bool,
    pub submission_draining: bool,
}

/// Shared by the live loop and the stepped owner tests: no new output may
/// pass a pending input, stale capture, privacy boundary, or speaker tail.
#[allow(clippy::too_many_arguments)]
pub(super) fn start(
    state: &mut Continuation,
    gates: Gates<'_>,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    playout: PlayoutStart,
    trace: &mut SessionState,
    publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Option<Snapshot>> {
    let ready = queue.is_empty()
        && !gates.speaker_pending
        && gates.admission.idle_for_reminder()
        && !gates.submission_draining
        && gates
            .snapshot
            .listening_ready(MonoTime::from_micros(gates.at_us))
        && reminder_quiet(
            gates.at_us,
            gates.cycle_started_us,
            gates.last_speech_us.max(state.last_speech_us),
            gates.last_capture_us,
        )
        && gates.playback.available(gates.at_us);
    let Some(promise) = state.take(
        gates.at_us,
        reply.as_ref().map(|current| current.owner.turn()),
        gates.session,
        gates.worker,
        ready,
    ) else {
        return Ok(None);
    };
    let snapshot = commit_written_turn(
        owner,
        reply,
        queue,
        crate::holding_answer::NOTE,
        false,
        "holding_answer",
        promise.session,
        playout,
        trace,
        publish,
    )?;
    let current = reply.as_mut().expect("written turn was committed");
    current.cloud.same_session_only = true;
    let next = current.owner.turn();
    state.started(next);
    record(
        trace,
        json!({"kind":"holding_answer_nudged","turn":promise.request,
        "next_turn":next,"provider_session":promise.session,"at_us":gates.at_us,
        "boundary":"continuation queued; not audible delivery"}),
    )?;
    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker() -> BootId {
        BootId::new([1; 16]).unwrap()
    }

    fn owed() -> Continuation {
        let mut state = Continuation::new(true);
        state.offer(1, 7, worker());
        state.settled(1, true, 2_000_000);
        state
    }

    #[test]
    fn one_nudge_after_spoken_settlement_and_never_two_in_a_row() {
        let mut state = Continuation::new(true);
        state.offer(1, 7, worker());
        assert!(
            state
                .take(1_000_000, Some(1), Some(7), worker(), true)
                .is_none()
        );
        state.settled(1, true, 2_000_000);
        assert_eq!(
            state
                .take(2_000_000, None, Some(7), worker(), true)
                .unwrap()
                .request,
            1
        );
        assert!(
            state
                .take(2_000_001, None, Some(7), worker(), true)
                .is_none()
        );
        state.started(2);
        state.offer(2, 7, worker());
        state.settled(2, true, 2_100_000);
        assert!(
            state
                .take(2_100_000, None, Some(7), worker(), true)
                .is_none()
        );
        state.offer(3, 7, worker());
        state.settled(3, true, 2_200_000);
        assert_eq!(
            state
                .take(2_200_000, None, Some(7), worker(), true)
                .unwrap()
                .request,
            3
        );
    }

    #[test]
    fn switch_off_unsaid_or_cancelled_reply_never_gets_a_continuation() {
        let mut off = Continuation::new(false);
        off.offer(1, 7, worker());
        off.settled(1, true, 2_000_000);
        assert!(off.take(2_000_000, None, Some(7), worker(), true).is_none());
        for spoken in [false, true] {
            let mut state = Continuation::new(true);
            state.offer(1, 7, worker());
            if !spoken {
                state.settled(1, false, 2_000_000);
            }
            assert!(
                state
                    .take(2_000_000, None, Some(7), worker(), true)
                    .is_none()
            );
            state.offer(1, 7, worker());
            state.settled(1, true, 2_000_001);
            assert!(
                state
                    .take(2_000_001, None, Some(7), worker(), true)
                    .is_none()
            );
        }
    }

    #[test]
    fn new_turn_lost_or_replaced_session_and_hand_stop_drop_the_promise() {
        for (turn, session) in [(Some(2), Some(7)), (None, None), (None, Some(8))] {
            let mut state = owed();
            assert!(
                state
                    .take(2_000_000, turn, session, worker(), true)
                    .is_none()
            );
            assert!(
                state
                    .take(2_000_001, None, Some(7), worker(), true)
                    .is_none()
            );
        }
        let mut state = owed();
        state.clear();
        assert!(
            state
                .take(2_000_000, None, Some(7), worker(), true)
                .is_none()
        );
    }

    #[test]
    fn another_worker_with_the_same_session_number_cannot_take_the_promise() {
        let mut state = owed();
        let replacement = BootId::new([2; 16]).unwrap();
        assert!(
            state
                .take(2_000_000, None, Some(7), replacement, true)
                .is_none()
        );
        // Returning to the original worker cannot revive the opportunity.
        assert!(
            state
                .take(2_000_001, None, Some(7), worker(), true)
                .is_none()
        );
    }

    #[test]
    fn quiet_and_readiness_wait_is_bounded() {
        let mut state = owed();
        assert!(
            state
                .take(2_000_000, None, Some(7), worker(), false)
                .is_none()
        );
        assert!(
            state
                .take(2_999_999, None, Some(7), worker(), false)
                .is_none()
        );
        assert!(
            state
                .take(3_000_000, None, Some(7), worker(), true)
                .is_some()
        );
        let mut state = owed();
        assert!(
            state
                .take(5_000_000, None, Some(7), worker(), true)
                .is_none()
        );
        assert!(
            state
                .take(5_000_001, None, Some(7), worker(), true)
                .is_none()
        );
    }
}
