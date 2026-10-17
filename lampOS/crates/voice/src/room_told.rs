//! How the room is, told to the voice.
//!
//! In an office that is talking, a lamp that answers whatever seems meant
//! for it answers colleagues: about thirty times in fifty minutes on
//! lamp-4ace on 2026-10-13. So its character answers only when called. In a
//! room where one person is left, the same rule makes the lamp deaf to
//! them: they speak, it says nothing, and they have to learn to say its
//! name first. A person at the next desk does neither. They join in less
//! the more talk there is around them.
//!
//! The provider worker sees every request and whether the lamp spoke in it.
//! From that alone:
//!
//! - **quiet**: nothing has been said for five minutes. What is said next
//!   is said into a quiet room, most likely to the lamp. A bump or a click
//!   that the coordinator dropped as a sound is not something said and does
//!   not start the five minutes again: on lamp-4ace, the evening the rule
//!   was first installed, two such sounds in six minutes kept the room
//!   from ever being quiet.
//! - **busy**: three things in a row, inside a minute, that the lamp said
//!   nothing to. People are talking to each other. Only something said
//!   through to its end counts: a cough or a click that the coordinator
//!   dropped as a sound is not talk. A quiet room needs only two inside a
//!   minute: on lamp-4ace on 2026-10-14 a meeting began beside it at 11:09
//!   in a room that had been quiet since 10:50; two things were left
//!   unanswered 54 s apart, the third came four minutes later, and in
//!   between, still taken for quiet, the lamp answered someone at the
//!   table.
//!
//! With `LAMP_SPOKEN_TO=on` (#829) only talk between people counts: a
//! request whose answer the lamp's own rule held is not talk, busy ends
//! three minutes after the last talk, whatever one person says meanwhile,
//! and a room not yet known is quiet after five minutes with no talk.
//! Two things left unanswered as talk since the lamp last spoke end a
//! conversation with it ([`RoomTold::talked_past_the_lamp`]).
//! On lamp-4ace on 16 October at 02:30, one person alone, right after a
//! two-person scene had left the room busy, had five requests held in a
//! row; each counted as more talk, so the five quiet minutes never came
//! and nothing more they asked was answered.
//!
//! Busy holds until the five quiet minutes come; quiet holds until the run.
//! Neither is known when the service starts. The session is told when the
//! room changes, and a new session is told how it is: in a turn that asks
//! for no answer when nothing has been heard or said for a moment
//! ([`crate::session_fact`] has how that is sent and what was tried), or
//! else inside the next request, beside what is being said, as the
//! lamp's name is ([`crate::called`]). A meeting leaves no quiet moment:
//! that same morning the room was busy from 11:14:50 and the session was
//! told at 11:16:30, and the lamp answered the table at 11:15:04. The
//! character file says what each means for who is being spoken to; here
//! only the room is described.
//!
//! Off unless the service's environment says `on`. Not yet heard on a lamp.
use std::{collections::VecDeque, sync::Mutex};

/// Where the coordinator reads whether the room is told. `on` only.
pub const ENVIRONMENT: &str = "LAMP_ROOM_FACTS";
/// The provider worker's flag for it.
pub const FLAG: &str = "--room-facts";
/// Where the coordinator reads whether a line said over the lamp's own
/// reply and left unanswered is kept out of the busy room's count. `on`
/// only; off unless set.
///
/// Why: on lamp-4ace on 17 October at 09:21 the first builder's ten
/// cut-ins over the lamp's stories made the room busy, with one person and
/// the lamp's own voice in it. Each cut-in the lamp left without a word (a
/// stop it obeyed, one that took no yield) counted as talk left alone, and
/// three in a minute make a busy room. Then "Hold on, I have a question.
/// What day comes after Monday?", heard whole, was held as office talk
/// (#828, the first builder at 09:27 and 10:14). What it costs: people who
/// keep talking over the lamp's answer in a busy office are not counted
/// either, so that room is known as busy later or not at all.
pub const CUT_INS_ENVIRONMENT: &str = "LAMP_CUT_INS_NOT_TALK";
/// The provider worker's flag for it, right after [`FLAG`].
pub const CUT_INS_FLAG: &str = "--cut-ins-not-talk";
/// How many requests begun over the lamp are kept marked at once.
const OVER_THE_LAMP_KEPT: usize = 8;

/// What a request the lamp left unanswered was ([`RoomTold::left_unanswered`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Left {
    /// Something said, left alone: it counts toward a busy room.
    Talk,
    /// A sound sent on only for the model to judge.
    Sound,
    /// A request whose answer the lamp's own rule held.
    Held,
    /// A line said while the lamp's own answer could be heard, with
    /// [`CUT_INS_ENVIRONMENT`].
    OverTheLamp,
}
pub const QUIET: &str = "(For you to know, not to answer: the room around you has gone quiet.)";
pub const BUSY: &str = "(For you to know, not to answer: the office around you is busy, with people talking to each other.)";

/// Nothing heard for this long and the room is quiet.
const QUIET_AFTER_US: u64 = 300_000_000;
/// This many things in a row that the lamp said nothing to, inside this
/// long, and the room is busy.
const BUSY_RUN: usize = 3;
const BUSY_WITHIN_US: u64 = 60_000_000;
/// From a quiet room, this many inside the same minute.
const QUIET_ENDS_RUN: usize = 2;
/// With [`RoomTold::count_only_talk`], busy ends this long after the last
/// thing left unanswered that counts as talk.
const BUSY_ENDS_AFTER_US: u64 = 180_000_000;
/// With [`RoomTold::count_only_talk`], this many things said and left
/// unanswered since the lamp last spoke, and a conversation with it is no
/// longer going on: the people have gone on talking to each other
/// ([`RoomTold::talked_past_the_lamp`]). One is a person saying "okay" or
/// thinking aloud between their questions. One line held as another voice
/// counts as this many ([`RoomTold::another_voice_held`]).
pub const TALK_ENDS_CONVERSATION: usize = 2;
/// The room is told only when nothing has been heard or said for this
/// long: a fact written into a request would be taken for part of it, and
/// one offered while an answer is owed is dropped. Longer than the four
/// seconds after which a request the model leaves alone is closed.
const SETTLED_US: u64 = 6_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Room {
    Unknown,
    Quiet,
    Busy,
}

impl Room {
    fn fact(self) -> Option<&'static str> {
        match self {
            Self::Unknown => None,
            Self::Quiet => Some(QUIET),
            Self::Busy => Some(BUSY),
        }
    }
}

pub struct RoomTold {
    /// When watching began, or when something was last heard.
    quiet_since_us: Option<u64>,
    /// When anything was last heard or said.
    last_sound_us: u64,
    /// The request in hand, while the lamp has said nothing in it, and
    /// whether what was said in it has ended.
    open: Option<(u64, bool)>,
    /// When each of the latest things the lamp said nothing to ended.
    unanswered: VecDeque<u64>,
    /// The request judged a sound and sent only for the model to judge
    /// (`LAMP_SHORT_SOUNDS`), until it is answered or closed.
    sound: Option<u64>,
    /// Only talk between people counts (`LAMP_SPOKEN_TO=on`): the request
    /// whose answer the lamp's own rule held, until it is closed, and when
    /// the last thing that counts as talk was left unanswered.
    talk_only: bool,
    held: Option<u64>,
    last_talk_us: Option<u64>,
    /// How many things said were left unanswered as talk since the lamp
    /// last spoke, and the latest request counted or answered, so that a
    /// request is counted once however its end is learned, and never once
    /// the lamp has spoken after it.
    talk_since_spoke: usize,
    talk_counted: u64,
    /// When watching began.
    watching_since_us: Option<u64>,
    /// A line said over the lamp's reply and left unanswered is not counted
    /// ([`CUT_INS_ENVIRONMENT`]): the requests begun so, each until it is
    /// closed, the newest [`OVER_THE_LAMP_KEPT`]. A cut-in heard as two or
    /// three requests is kept out whole (the reviewer on #1291).
    cut_ins_not_talk: bool,
    over_the_lamp: VecDeque<u64>,
    room: Room,
    /// The session last told, and what it was told.
    told: Option<(u64, Room)>,
}

worker_global! {
    /// This process's one room. The provider worker is one process.
    pub fn room() -> Mutex<RoomTold> = Mutex::new(RoomTold::new());
}

impl RoomTold {
    pub const fn new() -> Self {
        Self {
            quiet_since_us: None,
            last_sound_us: 0,
            open: None,
            unanswered: VecDeque::new(),
            sound: None,
            talk_only: false,
            held: None,
            last_talk_us: None,
            talk_since_spoke: 0,
            talk_counted: 0,
            watching_since_us: None,
            cut_ins_not_talk: false,
            over_the_lamp: VecDeque::new(),
            room: Room::Unknown,
            told: None,
        }
    }

    /// A request began: something was heard.
    pub fn heard(&mut self, request: u64, now_us: u64) {
        // The one before it, said to its end and then replaced by more
        // talk before the lamp spoke. One that never ended was a sound the
        // coordinator dropped, and is not counted.
        if let Some((previous, true)) = self.open.filter(|(previous, _)| *previous != request) {
            self.unanswered(previous, now_us);
        }
        self.open = Some((request, false));
        self.last_sound_us = now_us;
    }

    /// More of what is being said in a request has come.
    pub fn hearing(&mut self, now_us: u64) {
        self.last_sound_us = now_us;
    }

    /// What was said in `request` has ended.
    pub fn ended(&mut self, request: u64, now_us: u64) {
        if let Some((open, ended)) = self.open.as_mut()
            && *open == request
        {
            *ended = true;
        }
        self.quiet_since_us = Some(now_us);
        self.last_sound_us = now_us;
    }

    /// What was heard in `request` has ended, and it was judged a sound,
    /// sent on only for the model to judge by its sound. Left unanswered it
    /// is what a sound the coordinator drops is: not something said, so it
    /// neither starts the quiet five minutes again nor counts toward a busy
    /// room. On lamp-4ace on 2026-10-14 at 23:24, in the empty office at
    /// night, three played alert sounds sent on and left alone inside 37 s
    /// made the room busy, and the session was told people were talking.
    /// Answered, it was something said after all.
    pub fn ended_as_sound(&mut self, request: u64, now_us: u64) {
        self.sound = Some(request);
        self.last_sound_us = now_us;
    }

    /// Count only talk between people toward a busy room, from now on
    /// (`LAMP_SPOKEN_TO=on`, #829): a request whose answer the lamp's own
    /// rule held ([`Self::held`]) is not talk, and busy ends
    /// [`BUSY_ENDS_AFTER_US`] after the last talk.
    pub fn count_only_talk(&mut self, on: bool) {
        self.talk_only = on;
    }

    /// Keep lines said over the lamp's reply out of the count from now on
    /// ([`CUT_INS_ENVIRONMENT`]).
    pub fn leave_cut_ins_out(&mut self, on: bool) {
        self.cut_ins_not_talk = on;
    }

    /// `request` began while the lamp's own answer could be heard (its
    /// Start's `over_the_lamps_voice`). With [`Self::leave_cut_ins_out`],
    /// left unanswered it is not talk the lamp left alone.
    pub fn began_over_the_lamp(&mut self, request: u64) {
        if self.cut_ins_not_talk && !self.over_the_lamp.contains(&request) {
            if self.over_the_lamp.len() == OVER_THE_LAMP_KEPT {
                self.over_the_lamp.pop_front();
            }
            self.over_the_lamp.push_back(request);
        }
    }

    /// Whether `request`, left unanswered now, would be kept out as said
    /// over the lamp's reply.
    pub fn kept_out_over_the_lamp(&self, request: u64) -> bool {
        self.over_the_lamp.contains(&request)
    }

    /// The lamp's own rule held the answer to `request`. With
    /// [`Self::count_only_talk`], it is not talk when it is closed.
    pub fn held(&mut self, request: u64) {
        if self.talk_only {
            self.held = Some(request);
        }
    }

    /// The answer to `request` was held for being said in another voice than
    /// the conversation's (the same-voice check, `LAMP_SAME_VOICE_MODEL`,
    /// #829). With [`Self::count_only_talk`] that one line is talk past the
    /// lamp, as [`TALK_ENDS_CONVERSATION`] lines left alone are: two people
    /// are talking, and the conversation with the lamp is over until it
    /// speaks again, whoever says the next line. In the battery's scene 3 on
    /// lamp-4ace (17 October), with the umbrella line held, the asker's own
    /// "Yes, and we should leave a bit early.", to the other person, would
    /// otherwise have been answered as the conversation still going on.
    /// A sound let through is no one's voice, and no talk: it ends nothing
    /// (the reviewer on #1415).
    pub fn another_voice_held(&mut self, request: u64) {
        if self.sound == Some(request) {
            return;
        }
        if self.talk_only {
            self.talk_counted = self.talk_counted.max(request);
            self.talk_since_spoke = self.talk_since_spoke.max(TALK_ENDS_CONVERSATION);
        }
    }

    /// The lamp is speaking in `request`.
    pub fn spoke(&mut self, request: u64, now_us: u64) {
        if self.open.is_some_and(|(open, _)| open == request) {
            self.open = None;
        }
        if self.sound == Some(request) {
            self.sound = None;
        }
        self.unanswered.clear();
        self.talk_since_spoke = 0;
        self.talk_counted = self.talk_counted.max(request);
        self.quiet_since_us = Some(now_us);
        self.last_sound_us = now_us;
    }

    /// The lamp answered `request` by doing something, a light or a timer,
    /// whether or not it said a word: a call that is run, not a lookup
    /// such as the time or a memory, and not one the Policy held. With [`Self::count_only_talk`] that is an answer
    /// as speaking is: not talk past the lamp, and the count since it last
    /// spoke begins afresh (the sixth on #1029: "lights off", then
    /// "brighter", each done without a word, ended the conversation).
    /// Without it, nothing changes.
    pub fn answered_by_a_call(&mut self, request: u64, now_us: u64) {
        if self.talk_only {
            self.spoke(request, now_us);
        }
    }

    /// Whether people have gone on talking to each other since the lamp
    /// last spoke, with [`Self::count_only_talk`] (#829):
    /// [`TALK_ENDS_CONVERSATION`] things said since then and left
    /// unanswered as talk, not as sounds or held by the lamp's own rule, or
    /// one line held as another voice ([`Self::another_voice_held`]). A
    /// conversation with the lamp is then no longer going on, however
    /// lately it spoke. On lamp-4ace on 16 October at 06:07, Daniel asked
    /// the lamp for euros and was answered; Samantha and he each said a
    /// line to each other that it left alone; then his "Did you get my
    /// message about the budget?", to her, was answered ("I don't recall
    /// that message") as the conversation going on.
    pub fn talked_past_the_lamp(&self) -> bool {
        self.talk_only && self.talk_since_spoke >= TALK_ENDS_CONVERSATION
    }

    /// `request` ended with the lamp saying nothing. Whether it counts as
    /// something said that the lamp left alone: a sound does not.
    pub fn unanswered(&mut self, request: u64, now_us: u64) -> bool {
        self.left_unanswered(request, now_us) == Left::Talk
    }

    /// `request` ended with the lamp saying nothing: what it was, and so
    /// whether it counts. Only [`Left::Talk`] joins the busy room's run. A
    /// line kept out as said over the lamp still keeps a busy room busy
    /// (`last_talk_us`), so people talking over the lamp's answers in a
    /// busy office don't turn it quiet (the reviewer on #1291); it can't
    /// make a room busy.
    pub fn left_unanswered(&mut self, request: u64, now_us: u64) -> Left {
        if self.open.is_some_and(|(open, _)| open == request) {
            self.open = None;
        }
        let over_the_lamp = self
            .over_the_lamp
            .iter()
            .position(|marked| *marked == request)
            .map(|at| self.over_the_lamp.remove(at))
            .is_some();
        if self.sound == Some(request) {
            self.sound = None;
            return Left::Sound;
        }
        if self.held == Some(request) {
            self.held = None;
            return Left::Held;
        }
        if over_the_lamp {
            self.last_talk_us = Some(now_us);
            return Left::OverTheLamp;
        }
        if request > self.talk_counted {
            self.talk_counted = request;
            self.talk_since_spoke = self.talk_since_spoke.saturating_add(1);
        }
        if self.unanswered.len() == BUSY_RUN {
            self.unanswered.pop_front();
        }
        self.unanswered.push_back(now_us);
        self.last_talk_us = Some(now_us);
        Left::Talk
    }

    /// Whether the latest `run` things left unanswered all fell inside a
    /// minute.
    fn run_of(&self, run: usize) -> bool {
        self.unanswered.len() >= run
            && self
                .unanswered
                .iter()
                .rev()
                .nth(run - 1)
                .zip(self.unanswered.back())
                .is_some_and(|(first, last)| last.saturating_sub(*first) <= BUSY_WITHIN_US)
    }

    /// How the room is now.
    pub fn at(&mut self, now_us: u64) -> Room {
        let since = *self.quiet_since_us.get_or_insert(now_us);
        let watching = *self.watching_since_us.get_or_insert(now_us);
        let run = if self.room == Room::Quiet {
            QUIET_ENDS_RUN
        } else {
            BUSY_RUN
        };
        let talk_over = self.talk_only
            && self.room == Room::Busy
            && self
                .last_talk_us
                .is_some_and(|talk| now_us.saturating_sub(talk) >= BUSY_ENDS_AFTER_US);
        // With only talk counted: five minutes with no talk between people
        // is a quiet room, whatever one person says to the lamp meanwhile.
        // Without it a room where someone keeps talking to the lamp from
        // the start is never known: on lamp-4ace on 16 October from 05:39
        // the voice told the bus "unknown" all night (#829, 05:46).
        let no_talk = self.talk_only
            && self.room == Room::Unknown
            && now_us.saturating_sub(self.last_talk_us.unwrap_or(watching).max(watching))
                >= QUIET_AFTER_US;
        if self.run_of(run) {
            self.room = Room::Busy;
            // The run has been counted; the next three decide afresh.
            self.unanswered.clear();
        } else if talk_over || no_talk || now_us.saturating_sub(since) >= QUIET_AFTER_US {
            self.room = Room::Quiet;
            self.unanswered.clear();
        }
        self.room
    }

    /// What `session` should now be told, if anything: how the room is,
    /// when it is known, differs from what this session was told, and
    /// nothing has been heard or said for a moment.
    pub fn due(&mut self, session: u64, now_us: u64) -> Option<&'static str> {
        if now_us.saturating_sub(self.last_sound_us) < SETTLED_US {
            return None;
        }
        self.due_in_request(session, now_us)
    }

    /// The same, for a note inside the request being heard now, which
    /// needs no quiet moment.
    pub fn due_in_request(&mut self, session: u64, now_us: u64) -> Option<&'static str> {
        let room = self.at(now_us);
        if self.told == Some((session, room)) {
            return None;
        }
        room.fact()
    }

    /// `session` has been told how the room is now.
    pub fn told(&mut self, session: u64) {
        self.told = Some((session, self.room));
    }
}

impl Default for RoomTold {
    fn default() -> Self {
        Self::new()
    }
}

/// The provider worker's arguments for the two switches in the
/// environment.
pub fn arguments_from_environment() -> Vec<String> {
    let mut arguments = arguments(std::env::var(ENVIRONMENT).ok().as_deref());
    arguments.extend(cut_ins_arguments(
        std::env::var(CUT_INS_ENVIRONMENT).ok().as_deref(),
    ));
    arguments
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

fn cut_ins_arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![CUT_INS_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading `--room-facts` off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Take a leading `--cut-ins-not-talk` off a worker's flags, after
/// [`take`].
pub fn take_cut_ins(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == CUT_INS_FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    /// Something said to its end in `request`, from `at` for two seconds.
    fn said(room: &mut RoomTold, request: u64, at: u64) {
        room.heard(request, at * S);
        room.hearing((at + 1) * S);
        room.ended(request, (at + 2) * S);
    }

    /// #828, the first builder at 10:14: three cut-ins over the lamp's
    /// stories, each left without a word, within a minute. With the switch
    /// the room stays unknown, and "Hold on, I have a question. What day
    /// comes after Monday?", with no name, is played by the busy form of
    /// the rule. Without it the room is busy and the question is held, as
    /// on lamp-4ace at 09:21. A line not said over the lamp still counts.
    #[test]
    fn with_the_switch_cut_ins_left_unanswered_do_not_make_the_room_busy() {
        use crate::attention::{Attention, Verdict};
        let question = |room: Room| {
            let mut attention = Attention::default();
            attention.began_at(4, 3, 40 * S);
            attention.heard("Hold on, I have a question. What day comes after Monday?");
            attention.first_output_busy(4, room, false, true, Some(false))
        };
        for switch in [true, false] {
            let mut room = RoomTold::new();
            room.leave_cut_ins_out(switch);
            for request in 1..=3 {
                room.began_over_the_lamp(request);
                said(&mut room, request, request * 10);
                assert_eq!(room.kept_out_over_the_lamp(request), switch);
                assert_eq!(room.unanswered(request, (request * 10 + 2) * S), !switch);
            }
            let now = room.at(35 * S);
            assert_eq!(now == Room::Busy, !switch, "switch {switch}");
            assert_eq!(
                matches!(question(now), Some(Verdict::Play(_))),
                switch,
                "switch {switch}"
            );
        }
        let mut room = RoomTold::new();
        room.leave_cut_ins_out(true);
        for request in 1..=3 {
            said(&mut room, request, request * 10);
            assert!(room.unanswered(request, (request * 10 + 2) * S));
        }
        assert_eq!(room.at(35 * S), Room::Busy);
    }

    /// The reviewer on #1291. A cut-in heard as three requests, each
    /// marked as its Start comes and closed when the next is heard (the
    /// worker's order), is kept out whole: three such in a minute leave
    /// the room unknown. A sound begun over the lamp is a sound, its mark
    /// gone with it. With only talk counted, lines kept out keep a busy
    /// room busy past the 3 min after the last counted line; they can't
    /// make one.
    #[test]
    fn with_the_switch_a_split_cut_in_is_kept_out_whole_and_keeps_a_busy_room_busy() {
        let mut room = RoomTold::new();
        room.leave_cut_ins_out(true);
        for cut_in in 0..3u64 {
            for part in 0..3u64 {
                let request = 1 + cut_in * 3 + part;
                room.began_over_the_lamp(request);
                said(&mut room, request, 10 + cut_in * 15 + part * 3);
            }
            let last = 3 + cut_in * 3;
            assert_eq!(
                room.left_unanswered(last, (20 + cut_in * 15) * S),
                Left::OverTheLamp
            );
        }
        assert_eq!(room.at(60 * S), Room::Unknown);
        room.began_over_the_lamp(20);
        room.heard(20, 70 * S);
        room.ended_as_sound(20, 71 * S);
        assert_eq!(room.left_unanswered(20, 72 * S), Left::Sound);
        assert!(!room.kept_out_over_the_lamp(20));
        // A busy office, only talk counted: three lines left alone, then for
        // four minutes a line said over each of the lamp's answers.
        for keep in [true, false] {
            let mut room = RoomTold::new();
            room.count_only_talk(true);
            room.leave_cut_ins_out(true);
            for request in 1..=3 {
                said(&mut room, request, request * 10);
                assert!(room.unanswered(request, (request * 10 + 2) * S));
            }
            assert_eq!(room.at(35 * S), Room::Busy);
            for (n, at) in (60..=300).step_by(30).enumerate() {
                let request = 10 + n as u64;
                if keep {
                    room.began_over_the_lamp(request);
                }
                said(&mut room, request, at);
                room.left_unanswered(request, (at + 2) * S);
            }
            // Busy either way: counted, or kept out and still talk.
            assert_eq!(room.at(305 * S), Room::Busy, "kept out {keep}");
        }
    }

    #[test]
    fn the_cut_ins_switch_is_on_only_and_its_flag_follows_the_room_facts_one() {
        assert_eq!(cut_ins_arguments(Some("on")), [CUT_INS_FLAG]);
        for value in [None, Some(""), Some("off"), Some("ON")] {
            assert!(cut_ins_arguments(value).is_empty(), "{value:?}");
        }
        let both = [FLAG.to_owned(), CUT_INS_FLAG.to_owned(), "rest".to_owned()];
        let (facts, rest) = take(&both);
        assert_eq!((facts, take_cut_ins(rest)), (true, (true, &both[2..])));
        assert_eq!(take_cut_ins(&both), (false, &both[..]));
    }

    #[test]
    fn five_minutes_of_nothing_is_quiet_and_three_unanswered_in_a_minute_is_busy() {
        let mut room = RoomTold::new();
        // Just started: not known, nothing to tell.
        assert_eq!(room.at(10 * S), Room::Unknown);
        assert_eq!(room.due(1, 20 * S), None);
        // Five minutes on with nothing heard: quiet, and the session is told once.
        assert_eq!(room.due(1, 311 * S), Some(QUIET));
        room.told(1);
        assert_eq!(room.due(1, 320 * S), None);
        // Someone speaks into the quiet and is answered: still quiet.
        said(&mut room, 1, 400);
        room.spoke(1, 403 * S);
        assert_eq!(room.at(410 * S), Room::Quiet);
        // Colleagues start talking: three things the lamp says nothing to.
        // The first is closed unanswered, the second is replaced by the
        // third before any answer, the third is closed unanswered.
        said(&mut room, 2, 500);
        room.unanswered(2, 506 * S);
        said(&mut room, 3, 508);
        assert_eq!(room.at(510 * S), Room::Quiet);
        // Two inside the minute end a quiet room (the first and the
        // second, counted as the third began).
        said(&mut room, 4, 511);
        assert_eq!(room.at(514 * S), Room::Busy);
        room.unanswered(4, 517 * S);
        assert_eq!(room.at(518 * S), Room::Busy);
        // Told once nothing has been heard or said for a moment.
        assert_eq!(room.due(1, 518 * S), None);
        assert_eq!(room.due(1, 520 * S), Some(BUSY));
        room.told(1);
        assert_eq!(room.due(1, 530 * S), None);
        // A new session is told how the room is.
        assert_eq!(room.due(2, 531 * S), Some(BUSY));
        room.told(2);
        // Called and answered in a busy office: it is still busy. A third
        // session opening just then is told once the lamp has finished.
        said(&mut room, 5, 540);
        room.spoke(5, 543 * S);
        assert_eq!(room.due(3, 545 * S), None);
        assert_eq!(room.due(3, 550 * S), Some(BUSY));
        assert_eq!(room.at(600 * S), Room::Busy);
        // Everyone leaves. Five minutes after the last thing: quiet.
        assert_eq!(room.at(543 * S + 299 * S), Room::Busy);
        assert_eq!(room.due(2, 543 * S + 301 * S), Some(QUIET));
    }

    /// #829, the first builder's day scene on lamp-4ace (16 October,
    /// 02:30): two people leave the room busy, then one person alone asks
    /// the lamp something every 40 s and each answer is held. Without the
    /// switch each held request is more talk and the room stays busy while
    /// they speak. With it a held request is not talk, and busy ends three
    /// minutes after the last talk between people.
    #[test]
    fn with_the_switch_a_held_request_is_not_talk_and_busy_ends_after_the_last_talk() {
        let day = |talk_only: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(talk_only);
            // The two people's talk: three things left alone in a minute.
            for (request, at) in [(1, 0), (2, 15), (3, 30)] {
                said(&mut room, request, at);
                room.unanswered(request, (at + 6) * S);
            }
            assert_eq!(room.at(40 * S), Room::Busy);
            // One person, every 40 s, each answer held. A held request is
            // closed when the next one begins.
            (0..10u64)
                .map(|n| {
                    let at = 60 + n * 40;
                    said(&mut room, 10 + n, at);
                    room.held(10 + n);
                    room.at((at + 7) * S)
                })
                .collect::<Vec<_>>()
        };
        assert!(day(false).iter().all(|room| *room == Room::Busy));
        // The last talk was left alone at 36 s: busy until 216 s.
        let with = day(true);
        assert_eq!(with[..4], [Room::Busy; 4]);
        assert_eq!(with[4..], [Room::Quiet; 6]);
        // Talk between people still makes it busy again.
        let mut room = RoomTold::new();
        room.count_only_talk(true);
        for (request, at) in [(1, 0), (2, 15), (3, 30)] {
            said(&mut room, request, at);
            room.unanswered(request, (at + 6) * S);
        }
        assert_eq!(room.at(40 * S), Room::Busy);
        assert_eq!(room.at(220 * S), Room::Quiet);
        for (request, at) in [(4, 230), (5, 245)] {
            said(&mut room, request, at);
            room.unanswered(request, (at + 6) * S);
        }
        assert_eq!(room.at(252 * S), Room::Busy);
    }

    /// #829, the first builder at 06:15: the scene of 06:07 on lamp-4ace,
    /// line by line. Two lines between people that the lamp left alone, and
    /// the conversation with it is over, whoever speaks next; one is not.
    /// Sounds, held requests and lines older than the lamp's answer are not
    /// talk past it, a line is counted once however its end is learned, and
    /// the lamp speaking again begins the count afresh.
    #[test]
    fn with_the_switch_two_lines_left_alone_after_the_lamp_spoke_end_its_conversation() {
        let scene = |talk_only: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(talk_only);
            // "Hey Lamp, how much is one hundred dollars in euros?", answered.
            said(&mut room, 5, 20);
            room.spoke(5, 26 * S);
            assert!(!room.talked_past_the_lamp());
            // "Anyway, if we drop the trip we save almost half.", left alone:
            // counted when the next line begins, and again when the model's
            // silence closes it, once.
            said(&mut room, 6, 33);
            let one = room.talked_past_the_lamp();
            said(&mut room, 7, 38);
            room.unanswered(6, 39 * S);
            let still_one = room.talked_past_the_lamp();
            // "True, but the client wants to meet in person.", left alone.
            room.unanswered(7, 45 * S);
            // "Did you get my message about the budget?" begins.
            room.heard(8, 47 * S);
            (one, still_one, room.talked_past_the_lamp())
        };
        assert_eq!(scene(true), (false, false, true));
        assert_eq!(scene(false), (false, false, false));

        let mut room = RoomTold::new();
        room.count_only_talk(true);
        said(&mut room, 1, 0);
        room.spoke(1, 3 * S);
        // A sound and a request the rule held are not talk past the lamp.
        said(&mut room, 2, 5);
        room.ended_as_sound(2, 7 * S);
        room.unanswered(2, 11 * S);
        said(&mut room, 3, 12);
        room.held(3);
        room.unanswered(3, 18 * S);
        assert!(!room.talked_past_the_lamp());
        // A line left alone, then the lamp answers the next: afresh.
        said(&mut room, 4, 20);
        room.unanswered(4, 26 * S);
        said(&mut room, 5, 27);
        room.spoke(5, 30 * S);
        // A line from before that answer, closed only now, is not counted.
        room.unanswered(4, 31 * S);
        said(&mut room, 6, 32);
        room.unanswered(6, 38 * S);
        assert!(!room.talked_past_the_lamp());
        said(&mut room, 7, 40);
        room.unanswered(7, 46 * S);
        assert!(room.talked_past_the_lamp());
        room.spoke(8, 50 * S);
        assert!(!room.talked_past_the_lamp());
    }

    /// #829, the battery's scene 3 (17 October): one line held as another
    /// voice ends the conversation, as two lines left alone do, still when
    /// its end is learned later; the lamp speaking again begins afresh, and
    /// a sound held so ends nothing. Without the switch it changes nothing.
    #[test]
    fn with_the_switch_one_line_held_as_another_voice_ends_the_conversation() {
        let after = |talk_only: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(talk_only);
            // A's "Hey Lamp, what is the weather like…", answered.
            said(&mut room, 1, 0);
            room.spoke(1, 4 * S);
            // B's "I should bring an umbrella then.", held as another voice.
            said(&mut room, 2, 8);
            room.another_voice_held(2);
            let ended = room.talked_past_the_lamp();
            room.unanswered(2, 11 * S);
            (ended, room)
        };
        let (ended, mut room) = after(true);
        assert!(ended);
        assert!(room.talked_past_the_lamp());
        room.spoke(3, 20 * S);
        assert!(!room.talked_past_the_lamp());
        let (ended, room) = after(false);
        assert!(!ended && !room.talked_past_the_lamp());
        // A sound let through, a thump or a chair heard for 1.5 s, is no
        // voice: held, it ends nothing.
        let mut room = RoomTold::new();
        room.count_only_talk(true);
        said(&mut room, 1, 0);
        room.spoke(1, 4 * S);
        said(&mut room, 2, 8);
        room.ended_as_sound(2, 10 * S);
        room.another_voice_held(2);
        room.unanswered(2, 11 * S);
        assert!(!room.talked_past_the_lamp());
    }

    /// The sixth on #1029: lines answered by a call and no words are
    /// answers, with the switch, and leave nothing counted against the
    /// conversation; without it, the room is as before.
    #[test]
    fn with_the_switch_a_line_answered_by_a_call_alone_is_no_talk_past_the_lamp() {
        let room_after = |talk_only: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(talk_only);
            said(&mut room, 1, 0);
            room.spoke(1, 3 * S);
            // "Lights off", then "brighter": each done, not a word said.
            for (request, at) in [(2, 10), (3, 20)] {
                said(&mut room, request, at);
                room.answered_by_a_call(request, (at + 3) * S);
            }
            // "A bit more" begins.
            room.heard(4, 30 * S);
            room
        };
        assert!(!room_after(true).talked_past_the_lamp());
        // Without the switch the calls are not counted as answers, as before.
        let mut room = room_after(false);
        room.count_only_talk(true);
        assert!(room.talked_past_the_lamp());
    }

    /// #829, the first builder at 05:46: after the voice started, one person
    /// talked with the lamp all night, every request answered or held, and
    /// the room was "unknown" on the bus throughout. With only talk counted,
    /// five minutes with no talk between people is quiet; without it, as
    /// before, the room is not known while something is heard.
    #[test]
    fn with_the_switch_a_room_where_one_person_talks_with_the_lamp_is_quiet_in_five_minutes() {
        let night = |talk_only: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(talk_only);
            assert_eq!(room.at(0), Room::Unknown);
            (1..=12u64)
                .map(|n| {
                    let at = n * 30;
                    said(&mut room, n, at);
                    if n % 2 == 0 {
                        room.spoke(n, (at + 3) * S);
                    } else {
                        room.held(n);
                    }
                    (at, room.at((at + 4) * S))
                })
                .collect::<Vec<_>>()
        };
        for (at, room) in night(true) {
            let expected = if at + 4 >= 300 {
                Room::Quiet
            } else {
                Room::Unknown
            };
            assert_eq!(room, expected, "{at}");
        }
        assert!(night(false).iter().all(|(_, room)| *room == Room::Unknown));
        // Talk between people puts the five minutes back.
        let mut room = RoomTold::new();
        room.count_only_talk(true);
        assert_eq!(room.at(0), Room::Unknown);
        for (request, at) in [(1, 200), (2, 230)] {
            said(&mut room, request, at);
            room.unanswered(request, (at + 6) * S);
        }
        assert_eq!(room.at(400 * S), Room::Unknown);
        assert_eq!(room.at(537 * S), Room::Quiet);
    }

    #[test]
    fn a_meeting_that_begins_in_a_quiet_room_makes_it_busy_at_its_second_unanswered_thing() {
        let mut room = RoomTold::new();
        assert_eq!(room.at(S), Room::Unknown);
        assert_eq!(room.due(1, 302 * S), Some(QUIET));
        room.told(1);
        // One remark left alone: a person alone at their desk may mutter.
        said(&mut room, 1, 400);
        room.unanswered(1, 406 * S);
        assert_eq!(room.at(407 * S), Room::Quiet);
        // Another more than a minute later: still not a meeting.
        said(&mut room, 2, 470);
        room.unanswered(2, 476 * S);
        assert_eq!(room.at(477 * S), Room::Quiet);
        // The next inside the minute, as at 11:09:08 and 11:10:02: busy.
        said(&mut room, 3, 524);
        room.unanswered(3, 530 * S);
        assert_eq!(room.at(531 * S), Room::Busy);
        // The meeting goes on with no quiet moment: the session is not told
        // between requests, and is told inside the one being heard.
        said(&mut room, 4, 532);
        assert_eq!(room.due(1, 535 * S), None);
        assert_eq!(room.due_in_request(1, 535 * S), Some(BUSY));
        room.told(1);
        assert_eq!(room.due_in_request(1, 536 * S), None);
        // Busy now takes the run of three to stay as it is, as before.
        assert_eq!(room.at(600 * S), Room::Busy);
    }

    #[test]
    fn sounds_sent_on_for_the_model_to_judge_and_left_alone_are_not_talk() {
        let mut room = RoomTold::new();
        assert_eq!(room.at(S), Room::Unknown);
        assert_eq!(room.at(302 * S), Room::Quiet);
        // Played alert sounds on lamp-4ace, 2026-10-14 from 23:24:20, 15 s
        // and 16 s apart, each sent on and left alone. Counted, the second
        // would have made this quiet room busy.
        for (request, at) in [(1, 400), (2, 415), (3, 431), (4, 501)] {
            room.heard(request, at * S);
            room.ended_as_sound(request, (at + 1) * S);
            assert!(!room.unanswered(request, (at + 5) * S));
        }
        assert_eq!(room.at(510 * S), Room::Quiet);
        // Nor do they hold off the quiet that follows talk: five minutes
        // after the last thing said, with such sounds all through them, it
        // is quiet.
        let mut busy = RoomTold::new();
        for (request, at) in [(1, 10), (2, 14), (3, 18)] {
            said(&mut busy, request, at);
            assert!(busy.unanswered(request, (at + 3) * S));
        }
        assert_eq!(busy.at(22 * S), Room::Busy);
        for (request, at) in (4..40).zip((30..330).step_by(9)) {
            busy.heard(request, at * S);
            busy.ended_as_sound(request, (at + 1) * S);
            busy.unanswered(request, (at + 5) * S);
        }
        assert_eq!(busy.at(319 * S), Room::Busy);
        assert_eq!(busy.at(321 * S), Room::Quiet);
        // One the model answers was something said after all ("Hi."), and
        // the next thing left alone counts as it always did.
        room.heard(5, 520 * S);
        room.ended_as_sound(5, 521 * S);
        room.spoke(5, 522 * S);
        said(&mut room, 6, 530);
        assert!(room.unanswered(6, 536 * S));
    }

    #[test]
    fn sounds_and_talk_spread_over_an_afternoon_are_not_a_busy_room() {
        let mut room = RoomTold::new();
        assert_eq!(room.at(S), Room::Unknown);
        assert_eq!(room.at(302 * S), Room::Quiet);
        // Three things left alone, minutes apart.
        for (request, at) in [(1, 400), (2, 500), (3, 600)] {
            said(&mut room, request, at);
            room.unanswered(request, (at + 6) * S);
        }
        assert_eq!(room.at(610 * S), Room::Quiet);
        // A run of coughs and clicks: each begins a request that is
        // dropped as a sound before it ends. They are heard, not counted.
        for request in 4..10 {
            room.heard(request, (700 + request) * S);
        }
        assert_eq!(room.at(720 * S), Room::Quiet);
        // Nor do they keep a busy room busy: five minutes after the last
        // thing said, with clicks all through them, it is quiet.
        let mut busy = RoomTold::new();
        for (request, at) in [(1, 10), (2, 14), (3, 18)] {
            said(&mut busy, request, at);
            busy.unanswered(request, (at + 3) * S);
        }
        assert_eq!(busy.at(22 * S), Room::Busy);
        for (request, at) in (4..40).zip((30..330).step_by(9)) {
            busy.heard(request, at * S);
        }
        assert_eq!(busy.at(319 * S), Room::Busy);
        assert_eq!(busy.at(321 * S), Room::Quiet);
        // And they do not keep the room from being told afterwards.
        assert_eq!(room.due(1, 720 * S), Some(QUIET));
        room.told(1);
        // Nothing is told while someone is still speaking.
        room.heard(10, 800 * S);
        for second in 801..812 {
            room.hearing(second * S);
        }
        assert_eq!(room.due(2, 812 * S), None);
        room.ended(10, 812 * S);
        assert_eq!(room.due(2, 815 * S), None);
        assert_eq!(room.due(2, 819 * S), Some(QUIET));
    }

    #[test]
    fn the_facts_describe_the_room_and_ask_for_nothing() {
        for fact in [QUIET, BUSY] {
            let lower = fact.to_lowercase();
            for word in [
                "silen",
                "say nothing",
                "do not",
                "don't",
                "never",
                "answer only",
            ] {
                assert!(!lower.contains(word), "{word}");
            }
            assert!(lamp_gemini::wire::encode_context(fact).is_ok());
        }
        assert_eq!(arguments(Some("on")), [FLAG]);
        assert!(arguments(Some("1")).is_empty() && arguments(None).is_empty());
        let flags = [FLAG.to_owned(), "x".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }
}
