//! A skill's search on the second route (look-for-things): which of the
//! lamp's own turns go on with it, so the skill's next look is taken there
//! (`LAMP_SECOND_SKILL_LOOK`), and one note to a search whose look got an
//! answer that called nothing (`LAMP_SECOND_LOOK_NOTE`). The route says a
//! look's turn was answered with nothing called (`LookNoCall`); this owner
//! knows which look turns are a search's and asks once a search, as the
//! holding answer's continuation asks once.

use super::{
    BootId, Controller, PlayoutStart, ProviderInput, Reply, Result, SessionState, Snapshot,
    VecDeque, commit_written_turn, holding, json, record,
};
use std::io;

/// The note, written to the second route as the person's turn. It says
/// what to do whether the thing was seen or not, so it holds once a found
/// place is added to `look_for` too.
pub(super) const NOTE: &str = "Not found yet? Call look_for with the next place: left, right, far_left, then far_right. If you can see it, say where and stop.";

/// The most looks one search takes, its first included: past them its look
/// turns are no longer its own, so a look asked there is dropped as in any
/// turn the lamp wrote (#1146). It mirrors lamp-skills' bound on a skill's
/// `[search]` looks (`SEARCH_LOOKS_AT_MOST`, "1 to 8", in
/// `crates/skills/src/lib.rs` on lamp-voice): a backstop for an older
/// lamp-skills, or a skill answering `"look": true` with no `[search]`,
/// that would otherwise chain pictures with nobody speaking.
pub(super) const SEARCH_LOOKS_AT_MOST: u32 = 8;

/// The moment is gone if the note cannot begin within this window after the
/// look's answer finished playing, as for the holding answer's.
const WAIT_US: u64 = 3_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Promise {
    request: u64,
    session: u64,
    worker: BootId,
    settled_at_us: Option<u64>,
}

/// A skill's search: begun by a skill's look in a turn the person spoke,
/// and gone on by looks in the lamp's own turns after it.
#[derive(Clone, Copy, Debug, Default)]
struct Search {
    /// Its newest look's turn (`lamp_looks`).
    look_turn: Option<u64>,
    /// Its one note has gone.
    noted: bool,
    /// The looks it has taken.
    looks: u32,
    /// The look's turn left as the lamp's own once it took its looks.
    spent_turn: Option<u64>,
}

/// What `take` makes of the note owed.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Due {
    /// Nothing owed, or not yet.
    Wait,
    /// Dropped for good, for the look's turn, and why.
    Dropped(u64, &'static str),
    /// Goes now, on this session.
    Go { request: u64, session: u64 },
}

#[derive(Default)]
pub(super) struct LookNote {
    /// `LAMP_SECOND_LOOK_NOTE`: the note. A search is followed without it.
    enabled: bool,
    /// The turn whose skill's look is held, until its look's turn goes.
    held: Option<u64>,
    search: Option<Search>,
    pending: Option<Promise>,
    last_speech_us: u64,
}

impl LookNote {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// The second route asked a skill's look for `turn` and it is held. A
    /// turn the person spoke begins a new search, which may have its own
    /// note; a look in the lamp's own turn (a look's or the note's answer
    /// calling the next place) goes on with the search in hand.
    pub fn asked(&mut self, turn: u64, heard: bool) {
        self.held = Some(turn);
        if heard {
            self.search = Some(Search::default());
            self.pending = None;
        }
    }

    /// The held look of `of` went as the look's turn `look_turn`: whether
    /// that turn goes on with a search. Its last look's turn
    /// ([`SEARCH_LOOKS_AT_MOST`]) does not: a look asked there is dropped.
    pub fn looked(&mut self, of: u64, look_turn: u64) -> bool {
        if self.held.take_if(|held| *held == of).is_some()
            && let Some(search) = self.search.as_mut()
        {
            search.look_turn = Some(look_turn);
            search.looks += 1;
            if search.looks >= SEARCH_LOOKS_AT_MOST {
                search.spent_turn = Some(look_turn);
                return false;
            }
            return true;
        }
        false
    }

    /// Whether `turn` is a search's look's turn left as the lamp's own
    /// because the search took its looks: a look dropped there says so.
    pub fn took_its_looks(&self, turn: u64) -> bool {
        self.search
            .is_some_and(|search| search.spent_turn == Some(turn))
    }

    /// Called only for the current owner's validated `LookNoCall`: the
    /// answer to the look's turn `request` called nothing. Owed only for a
    /// skill's search's newest look, once a search; why not, otherwise.
    pub fn answered(
        &mut self,
        request: u64,
        session: u64,
        worker: BootId,
    ) -> std::result::Result<(), &'static str> {
        if !self.enabled {
            return Ok(());
        }
        let Some(search) = self
            .search
            .filter(|search| search.look_turn == Some(request))
        else {
            return Err("not a skill's search's look");
        };
        if search.noted {
            return Err("its search had its note");
        }
        // No next place to call for: the search has taken its looks.
        if search.looks >= SEARCH_LOOKS_AT_MOST {
            return Err("the search took its looks");
        }
        self.pending = Some(Promise {
            request,
            session,
            worker,
            settled_at_us: None,
        });
        Ok(())
    }

    /// As the holding answer's: the lamp's own voice may delay the note,
    /// it must not hide a person's last word.
    pub fn observe_capture(&mut self, frame: &crate::wire::MicrophoneFrame) {
        if self.enabled && frame.probability.is_finite() && frame.probability >= 0.5 {
            self.last_speech_us = self.last_speech_us.max(frame.read_completed_at_us);
        }
    }

    /// The look's answer is over: owed on only if it played through.
    pub fn settled(&mut self, request: u64, spoken: bool, at_us: u64) -> Option<&'static str> {
        let pending = self.pending.as_mut().filter(|p| p.request == request)?;
        if spoken {
            pending.settled_at_us = Some(at_us);
            None
        } else {
            self.pending = None;
            Some("its answer was not heard whole")
        }
    }

    /// A stop by hand ends the search, and any note owed with it.
    pub fn clear(&mut self) {
        self.held = None;
        self.search = None;
        self.pending = None;
    }

    /// The person speaking, another turn, a session or worker change, or a
    /// moment gone drops the note; the quiet and speaker gates (`ready`)
    /// may defer it briefly.
    pub fn take(
        &mut self,
        at_us: u64,
        current: Option<u64>,
        session: Option<u64>,
        worker: BootId,
        hearing: bool,
        ready: bool,
    ) -> Due {
        let Some(pending) = self.pending else {
            return Due::Wait;
        };
        let why = if worker != pending.worker || session != Some(pending.session) {
            Some("the session changed")
        } else if hearing {
            Some("the person is speaking")
        } else if current.is_some_and(|turn| turn != pending.request) {
            Some("another turn began")
        } else if pending.settled_at_us.is_none() && current.is_none() {
            Some("its answer was not heard whole")
        } else if pending
            .settled_at_us
            .is_some_and(|at| at_us.checked_sub(at).is_none_or(|age| age >= WAIT_US))
        {
            Some("the moment passed")
        } else {
            None
        };
        if let Some(why) = why {
            self.pending = None;
            return Due::Dropped(pending.request, why);
        }
        if pending.settled_at_us.is_none() || current.is_some() || !ready {
            return Due::Wait;
        }
        self.pending = None;
        // Once a search: a later look of it that the model leaves is not
        // noted again.
        if let Some(search) = self.search.as_mut() {
            search.noted = true;
        }
        Due::Go {
            request: pending.request,
            session: pending.session,
        }
    }
}

/// Marks the written turn `turn` just queued as one going on with a skill's
/// search: the second route takes the skill's next look asked in it, which
/// in every other turn the lamp wrote it drops (#1146).
pub(super) fn mark_search(queue: &mut VecDeque<ProviderInput>, turn: u64) {
    if let Some(ProviderInput::Written { search, .. }) = queue
        .iter_mut()
        .rev()
        .find(|input| matches!(input, ProviderInput::Written { request, .. } if *request == turn))
    {
        *search = true;
    }
}

/// The live loop's step: the note as a written turn on the session that
/// answered the look, once the look's answer has played and the room has
/// been quiet a second, under the holding answer's gates.
#[allow(clippy::too_many_arguments)]
pub(super) fn start(
    state: &mut LookNote,
    gates: holding::Gates<'_>,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    playout: PlayoutStart,
    trace: &mut SessionState,
    publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Option<Snapshot>> {
    let ready = holding::ready(&gates, queue.is_empty(), state.last_speech_us);
    match state.take(
        gates.at_us,
        reply.as_ref().map(|current| current.owner.turn()),
        gates.session,
        gates.worker,
        gates.admission.owns_stream(),
        ready,
    ) {
        Due::Wait => Ok(None),
        Due::Dropped(turn, why) => {
            record(
                trace,
                json!({"kind":"look_note_dropped","turn":turn,"why":why,"at_us":gates.at_us}),
            )?;
            Ok(None)
        }
        Due::Go { request, session } => {
            let snapshot = commit_written_turn(
                owner,
                reply,
                queue,
                NOTE,
                false,
                "look_note",
                session,
                playout,
                trace,
                publish,
            )?;
            let current = reply.as_mut().expect("written turn was committed");
            current.cloud.same_session_only = true;
            let next = current.owner.turn();
            // The note's turn goes on with the search: the next place it
            // asks for is looked at.
            mark_search(queue, next);
            record(
                trace,
                json!({"kind":"look_note_sent","turn":request,"next_turn":next,
                "provider_session":session,"at_us":gates.at_us,
                "boundary":"note queued; not audible delivery"}),
            )?;
            Ok(Some(snapshot))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker() -> BootId {
        BootId::new([1; 16]).unwrap()
    }

    /// A search begun by turn 1, its look's turn 2 answered with nothing
    /// called and played through at 2 s.
    fn owed() -> LookNote {
        let mut state = LookNote::new(true);
        state.asked(1, true);
        state.looked(1, 2);
        assert_eq!(state.answered(2, 7, worker()), Ok(()));
        assert_eq!(state.settled(2, true, 2_000_000), None);
        state
    }

    #[test]
    fn one_note_a_search_after_its_look_s_answer_has_played() {
        let mut state = LookNote::new(true);
        state.asked(1, true);
        state.looked(1, 2);
        assert_eq!(state.answered(2, 7, worker()), Ok(()));
        // Still playing: not yet.
        assert_eq!(
            state.take(1_000_000, Some(2), Some(7), worker(), false, true),
            Due::Wait
        );
        state.settled(2, true, 2_000_000);
        // Not quiet yet: waits.
        assert_eq!(
            state.take(2_000_000, None, Some(7), worker(), false, false),
            Due::Wait
        );
        assert_eq!(
            state.take(2_500_000, None, Some(7), worker(), false, true),
            Due::Go {
                request: 2,
                session: 7
            }
        );
        // The note's answer calls the next place (turn 3 is the note's),
        // and that look's answer calls nothing: the search had its note.
        state.asked(3, false);
        state.looked(3, 4);
        assert_eq!(
            state.answered(4, 7, worker()),
            Err("its search had its note")
        );
        assert_eq!(
            state.take(4_000_000, None, Some(7), worker(), false, true),
            Due::Wait
        );
        // A new search, asked by the person: its own one note.
        state.asked(5, true);
        state.looked(5, 6);
        assert_eq!(state.answered(6, 7, worker()), Ok(()));
        state.settled(6, true, 5_000_000);
        assert_eq!(
            state.take(5_000_000, None, Some(7), worker(), false, true),
            Due::Go {
                request: 6,
                session: 7
            }
        );
    }

    #[test]
    fn switch_off_an_own_look_and_an_unplayed_answer_get_no_note() {
        let mut off = LookNote::new(false);
        off.asked(1, true);
        off.looked(1, 2);
        assert_eq!(off.answered(2, 7, worker()), Ok(()));
        off.settled(2, true, 2_000_000);
        assert_eq!(
            off.take(2_000_000, None, Some(7), worker(), false, true),
            Due::Wait
        );
        // A look no skill asked (`take_a_look`) is no search's.
        let mut state = LookNote::new(true);
        assert_eq!(
            state.answered(2, 7, worker()),
            Err("not a skill's search's look")
        );
        // Cut short: dropped, and not owed again.
        let mut state = LookNote::new(true);
        state.asked(1, true);
        state.looked(1, 2);
        assert_eq!(state.answered(2, 7, worker()), Ok(()));
        assert_eq!(
            state.settled(2, false, 2_000_000),
            Some("its answer was not heard whole")
        );
        assert_eq!(
            state.take(2_000_000, None, Some(7), worker(), false, true),
            Due::Wait
        );
    }

    #[test]
    fn speaking_another_turn_a_lost_session_and_a_hand_stop_drop_the_note() {
        for (turn, session, hearing, why) in [
            (None, Some(7), true, "the person is speaking"),
            (Some(3), Some(7), false, "another turn began"),
            (None, Some(8), false, "the session changed"),
            (None, None, false, "the session changed"),
        ] {
            let mut state = owed();
            assert_eq!(
                state.take(2_000_000, turn, session, worker(), hearing, true),
                Due::Dropped(2, why)
            );
            assert_eq!(
                state.take(2_000_001, None, Some(7), worker(), false, true),
                Due::Wait
            );
        }
        let mut state = owed();
        assert_eq!(
            state.take(
                2_000_000,
                None,
                Some(7),
                BootId::new([2; 16]).unwrap(),
                false,
                true
            ),
            Due::Dropped(2, "the session changed")
        );
        let mut state = owed();
        state.clear();
        assert_eq!(
            state.take(2_000_000, None, Some(7), worker(), false, true),
            Due::Wait
        );
        // Waiting for quiet is bounded.
        let mut state = owed();
        assert_eq!(
            state.take(4_999_999, None, Some(7), worker(), false, false),
            Due::Wait
        );
        assert_eq!(
            state.take(5_000_000, None, Some(7), worker(), false, true),
            Due::Dropped(2, "the moment passed")
        );
    }
}
