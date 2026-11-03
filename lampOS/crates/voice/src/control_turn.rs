//! A turn a paired computer opens through lamp-control (`docs/control.md`,
//! "Turns and listening"), the voice's side of it: the say socket's
//! `turn: "start"` and `turn: "end"` ([`crate::say`]), decided here and
//! carried out by the coordinator.
//!
//! - **Opened** as a tap on the head opens one: the listening cue and face,
//!   then a window of [`OPEN_FOR_US`] in which the next line the person
//!   starts is answered as spoken to the lamp. With a prompt, the lamp says
//!   it first, as a written say, and the window opens when it has been
//!   heard whole. The asker is told `begun`, with the turn's id, as the
//!   window opens.
//! - **Refused**, never queued, while the lamp is muted or paused, while a
//!   conversation goes on and while the lamp speaks its own words
//!   ([`refused`]). Every other gate is a say's: the turn waits for them as
//!   a say does, until its own expiry.
//! - **Answered** `spoken` once the person spoke in it and the lamp's
//!   answer sounded (played whole or cut), `dropped` with "nobody spoke"
//!   when the window passes unused, or `dropped` with the voice's why.
//! - **Ended** by `turn: "end"` with its id, only while its window is open
//!   or the conversation its first line began goes on: the coordinator then
//!   cancels as the button's stop does. Any other end is refused, so a
//!   computer never ends the person's own conversation or the lamp's words.
//! - **The person wins:** the privacy switch, the button, sleep and "stop"
//!   end a computer's turn as any other ([`ControlTurns::cancelled`]).
//!
//! The journal names the skill and the turn's id, never a word heard.
use crate::say::{Answer, Outcome, Ticket};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc,
};

/// How long after a computer's turn opens a line the person starts is
/// answered as spoken to the lamp. Longer than the tap's
/// ([`crate::attention::TAPPED_BEFORE_US`]): the person was not the one who
/// asked, and may need a moment to notice. The design's 8 s, not measured.
pub const OPEN_FOR_US: u64 = 8_000_000;
/// Why a turn whose window passed unused is dropped.
pub const NOBODY_SPOKE: &str = "nobody spoke";
/// Why a turn its computer ended is dropped, when nothing was answered.
pub const ENDED_BY_COMPUTER: &str = "ended by the computer";
/// Why an end that names no turn of a computer's going on is refused.
pub const NOT_THIS_TURN: &str = "not this computer's turn";
/// Why a turn is refused while the microphone is not allowed.
pub const MUTED: &str = "the lamp is muted";
/// Why a turn is refused while the keeper's pause holds.
pub const PAUSED: &str = "the lamp is paused";
/// Why a turn is refused while someone talks with the lamp.
pub const CONVERSATION: &str = "a conversation is going on";
/// Why a turn is refused while the lamp says its own words.
pub const SPEAKING: &str = "the lamp is speaking";
/// Why a turn whose person's line was not answered is dropped.
pub const NOT_ANSWERED: &str = "not answered";

/// Turn ids, unique for the voice's life, so an id from an earlier
/// conversation never names a later turn.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    /// The id of the turn this thread last opened, for a test of the loop
    /// that ends it.
    static LAST_OPENED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The id of the turn this thread's loop last opened.
#[cfg(test)]
pub(crate) fn last_opened_for_test() -> u64 {
    LAST_OPENED.with(std::cell::Cell::get)
}

/// Whether a person's line in hand is an answer the lamp gives: something
/// of an answer has sounded or come. Before that the second route has not
/// decided the line was said to the lamp; a room line heard and then held
/// back as said to nobody is no person's answer and no words of the
/// lamp's (lamp-75c9, 3 Nov 17:51: such a line refused a computer's turn).
pub fn answer_decided(had_audio: bool, sounded: bool) -> bool {
    had_audio || sounded
}

/// What the lamp is doing as a turn start is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartNow {
    /// The microphone is not allowed: the switch, the button's mute, sleep.
    pub muted: bool,
    /// The keeper's pause.
    pub paused: bool,
    /// Someone talks with the lamp: a conversation with it going on as the
    /// second route judges one (the thread's exchange with the lamp's words
    /// in it, within `thread::GOING_ON_FOR`), a person's answer in hand, or
    /// a computer's turn still going on. Never the room's speech, a line
    /// heard and held back, or a look the voice wanted.
    pub conversation: bool,
    /// The lamp's own words or sounds are in hand or playing.
    pub speaking: bool,
}

/// Why a turn start is refused now, if it is: the person's mute first,
/// then the conversation, then the lamp's own words.
pub fn refused(now: &StartNow) -> Option<&'static str> {
    if now.muted {
        Some(MUTED)
    } else if now.paused {
        Some(PAUSED)
    } else if now.conversation {
        Some(CONVERSATION)
    } else if now.speaking {
        Some(SPEAKING)
    } else {
        None
    }
}

enum Phase {
    /// Its prompt is said, as a written say whose answer comes here.
    Prompting(mpsc::Receiver<Answer>),
    /// The window, open since this moment.
    Open { since_us: u64 },
    /// A line the person began in it, the request whose answer is waited
    /// for, and whether that answer has sounded; once answered, the
    /// conversation it began goes on until it is quiet.
    Grown {
        request: u64,
        sounded: bool,
        answered: bool,
    },
}

struct Turn {
    id: u64,
    skill: String,
    /// The start's ticket, until its outcome is told.
    asker: Option<Ticket>,
    phase: Phase,
}

impl Turn {
    fn tell(&mut self, outcome: Outcome) {
        if let Some(asker) = self.asker.take() {
            asker.answer(outcome);
        }
    }

    fn open(&mut self, at_us: u64) {
        #[cfg(test)]
        LAST_OPENED.with(|last| last.set(self.id));
        if let Some(asker) = self.asker.as_ref() {
            asker.begun_turn(at_us, self.id);
        }
        eprintln!(
            "lamp-voice: the skill {} opened turn {}: a line begun within {} s is answered",
            self.skill,
            self.id,
            OPEN_FOR_US / 1_000_000
        );
    }
}

/// A computer's turn, at most one at a time.
#[derive(Default)]
pub struct ControlTurns {
    turn: Option<Turn>,
}

impl ControlTurns {
    /// Whether a computer's turn goes on: its prompt, its window, or the
    /// conversation that grew from it. Another start is refused meanwhile.
    pub fn going_on(&self) -> bool {
        self.turn.is_some()
    }

    /// The turn's id, if one goes on.
    pub fn id(&self) -> Option<u64> {
        self.turn.as_ref().map(|turn| turn.id)
    }

    /// Open a turn with no prompt at `at_us`, as the listening cue begins:
    /// the window opens now, and the asker is told `begun` with its id.
    pub fn open(&mut self, asker: Ticket, at_us: u64) -> u64 {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut turn = Turn {
            id,
            skill: asker.skill().to_owned(),
            asker: Some(asker),
            phase: Phase::Open { since_us: at_us },
        };
        turn.open(at_us);
        self.turn = Some(turn);
        id
    }

    /// Begin a turn whose prompt is said first: the ticket for the prompt's
    /// written say, whose answer opens the window ([`Self::tick`]).
    pub fn prompt(&mut self, asker: Ticket) -> Ticket {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (said, answered) = Ticket::own(asker.skill());
        self.turn = Some(Turn {
            id,
            skill: asker.skill().to_owned(),
            asker: Some(asker),
            phase: Phase::Prompting(answered),
        });
        said
    }

    /// The person's line was admitted as `request` at `at_us`: whether it
    /// began in the turn, so that it is marked a call
    /// (`ProviderInput::TurnOpened`). A line over the prompt answers it, and
    /// one more while the first is unanswered goes on with it.
    pub fn admitted(&mut self, request: u64, at_us: u64) -> bool {
        let Some(turn) = self.turn.as_mut() else {
            return false;
        };
        let prompting = match &turn.phase {
            Phase::Prompting(_) => true,
            Phase::Open { since_us } => {
                if at_us > since_us.saturating_add(OPEN_FOR_US) {
                    return false;
                }
                false
            }
            Phase::Grown { answered, .. } => {
                if *answered {
                    return false;
                }
                false
            }
        };
        if prompting {
            turn.open(at_us);
        }
        turn.phase = Phase::Grown {
            request,
            sounded: false,
            answered: false,
        };
        true
    }

    /// Each pass of the voice's loop, at `now_us`: the turn in hand and
    /// whether it has sounded, and whether the conversation is over (no
    /// turn in hand, quiet as long as a say waits for).
    pub fn tick(&mut self, now_us: u64, in_hand: Option<(u64, bool)>, quiet: bool) {
        let Some(turn) = self.turn.as_mut() else {
            return;
        };
        let mut over = false;
        let mut opens = false;
        match &mut turn.phase {
            Phase::Prompting(said) => match said.try_recv() {
                Ok(Answer::Done(Outcome::Spoken)) => opens = true,
                Ok(Answer::Done(outcome)) => {
                    let why = match outcome {
                        Outcome::Dropped(why) | Outcome::Refused(why) => why,
                        _ => "the prompt was not said".to_owned(),
                    };
                    turn.tell(Outcome::Dropped(why));
                    over = true;
                }
                Ok(Answer::Begun(_) | Answer::BegunTurn { .. })
                | Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    turn.tell(Outcome::Dropped("ended".into()));
                    over = true;
                }
            },
            Phase::Open { since_us } => {
                if now_us > since_us.saturating_add(OPEN_FOR_US) {
                    turn.tell(Outcome::Dropped(NOBODY_SPOKE.into()));
                    over = true;
                }
            }
            Phase::Grown {
                request,
                sounded,
                answered,
            } if !*answered => {
                let outcome = match in_hand {
                    Some((turn_in_hand, its_sound)) if turn_in_hand == *request => {
                        *sounded |= its_sound;
                        None
                    }
                    // The same line going on as a newer request: its answer
                    // is the one waited for.
                    Some((turn_in_hand, its_sound)) if turn_in_hand > *request => {
                        *request = turn_in_hand;
                        *sounded = its_sound;
                        None
                    }
                    _ => {
                        *answered = true;
                        Some(if *sounded {
                            Outcome::Spoken
                        } else {
                            Outcome::Dropped(NOT_ANSWERED.into())
                        })
                    }
                };
                if let Some(outcome) = outcome {
                    turn.tell(outcome);
                }
            }
            Phase::Grown { .. } => over = quiet,
        }
        if opens {
            turn.phase = Phase::Open { since_us: now_us };
            turn.open(now_us);
        }
        if over {
            self.turn = None;
        }
    }

    /// Whether a `turn: "end"` naming `id` ends something: its prompt, its
    /// open window, or the conversation that grew from it, not yet over.
    pub fn ends(&self, id: u64) -> bool {
        self.turn.as_ref().is_some_and(|turn| turn.id == id)
    }

    /// The conversation was ended for `why`, by the person, the voice or
    /// the turn's computer: the turn ends with it, answered `spoken` if its
    /// answer had sounded, else `dropped` with why.
    pub fn cancelled(&mut self, why: &str) {
        let Some(mut turn) = self.turn.take() else {
            return;
        };
        let sounded = matches!(turn.phase, Phase::Grown { sounded: true, .. });
        turn.tell(if sounded {
            Outcome::Spoken
        } else {
            Outcome::Dropped(why.to_owned())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_person_line_is_an_answer_in_hand_only_once_something_of_it_came() {
        // Heard and still judged, or held back as said to nobody.
        assert!(!answer_decided(false, false));
        assert!(answer_decided(true, false));
        assert!(answer_decided(false, true));
    }
    use crate::say::ticket_for_test;

    const S: u64 = 1_000_000;

    fn answers(answered: &mpsc::Receiver<Answer>) -> Vec<Answer> {
        answered.try_iter().collect()
    }

    #[test]
    fn a_start_is_refused_while_muted_paused_in_a_conversation_or_speaking() {
        assert_eq!(refused(&StartNow::default()), None);
        let all = StartNow {
            muted: true,
            paused: true,
            conversation: true,
            speaking: true,
        };
        assert_eq!(refused(&all), Some(MUTED));
        assert_eq!(
            refused(&StartNow {
                muted: false,
                ..all
            }),
            Some(PAUSED)
        );
        assert_eq!(
            refused(&StartNow {
                conversation: true,
                ..StartNow::default()
            }),
            Some(CONVERSATION)
        );
        assert_eq!(
            refused(&StartNow {
                speaking: true,
                ..StartNow::default()
            }),
            Some(SPEAKING)
        );
        // A turn of a computer's going on is a conversation: no second.
        let mut turns = ControlTurns::default();
        let (asker, _answered) = ticket_for_test("control");
        turns.open(asker, 10 * S);
        assert!(turns.going_on());
    }

    #[test]
    fn the_window_opens_with_an_id_and_a_line_within_eight_seconds_is_the_turns() {
        let mut turns = ControlTurns::default();
        let (asker, answered) = ticket_for_test("control");
        let id = turns.open(asker, 10 * S);
        assert_eq!(
            answers(&answered),
            vec![Answer::BegunTurn { at_us: 10 * S, id }]
        );
        assert_eq!(OPEN_FOR_US, 8 * S);
        // Within the window, at its last moment too.
        turns.tick(10 * S + OPEN_FOR_US, None, false);
        assert!(turns.admitted(7, 10 * S + OPEN_FOR_US));
        // Its answer sounds, then ends: spoken.
        turns.tick(19 * S, Some((7, false)), false);
        turns.tick(20 * S, Some((7, true)), false);
        assert!(answers(&answered).is_empty());
        turns.tick(21 * S, None, false);
        assert_eq!(answers(&answered), vec![Answer::Done(Outcome::Spoken)]);
        // The conversation goes on, and may still be ended, until quiet.
        assert!(turns.ends(id));
        turns.tick(40 * S, None, true);
        assert!(!turns.going_on());
        assert!(!turns.ends(id));
    }

    #[test]
    fn a_line_after_the_window_is_not_the_turns_which_is_dropped_as_nobody_spoke() {
        let mut turns = ControlTurns::default();
        let (asker, answered) = ticket_for_test("control");
        turns.open(asker, 10 * S);
        let _ = answers(&answered);
        assert!(!turns.admitted(3, 10 * S + OPEN_FOR_US + 1));
        turns.tick(10 * S + OPEN_FOR_US + 1, Some((3, true)), false);
        assert_eq!(
            answers(&answered),
            vec![Answer::Done(Outcome::Dropped(NOBODY_SPOKE.into()))]
        );
        assert!(!turns.going_on());
    }

    #[test]
    fn a_line_not_answered_is_dropped_and_one_going_on_is_followed() {
        let mut turns = ControlTurns::default();
        let (asker, answered) = ticket_for_test("control");
        turns.open(asker, 10 * S);
        let _ = answers(&answered);
        assert!(turns.admitted(4, 11 * S));
        // Its line goes on as a newer request, which is answered.
        assert!(turns.admitted(5, 12 * S));
        turns.tick(13 * S, Some((6, true)), false);
        turns.tick(14 * S, None, false);
        assert_eq!(answers(&answered), vec![Answer::Done(Outcome::Spoken)]);
        // Another turn whose line is held unanswered.
        let mut turns = ControlTurns::default();
        let (asker, answered) = ticket_for_test("control");
        turns.open(asker, 10 * S);
        let _ = answers(&answered);
        assert!(turns.admitted(4, 11 * S));
        turns.tick(12 * S, Some((4, false)), false);
        turns.tick(13 * S, None, false);
        assert_eq!(
            answers(&answered),
            vec![Answer::Done(Outcome::Dropped(NOT_ANSWERED.into()))]
        );
    }

    #[test]
    fn a_prompt_heard_whole_opens_the_window_and_one_cut_drops_the_turn() {
        let mut turns = ControlTurns::default();
        let (asker, answered) = ticket_for_test("control");
        let said = turns.prompt(asker);
        said.begun(5 * S);
        turns.tick(5 * S, Some((2, true)), false);
        // Nothing is told while the prompt plays.
        assert!(answers(&answered).is_empty());
        said.answer(Outcome::Spoken);
        turns.tick(8 * S, None, false);
        let id = turns.id().unwrap();
        assert_eq!(
            answers(&answered),
            vec![Answer::BegunTurn { at_us: 8 * S, id }]
        );
        // The window runs from the prompt's end.
        turns.tick(8 * S + OPEN_FOR_US, None, false);
        assert!(turns.going_on());
        turns.tick(8 * S + OPEN_FOR_US + 1, None, false);
        assert_eq!(
            answers(&answered),
            vec![Answer::Done(Outcome::Dropped(NOBODY_SPOKE.into()))]
        );
        // A prompt cut by the button: dropped with why.
        let mut turns = ControlTurns::default();
        let (asker, answered) = ticket_for_test("control");
        let said = turns.prompt(asker);
        said.answer(Outcome::Dropped("hand_stop".into()));
        turns.tick(6 * S, None, false);
        assert_eq!(
            answers(&answered),
            vec![Answer::Done(Outcome::Dropped("hand_stop".into()))]
        );
        assert!(!turns.going_on());
        // A line said over the prompt answers it: the window opens there.
        let mut turns = ControlTurns::default();
        let (asker, answered) = ticket_for_test("control");
        let said = turns.prompt(asker);
        assert!(turns.admitted(9, 7 * S));
        drop(said);
        let id = turns.id().unwrap();
        turns.tick(7 * S, Some((9, false)), false);
        assert_eq!(
            answers(&answered),
            vec![Answer::BegunTurn { at_us: 7 * S, id }]
        );
    }

    #[test]
    fn an_end_names_only_the_turn_going_on_and_ends_it_once() {
        let mut turns = ControlTurns::default();
        let (asker, answered) = ticket_for_test("control");
        let old = turns.open(asker, 10 * S);
        turns.tick(10 * S + OPEN_FOR_US + 1, None, false);
        let _ = answers(&answered);
        // An old id names nothing, and nothing goes on.
        assert!(!turns.ends(old));
        let (asker, answered) = ticket_for_test("control");
        let id = turns.open(asker, 30 * S);
        assert_ne!(id, old);
        let _ = answers(&answered);
        assert!(!turns.ends(old));
        assert!(!turns.ends(id + 1));
        assert!(turns.ends(id));
        // Ended by its computer before anyone spoke.
        turns.cancelled(ENDED_BY_COMPUTER);
        assert_eq!(
            answers(&answered),
            vec![Answer::Done(Outcome::Dropped(ENDED_BY_COMPUTER.into()))]
        );
        assert!(!turns.ends(id));
        // Ended by the person's stop while its answer played: spoken, as
        // an answer cut is.
        let (asker, answered) = ticket_for_test("control");
        let id = turns.open(asker, 50 * S);
        let _ = answers(&answered);
        assert!(turns.admitted(12, 51 * S));
        turns.tick(52 * S, Some((12, true)), false);
        turns.cancelled("hand_stop");
        assert_eq!(answers(&answered), vec![Answer::Done(Outcome::Spoken)]);
        assert!(!turns.ends(id));
    }
}
