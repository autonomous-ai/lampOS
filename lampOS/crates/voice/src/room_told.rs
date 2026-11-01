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
pub(crate) const BUSY_ENDS_AFTER_US: u64 = 180_000_000;
/// With [`RoomTold::end_busy_once_answered`] as well, busy ends this long
/// after the lamp's first answer since the last talk.
const BUSY_ENDS_ONCE_ANSWERED_US: u64 = 60_000_000;
/// With [`RoomTold::end_busy_sooner`] as well, busy ends this long after the
/// last talk when the lamp has not answered since.
const BUSY_ENDS_UNANSWERED_US: u64 = 60_000_000;
/// With [`RoomTold::count_only_talk`], this many things said and left
/// unanswered since the lamp last spoke, and a conversation with it is no
/// longer going on: the people have gone on talking to each other
/// ([`RoomTold::talked_past_the_lamp`]). One is a person saying "okay" or
/// thinking aloud between their questions. One line held as another voice
/// counts as this many ([`RoomTold::another_voice_held`]).
pub const TALK_ENDS_CONVERSATION: usize = 2;
/// How many requests held by the lamp's own rule are kept until they are
/// closed ([`RoomTold::held`]).
const HELD_KEPT: usize = 8;
/// The room is told only when nothing has been heard or said for this
/// long: a fact written into a request would be taken for part of it, and
/// one offered while an answer is owed is dropped. Longer than the four
/// seconds after which a request the model leaves alone is closed.
const SETTLED_US: u64 = 6_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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
    /// Every such request not yet closed, the latest [`HELD_KEPT`]: two
    /// held lines judged before either is closed are both no talk (#4175).
    held: VecDeque<u64>,
    last_talk_us: Option<u64>,
    /// The request whose line the Policy held as not the lamp's in a
    /// conversation going on (`LAMP_GOING_ON_ASKS`), until it is closed: it
    /// is talk for the busy room, its run and its clock, but doesn't end the
    /// conversation ([`Self::judged`]).
    held_by_policy: Option<u64>,
    /// How many things said were left unanswered as talk since the lamp
    /// last spoke, and the latest request counted or answered, so that a
    /// request is counted once however its end is learned, and never once
    /// the lamp has spoken after it.
    talk_since_spoke: usize,
    talk_counted: u64,
    /// Since the lamp last spoke, only lines held as another voice have
    /// ended the conversation, no talk left unanswered
    /// ([`Self::ended_only_by_another_voice`]).
    other_voice_only: bool,
    /// When watching began.
    watching_since_us: Option<u64>,
    /// A line said over the lamp's reply and left unanswered is not counted
    /// ([`CUT_INS_ENVIRONMENT`]): the requests begun so, each until it is
    /// closed, the newest [`OVER_THE_LAMP_KEPT`]. A cut-in heard as two or
    /// three requests is kept out whole (the reviewer on #1291).
    cut_ins_not_talk: bool,
    over_the_lamp: VecDeque<u64>,
    /// Busy ends [`BUSY_ENDS_ONCE_ANSWERED_US`] after the lamp's first answer
    /// since the last talk ([`Self::end_busy_once_answered`]), and when that
    /// answer came.
    ends_once_answered: bool,
    answered_us: Option<u64>,
    /// Busy ends [`BUSY_ENDS_UNANSWERED_US`] after the last talk when the
    /// lamp has not answered since ([`Self::end_busy_sooner`]).
    ends_sooner: bool,
    /// `LAMP_NAME_OR_FOLLOW_UP` ([`Self::count_held_talk`]), and the request
    /// said with someone in view and not turned to the lamp
    /// ([`Self::seen_talking`]).
    held_talk: bool,
    talking_seen: Option<u64>,
    /// The request whose face turned to the lamp was seen not speaking it
    /// (`LAMP_TURNED_FACE_SPEAKS`, [`Self::face_not_speaking`]).
    not_speaking: Option<u64>,
    /// The request let out only as the same speaker's conversation carried
    /// on ([`Self::carried`]), until it is closed.
    carried: Option<u64>,
    /// The request held as someone else's while a carry stood, its first
    /// in a row ([`Self::colleague_held`]).
    colleague_held: Option<u64>,
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
            held: VecDeque::new(),
            last_talk_us: None,
            held_by_policy: None,
            talk_since_spoke: 0,
            talk_counted: 0,
            other_voice_only: false,
            watching_since_us: None,
            cut_ins_not_talk: false,
            over_the_lamp: VecDeque::new(),
            ends_once_answered: false,
            answered_us: None,
            ends_sooner: false,
            held_talk: false,
            talking_seen: None,
            not_speaking: None,
            carried: None,
            colleague_held: None,
            room: Room::Unknown,
            told: None,
        }
    }

    /// With `LAMP_NAME_OR_FOLLOW_UP` (`crate::name_or_follow_up`), from now
    /// on: in a room not known to be busy, a line the lamp's rule held as
    /// said to someone else by name, or as said to nobody with someone in
    /// view and not turned to the lamp ([`Self::seen_talking`]), is talk when
    /// it is closed, with only talk counted too ([`Self::judged`]); and two
    /// people seen or heard make the room busy at once ([`Self::two_people`]).
    /// A lone voice's lines held with nobody in view are not talk: on
    /// lamp-4ace on 22 October one person's call, heard from a speaker with
    /// no face in view, was misheard and held, the next lines held as said
    /// to nobody turned the room busy, and the whole scene was lost. The switch
    /// answers a person alone as before the switch, and keeps its strict
    /// rule for a busy room, so the room must turn busy from the talk it
    /// holds: at the owner's desk on 22 October it answered everything, and
    /// two lines in a row were never left alone.
    pub fn count_held_talk(&mut self, on: bool) {
        self.held_talk = on;
    }

    /// `request` was said while the only faces turned to the lamp were seen
    /// not speaking it (`LAMP_TURNED_FACE_SPEAKS`,
    /// `speaking_faces::SpeakingFaces::turned_not_seen_speaking`): someone
    /// at the lamp listening while another voice talks. Held, in any room,
    /// it is talk, and joins the run that makes the room busy, so a desk
    /// where two people talk past a still face turned to the lamp reads
    /// busy (lamp-52e6 on 24 Oct at 05:31). Not a lone voice with nobody in
    /// view ([`Self::count_held_talk`]'s note): a face is in view, and its
    /// mouth is read still while the voice talks.
    pub fn face_not_speaking(&mut self, request: u64) {
        self.not_speaking = Some(request);
    }

    /// `request` was said with someone in view and not turned to the lamp
    /// (`bus_out::someone_in_view_turned_away`): with [`Self::count_held_talk`]
    /// and held as said to nobody, it is people talking to each other.
    pub fn seen_talking(&mut self, request: u64) {
        if self.held_talk {
            self.talking_seen = Some(request);
        }
    }

    /// Two people are at the lamp at `now_us`: the eyes see two faces or
    /// more, or a line was in another voice than the conversation's. With
    /// [`Self::count_held_talk`] the room is busy at once, as after a run
    /// of talk, and its clocks run from now, so it stays busy while they
    /// stay and ends as after talk. Whether it was not busy before.
    pub fn two_people(&mut self, now_us: u64) -> bool {
        if !self.held_talk {
            return false;
        }
        let was = self.room;
        self.room = Room::Busy;
        self.last_talk_us = Some(self.last_talk_us.map_or(now_us, |talk| talk.max(now_us)));
        self.quiet_since_us = Some(
            self.quiet_since_us
                .map_or(now_us, |since| since.max(now_us)),
        );
        was != Room::Busy
    }

    /// With only talk counted, end a busy room [`BUSY_ENDS_ONCE_ANSWERED_US`]
    /// after the lamp's first answer since the last talk, from now on, and
    /// never later than [`BUSY_ENDS_AFTER_US`] after the talk
    /// (`LAMP_GOING_ON_ASKS`, the first builder on #1522, 19:08). People
    /// talk, then one of them turns to the lamp: on lamp-4ace at 18:51 a day
    /// line said to the lamp 108 s after two people's talk was still in a
    /// busy room, which the 180 s keep. While people go on talking it is
    /// busy as before, their lines the Policy holds among them, so a call
    /// answered amid their talk ends nothing (scene 3). A reminder the lamp
    /// says is no answer ([`Self::spoke_unasked`]).
    pub fn end_busy_once_answered(&mut self, on: bool) {
        self.ends_once_answered = on;
    }

    /// With only talk counted, end a busy room [`BUSY_ENDS_UNANSWERED_US`]
    /// after the last talk when nothing since was talk or answered, from now
    /// on: lines held since (`Left::Held`), sounds and silence leave it to
    /// end (`LAMP_BUSY_ENDS_SOONER`, the first builder on Queue #1722,
    /// 04:00). A line held only because the room is busy and it wasn't
    /// called is talk with it ([`Self::judged`]). A room is busy while people are talking, not for three
    /// minutes after: someone who walks up a minute after a chat and says
    /// "Hey Lamp, good morning!" is talking to the lamp.
    pub fn end_busy_sooner(&mut self, on: bool) {
        self.ends_sooner = on;
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
        if self.talk_only && !self.held.contains(&request) {
            if self.held.len() == HELD_KEPT {
                self.held.pop_front();
            }
            self.held.push_back(request);
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
    /// (the reviewer on #1415). A question in the conversation's own voice
    /// still goes on with it while nothing else has ended it
    /// ([`Self::ended_only_by_another_voice`], #829).
    pub fn another_voice_held(&mut self, request: u64) {
        if self.sound == Some(request) {
            return;
        }
        if self.talk_only {
            // Already ended by talk, it stays so; otherwise only another
            // voice has ended it.
            if !self.talked_past_the_lamp() {
                self.other_voice_only = true;
            }
            self.talk_counted = self.talk_counted.max(request);
            self.talk_since_spoke = self.talk_since_spoke.max(TALK_ENDS_CONVERSATION);
        }
    }

    /// `request` was let out only as the same speaker's conversation carried
    /// on (`LAMP_SAME_SPEAKER_CARRIES`, `second_provider::worker::Heeding`):
    /// the one the lamp answered, by their voice or face. Left unanswered by
    /// the model it is no talk, so it neither ends the conversation nor keeps
    /// the room busy: on lamp-4ace on 23 Oct at 18:5x the owner's own lines
    /// the model chose not to answer were counted as people talking.
    pub fn carried(&mut self, request: u64) {
        self.carried = Some(request);
    }

    /// The room is known quiet, for a test.
    #[cfg(test)]
    pub fn set_quiet(&mut self) {
        self.room = Room::Quiet;
    }

    /// `request`, a line while the same speaker's conversation was carried,
    /// was held and left the carry standing (`LAMP_SAME_SPEAKER_CARRIES`,
    /// the first builder at 09:34 on #3296): a colleague's one
    /// interjection, or a line in a voice not surely the owner's.
    /// It is no talk and no second person: its verdict does not make the
    /// room busy by its voice ([`Self::judged`]), and left unanswered it
    /// neither ends the conversation nor joins the busy room's run
    /// ([`Self::left_unanswered`]). A second line in a row ends the carry in
    /// the worker, and is counted as before.
    pub fn colleague_held(&mut self, request: u64) {
        self.colleague_held = Some(request);
    }
    /// What the rule's verdict on `request` tells the room, as the worker
    /// gives it: a request whose answer is held is not talk between people
    /// ([`Self::held`]); one held as another voice is, and ends the
    /// conversation ([`Self::another_voice_held`]); and one held as a remark
    /// to the room (`LAMP_REMARKS`, [`crate::remarks::REMARK`]) is talk, so
    /// left unanswered it counts toward [`TALK_ENDS_CONVERSATION`]. On
    /// lamp-4ace on 18 October at 14:44 the lamp answered every line of two
    /// people's talk and spoke their words back, its "conversation going on"
    /// re-armed by its own replies (#1522): two remarks held in a row end it.
    /// One the Policy held as not the lamp's (`LAMP_GOING_ON_ASKS`) is talk
    /// for the busy room, its run and its clock, so two people talking on
    /// keep it busy, but it ends no conversation: their next question to the lamp is still
    /// asked of the Policy (the reviewer on #1594, 20:53: held as nothing,
    /// their talk let the room end a minute after the call, and from then on
    /// every line of it was answered). With [`Self::end_busy_sooner`] a line
    /// held only because the room is busy and it wasn't called
    /// (`BUSY_UNCALLED`, `BUSY_NAMED_UNTURNED`, and with
    /// `LAMP_NAME_OR_FOLLOW_UP` `BUSY_TURNED_ALONE`) is the same: two people
    /// talking on keep the room busy, and its minute runs from their last
    /// line (the reviewer's P2 on #2372, the first builder at 04:36). With
    /// [`Self::count_held_talk`], in a room not known to be busy, a line held
    /// as said to someone else (`attention::said_to_another`), or as said to
    /// nobody with someone in view and not turned to the lamp
    /// ([`Self::seen_talking`]), is talk, and one in another voice makes the
    /// room busy at once ([`Self::two_people`]).
    pub fn judged(
        &mut self,
        request: u64,
        verdict: Option<crate::attention::Verdict>,
        other_voice: bool,
    ) {
        // `LAMP_SAME_SPEAKER_CARRIES`: a colleague's one line held under a
        // carry is no second person and no talk.
        if self.colleague_held == Some(request) {
            return;
        }
        // `LAMP_NAME_OR_FOLLOW_UP`: another voice is a second person.
        if other_voice {
            self.two_people(self.last_sound_us);
        }
        match verdict {
            Some(crate::attention::Verdict::Hold(_)) if other_voice => {
                self.another_voice_held(request)
            }
            Some(crate::attention::Verdict::Hold(why)) if why == crate::remarks::REMARK => {}
            Some(crate::attention::Verdict::Hold(why))
                if why == crate::going_on_asks::NOT_THE_LAMPS
                    || self.ends_sooner
                        && (why == crate::attention::BUSY_UNCALLED
                            || why == crate::attention::BUSY_NAMED_UNTURNED
                            || why == crate::attention::BUSY_TURNED_ALONE) =>
            {
                if self.talk_only {
                    self.held_by_policy = Some(request);
                }
            }
            // `LAMP_NAME_OR_FOLLOW_UP`: in a room not known to be busy, said to
            // someone else, or to nobody with someone in view and turned
            // away, it is talk; a lone voice's line is not.
            Some(crate::attention::Verdict::Hold(why))
                if self.held_talk
                    && self.room != Room::Busy
                    && (crate::attention::said_to_another(why)
                        || why == crate::attention::NOT_SPOKEN_TO
                            && self.talking_seen == Some(request)) => {}
            // `LAMP_TURNED_FACE_SPEAKS`: said past a face turned to the lamp
            // and seen not speaking it, in any room, it is talk.
            Some(crate::attention::Verdict::Hold(_)) if self.not_speaking == Some(request) => {}
            Some(crate::attention::Verdict::Hold(_)) => self.held(request),
            _ => {}
        }
    }

    /// The lamp is speaking in `request`, an answer to someone.
    pub fn spoke(&mut self, request: u64, now_us: u64) {
        self.sounded(request, now_us);
        // The lamp's first answer since the last talk, not its newest: the
        // minute runs from it (Eighth on #1594).
        if self
            .answered_us
            .is_none_or(|answered| self.last_talk_us.is_some_and(|talk| answered < talk))
        {
            self.answered_us = Some(now_us);
        }
    }

    /// The lamp is speaking in `request`, which nobody asked for: a reminder
    /// it says on its own. As [`Self::spoke`], but it is no answer, so it
    /// never starts a busy room's last minute (Jarvis on #1594: a reminder
    /// rung into two people's pause had ended their busy room a minute
    /// later, and their next line was judged in a quiet one).
    pub fn spoke_unasked(&mut self, request: u64, now_us: u64) {
        self.sounded(request, now_us);
    }

    fn sounded(&mut self, request: u64, now_us: u64) {
        if self.open.is_some_and(|(open, _)| open == request) {
            self.open = None;
        }
        if self.sound == Some(request) {
            self.sound = None;
        }
        self.unanswered.clear();
        self.talk_since_spoke = 0;
        self.other_voice_only = false;
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

    /// Someone called the lamp for itself in `request` (its name, a face
    /// turned to it) and the rule let it out (the second route, #1712).
    /// With [`Self::count_only_talk`], the talk left alone before the call
    /// ends no conversation the call began, as the lamp speaking ends that
    /// count, and the call itself, left unanswered, is no talk. On lamp-4ace
    /// on 19 October the owner's answers were cut before their first word
    /// by the next line, so the lamp never spoke, the office's talk before
    /// still counted, and the owner's lines without the name were held.
    pub fn called(&mut self, request: u64) {
        if self.talk_only {
            self.talk_since_spoke = 0;
            self.other_voice_only = false;
            self.talk_counted = self.talk_counted.max(request);
        }
    }

    /// How many lines were left unanswered as talk since the lamp last
    /// spoke, as [`Self::talked_past_the_lamp`] counts them: for the journal.
    pub fn left_alone_since_spoke(&self) -> usize {
        self.talk_since_spoke
    }

    /// Whether the conversation is over only because lines were held as
    /// another voice since the lamp last spoke ([`Self::another_voice_held`]),
    /// with no talk left unanswered. The same-voice check then lets a
    /// question in the conversation's own voice go on with it (#829: on
    /// lamp-4ace on 19 October, Samantha's "Pepper", held as another voice,
    /// ended Daniel's conversation, and his "What about a name for a black
    /// kitten then?" was held). A statement in that voice, or any line
    /// after talk, still finds it over.
    pub fn ended_only_by_another_voice(&self) -> bool {
        self.talked_past_the_lamp() && self.other_voice_only
    }

    /// A line in the conversation's own voice that is no question was said
    /// after another voice's line: talk past the lamp, which ends the
    /// conversation for every voice, as talk left unanswered does, though
    /// the lamp's rule held it rather than leaving it to the model (the
    /// reviewer on #1636: Daniel's "But the client wants to meet in
    /// person.", held, had left his "budget" question to go on).
    pub fn own_voice_talked(&mut self) {
        self.other_voice_only = false;
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
        if let Some(at) = self.held.iter().position(|held| *held == request) {
            self.held.remove(at);
            return Left::Held;
        }
        if self.carried == Some(request) {
            self.carried = None;
            return Left::Held;
        }
        if self.colleague_held == Some(request) {
            self.colleague_held = None;
            return Left::Held;
        }
        if self.held_by_policy == Some(request) {
            self.held_by_policy = None;
            self.join_the_run(now_us);
            return Left::Held;
        }
        if over_the_lamp {
            self.last_talk_us = Some(now_us);
            return Left::OverTheLamp;
        }
        if request > self.talk_counted {
            self.talk_counted = request;
            self.talk_since_spoke = self.talk_since_spoke.saturating_add(1);
            self.other_voice_only = false;
        }
        self.join_the_run(now_us);
        Left::Talk
    }

    /// Something said and left alone at `now_us`, for the busy room: it joins
    /// the run that makes a room busy, and the busy room's clock runs from it.
    fn join_the_run(&mut self, now_us: u64) {
        if self.unanswered.len() == BUSY_RUN {
            self.unanswered.pop_front();
        }
        self.unanswered.push_back(now_us);
        self.last_talk_us = Some(now_us);
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
        // Once the lamp has answered since the last talk, busy ends a minute
        // after the later of the two: that talk and the lamp's first answer
        // since it (Eighth on #1594). A minute after the talk alone would end
        // it at a call made a minute or more after people stopped talking,
        // and the talk after the call would then be answered as in a room
        // not known to be busy, which undoes #1522's fix. Never later than
        // the three minutes without the switch (Jarvis on #1594).
        let busy_until = |talk: u64| match self.answered_us {
            Some(answered) if self.ends_once_answered && answered > talk => answered
                .saturating_add(BUSY_ENDS_ONCE_ANSWERED_US)
                .min(talk.saturating_add(BUSY_ENDS_AFTER_US)),
            // No answer since the last talk: a minute after it, with
            // `LAMP_BUSY_ENDS_SOONER`.
            answered if self.ends_sooner && answered.is_none_or(|answered| answered <= talk) => {
                talk.saturating_add(BUSY_ENDS_UNANSWERED_US)
            }
            _ => talk.saturating_add(BUSY_ENDS_AFTER_US),
        };
        let talk_over = self.talk_only
            && self.room == Room::Busy
            && self
                .last_talk_us
                .is_some_and(|talk| now_us >= busy_until(talk));
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

    /// Whether the room is unknown now only for want of history: watching
    /// began less than the five quiet minutes ago, and since then nothing
    /// was left alone as talk and no two people made it busy. Right after a
    /// start that is every room, an empty one and a meeting alike; what
    /// tells them apart is for the caller to measure
    /// (`LAMP_FIRST_SECONDS_QUIET`, `second_provider::worker::Heeding`).
    pub fn unknown_for_want_of_history(&mut self, now_us: u64) -> bool {
        self.at(now_us) == Room::Unknown
            && self
                .watching_since_us
                .is_some_and(|since| now_us.saturating_sub(since) < QUIET_AFTER_US)
            && self.unanswered.is_empty()
            && self.last_talk_us.is_none()
            && self.talk_since_spoke == 0
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

/// Where the coordinator reads the path of the room's file (#1465): the
/// room and its clocks, kept so that a voice started again a moment later
/// knows the room it left, instead of not knowing it for five minutes. On
/// lamp-4ace on 18 October at 10:57, a minute after the voice had been
/// restarted for an install, the Policy read the room as unknown and held a
/// plain follow-up. An absolute short path, as the thread's
/// (`crate::thread`); unset, the room is in memory only. Off unless set.
pub const FILE_ENVIRONMENT: &str = "LAMP_ROOM_FILE";
/// The provider worker's flag for it, with the path, after
/// [`CUT_INS_FLAG`].
pub const FILE_FLAG: &str = "--room-file";
/// A file written longer ago than this is not taken: the room may have
/// changed since in ways no voice heard. Five minutes, as a quiet room
/// takes to be known.
const KEPT_FOR_US: u64 = QUIET_AFTER_US;
/// The file is written within a second of the room changing, and this
/// often besides, so that a quiet room that has not changed for an hour
/// is still taken back.
const KEPT_EVERY_US: u64 = 30_000_000;
/// Of the time no voice listened, at most this much counts toward a room
/// not yet known going quiet: a restart is seconds, and a meeting may have
/// begun in a longer gap (the reviewer on #1487). A quiet or busy room is
/// kept as it was.
const GAP_COUNTED_US: u64 = 30_000_000;
/// Where Linux keeps this boot's id.
pub const MACHINE_BOOT_FILE: &str = "/proc/sys/kernel/random/boot_id";

/// The provider worker's arguments for the switches and the room's file
/// in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    let mut arguments = arguments(std::env::var(ENVIRONMENT).ok().as_deref());
    arguments.extend(cut_ins_arguments(
        std::env::var(CUT_INS_ENVIRONMENT).ok().as_deref(),
    ));
    arguments.extend(file_arguments(
        std::env::var(FILE_ENVIRONMENT).ok().as_deref(),
    ));
    arguments
}

fn file_arguments(value: Option<&str>) -> Vec<String> {
    value
        .filter(|path| crate::thread::usable(std::path::Path::new(path)))
        .map(|path| vec![FILE_FLAG.to_owned(), path.to_owned()])
        .unwrap_or_default()
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

/// Take a leading `--room-file PATH` off a worker's flags, after
/// [`take_cut_ins`].
pub fn take_file(flags: &[String]) -> std::io::Result<(Option<std::path::PathBuf>, &[String])> {
    match flags {
        [flag, path, rest @ ..]
            if flag == FILE_FLAG && crate::thread::usable(std::path::Path::new(path)) =>
        {
            Ok((Some(path.into()), rest))
        }
        [flag, ..] if flag == FILE_FLAG => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the room's file needs a short absolute path",
        )),
        _ => Ok((None, flags)),
    }
}

/// The room as its file keeps it: the room and its clocks, nothing of the
/// requests, which end with the worker that heard them.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Kept {
    /// The boot it was written in, as hex: a file from another boot is
    /// never taken, since the clocks below are that boot's.
    pub boot: String,
    /// When it was written, by the shared monotonic clock.
    pub at_us: u64,
    pub room: Room,
    pub quiet_since_us: Option<u64>,
    pub watching_since_us: Option<u64>,
    pub last_talk_us: Option<u64>,
    pub last_sound_us: u64,
}

impl Kept {
    /// The same room and clocks, whenever written.
    fn same(&self, other: &Self) -> bool {
        Self {
            at_us: 0,
            ..self.clone()
        } == Self {
            at_us: 0,
            ..other.clone()
        }
    }
}

/// The machine's boot, read from `path` as `lamp-schedule` reads
/// [`MACHINE_BOOT_FILE`]: the boot the room's clocks are of. Never the
/// voice's session `BootId`, which is drawn afresh for each session, so
/// that no worker ever shared one with the worker before it and no room
/// was taken back (the reviewer on #1487). None where it can't be read.
fn boot_from(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let boot = text.trim();
    (!boot.is_empty()
        && boot.len() <= 64
        && boot
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-'))
    .then(|| boot.to_owned())
}

/// At the worker's start, with the room's file at `path` and the
/// machine's boot read from `boot_file`: the journal's line, and the boot
/// the room is kept under from now on. The room is taken back into `room`
/// if it may be. Where the boot can't be read, nothing is taken back or
/// kept.
pub fn start_from(
    path: &std::path::Path,
    boot_file: &std::path::Path,
    room: &mut RoomTold,
    now_us: u64,
) -> (Option<String>, Option<String>) {
    let Some(boot) = boot_from(boot_file) else {
        return (
            Some("the room is not kept: this machine's boot could not be read".into()),
            None,
        );
    };
    (take_back_from(path, room, &boot, now_us), Some(boot))
}

impl RoomTold {
    /// The room and its clocks now, for its file.
    pub fn kept(&self, boot: &str, now_us: u64) -> Kept {
        Kept {
            boot: boot.to_owned(),
            at_us: now_us,
            room: self.room,
            quiet_since_us: self.quiet_since_us,
            watching_since_us: self.watching_since_us,
            last_talk_us: self.last_talk_us,
            last_sound_us: self.last_sound_us,
        }
    }

    /// The room a voice before this one kept, taken if it is from this
    /// boot and was written within [`KEPT_FOR_US`]: how old it was, or why
    /// it is not taken. Taken, the room and its clocks are as they were:
    /// a quiet room is quiet at once, a busy one ends as it would have,
    /// and the time no voice was listening counts as nothing heard.
    pub fn take_back(&mut self, kept: &Kept, boot: &str, now_us: u64) -> Result<u64, String> {
        if kept.boot != boot {
            return Err("it is from another boot".into());
        }
        let age = now_us
            .checked_sub(kept.at_us)
            .ok_or("it was written later than now")?;
        if age > KEPT_FOR_US {
            return Err(format!("it is {} s old", age / 1_000_000));
        }
        // A room not yet known counts only the first of the gap as
        // listened to.
        let skipped = if kept.room == Room::Unknown {
            age.saturating_sub(GAP_COUNTED_US)
        } else {
            0
        };
        self.room = kept.room;
        self.quiet_since_us = kept.quiet_since_us.map(|at| at + skipped);
        self.watching_since_us = kept.watching_since_us.map(|at| at + skipped);
        self.last_talk_us = kept.last_talk_us;
        self.last_sound_us = kept.last_sound_us;
        Ok(age)
    }
}

/// At the worker's start: the room kept in `path`, taken back into `room`
/// if it may be, and the journal's line for it. Nothing for a lamp's first
/// start, when there is no file yet.
pub fn take_back_from(
    path: &std::path::Path,
    room: &mut RoomTold,
    boot: &str,
    now_us: u64,
) -> Option<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => return Some(format!("the room's file could not be read: {error}")),
    };
    let kept: Kept = match serde_json::from_str(&text) {
        Ok(kept) => kept,
        Err(error) => return Some(format!("the room's file is not one: {error}")),
    };
    Some(match room.take_back(&kept, boot, now_us) {
        Ok(age) => format!(
            "the room as it was {} s ago: {}",
            age / 1_000_000,
            match kept.room {
                Room::Unknown => "not yet known",
                Room::Quiet => "quiet",
                Room::Busy => "busy",
            }
        ),
        Err(why) => format!("the room's file is not taken: {why}"),
    })
}

/// In the worker's loop, about once a second: the room written to `path`
/// when it has changed since `last`, or [`KEPT_EVERY_US`] after it was last
/// written. A file that cannot be written loses the room at the next start
/// and nothing else.
pub fn keep_in(
    path: &std::path::Path,
    room: &RoomTold,
    boot: &str,
    now_us: u64,
    last: &mut Option<Kept>,
) -> std::io::Result<bool> {
    let kept = room.kept(boot, now_us);
    if last
        .as_ref()
        .is_some_and(|last| last.same(&kept) && now_us.saturating_sub(last.at_us) < KEPT_EVERY_US)
    {
        return Ok(false);
    }
    let text = serde_json::to_string(&kept).map_err(std::io::Error::other)?;
    crate::thread::write_file(path, &text)?;
    *last = Some(kept);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1712: a call let out ends the count of talk past the lamp, as the
    /// lamp speaking does, and is no talk itself if it ends unanswered.
    #[test]
    fn a_call_let_out_ends_the_count_of_talk_past_the_lamp() {
        let mut room = RoomTold::new();
        room.count_only_talk(true);
        for request in 1..=3 {
            room.unanswered(request, 1_000_000 * request);
        }
        assert!(room.talked_past_the_lamp());
        room.called(4);
        assert!(!room.talked_past_the_lamp());
        assert_eq!(room.left_alone_since_spoke(), 0);
        room.unanswered(4, 5_000_000);
        assert_eq!(room.left_alone_since_spoke(), 0, "the call is no talk");
        room.unanswered(5, 6_000_000);
        room.unanswered(6, 7_000_000);
        assert!(room.talked_past_the_lamp(), "talk after it still counts");
        // Without the switch, nothing is counted either way.
        let mut plain = RoomTold::new();
        plain.called(1);
        assert!(!plain.talked_past_the_lamp());
    }

    const S: u64 = 1_000_000;

    /// Something said to its end in `request`, from `at` for two seconds.
    fn said(room: &mut RoomTold, request: u64, at: u64) {
        room.heard(request, at * S);
        room.hearing((at + 1) * S);
        room.ended(request, (at + 2) * S);
    }

    const BOOT: &str = "0123456789abcdef0123456789abcdef";

    /// #1465, the first builder at 10:57 on lamp-4ace: a voice started
    /// again a minute after it left a quiet room knows the room is quiet at
    /// once, not five minutes later. A busy room taken back ends three
    /// minutes after its last talk, as it would have. A room kept by
    /// another boot, or longer ago than five minutes, is not taken.
    #[test]
    fn a_room_kept_by_the_voice_before_is_taken_back_if_from_this_boot_and_under_five_minutes_old()
    {
        let mut before = RoomTold::new();
        before.count_only_talk(true);
        assert_eq!(before.at(0), Room::Unknown);
        assert_eq!(before.at(301 * S), Room::Quiet);
        let kept = before.kept(BOOT, 302 * S);
        // A minute later, a new voice.
        let mut after = RoomTold::new();
        after.count_only_talk(true);
        assert_eq!(after.take_back(&kept, BOOT, 362 * S), Ok(60 * S));
        assert_eq!(after.at(362 * S), Room::Quiet);
        // Without it, a new voice doesn't know for five minutes.
        let mut fresh = RoomTold::new();
        fresh.count_only_talk(true);
        assert_eq!(fresh.at(362 * S), Room::Unknown);
        // Another boot, or over five minutes: not taken.
        let mut other = RoomTold::new();
        assert_eq!(
            other.take_back(&kept, "ffffffffffffffffffffffffffffffff", 362 * S),
            Err("it is from another boot".into())
        );
        assert_eq!(
            other.take_back(&kept, BOOT, 603 * S),
            Err("it is 301 s old".into())
        );
        assert_eq!(other.at(603 * S), Room::Unknown);
        // A busy room ends 3 min after its last talk, restart or not.
        let mut busy = RoomTold::new();
        busy.count_only_talk(true);
        for (request, at) in [(1, 0), (2, 15), (3, 30)] {
            said(&mut busy, request, at);
            busy.unanswered(request, (at + 6) * S);
        }
        assert_eq!(busy.at(40 * S), Room::Busy);
        let kept = busy.kept(BOOT, 41 * S);
        let mut after = RoomTold::new();
        after.count_only_talk(true);
        assert!(after.take_back(&kept, BOOT, 70 * S).is_ok());
        assert_eq!(after.at(70 * S), Room::Busy);
        assert_eq!(after.at(215 * S), Room::Busy);
        assert_eq!(after.at(217 * S), Room::Quiet);
    }

    /// The room's file: written when the room changes and every 30 s
    /// besides, whole, and read back at the next start with one line
    /// saying what was taken. No file is no line; a broken one is said.
    #[test]
    fn the_room_goes_to_its_file_and_comes_back_from_it() {
        let dir = std::env::temp_dir().join(format!(
            "lamp-room-file-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("room.json");
        let mut fresh = RoomTold::new();
        assert_eq!(take_back_from(&path, &mut fresh, BOOT, S), None);
        let mut room = RoomTold::new();
        assert_eq!(room.at(0), Room::Unknown);
        assert_eq!(room.at(301 * S), Room::Quiet);
        let mut last = None;
        assert!(keep_in(&path, &room, BOOT, 301 * S, &mut last).unwrap());
        // Unchanged: not written again within 30 s, and written after.
        assert!(!keep_in(&path, &room, BOOT, 320 * S, &mut last).unwrap());
        assert!(keep_in(&path, &room, BOOT, 332 * S, &mut last).unwrap());
        let mut after = RoomTold::new();
        assert_eq!(
            take_back_from(&path, &mut after, BOOT, 350 * S).as_deref(),
            Some("the room as it was 18 s ago: quiet")
        );
        assert_eq!(after.at(350 * S), Room::Quiet);
        std::fs::write(&path, "not json").unwrap();
        assert!(
            take_back_from(&path, &mut RoomTold::new(), BOOT, 350 * S)
                .is_some_and(|line| line.starts_with("the room's file is not one"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A room file a later build wrote (#1505's rule): a field this build
    /// doesn't know is read past and the room taken back; a room it doesn't
    /// know is not taken, said in one line, and the room starts as on a
    /// lamp's first start.
    #[test]
    fn a_room_file_from_a_later_build_is_taken_back_or_said() {
        let dir = std::env::temp_dir().join(format!("lamp-room-later-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("room.json");
        let mut room = RoomTold::new();
        assert_eq!(room.at(0), Room::Unknown);
        assert_eq!(room.at(301 * S), Room::Quiet);
        let mut later = serde_json::to_value(room.kept(BOOT, 301 * S)).unwrap();
        later["seats"] = serde_json::json!(2);
        std::fs::write(&path, later.to_string()).unwrap();
        let mut after = RoomTold::new();
        assert_eq!(
            take_back_from(&path, &mut after, BOOT, 320 * S).as_deref(),
            Some("the room as it was 19 s ago: quiet")
        );
        later["room"] = serde_json::json!("lively");
        std::fs::write(&path, later.to_string()).unwrap();
        let mut fresh = RoomTold::new();
        let line = take_back_from(&path, &mut fresh, BOOT, 330 * S).unwrap();
        assert!(line.starts_with("the room's file is not one"), "{line}");
        assert_eq!(
            fresh.kept(BOOT, 330 * S),
            RoomTold::new().kept(BOOT, 330 * S)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The reviewer on #1487: the room is kept under the machine's boot.
    /// Two workers started one after the other in one boot read the same
    /// boot, so the second takes back the room the first kept. The voice's
    /// session `BootId`, new for every session, is not what keys it: no
    /// worker is given one here. Another boot's file is not taken, and with
    /// no boot to read nothing is kept.
    #[test]
    fn two_workers_in_one_boot_share_the_room_under_the_machines_boot() {
        let dir = std::env::temp_dir().join(format!(
            "lamp-room-boot-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (path, boot_file) = (dir.join("room.json"), dir.join("boot_id"));
        std::fs::write(&boot_file, "6d1f0f2e-9a3b-4c11-8f5e-0b7c3d2a1e44\n").unwrap();
        // The first worker: no file yet, the boot read.
        let mut first = RoomTold::new();
        let (line, boot) = start_from(&path, &boot_file, &mut first, 0);
        assert_eq!(line, None);
        let boot = boot.unwrap();
        assert_eq!(boot, "6d1f0f2e-9a3b-4c11-8f5e-0b7c3d2a1e44");
        assert_eq!(first.at(0), Room::Unknown);
        assert_eq!(first.at(301 * S), Room::Quiet);
        keep_in(&path, &first, &boot, 301 * S, &mut None).unwrap();
        // The second, a minute later, in the same boot.
        let mut second = RoomTold::new();
        let (line, again) = start_from(&path, &boot_file, &mut second, 361 * S);
        assert_eq!(line.as_deref(), Some("the room as it was 60 s ago: quiet"));
        assert_eq!(again.as_deref(), Some(boot.as_str()));
        assert_eq!(second.at(361 * S), Room::Quiet);
        // After a reboot, another boot: not taken.
        std::fs::write(&boot_file, "00000000-0000-0000-0000-000000000000").unwrap();
        let mut rebooted = RoomTold::new();
        let (line, _) = start_from(&path, &boot_file, &mut rebooted, 362 * S);
        assert_eq!(
            line.as_deref(),
            Some("the room's file is not taken: it is from another boot")
        );
        // No boot to read: nothing kept.
        std::fs::remove_file(&boot_file).unwrap();
        let (line, none) = start_from(&path, &boot_file, &mut RoomTold::new(), 363 * S);
        assert_eq!(none, None);
        assert!(line.is_some_and(|line| line.starts_with("the room is not kept")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The reviewer on #1487: for a room not yet known, only the first
    /// 30 s of the time no voice listened counts toward its five quiet
    /// minutes. A voice up a minute, then down four, isn't quiet at once,
    /// in case a meeting began meanwhile. A quiet room is kept as it was.
    #[test]
    fn a_room_not_yet_known_counts_only_the_first_of_a_long_gap() {
        let mut before = RoomTold::new();
        assert_eq!(before.at(0), Room::Unknown);
        assert_eq!(before.at(60 * S), Room::Unknown);
        let kept = before.kept("b", 60 * S);
        let mut after = RoomTold::new();
        assert!(after.take_back(&kept, "b", 300 * S).is_ok());
        // Watched 60 s, then 30 s of the gap: quiet 210 s later than five
        // minutes after it began, not at once.
        assert_eq!(after.at(310 * S), Room::Unknown);
        assert_eq!(after.at(509 * S), Room::Unknown);
        assert_eq!(after.at(510 * S), Room::Quiet);
    }

    #[test]
    fn the_room_files_flag_takes_a_short_absolute_path_after_the_switches() {
        assert_eq!(
            file_arguments(Some("/run/lampos-voice/room.json")),
            [FILE_FLAG, "/run/lampos-voice/room.json"]
        );
        assert!(file_arguments(Some("room.json")).is_empty());
        assert!(file_arguments(None).is_empty());
        let flags = [CUT_INS_FLAG, FILE_FLAG, "/run/room.json", "--rest"].map(str::to_owned);
        let (cut_ins, rest) = take_cut_ins(&flags);
        assert!(cut_ins);
        let (file, rest) = take_file(rest).unwrap();
        assert_eq!(
            file.as_deref(),
            Some(std::path::Path::new("/run/room.json"))
        );
        assert_eq!(rest, ["--rest"]);
        assert!(take_file(&[FILE_FLAG.to_owned(), "room.json".into()]).is_err());
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

    /// `LAMP_BUSY_ENDS_SOONER` (the first builder on Queue #1722, 04:00):
    /// two people talk, then a minute and more later one walks up and talks
    /// to the lamp, each line held. Busy ends a minute after the last talk
    /// with the switch, three minutes after it without; talk at 100 s keeps
    /// it busy a minute from then.
    #[test]
    fn with_the_switch_busy_ends_a_minute_after_the_last_talk_when_only_held_lines_follow() {
        let room_after = |sooner: bool, more_talk: Option<u64>| {
            let mut room = RoomTold::new();
            room.count_only_talk(true);
            room.end_busy_sooner(sooner);
            for (request, at) in [(1, 0), (2, 15), (3, 30)] {
                said(&mut room, request, at);
                room.unanswered(request, (at + 6) * S);
            }
            assert_eq!(room.at(40 * S), Room::Busy);
            if let Some(at) = more_talk {
                said(&mut room, 4, at);
                assert!(room.unanswered(4, (at + 6) * S));
            }
            for (request, at) in [(10, 120), (11, 137), (12, 159)] {
                said(&mut room, request, at);
                room.held(request);
                assert_eq!(room.left_unanswered(request, (at + 2) * S), Left::Held);
            }
            room.at(161 * S)
        };
        assert_eq!(room_after(false, None), Room::Busy, "without: 216 s");
        assert_eq!(room_after(true, None), Room::Quiet, "with: 96 s");
        assert_eq!(
            room_after(true, Some(100)),
            Room::Busy,
            "talk at 106 s: 166 s"
        );
    }

    /// `LAMP_BUSY_ENDS_SOONER` (the reviewer's P2 on #2372, the first
    /// builder at 04:36): two people talk on in the busy room they made,
    /// each line held as `BUSY_UNCALLED` or `BUSY_NAMED_UNTURNED`. With the
    /// switch their lines are talk: it stays busy while they talk, and quiet
    /// comes a minute after their last. Without it, held lines are not talk
    /// and busy ends three minutes after the run.
    #[test]
    fn with_the_switch_lines_held_only_as_a_busy_rooms_are_talk() {
        use crate::attention::{BUSY_NAMED_UNTURNED, BUSY_UNCALLED, Verdict};
        let rooms = |sooner: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(true);
            room.end_busy_sooner(sooner);
            for (request, at) in [(1, 0), (2, 15), (3, 30)] {
                said(&mut room, request, at);
                room.unanswered(request, (at + 6) * S);
            }
            assert_eq!(room.at(40 * S), Room::Busy);
            // A held line every 10 s from 40 to 150 s, closed 2 s after
            // it began: the last talk at 152 s.
            for n in 0..12u64 {
                let at = 40 + n * 10;
                let why = if n % 2 == 0 {
                    BUSY_UNCALLED
                } else {
                    BUSY_NAMED_UNTURNED
                };
                said(&mut room, 10 + n, at);
                room.judged(10 + n, Some(Verdict::Hold(why)), false);
                room.left_unanswered(10 + n, (at + 2) * S);
                assert_eq!(room.at((at + 3) * S), Room::Busy, "{sooner} at {at} s");
            }
            [room.at(211 * S), room.at(213 * S)]
        };
        assert_eq!(rooms(true), [Room::Busy, Room::Quiet], "with: 212 s");
        // Without: the run's last talk at 36 s, busy until 216 s.
        assert_eq!(rooms(false), [Room::Busy, Room::Busy], "without");
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

    /// #1522 (the first builder, 19:08; Eighth on #1594): with
    /// `LAMP_GOING_ON_ASKS`, once the lamp has answered since two people last
    /// talked, their busy room ends a minute after the lamp's first answer
    /// since that talk, not three minutes after the talk; while they go on
    /// talking it stays busy. On lamp-4ace at 18:51 a day line 108 s after
    /// the talk was still in a busy room. A call made more than a minute
    /// after the talk keeps the room busy for a minute from its answer, so
    /// the talk after it is still judged in a busy room. Without the switch,
    /// three minutes.
    #[test]
    fn with_the_switch_a_busy_room_ends_a_minute_after_the_lamps_first_answer_since_its_talk() {
        const S: u64 = 1_000_000;
        let busy_then_answered = |on: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(true);
            room.end_busy_once_answered(on);
            for (request, at) in [(1, 10), (2, 20), (3, 30)] {
                room.heard(request, at * S);
                room.ended(request, at * S + S / 2);
                room.left_unanswered(request, at * S + S);
            }
            assert_eq!(room.at(31 * S), Room::Busy);
            // Someone turns to the lamp, and it answers.
            room.heard(4, 36 * S);
            room.ended(4, 37 * S);
            room.spoke(4, 38 * S);
            room
        };
        // The last talk was left at 31 s, the lamp's first answer since it
        // came at 38 s: busy until a minute after that.
        let mut room = busy_then_answered(true);
        assert_eq!(room.at(97 * S), Room::Busy);
        assert_eq!(room.at(99 * S), Room::Quiet);
        // A later answer doesn't move it: the first one counts.
        let mut room = busy_then_answered(true);
        room.heard(6, 60 * S);
        room.ended(6, 61 * S);
        room.spoke(6, 62 * S);
        assert_eq!(room.at(99 * S), Room::Quiet);
        // A call answered 70 s after the talk (Eighth's probe): still busy
        // at it and for a minute after, not quiet from the answer on.
        let mut room = RoomTold::new();
        room.count_only_talk(true);
        room.end_busy_once_answered(true);
        for (request, at) in [(1, 10), (2, 20), (3, 30)] {
            room.heard(request, at * S);
            room.ended(request, at * S + S / 2);
            room.left_unanswered(request, at * S + S);
        }
        assert_eq!(room.at(31 * S), Room::Busy);
        room.heard(4, 99 * S);
        room.ended(4, 100 * S);
        room.spoke(4, 101 * S);
        assert_eq!(room.at(102 * S), Room::Busy);
        assert_eq!(room.at(160 * S), Room::Busy);
        assert_eq!(room.at(162 * S), Room::Quiet);
        let mut room = busy_then_answered(false);
        assert_eq!(room.at(92 * S), Room::Busy);
        assert_eq!(room.at(212 * S), Room::Quiet);
        // Talk after the answer: busy as before, three minutes from it.
        let mut room = busy_then_answered(true);
        room.heard(5, 40 * S);
        room.ended(5, 41 * S);
        room.left_unanswered(5, 42 * S);
        assert_eq!(room.at(103 * S), Room::Busy);
        assert_eq!(room.at(223 * S), Room::Quiet);
    }

    /// The reviewer and Jarvis on #1594, with `LAMP_GOING_ON_ASKS`, as the
    /// first builder ruled (20:58): two people's lines the Policy holds keep
    /// their busy room busy and end no conversation; a reminder the lamp
    /// rings answers no one; the minute never runs past the three minutes;
    /// and a restart, which keeps no answer, falls back to them.
    #[test]
    fn with_the_switch_held_talk_keeps_a_room_busy_and_a_reminder_or_a_restart_ends_nothing() {
        const S: u64 = 1_000_000;
        let busy = |on: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(true);
            room.end_busy_once_answered(on);
            for (request, at) in [(1, 10), (2, 20), (3, 30)] {
                room.heard(request, at * S);
                room.ended(request, at * S + S / 2);
                room.left_unanswered(request, at * S + S);
            }
            assert_eq!(room.at(31 * S), Room::Busy);
            room
        };
        let answered = |room: &mut RoomTold, request: u64, at: u64| {
            room.heard(request, (at - 2) * S);
            room.ended(request, (at - 1) * S);
            room.spoke(request, at * S);
        };
        // A call answered at 38 s, then the two talk on every 6 s, each line
        // held by the Policy: busy throughout, and the conversation with the
        // lamp not ended, so a question to it is still asked of the Policy.
        let held = crate::attention::Verdict::Hold(crate::going_on_asks::NOT_THE_LAMPS);
        let mut room = busy(true);
        answered(&mut room, 4, 38);
        for (request, at) in (5..).zip((44..=200).step_by(6)) {
            room.heard(request, at * S);
            room.ended(request, at * S + S / 2);
            room.judged(request, Some(held), false);
            assert_eq!(room.left_unanswered(request, at * S + S), Left::Held);
            assert_eq!(room.at(at * S + S), Room::Busy, "{at} s");
            assert!(!room.talked_past_the_lamp(), "{at} s");
        }
        // The last of it left at 201 s: three minutes from it.
        assert_eq!(room.at(380 * S), Room::Busy);
        assert_eq!(room.at(382 * S), Room::Quiet);
        // A reminder rung at 36 s, in their pause, answers no one: busy for
        // three minutes from the talk, as without it.
        let mut room = busy(true);
        room.spoke_unasked(40, 36 * S);
        assert_eq!(room.at(97 * S), Room::Busy);
        assert_eq!(room.at(210 * S), Room::Busy);
        assert_eq!(room.at(212 * S), Room::Quiet);
        // Like any talk, two lines the Policy holds inside a minute make the
        // quiet room busy again: they join the run as well as the clock.
        for (request, at) in [(41, 220), (42, 230)] {
            room.heard(request, at * S);
            room.ended(request, at * S + S / 2);
            room.judged(request, Some(held), false);
            room.left_unanswered(request, at * S + S);
        }
        assert_eq!(room.at(231 * S), Room::Busy);
        // A first answer 170 s after the talk: busy no longer than the three
        // minutes, not a minute past them.
        let mut room = busy(true);
        answered(&mut room, 4, 201);
        assert_eq!(room.at(210 * S), Room::Busy);
        assert_eq!(room.at(212 * S), Room::Quiet);
        // A new voice at 55 s, after an answer at 38 s, keeps the busy room
        // and its talk but not the answer: three minutes from the talk, as
        // without the switch, unless it answers again.
        let mut room = busy(true);
        answered(&mut room, 4, 38);
        let kept = room.kept(BOOT, 55 * S);
        let mut after = RoomTold::new();
        after.count_only_talk(true);
        after.end_busy_once_answered(true);
        assert!(after.take_back(&kept, BOOT, 56 * S).is_ok());
        assert_eq!(after.at(100 * S), Room::Busy);
        assert_eq!(after.at(210 * S), Room::Busy);
        assert_eq!(after.at(212 * S), Room::Quiet);
    }

    /// #829: the conversation is over only by another voice until talk is
    /// left unanswered, or the lamp speaks; talk first, then another voice,
    /// is over by talk.
    #[test]
    fn a_conversation_is_over_only_by_another_voice_until_talk_or_the_lamp() {
        const S: u64 = 1_000_000;
        let mut room = RoomTold::new();
        room.count_only_talk(true);
        room.heard(1, S);
        room.ended(1, 2 * S);
        room.spoke(1, 3 * S);
        assert!(!room.ended_only_by_another_voice());
        room.heard(2, 4 * S);
        room.ended(2, 5 * S);
        room.another_voice_held(2);
        assert!(room.talked_past_the_lamp() && room.ended_only_by_another_voice());
        room.left_unanswered(2, 6 * S);
        assert!(
            room.ended_only_by_another_voice(),
            "its own close counts no talk"
        );
        room.heard(3, 7 * S);
        room.ended(3, 8 * S);
        room.left_unanswered(3, 9 * S);
        assert!(room.talked_past_the_lamp() && !room.ended_only_by_another_voice());
        room.spoke(4, 10 * S);
        assert!(!room.talked_past_the_lamp() && !room.ended_only_by_another_voice());
        // Talk first, then another voice: over by talk.
        for request in [5, 6] {
            room.heard(request, request * 2 * S);
            room.ended(request, request * 2 * S + S / 2);
            room.left_unanswered(request, request * 2 * S + S);
        }
        assert!(room.talked_past_the_lamp());
        room.another_voice_held(7);
        assert!(!room.ended_only_by_another_voice());
        // Without only talk counted, nothing is over.
        let mut off = RoomTold::new();
        off.another_voice_held(1);
        assert!(!off.ended_only_by_another_voice());
    }

    /// `LAMP_NAME_OR_FOLLOW_UP`, with only talk counted (`LAMP_SPOKEN_TO`):
    /// in a quiet room, lines the lamp's rule held as said to nobody, said
    /// with someone in view and turned away, are talk, and the second makes
    /// the room busy. A lone voice's held lines, nobody in view, are not: its
    /// three leave the room quiet (lamp-4ace, 22 Oct: one unseen person's
    /// misheard call and the lines after it turned the room busy, and the
    /// scene was lost). Without the switch they are held lines; one misheard
    /// as another script is no talk; and in a busy room held lines are as
    /// before.
    #[test]
    fn with_name_or_follow_up_held_lines_are_talk_only_with_someone_in_view_turned_away() {
        use crate::attention::{NOT_SPOKEN_TO, Verdict};
        const S: u64 = 1_000_000;
        let quiet = |on: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(true);
            room.count_held_talk(on);
            room.at(0);
            assert_eq!(room.at(301 * S), Room::Quiet);
            room
        };
        let line = |room: &mut RoomTold, request: u64, at: u64, why: &'static str, seen: bool| {
            room.heard(request, at * S);
            room.ended(request, at * S + S / 2);
            if seen {
                room.seen_talking(request);
            }
            room.judged(request, Some(Verdict::Hold(why)), false);
            room.left_unanswered(request, at * S + S)
        };
        // A lone voice, nobody in view.
        let mut room = quiet(true);
        for (request, at) in [(1, 310), (2, 315), (3, 320)] {
            assert_eq!(
                line(&mut room, request, at, NOT_SPOKEN_TO, false),
                Left::Held
            );
        }
        assert_eq!(room.at(322 * S), Room::Quiet);
        // Someone in view, turned away.
        let mut room = quiet(true);
        assert_eq!(line(&mut room, 1, 310, NOT_SPOKEN_TO, true), Left::Talk);
        assert_eq!(room.at(312 * S), Room::Quiet);
        assert_eq!(line(&mut room, 2, 315, NOT_SPOKEN_TO, true), Left::Talk);
        assert_eq!(room.at(317 * S), Room::Busy);
        // Busy now: a held line is a held line again.
        assert_eq!(line(&mut room, 3, 320, NOT_SPOKEN_TO, true), Left::Held);
        let mut room = quiet(false);
        assert_eq!(line(&mut room, 1, 310, NOT_SPOKEN_TO, true), Left::Held);
        assert_eq!(line(&mut room, 2, 315, NOT_SPOKEN_TO, true), Left::Held);
        assert_eq!(room.at(317 * S), Room::Quiet);
        let mut room = quiet(true);
        let misheard = crate::not_speech::OTHER_SCRIPT;
        assert_eq!(line(&mut room, 1, 310, misheard, true), Left::Held);
        assert_eq!(line(&mut room, 2, 315, misheard, true), Left::Held);
        assert_eq!(room.at(317 * S), Room::Quiet);
        // Another voice, nobody in view: busy at once, as before.
        let mut room = quiet(true);
        room.heard(1, 310 * S);
        room.ended(1, 311 * S);
        room.judged(1, Some(Verdict::Hold(NOT_SPOKEN_TO)), true);
        assert_eq!(room.at(312 * S), Room::Busy);
    }

    /// `LAMP_TURNED_FACE_SPEAKS` (lamp-52e6, 24 Oct 05:31): lines held
    /// while the only face turned to the lamp was seen not speaking them are
    /// talk, whatever `LAMP_NAME_OR_FOLLOW_UP`: three within a minute make a
    /// room not yet known busy (two a quiet one, `QUIET_ENDS_RUN`), and in a
    /// busy room they keep it so. The same lines
    /// held with no such face are held lines, and the room stays quiet: a
    /// lone voice with nobody in view still never makes it busy.
    #[test]
    fn lines_held_past_a_still_face_turned_to_the_lamp_are_talk() {
        use crate::attention::{BUSY_UNCALLED, NOT_SPOKEN_TO, Verdict};
        const S: u64 = 1_000_000;
        let unknown = || {
            let mut room = RoomTold::new();
            room.count_only_talk(true);
            room
        };
        let quiet = || {
            let mut room = unknown();
            room.at(0);
            assert_eq!(room.at(301 * S), Room::Quiet);
            room
        };
        let line = |room: &mut RoomTold, request: u64, at: u64, why: &'static str, still: bool| {
            room.heard(request, at * S);
            room.ended(request, at * S + S / 2);
            if still {
                room.face_not_speaking(request);
            }
            room.judged(request, Some(Verdict::Hold(why)), false);
            room.left_unanswered(request, at * S + S)
        };
        let mut room = unknown();
        for (request, at) in [(1, 310), (2, 320)] {
            assert_eq!(
                line(&mut room, request, at, NOT_SPOKEN_TO, true),
                Left::Talk
            );
            assert_eq!(room.at(at * S + 2 * S), Room::Unknown, "line {request}");
        }
        assert_eq!(line(&mut room, 3, 330, NOT_SPOKEN_TO, true), Left::Talk);
        assert_eq!(room.at(332 * S), Room::Busy, "three within a minute");
        assert_eq!(line(&mut room, 4, 340, BUSY_UNCALLED, true), Left::Talk);
        assert_eq!(room.at(342 * S), Room::Busy);
        let mut room = quiet();
        assert_eq!(line(&mut room, 1, 310, NOT_SPOKEN_TO, true), Left::Talk);
        assert_eq!(room.at(312 * S), Room::Quiet);
        assert_eq!(line(&mut room, 2, 320, NOT_SPOKEN_TO, true), Left::Talk);
        assert_eq!(room.at(322 * S), Room::Busy, "two from a quiet room");
        for mut room in [unknown(), quiet()] {
            for (request, at) in [(1, 310), (2, 320), (3, 330)] {
                assert_eq!(
                    line(&mut room, request, at, NOT_SPOKEN_TO, false),
                    Left::Held
                );
            }
            assert_ne!(room.at(332 * S), Room::Busy, "no face read still");
        }
    }

    /// `LAMP_NAME_OR_FOLLOW_UP`: two people at the lamp, two faces in view
    /// or a line in another voice, make the room busy at once, and it ends
    /// as after talk, here three minutes after they were last seen. Without
    /// the switch nothing changes.
    #[test]
    fn with_name_or_follow_up_two_people_make_the_room_busy_at_once() {
        const S: u64 = 1_000_000;
        let mut room = RoomTold::new();
        room.count_only_talk(true);
        room.count_held_talk(true);
        assert_eq!(room.at(10 * S), Room::Unknown);
        assert!(room.two_people(20 * S));
        assert_eq!(room.at(21 * S), Room::Busy);
        assert!(!room.two_people(60 * S), "busy already");
        assert_eq!(room.at(239 * S), Room::Busy);
        assert_eq!(room.at(241 * S), Room::Quiet);
        // Another voice's line, held or played.
        let mut voices = RoomTold::new();
        voices.count_only_talk(true);
        voices.count_held_talk(true);
        voices.heard(1, 30 * S);
        voices.ended(1, 31 * S);
        voices.judged(1, Some(crate::attention::Verdict::Play("a face")), true);
        assert_eq!(voices.at(32 * S), Room::Busy);
        let mut off = RoomTold::new();
        off.count_only_talk(true);
        assert!(!off.two_people(20 * S));
        off.heard(1, 30 * S);
        off.ended(1, 31 * S);
        off.judged(1, Some(crate::attention::Verdict::Play("a face")), true);
        assert_eq!(off.at(32 * S), Room::Unknown);
    }

    /// `LAMP_SAME_SPEAKER_CARRIES`: lines let out only as the same speaker's
    /// conversation carried on, then left unanswered by the model, are held,
    /// not talk: three in a minute neither end the conversation nor make the
    /// room busy. The same lines not carried are talk, and do both.
    #[test]
    fn a_carried_line_left_unanswered_is_not_talk() {
        let room_after = |carried: bool| {
            let mut room = RoomTold::new();
            room.count_only_talk(true);
            room.end_busy_sooner(true);
            for (request, at) in [(1, 0), (2, 15), (3, 30)] {
                said(&mut room, request, at);
                if carried {
                    room.carried(request);
                }
                let left = room.left_unanswered(request, (at + 6) * S);
                assert_eq!(left == Left::Held, carried, "request {request}");
            }
            (room.at(40 * S), room.talked_past_the_lamp())
        };
        assert_eq!(room_after(true), (Room::Unknown, false), "carried");
        assert_eq!(room_after(false), (Room::Busy, true), "not carried");
    }
}
