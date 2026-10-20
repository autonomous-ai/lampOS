//! A skill's search on the second route (look-for-things): which of the
//! lamp's own turns go on with it, so the skill's next look is taken there
//! (`LAMP_SECOND_SKILL_LOOK`), one note to a search whose look got an
//! answer that called nothing (`LAMP_SECOND_LOOK_NOTE`), and, when that is
//! left too, the walk on to the next place (`LAMP_SECOND_LOOK_WALK`). The
//! route says a look's turn, or a search's note turn, was answered with
//! nothing called (`LookNoCall`); this owner knows which turns are a
//! search's and asks once a search, as the holding answer's continuation
//! asks once.
//!
//! The walk: the coordinator calls the search's skill itself, as the model
//! called it, with the next place in [`WALK_ORDER`] it has not asked, on a
//! thread of its own through the voice's own call path
//! (`crate::tools::run_call`), so nothing waits on it. A look in its answer
//! is taken as a skill's look is, and goes as the search's next look turn,
//! naming the place, so the finding is still the model's. The skill's
//! refusal past its looks (its `give_up`) goes once as a written turn, and
//! the search is over. The person speaking, another turn, a session change
//! or a stop by hand ends it at once.

use super::{
    BootId, Controller, PlayoutStart, ProviderInput, Reply, Result, SessionState, Snapshot,
    VecDeque, commit_written_turn, holding, json, record,
};
use crate::provider_worker::SearchCall;
use serde_json::Value;
use std::{
    io,
    sync::{Arc, mpsc},
};

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

/// The places the walk goes on to, in look-for-things' order (its note's
/// and its `say`'s), each once a search: a place already looked at is
/// skipped. The desk comes last, for a search the person began at a place
/// they named; one begun at the desk has already looked there.
pub(super) const WALK_ORDER: [&str; 5] = ["left", "right", "far_left", "far_right", "desk"];

/// The moment is gone if the note cannot begin within this window after the
/// look's answer finished playing, as for the holding answer's.
const WAIT_US: u64 = 3_000_000;

/// Calls a search's skill with the call given (the model's, with the next
/// place) for the turn given, and answers what to tell the model and
/// whether the skill answered (`crate::tools::run_call`). Blocking: run on
/// a thread of its own.
pub(super) type SkillCaller = Arc<dyn Fn(&SearchCall, u64) -> (Value, bool) + Send + Sync>;

/// The voice's own way to a skill: the skills' folders and `use_skill` as
/// the worker is told them (`crate::tools::arguments_from_environment`).
/// Only a skill is called, never the executive's or the voice's own.
pub(super) fn skills_caller() -> SkillCaller {
    let settings = crate::tools::take(&crate::tools::arguments_from_environment())
        .map(|(settings, _)| settings)
        .unwrap_or_default();
    Arc::new(move |call, request| {
        let route = crate::tools::route(&settings, &call.name);
        if !(crate::tools::is_skills(&settings, &route)
            || matches!(route, crate::tools::Route::UseSkill(_)))
        {
            return (crate::tools::unavailable(), false);
        }
        let call = lamp_gemini::ToolCall {
            id: String::new(),
            name: call.name.clone(),
            args: call.args.clone(),
        };
        crate::tools::run_call(&route, &call, request, false).1
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Promise {
    request: u64,
    session: u64,
    worker: BootId,
    settled_at_us: Option<u64>,
    /// A walk step, not the note.
    walk: bool,
}

/// A skill's search: begun by a skill's look in a turn the person spoke,
/// and gone on by looks in the lamp's own turns after it.
#[derive(Clone, Debug, Default)]
struct Search {
    /// Its newest look's turn (`lamp_looks`).
    look_turn: Option<u64>,
    /// Its one note has gone.
    noted: bool,
    /// The looks it has taken.
    looks: u32,
    /// The look's turn left as the lamp's own once it took its looks.
    spent_turn: Option<u64>,
    /// The skill's call that began it, as the model made it: the walk
    /// calls it again with the next place.
    call: Option<SearchCall>,
    /// The places asked in it, by the model or by the walk, in order.
    places: Vec<String>,
    /// The note's turn, whose answer left with no call walks on too.
    note_turn: Option<u64>,
    /// The walk asked again once with no place left in its order, to hear
    /// the skill's word on its looks.
    closing: bool,
    /// Its walk ended for good: dropped, or the places looked at.
    walk_over: bool,
}

/// A walk step's call to the skill, on its thread, and its answer.
struct Step {
    /// The turn whose answer left the search (the look's or the note's).
    turn: u64,
    session: u64,
    worker: BootId,
    answer: mpsc::Receiver<(Value, bool)>,
    /// A give-up line waiting for the room's quiet, and since when.
    give_up: Option<(String, u64)>,
}

/// What `take` makes of the note or walk step owed.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Due {
    /// Nothing owed, or not yet.
    Wait,
    /// Dropped for good, for the turn, why, and whether a walk step.
    Dropped(u64, &'static str, bool),
    /// The note goes now, on this session.
    Go { request: u64, session: u64 },
    /// The walk's next step goes now, on this session.
    Walk { request: u64, session: u64 },
}

/// What a walk step's answer is.
#[derive(Debug, PartialEq)]
pub(super) enum Answer {
    /// A look: taken for the turn, on the session.
    Look { turn: u64, session: u64 },
    /// The skill's give-up line, once.
    GiveUp {
        turn: u64,
        session: u64,
        text: String,
    },
    /// The walk ends here, for the turn, and why.
    Dropped(u64, &'static str),
}

#[derive(Default)]
pub(super) struct LookNote {
    /// `LAMP_SECOND_LOOK_NOTE`: the note. A search is followed without it.
    enabled: bool,
    /// `LAMP_SECOND_LOOK_WALK`, with the note: the walk on.
    walking: bool,
    /// The turn whose skill's look is held, until its look's turn goes.
    held: Option<u64>,
    search: Option<Search>,
    pending: Option<Promise>,
    last_speech_us: u64,
    /// The walk's call in flight, or its give-up line owed.
    step: Option<Step>,
    /// The walk's line for its look turn, by the turn it was asked for.
    walk_line: Option<(u64, String)>,
}

impl LookNote {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// With the walk on (`LAMP_SECOND_LOOK_WALK`): only with the note.
    pub fn with_walk(mut self, on: bool) -> Self {
        self.walking = self.enabled && on;
        self
    }

    /// The second route asked a skill's look for `turn` and it is held. A
    /// turn the person spoke begins a new search, which may have its own
    /// note; a look in the lamp's own turn (a look's or the note's answer
    /// calling the next place, or the walk's) goes on with the search in
    /// hand. `call` is the skill's call that asked it, with its place.
    pub fn asked(&mut self, turn: u64, heard: bool, call: Option<SearchCall>) {
        self.held = Some(turn);
        if heard {
            self.search = Some(Search {
                places: vec![place_of(call.as_ref())],
                call,
                ..Search::default()
            });
            self.pending = None;
            self.step = None;
        } else if let (Some(search), Some(call)) = (self.search.as_mut(), call) {
            search.places.push(place_of(Some(&call)));
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
            .as_ref()
            .is_some_and(|search| search.spent_turn == Some(turn))
    }

    /// Called only for the current owner's validated `LookNoCall`: the
    /// answer to the look's turn (or the note's) `request` called nothing.
    /// The note is owed only for a skill's search's newest look, once a
    /// search; after it, with the walk on, a walk step for that look or
    /// the note; why not, otherwise.
    pub fn answered(
        &mut self,
        request: u64,
        session: u64,
        worker: BootId,
    ) -> std::result::Result<(), &'static str> {
        if !self.enabled {
            return Ok(());
        }
        let Some(search) = self.search.as_ref().filter(|search| {
            search.look_turn == Some(request) || search.note_turn == Some(request)
        }) else {
            return Err("not a skill's search's look");
        };
        // No next place to call for: the search has taken its looks.
        if search.looks >= SEARCH_LOOKS_AT_MOST {
            return Err("the search took its looks");
        }
        let walk = search.noted;
        if walk && !self.walking {
            return Err("its search had its note");
        }
        if walk && search.walk_over {
            return Err("its walk is over");
        }
        self.pending = Some(Promise {
            request,
            session,
            worker,
            settled_at_us: None,
            walk,
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

    /// The answer is over: owed on only if it played through. Dropped, the
    /// trace's kind for it and why.
    pub fn settled(
        &mut self,
        request: u64,
        spoken: bool,
        at_us: u64,
    ) -> Option<(&'static str, &'static str)> {
        let pending = self.pending.as_mut().filter(|p| p.request == request)?;
        if spoken {
            pending.settled_at_us = Some(at_us);
            None
        } else {
            let walk = pending.walk;
            self.pending = None;
            if walk {
                self.end_walk();
            }
            Some((kind(walk), "its answer was not heard whole"))
        }
    }

    /// A stop by hand ends the search, and any note or walk owed with it.
    pub fn clear(&mut self) {
        self.held = None;
        self.search = None;
        self.pending = None;
        self.step = None;
        self.walk_line = None;
    }

    /// The person speaking, another turn, a session or worker change, or a
    /// moment gone drops the note or walk step (a walk's for good); the
    /// quiet and speaker gates (`ready`) may defer it briefly.
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
            if pending.walk {
                self.end_walk();
            }
            return Due::Dropped(pending.request, why, pending.walk);
        }
        if pending.settled_at_us.is_none() || current.is_some() || !ready {
            return Due::Wait;
        }
        self.pending = None;
        if pending.walk {
            return Due::Walk {
                request: pending.request,
                session: pending.session,
            };
        }
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

    /// The note went as `turn`: its answer, left with no call, walks on.
    fn noted_in(&mut self, turn: u64) {
        if let Some(search) = self.search.as_mut() {
            search.note_turn = Some(turn);
        }
    }

    fn end_walk(&mut self) {
        self.step = None;
        if let Some(search) = self.search.as_mut() {
            search.walk_over = true;
        }
    }

    /// The walk's next call: the search's own call with the next place in
    /// [`WALK_ORDER`] not yet asked, or, with none left, once more with its
    /// newest place, which past the skill's looks answers its give-up line.
    /// Why not, otherwise. The place is counted as asked.
    fn next_call(&mut self) -> std::result::Result<(SearchCall, String), &'static str> {
        let Some(search) = self.search.as_mut() else {
            return Err("its search is over");
        };
        let Some(call) = search.call.clone() else {
            return Err("its skill's call is not known");
        };
        let next = WALK_ORDER
            .iter()
            .find(|place| !search.places.iter().any(|asked| asked == *place));
        let place = match next {
            Some(place) => (*place).to_owned(),
            None if !search.closing => {
                search.closing = true;
                search
                    .places
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "desk".into())
            }
            None => return Err("its places are all looked at"),
        };
        search.places.push(place.clone());
        Ok((with_place(&call, &place), place))
    }

    /// Begins the walk's step for the turn `request` left: its call on a
    /// thread of its own. The place asked, or why the walk ends.
    fn walk_on(
        &mut self,
        request: u64,
        session: u64,
        worker: BootId,
        caller: &SkillCaller,
    ) -> std::result::Result<String, &'static str> {
        let (call, place) = match self.next_call() {
            Ok(next) => next,
            Err(why) => {
                self.end_walk();
                return Err(why);
            }
        };
        let (tell, answer) = mpsc::channel();
        let caller = caller.clone();
        let begun = std::thread::Builder::new()
            .name("lamp-voice-look-walk".into())
            .spawn(move || {
                // The step may be gone by now: nobody hears the answer.
                let _ = tell.send(caller(&call, request));
            });
        if begun.is_err() {
            self.end_walk();
            return Err("its call could not begin");
        }
        self.step = Some(Step {
            turn: request,
            session,
            worker,
            answer,
            give_up: None,
        });
        Ok(place)
    }

    /// The walk's step in hand: dropped at once by the person speaking,
    /// another turn, or a session or worker change; its answer once it has
    /// come, a give-up line only once the room is ready for it.
    fn step_answer(
        &mut self,
        at_us: u64,
        current: Option<u64>,
        session: Option<u64>,
        worker: BootId,
        hearing: bool,
        ready: bool,
    ) -> Option<Answer> {
        let step = self.step.as_mut()?;
        let turn = step.turn;
        let why = if worker != step.worker || session != Some(step.session) {
            Some("the session changed")
        } else if hearing {
            Some("the person is speaking")
        } else if current.is_some() {
            Some("another turn began")
        } else {
            None
        };
        if let Some(why) = why {
            self.end_walk();
            return Some(Answer::Dropped(turn, why));
        }
        if step.give_up.is_none() {
            let (told, answered) = match step.answer.try_recv() {
                Ok(answer) => answer,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => (Value::Null, false),
            };
            if !answered {
                self.end_walk();
                return Some(Answer::Dropped(turn, "the skill did not answer"));
            }
            if told["look"] == true {
                let session = step.session;
                self.step = None;
                let place = self
                    .search
                    .as_ref()
                    .and_then(|search| search.places.last().cloned())
                    .unwrap_or_default();
                self.walk_line = Some((turn, walk_line(&place, &told)));
                return Some(Answer::Look { turn, session });
            }
            let Some(error) = told["error"].as_str() else {
                self.end_walk();
                return Some(Answer::Dropped(turn, "the skill's answer took no look"));
            };
            step.give_up = Some((error.to_owned(), at_us));
        }
        let (text, since) = step.give_up.clone()?;
        if at_us.saturating_sub(since) >= WAIT_US {
            self.end_walk();
            return Some(Answer::Dropped(turn, "the moment passed"));
        }
        if !ready {
            return None;
        }
        let session = step.session;
        // Once: the search is over with its give-up line.
        self.clear();
        Some(Answer::GiveUp {
            turn,
            session,
            text,
        })
    }

    /// The walk's line for the look turn of the step asked for `turn`, once.
    pub fn walk_line(&mut self, turn: u64) -> Option<String> {
        self.walk_line
            .take_if(|(of, _)| *of == turn)
            .map(|(_, line)| line)
    }
}

/// The trace's kind for a dropped note or walk step.
pub(super) fn kind(walk: bool) -> &'static str {
    if walk {
        "look_walk_dropped"
    } else {
        "look_note_dropped"
    }
}

/// The place a search's call asked: its `place` (or `use_skill`'s
/// `choice`), or the desk, look-for-things' default.
fn place_of(call: Option<&SearchCall>) -> String {
    call.and_then(|call| {
        call.args
            .get("place")
            .or_else(|| call.args.get("choice"))
            .and_then(Value::as_str)
    })
    .unwrap_or("desk")
    .to_owned()
}

/// The search's call with another place: in its `place`, or, called
/// through `use_skill` without it, its `choice`.
fn with_place(call: &SearchCall, place: &str) -> SearchCall {
    let mut args = match &call.args {
        Value::Object(args) => args.clone(),
        _ => serde_json::Map::new(),
    };
    let key = if call.name == crate::use_skill::NAME && !args.contains_key("place") {
        "choice"
    } else {
        "place"
    };
    args.insert(key.to_owned(), Value::from(place));
    SearchCall {
        name: call.name.clone(),
        args: Value::Object(args),
    }
}

/// What the walk's look turn adds after the look's question: the place it
/// looked, and the skill's note for its picture, as the second route adds
/// it to a look the model asked for.
fn walk_line(place: &str, told: &Value) -> String {
    let toward = match place {
        "left" => "to the left",
        "right" => "to the right",
        "far_left" => "far to the left",
        "far_right" => "far to the right",
        _ => "at the desk",
    };
    let mut line = format!("\n\n(You looked {toward}; the picture is that look.)");
    if let Some(note) = crate::second_provider::picture_note(told) {
        line.push_str(crate::second_provider::PICTURE_NOTE_BEFORE);
        line.push_str(&note);
        line.push_str(crate::second_provider::PICTURE_NOTE_AFTER);
    }
    line
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

/// What the live loop's step did.
pub(super) enum Started {
    Nothing,
    /// A written turn went: the note, or the walk's give-up line.
    Turn(Snapshot),
    /// The walk's skill answered with a look, for the turn, on the session:
    /// the loop takes it as a skill's look.
    Look {
        turn: u64,
        session: u64,
    },
}

/// The live loop's step: the note as a written turn on the session that
/// answered the look, once the look's answer has played and the room has
/// been quiet a second, under the holding answer's gates; after it, under
/// the same gates, the walk's next call, and its answer when it comes.
#[allow(clippy::too_many_arguments)]
pub(super) fn start(
    state: &mut LookNote,
    gates: holding::Gates<'_>,
    caller: &SkillCaller,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    playout: PlayoutStart,
    trace: &mut SessionState,
    publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Started> {
    let ready = holding::ready(&gates, queue.is_empty(), state.last_speech_us);
    let current = reply.as_ref().map(|current| current.owner.turn());
    let hearing = gates.admission.owns_stream();
    if let Some(answer) = state.step_answer(
        gates.at_us,
        current,
        gates.session,
        gates.worker,
        hearing,
        ready,
    ) {
        return match answer {
            Answer::Look { turn, session } => Ok(Started::Look { turn, session }),
            Answer::Dropped(turn, why) => {
                record(
                    trace,
                    json!({"kind":"look_walk_dropped","turn":turn,"why":why,"at_us":gates.at_us}),
                )?;
                Ok(Started::Nothing)
            }
            Answer::GiveUp {
                turn,
                session,
                text,
            } => {
                let snapshot = commit_written_turn(
                    owner,
                    reply,
                    queue,
                    &text,
                    false,
                    "look_walk_gave_up",
                    session,
                    playout,
                    trace,
                    publish,
                )?;
                let current = reply.as_mut().expect("written turn was committed");
                current.cloud.same_session_only = true;
                record(
                    trace,
                    json!({"kind":"look_walk_gave_up","turn":turn,
                    "next_turn":current.owner.turn(),"provider_session":session,
                    "at_us":gates.at_us,"boundary":"give-up line queued; not audible delivery"}),
                )?;
                Ok(Started::Turn(snapshot))
            }
        };
    }
    match state.take(
        gates.at_us,
        current,
        gates.session,
        gates.worker,
        hearing,
        ready,
    ) {
        Due::Wait => Ok(Started::Nothing),
        Due::Dropped(turn, why, walk) => {
            record(
                trace,
                json!({"kind":kind(walk),"turn":turn,"why":why,"at_us":gates.at_us}),
            )?;
            Ok(Started::Nothing)
        }
        Due::Walk { request, session } => {
            let asked = state.walk_on(request, session, gates.worker, caller);
            match asked {
                Ok(place) => record(
                    trace,
                    json!({"kind":"look_walk_asked","turn":request,"place":place,"at_us":gates.at_us}),
                )?,
                Err(why) => record(
                    trace,
                    json!({"kind":"look_walk_dropped","turn":request,"why":why,"at_us":gates.at_us}),
                )?,
            }
            Ok(Started::Nothing)
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
            // asks for is looked at, and with the walk on, its answer left
            // with no call walks on.
            mark_search(queue, next);
            state.noted_in(next);
            record(
                trace,
                json!({"kind":"look_note_sent","turn":request,"next_turn":next,
                "provider_session":session,"at_us":gates.at_us,
                "boundary":"note queued; not audible delivery"}),
            )?;
            Ok(Started::Turn(snapshot))
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
        state.asked(1, true, None);
        state.looked(1, 2);
        assert_eq!(state.answered(2, 7, worker()), Ok(()));
        assert_eq!(state.settled(2, true, 2_000_000), None);
        state
    }

    #[test]
    fn one_note_a_search_after_its_look_s_answer_has_played() {
        let mut state = LookNote::new(true);
        state.asked(1, true, None);
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
        state.asked(3, false, None);
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
        state.asked(5, true, None);
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
        off.asked(1, true, None);
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
        state.asked(1, true, None);
        state.looked(1, 2);
        assert_eq!(state.answered(2, 7, worker()), Ok(()));
        assert_eq!(
            state.settled(2, false, 2_000_000),
            Some(("look_note_dropped", "its answer was not heard whole"))
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
                Due::Dropped(2, why, false)
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
            Due::Dropped(2, "the session changed", false)
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
            Due::Dropped(2, "the moment passed", false)
        );
    }

    fn look_for(args: Value) -> SearchCall {
        SearchCall {
            name: "look_for".into(),
            args,
        }
    }

    #[test]
    fn the_walk_goes_on_to_places_not_yet_asked_and_asks_once_past_them() {
        let mut state = LookNote::new(true).with_walk(true);
        state.asked(1, true, Some(look_for(json!({"place": "left"}))));
        let mut places = Vec::new();
        while let Ok((call, place)) = state.next_call() {
            assert_eq!(call.args["place"], place.as_str());
            places.push(place);
        }
        // The desk last, then once more with the newest place, for the
        // skill's word on its looks.
        assert_eq!(places, ["right", "far_left", "far_right", "desk", "desk"]);
        assert_eq!(state.next_call(), Err("its places are all looked at"));
        // Begun at the desk (no place named), through `use_skill`.
        let mut state = LookNote::new(true).with_walk(true);
        state.asked(
            1,
            true,
            Some(SearchCall {
                name: crate::use_skill::NAME.into(),
                args: json!({"skill": "look_for"}),
            }),
        );
        let (call, place) = state.next_call().unwrap();
        assert_eq!(place, "left");
        assert_eq!(call.args, json!({"skill": "look_for", "choice": "left"}));
        // The walk needs the note.
        assert!(!LookNote::new(false).with_walk(true).walking);
    }

    /// A walk step's answer waits for it, and is dropped at once by the
    /// person speaking; a give-up line goes once, and ends the search.
    #[test]
    fn a_walk_step_s_give_up_goes_once_and_speech_drops_it() {
        let give_up = |answer: Value| {
            let mut state = LookNote::new(true).with_walk(true);
            state.asked(1, true, Some(look_for(json!({}))));
            state.looked(1, 2);
            // Its note has gone: a look left now walks on.
            state.search.as_mut().unwrap().noted = true;
            let skill: SkillCaller = Arc::new(move |_, _| (answer.clone(), true));
            assert!(state.walk_on(2, 7, worker(), &skill).is_ok());
            state
        };
        let wait = |state: &mut LookNote, at, hearing, ready| loop {
            match state.step_answer(at, None, Some(7), worker(), hearing, ready) {
                None if state.step.as_ref().is_some_and(|s| s.give_up.is_none()) => {
                    std::thread::yield_now()
                }
                answer => break answer,
            }
        };
        let mut state = give_up(json!({"error": "Let it go."}));
        // Not ready yet: the line waits.
        assert_eq!(wait(&mut state, 1_000_000, false, false), None);
        assert_eq!(
            wait(&mut state, 1_500_000, false, true),
            Some(Answer::GiveUp {
                turn: 2,
                session: 7,
                text: "Let it go.".into()
            })
        );
        assert_eq!(wait(&mut state, 1_600_000, false, true), None);
        assert!(state.search.is_none());
        // The person speaking drops the step, and the walk is over.
        let mut state = give_up(json!({"error": "Let it go."}));
        assert_eq!(
            wait(&mut state, 1_000_000, true, true),
            Some(Answer::Dropped(2, "the person is speaking"))
        );
        assert!(state.search.as_ref().unwrap().walk_over);
        assert_eq!(state.answered(2, 7, worker()), Err("its walk is over"));
        // A look: its line names the place and carries the skill's note.
        let mut state =
            give_up(json!({"next": "Look. When the picture comes: say it.", "look": true}));
        assert_eq!(
            wait(&mut state, 1_000_000, false, false),
            Some(Answer::Look {
                turn: 2,
                session: 7
            })
        );
        let line = state.walk_line(2).unwrap();
        assert!(line.contains("(You looked to the left; the picture is that look.)"));
        assert!(line.contains("say it."));
        assert_eq!(state.walk_line(2), None);
    }
}
