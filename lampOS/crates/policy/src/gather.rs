//! Putting together what was heard, and keeping what the lamp itself said.
//!
//! The voice publishes what it hears a piece at a time, as the recogniser
//! writes it. A judgement is about a whole thing said, so the pieces are
//! gathered here into utterances ([`Gather`]). The voice also publishes
//! what the lamp says and whether it is speaking, and [`Lamp`] keeps the
//! little of that a judgement needs.
//!
//! What is known of the lamp goes with an utterance as it was when the
//! utterance's first piece came, and not as it is when the utterance ends.
//! By its end the lamp may have begun to answer it, and whether to answer
//! must not be judged from the answer: the log these judgements go to is
//! what the weights are fitted from, and a context that held the lamp's
//! own reaction would give away what is to be learned.
//!
//! Neither reads a clock: the caller says what time it is, so that a test
//! can say so too.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// An utterance is over when no further piece has come for this long.
/// Today the voice marks no piece as the last one, so this is what ends
/// every utterance. It is also how late every judgement starts.
pub const PAUSE: Duration = Duration::from_millis(300);
/// An utterance is judged at this length whatever follows: talk that never
/// pauses is judged in parts, and the state sent to a model stays small.
pub const MOST_HEARD_BYTES: usize = 2_000;
/// More than this of what the lamp is saying is not kept: a judgement is
/// shown the end of it.
const MOST_SAID_BYTES: usize = 1_000;

/// One whole thing that was heard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Utterance {
    /// The pieces put together, without space at either end.
    pub text: String,
    /// How many pieces it came in.
    pub pieces: u32,
    /// The request it belongs to, if any piece said.
    pub request: Option<u64>,
    /// When its first piece came, and its last.
    pub began: Instant,
    pub ended: Instant,
    /// The same by the stamps the pieces were sent with
    /// ([`crate::outcome::sent_at`]): for the outcome only, never for when
    /// the utterance is over (#1652).
    pub sent: (Instant, Instant),
    /// What was known of the lamp when the first piece came.
    pub lamp: LampThen,
    /// It was ended by its length and not by a pause.
    pub cut: bool,
}

/// What was known of the lamp's own speech at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LampThen {
    /// It was speaking.
    pub speaking: bool,
    /// What it had last said, as far as that is kept. Empty if nothing.
    pub said: String,
    /// When it had last spoken: that very moment if it was speaking.
    pub spoke: Option<Instant>,
}

struct Open {
    text: String,
    pieces: u32,
    request: Option<u64>,
    began: Instant,
    last: Instant,
    /// When its first and newest pieces were sent, by their stamps.
    sent: (Instant, Instant),
    lamp: LampThen,
    /// The person had stopped speaking when its newest piece came.
    stopped: bool,
}

/// How long an utterance waits for another piece once the voice has said
/// the person stopped speaking. The text of what was said comes about half
/// a second after that, usually whole; this is for a second piece that
/// follows on its heels. On lamp-4ace on 2026-10-14, waiting the full
/// pause every time, a decision was known about when the lamp began to
/// answer: too late to be of use to it.
pub const AFTER_STOP: Duration = Duration::from_millis(80);

/// Pieces of what was heard, gathered into utterances.
pub struct Gather {
    pause: Duration,
    /// The voice is no longer listening: the person has stopped.
    stopped: bool,
    open: Option<Open>,
    ended: VecDeque<Utterance>,
    empty: u64,
}

impl Gather {
    pub fn new(pause: Duration) -> Self {
        Self {
            pause,
            stopped: false,
            open: None,
            ended: VecDeque::new(),
            empty: 0,
        }
    }

    /// The voice said whether it is still listening to someone. While it
    /// is, more of what they say may come and an utterance waits the whole
    /// pause; once it is not, what comes is the text of something already
    /// said and waits [`AFTER_STOP`]. Not known is taken as still speaking.
    pub fn person_stopped(&mut self, stopped: bool) {
        self.stopped = stopped;
    }

    /// A piece came at `now`. The pieces are what the recogniser wrote
    /// next, each with its own spaces, so they are put end to end. One
    /// marked `finished` is the last of its utterance. `lamp` is asked how
    /// things stand only by a piece that begins an utterance.
    pub fn piece(
        &mut self,
        text: &str,
        finished: bool,
        request: Option<u64>,
        lamp: &Lamp,
        now: Instant,
    ) {
        self.piece_sent(text, finished, request, lamp, now, now);
    }

    /// [`Self::piece`], for a piece its stamp says was sent at `sent`: the
    /// utterance's `sent` times are these (#1652).
    pub fn piece_sent(
        &mut self,
        text: &str,
        finished: bool,
        request: Option<u64>,
        lamp: &Lamp,
        now: Instant,
        sent: Instant,
    ) {
        // A pause nobody noticed in time still ended what came before it.
        self.tick(now);
        // A piece of another of the voice's turns ends what is open: a
        // decision is about one turn's words, and carries that turn's
        // number, never a later one's (the reviewer on #908, 06:40).
        if let (Some(open), Some(request)) = (self.open.as_ref(), request)
            && open.request.is_some_and(|of| of != request)
        {
            self.end(false);
        }
        let mut rest = text;
        loop {
            let open = self.open.get_or_insert_with(|| Open {
                text: String::new(),
                pieces: 0,
                request: None,
                began: now,
                last: now,
                sent: (sent, sent),
                lamp: lamp.then(now),
                stopped: false,
            });
            let mut fits = rest.len().min(MOST_HEARD_BYTES - open.text.len());
            while !rest.is_char_boundary(fits) {
                fits -= 1;
            }
            open.text.push_str(&rest[..fits]);
            open.pieces += 1;
            open.request = request.or(open.request);
            open.last = now;
            open.sent.1 = sent;
            open.stopped = self.stopped;
            rest = &rest[fits..];
            if rest.is_empty() {
                break;
            }
            // Full: it is judged as it is, and the rest begins another.
            self.end(true);
        }
        if finished {
            self.end(false);
        }
    }

    /// Time has passed. An utterance that has had no piece for the pause
    /// is over.
    pub fn tick(&mut self, now: Instant) {
        let pause = self.pause;
        if self.open.as_ref().is_some_and(|open| {
            let wait = if open.stopped {
                AFTER_STOP.min(pause)
            } else {
                pause
            };
            now.saturating_duration_since(open.last) >= wait
        }) {
            self.end(false);
        }
    }

    fn end(&mut self, cut: bool) {
        let Some(open) = self.open.take() else {
            return;
        };
        let text = open.text.trim();
        // A closing mark with no words before it, or only spaces.
        if text.is_empty() {
            self.empty += 1;
            return;
        }
        self.ended.push_back(Utterance {
            text: text.to_owned(),
            pieces: open.pieces,
            request: open.request,
            began: open.began,
            ended: open.last,
            sent: open.sent,
            lamp: open.lamp,
            cut,
        });
    }

    /// The next utterance that is over, oldest first.
    pub fn over(&mut self) -> Option<Utterance> {
        self.ended.pop_front()
    }

    /// How many utterances had no words in them and were let go.
    pub fn empty(&self) -> u64 {
        self.empty
    }
}

/// What the lamp is saying and has said, as far as a judgement needs it.
#[derive(Default)]
pub struct Lamp {
    speaking: bool,
    /// The pieces of its newest answer, end to end.
    said: String,
    request: Option<u64>,
    /// When it last spoke: the later of the last piece of what it said and
    /// the last moment its state was speaking. The text of an answer can
    /// come before its sound has been played to the end, so the last piece
    /// alone could date its speech too early.
    spoke: Option<Instant>,
}

impl Lamp {
    /// The voice said what it is doing: speaking, or something else.
    pub fn state(&mut self, speaking: bool, now: Instant) {
        // One that was speaking until this message spoke until about now.
        if speaking || self.speaking {
            self.spoke = Some(now);
        }
        self.speaking = speaking;
    }

    /// A piece of what the lamp said in answer to `request`. A piece of
    /// another answer than the one held begins anew.
    pub fn said(&mut self, text: &str, request: Option<u64>, now: Instant) {
        if request != self.request {
            self.said.clear();
            self.request = request;
        }
        if text.is_empty() {
            return;
        }
        self.said.push_str(text);
        let mut from = self.said.len().saturating_sub(MOST_SAID_BYTES);
        while !self.said.is_char_boundary(from) {
            from += 1;
        }
        self.said.drain(..from);
        self.spoke = Some(now);
    }

    /// The voice has gone from the bus: whatever it was doing, it is not
    /// speaking now.
    pub fn gone(&mut self) {
        self.speaking = false;
    }

    /// Its newest answer: the request it belongs to, and its pieces end
    /// to end as far as they are kept.
    pub fn answer(&self) -> (Option<u64>, &str) {
        (self.request, &self.said)
    }

    /// How things stand at `now`, to go with an utterance that begins.
    pub fn then(&self, now: Instant) -> LampThen {
        LampThen {
            speaking: self.speaking,
            said: self.said.clone(),
            spoke: if self.speaking { Some(now) } else { self.spoke },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn pieces_are_put_end_to_end_and_a_pause_ends_the_utterance() {
        let start = Instant::now();
        let (mut gather, lamp) = (Gather::new(PAUSE), Lamp::default());
        for (at, piece) in [
            (0, " Hey"),
            (120, " Lamp,"),
            (260, " what"),
            (540, " time is it?"),
        ] {
            gather.piece(piece, false, None, &lamp, start + ms(at));
            assert_eq!(gather.over(), None, "{piece:?}");
        }
        // 299 ms after the last piece it is still being said.
        gather.tick(start + ms(839));
        assert_eq!(gather.over(), None);
        gather.tick(start + ms(840));
        let whole = gather.over().expect("over");
        assert_eq!(whole.text, "Hey Lamp, what time is it?");
        assert_eq!((whole.pieces, whole.request, whole.cut), (4, None, false));
        assert_eq!((whole.began, whole.ended), (start, start + ms(540)));
        assert_eq!(whole.lamp, lamp.then(start));
        assert_eq!(gather.over(), None);
        // Nothing is open: time passing ends nothing.
        gather.tick(start + ms(5_000));
        assert_eq!(gather.over(), None);
    }

    #[test]
    fn a_piece_marked_finished_ends_the_utterance_at_once() {
        let start = Instant::now();
        let (mut gather, lamp) = (Gather::new(PAUSE), Lamp::default());
        gather.piece("Good", false, None, &lamp, start);
        gather.piece(" night.", true, Some(9), &lamp, start + ms(50));
        let whole = gather.over().expect("over");
        assert_eq!((whole.text.as_str(), whole.pieces), ("Good night.", 2));
        assert_eq!(whole.request, Some(9));
        // A closing mark may carry no text of its own.
        gather.piece("Thanks", false, None, &lamp, start + ms(400));
        gather.piece("", true, None, &lamp, start + ms(450));
        assert_eq!(gather.over().expect("over").text, "Thanks");
        // One with nothing before it, or only spaces, is no utterance.
        gather.piece("", true, None, &lamp, start + ms(900));
        gather.piece("  \n", false, None, &lamp, start + ms(1_000));
        gather.tick(start + ms(1_400));
        assert_eq!(gather.over(), None);
        assert_eq!(gather.empty(), 2);
    }

    /// The reviewer on #908 (06:40): turn 1's words and, 50 ms later,
    /// turn 2's, neither marked finished, are two utterances, each with its
    /// own number. A piece with no number joins what is open.
    #[test]
    fn a_piece_of_another_turn_begins_another_utterance() {
        let start = Instant::now();
        let (mut gather, lamp) = (Gather::new(PAUSE), Lamp::default());
        gather.piece("Hey Lamp, turn it up. ", false, Some(1), &lamp, start);
        gather.piece("Thailand.", false, Some(2), &lamp, start + ms(50));
        let first = gather.over().expect("turn 1's words are over");
        assert_eq!(
            (first.text.as_str(), first.request, first.pieces, first.cut),
            ("Hey Lamp, turn it up.", Some(1), 1, false)
        );
        assert_eq!(gather.over(), None);
        gather.piece(" Yes.", false, None, &lamp, start + ms(100));
        gather.tick(start + ms(100) + PAUSE);
        let second = gather.over().expect("turn 2's words are over");
        assert_eq!(
            (second.text.as_str(), second.request, second.pieces),
            ("Thailand. Yes.", Some(2), 2)
        );
    }

    #[test]
    fn once_the_person_has_stopped_an_utterance_waits_a_moment_and_not_the_whole_pause() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let lamp = Lamp::default();
        let mut gather = Gather::new(Duration::from_millis(300));
        // Still speaking, or not known: the whole pause.
        gather.piece("What is", false, None, &lamp, at(0));
        gather.tick(at(299));
        assert!(gather.over().is_none());
        // The voice says they have stopped, and the rest of the text comes.
        gather.person_stopped(true);
        gather.piece(" two plus two?", false, None, &lamp, at(299));
        gather.tick(at(378));
        assert!(gather.over().is_none());
        gather.tick(at(379));
        assert_eq!(gather.over().unwrap().text, "What is two plus two?");
        // Someone speaks again: the whole pause again.
        gather.person_stopped(false);
        gather.piece("And", false, None, &lamp, at(1_000));
        gather.tick(at(1_200));
        assert!(gather.over().is_none());
        gather.tick(at(1_300));
        assert_eq!(gather.over().unwrap().text, "And");
    }

    #[test]
    fn a_piece_that_comes_after_an_unnoticed_pause_begins_another_utterance() {
        let start = Instant::now();
        let (mut gather, lamp) = (Gather::new(PAUSE), Lamp::default());
        gather.piece("Is it raining?", false, None, &lamp, start);
        // Nobody looked for half a second, and then a piece came.
        gather.piece("Tom, the printer.", false, None, &lamp, start + ms(500));
        assert_eq!(gather.over().expect("the first").text, "Is it raining?");
        assert_eq!(gather.over(), None);
        gather.tick(start + ms(800));
        let second = gather.over().expect("the second");
        assert_eq!(second.text, "Tom, the printer.");
        assert_eq!(second.began, start + ms(500));
    }

    #[test]
    fn what_the_lamp_was_doing_is_taken_when_the_utterance_begins_and_not_after() {
        let start = Instant::now();
        let (mut gather, mut lamp) = (Gather::new(PAUSE), Lamp::default());
        lamp.said("It is ten past four.", Some(3), start);
        lamp.state(true, start + ms(100));
        lamp.state(false, start + ms(2_000));
        // Four seconds after the lamp stopped, somebody speaks.
        gather.piece("And", false, None, &lamp, start + ms(6_000));
        // Before they have finished the lamp has begun to answer them.
        lamp.said("Tomorrow it will", Some(4), start + ms(6_100));
        lamp.state(true, start + ms(6_150));
        gather.piece(" tomorrow?", false, None, &lamp, start + ms(6_200));
        gather.tick(start + ms(6_500));
        let whole = gather.over().expect("over");
        assert_eq!(whole.text, "And tomorrow?");
        assert_eq!(
            whole.lamp,
            LampThen {
                speaking: false,
                said: "It is ten past four.".into(),
                spoke: Some(start + ms(2_000)),
            }
        );
        // Said over the lamp: it was speaking at that very moment, however
        // long ago its state was last told.
        gather.piece("Stop.", true, None, &lamp, start + ms(7_000));
        let over = gather.over().expect("over");
        assert!(over.lamp.speaking);
        assert_eq!(over.lamp.said, "Tomorrow it will");
        assert_eq!(over.lamp.spoke, Some(start + ms(7_000)));
    }

    #[test]
    fn talk_that_never_pauses_is_judged_in_parts_cut_at_a_whole_character() {
        let start = Instant::now();
        let (mut gather, lamp) = (Gather::new(PAUSE), Lamp::default());
        // 1,998 bytes, then a piece of two-byte letters across the limit.
        gather.piece(&"a".repeat(1_997), false, None, &lamp, start);
        gather.piece("b", false, None, &lamp, start + ms(10));
        gather.piece("ơơơ", false, None, &lamp, start + ms(20));
        let first = gather.over().expect("full");
        assert_eq!(first.text.len(), 2_000);
        assert!(first.cut && first.text.ends_with("abơ"));
        assert_eq!(first.pieces, 3);
        gather.tick(start + ms(320));
        let rest = gather.over().expect("the rest");
        assert_eq!((rest.text.as_str(), rest.cut), ("ơơ", false));
        // One piece far longer than the limit is several utterances.
        let letter = "ệ";
        assert_eq!(letter.len(), 3);
        gather.piece(&letter.repeat(1_500), true, None, &lamp, start + ms(1_000));
        let (mut bytes, mut parts) = (0, 0);
        while let Some(part) = gather.over() {
            assert!(part.text.len() <= MOST_HEARD_BYTES);
            assert!(part.text.chars().all(|c| c == 'ệ'));
            bytes += part.text.len();
            parts += 1;
        }
        assert_eq!((bytes, parts), (4_500, 3));
    }

    #[test]
    fn the_lamp_keeps_its_newest_answer_and_when_it_last_spoke() {
        let start = Instant::now();
        let mut lamp = Lamp::default();
        let then = |lamp: &Lamp, at: u64| lamp.then(start + ms(at));
        assert_eq!(
            then(&lamp, 0),
            LampThen {
                speaking: false,
                said: String::new(),
                spoke: None
            }
        );
        lamp.said("It is", Some(7), start);
        lamp.said(" ten past four.", Some(7), start + ms(200));
        assert_eq!(then(&lamp, 300).said, "It is ten past four.");
        assert_eq!(then(&lamp, 300).spoke, Some(start + ms(200)));
        // A closing mark with no text says nothing new.
        lamp.said("", Some(7), start + ms(900));
        assert_eq!(then(&lamp, 950).spoke, Some(start + ms(200)));
        // The words can come well before the sound has all been played:
        // while its state is speaking, it is speaking now.
        lamp.state(true, start + ms(1_000));
        assert!(then(&lamp, 1_700).speaking);
        assert_eq!(then(&lamp, 1_700).spoke, Some(start + ms(1_700)));
        // It spoke until about the message that says it stopped.
        lamp.state(false, start + ms(2_600));
        assert!(!then(&lamp, 3_000).speaking);
        assert_eq!(then(&lamp, 3_000).spoke, Some(start + ms(2_600)));
        lamp.state(false, start + ms(3_600));
        assert_eq!(then(&lamp, 4_000).spoke, Some(start + ms(2_600)));
        // Another answer replaces the one held.
        lamp.said("Black.", Some(8), start + ms(9_000));
        assert_eq!(then(&lamp, 9_100).said, "Black.");
        assert_eq!(then(&lamp, 9_100).spoke, Some(start + ms(9_000)));
        // A voice that has gone is not speaking, whatever it last said.
        lamp.state(true, start + ms(9_500));
        lamp.gone();
        assert!(!then(&lamp, 12_000).speaking);
        assert_eq!(then(&lamp, 12_000).spoke, Some(start + ms(9_500)));
    }

    #[test]
    fn a_long_answer_is_kept_by_its_end() {
        let start = Instant::now();
        let mut lamp = Lamp::default();
        for _ in 0..40 {
            lamp.said(
                "Hôm nay trời đẹp quá, chúng ta đi dạo nhé. ",
                Some(1),
                start,
            );
        }
        let said = lamp.then(start).said;
        assert!(said.len() <= 1_000 && said.len() > 990, "{}", said.len());
        assert!(said.ends_with("đi dạo nhé. "));
    }
}
